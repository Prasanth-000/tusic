//! KALKI mpv engine — persistent mpv over unix-socket JSON IPC.
//! Mirrors the proven legacy Python engine design:
//!  - spawn: mpv --no-video --idle --input-ipc-server=/tmp/kalki.sock
//!  - audio: pass XDG_RUNTIME_DIR + explicit --ao=pipewire,pulse,alsa so the
//!    spawned child reaches PipeWire (without it, ao=[] and no sound).
//!  - single-track replace-load; queue/advance logic lives in app state

use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const SOCK_PATH: &str = "/tmp/kalki.sock";

#[derive(Debug, Clone, Default)]
pub struct PlayerState {
    pub time_pos: f64,
    pub duration: f64,
    pub paused: bool,
    pub idle: bool,
    pub volume: i64,
    pub media_title: String,
    pub path: String,
    pub repeat_one: bool,
    pub repeat_all: bool,
}

pub struct Engine {
    proc: Mutex<Child>,
    // (write stream, persistent buffered reader) — one lock for the whole
    // request/response cycle. Per-call BufReaders over-clone and LOSE
    // buffered bytes; mpv also pushes start-file/end-file events, so we
    // correlate responses by request_id and skip event lines.
    io: Mutex<(UnixStream, BufReader<UnixStream>)>,
    pub state: Arc<Mutex<PlayerState>>,
    stopping: Arc<AtomicBool>,
}

static REQ_ID: AtomicU64 = AtomicU64::new(1);

fn load_uri_safe(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '\n' | '\r' => ' ',
            _ => c,
        })
        .collect()
}

impl Engine {
    /// Spawn persistent idle mpv and wait for the IPC socket.
    /// Guard: kill any stale mpv still bound to our socket path first
    /// (a zombie from a crashed run would steal every IPC command otherwise).
    pub fn spawn(volume: i64) -> Result<Arc<Self>> {
        let _ = std::process::Command::new("pkill")
            .args(["-f", &format!("input-ipc-server={SOCK_PATH}")])
            .status();
        std::thread::sleep(Duration::from_millis(150));
        let _ = std::fs::remove_file(SOCK_PATH);
        // Pass through the session env so the spawned mpv can reach
        // PipeWire/PulseAudio. WITHOUT these, the child can't find the user's
        // audio session → ao=[] (no output device) even though system sound
        // works. Key points (learned from live debugging + research agents):
        //   - XDG_RUNTIME_DIR: where PipeWire socket ($xdg/pipewire-0) lives.
        //   - DBUS_SESSION_BUS_ADDRESS: share the user's session bus (mpv's
        //     PipeWire backend resolves the node over the bus).
        //   - PULSE_SERVER: DO NOT hardcode. If set to a stale/nonexistent
        //     socket, mpv connects to dead addr and is silent. Let mpv
        //     discover it via XDG_RUNTIME_DIR, or pass the parent's real value.
        let xdg = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/run/user/1000".into());
        let mut cmd = Command::new("mpv");
        cmd.env("XDG_RUNTIME_DIR", &xdg);
        if let Ok(bus) = std::env::var("DBUS_SESSION_BUS_ADDRESS") {
            cmd.env("DBUS_SESSION_BUS_ADDRESS", bus);
        }
        // Only forward PULSE_SERVER if the parent actually had a valid one;
        // never invent a path that may not exist.
        if let Ok(ps) = std::env::var("PULSE_SERVER") {
            cmd.env("PULSE_SERVER", ps);
        }
        cmd.args([
            "--no-video",
            "--idle",
            "--ao=pipewire,pulse,alsa",
            "--audio-device=auto",
            "--audio-fallback-to-null=no",
            "--gapless-audio=no",
            "--cache=yes",
            "--demuxer-max-bytes=128M",
            &format!("--volume={volume}"),
            &format!("--input-ipc-server={SOCK_PATH}"),
        ])
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let proc = cmd
            .spawn()
            .map_err(|e| anyhow!("mpv spawn failed: {e} (is mpv installed?)"))?;

        // wait for socket (max 3s)
        let mut sock = None;
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if std::path::Path::new(SOCK_PATH).exists() {
                match UnixStream::connect(SOCK_PATH) {
                    Ok(s) => {
                        sock = Some(s);
                        break;
                    }
                    Err(_) => {}
                }
            }
            std::thread::sleep(Duration::from_millis(80));
        }
        let stream = sock.ok_or_else(|| {
            anyhow!("mpv IPC socket never appeared (mpv died? check: ps aux | grep mpv)")
        })?;
        let write_stream = stream.try_clone()?;

        Ok(Arc::new(Engine {
            proc: Mutex::new(proc),
            io: Mutex::new((write_stream, BufReader::new(stream))),
            state: Arc::new(Mutex::new(PlayerState::default())),
            stopping: Arc::new(AtomicBool::new(false)),
        }))
    }

    /// Send one command, return the parsed JSON response. mpv pushes
    /// start-file/end-file events between responses, so read lines until the
    /// matching request_id arrives (skip event lines).
    pub fn cmd(&self, args: Vec<Value>) -> Result<Value> {
        let mut io = self.io.lock().unwrap();
        let rid = REQ_ID.fetch_add(1, Ordering::Relaxed);
        let req = json!({"command": args, "request_id": rid});
        let mut line = serde_json::to_string(&req)?;
        line.push('\n');
        io.0.write_all(line.as_bytes())?;
        io.0.flush()?;
        loop {
            let mut resp = String::new();
            let n = io.1.read_line(&mut resp)?;
            if n == 0 {
                return Err(anyhow!("mpv IPC closed"));
            }
            let v: Value = serde_json::from_str(&resp)?;
            if let Some(rid2) = v.get("request_id").and_then(|r| r.as_u64()) {
                if rid2 == rid {
                    if let Some(err) = v.get("error") {
                        if err != "success" {
                            return Err(anyhow!("mpv error: {err}"));
                        }
                    }
                    return Ok(v.get("data").cloned().unwrap_or(Value::Null));
                }
            }
        }
    }

    pub fn get_prop(&self, name: &str) -> Result<Value> {
        self.cmd(vec![json!("get_property"), json!(name)])
    }

    pub fn set_prop(&self, name: &str, value: Value) -> Result<()> {
        self.cmd(vec![json!("set_property"), json!(name), value])?;
        Ok(())
    }

    /// Replace-current playback (loadfile <uri> replace).
    pub fn play(&self, uri: &str) -> Result<()> {
        self.cmd(vec![json!("loadfile"), json!(load_uri_safe(uri)), json!("replace")])?;
        Ok(())
    }

    pub fn pause(&self, p: bool) -> Result<()> {
        self.set_prop("pause", json!(p))
    }

    pub fn toggle_pause(&self) -> Result<()> {
        self.cmd(vec![json!("cycle"), json!("pause")])?;
        Ok(())
    }

    pub fn seek(&self, pos: f64) -> Result<()> {
        self.cmd(vec![json!("seek"), json!(pos), json!("absolute")])?;
        Ok(())
    }

    pub fn seek_rel(&self, delta: f64) -> Result<()> {
        self.cmd(vec![json!("seek"), json!(delta), json!("relative")])?;
        Ok(())
    }

    pub fn set_volume(&self, vol: i64) -> Result<()> {
        self.set_prop("volume", json!(vol))
    }

    pub fn stop(&self) -> Result<()> {
        self.cmd(vec![json!("stop")])?;
        Ok(())
    }

    pub fn set_repeat_one(&self, on: bool) -> Result<()> {
        self.set_prop("loop-file", json!(on))
    }

    pub fn set_repeat_all(&self, on: bool) -> Result<()> {
        self.set_prop("loop-playlist", json!(on))
    }

    /// Poll mpv state into the shared PlayerState. Call from a thread every ~0.4s.
    pub fn poll_state(&self) {
        let mut st = self.state.lock().unwrap();
        if let Ok(v) = self.get_prop("time-pos") {
            if let Some(f) = v.as_f64() {
                st.time_pos = f;
            }
        }
        if let Ok(v) = self.get_prop("duration") {
            if let Some(f) = v.as_f64() {
                st.duration = f;
            }
        }
        if let Ok(v) = self.get_prop("pause") {
            if let Some(b) = v.as_bool() {
                st.paused = b;
            }
        }
        if let Ok(v) = self.get_prop("idle-active") {
            if let Some(b) = v.as_bool() {
                st.idle = b;
            }
        }
        if let Ok(v) = self.get_prop("volume") {
            if let Some(n) = v.as_i64() {
                st.volume = n;
            } else if let Some(f) = v.as_f64() {
                st.volume = f.round() as i64;
            }
        }
        if let Ok(v) = self.get_prop("media-title") {
            if let Some(s) = v.as_str() {
                st.media_title = s.to_string();
            }
        }
        if let Ok(v) = self.get_prop("path") {
            if let Some(s) = v.as_str() {
                st.path = s.to_string();
            }
        }
        if let Ok(v) = self.get_prop("loop-file") {
            if let Some(b) = v.as_bool() {
                st.repeat_one = b;
            }
        }
        if let Ok(v) = self.get_prop("loop-playlist") {
            if let Some(b) = v.as_bool() {
                st.repeat_all = b;
            }
        }
    }

    /// Spawn the state poller thread (0.4s). Returns a stop flag to signal exit.
    pub fn start_poller(engine: Arc<Self>) -> Arc<AtomicBool> {
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = stop.clone();
        std::thread::spawn(move || {
            while !stop2.load(Ordering::Relaxed) {
                engine.poll_state();
                std::thread::sleep(Duration::from_millis(400));
            }
        });
        stop
    }

    /// Clean shutdown: quit, kill, wait (plain wait() hangs on --idle mpv).
    pub fn shutdown(&self) {
        let _ = self.cmd(vec![json!("quit")]);
        let mut p = self.proc.lock().unwrap();
        let _ = p.kill();
        let _ = p.wait();
        drop(p);
        let _ = std::fs::remove_file(SOCK_PATH);
        self.stopping.store(true, Ordering::SeqCst);
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        if !self.stopping.swap(true, Ordering::SeqCst) {
            let mut p = self.proc.lock().unwrap();
            let _ = p.kill();
            let _ = p.wait();
            drop(p);
            let _ = std::fs::remove_file(SOCK_PATH);
        }
    }
}
