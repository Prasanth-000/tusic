//! KALKI app core — state, key handling, drawing, playback orchestration.

use crate::{engine::Engine, lyrics, music_visual::MusicVisual, sources::Track, viz::Spectrum};
use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const TABS: [&str; 5] = ["Results", "Queue", "Library", "Stats", "Playlists"];

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Mode {
    Normal,
    Search,
    Command,
    Help,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Repeat {
    Off,
    All,
    One,
}

pub struct App {
    pub engine: Option<Arc<Engine>>,
    pub results: Vec<Track>,
    pub queue: Vec<Track>,
    pub queue_pos: Option<usize>,
    pub sel: usize,
    pub qsel: usize,
    pub active_tab: usize,
    pub mode: Mode,
    pub query: String,
    pub cmd: String,
    pub status: String,
    pub quit: bool,

    pub volume: i64,
    pub repeat: Repeat,
    pub shuffle: bool,
    pub current: Option<Track>,
    was_playing: bool,
    last_path: String,
    last_poll: Instant,
    pub display_pos: f64,
    pub paused: bool,

    pub lyrics: Option<lyrics::Lyrics>,
    pub music_visual: MusicVisual,
    pub spectrum: Spectrum,
    // --- EQ engine (cava-style, rebuilt from scratch): per-band pipeline
    //     raw → 4-frame integral → Monstercat EMA (0.81) → gravity fall
    //     → peak-hold cap. Every value traces to real audio; no wobble. ---
    eq_hist: [[f32; 4]; crate::viz::BANDS], // integral ring buffers
    eq_hidx: [usize; crate::viz::BANDS],    // ring write index
    eq_ema: [f32; crate::viz::BANDS],       // Monstercat smoothed value
    eq_disp: [f32; crate::viz::BANDS],      // gravity-fallen display value
    eq_peak: [f32; crate::viz::BANDS],      // peak-hold cap
    eq_gain: f32,                           // auto-gain for quiet sinks
    eq_last: Instant,                       // dt clock (frame-rate independent)
    pub romanize: bool,
    pub sync_offsets: HashMap<String, f64>,
    autoplay: bool,   // set by `:play <q>` — auto-play first result on arrival
    pub searching: bool, // true while a search is in flight (drives loading bar)
    pub scanning: bool,  // true while ~/Music scan runs
    // history / back-navigation
    hist_stack: Vec<Track>,   // previously played tracks (most recent first)
    hist_suppress: bool,      // one-shot: don't push current (back navigation)
    pub hist_list: Vec<Track>, // History tab contents (db recent_history)
    pub hist_sel: usize,       // History tab selection (separate from results)
    lyrics_rx: Receiver<Option<lyrics::Lyrics>>,
    search_rx: Receiver<Vec<Track>>,
    scan_rx: Receiver<usize>,
    lyrics_tx: std::sync::mpsc::Sender<Option<lyrics::Lyrics>>,
    search_tx: std::sync::mpsc::Sender<Vec<Track>>,
    pub scan_tx: std::sync::mpsc::Sender<usize>,
    lyrics_scroll: usize,

    pub sleep_until: Option<Instant>,
    /// When the last play() was issued. Used to detect playback failure
    /// (mpv stays idle past a timeout → source 429/dead → auto-skip).
    play_attempt_at: Option<Instant>,
}

impl Default for App {
    fn default() -> Self {
        let (lyrics_tx, lyrics_rx) = channel();
        let (search_tx, search_rx) = channel();
        let (scan_tx, scan_rx) = channel();
        App {
            engine: None,
            results: Vec::new(),
            queue: Vec::new(),
            queue_pos: None,
            sel: 0,
            qsel: 0,
            active_tab: 0,
            mode: Mode::Normal,
            query: String::new(),
            cmd: String::new(),
            status: "press / to search — space play/pause — ? keys".into(),
            quit: false,
            volume: 70,
            repeat: Repeat::Off,
            shuffle: false,
            current: None,
            was_playing: false,
            last_path: String::new(),
            last_poll: Instant::now(),
            display_pos: 0.0,
            paused: true,
            lyrics: None,
            music_visual: MusicVisual::default(),
            spectrum: Spectrum::start(),
            eq_hist: [[0.0f32; 4]; crate::viz::BANDS],
            eq_hidx: [0usize; crate::viz::BANDS],
            eq_ema: [0.0f32; crate::viz::BANDS],
            eq_disp: [0.0f32; crate::viz::BANDS],
            eq_peak: [0.0f32; crate::viz::BANDS],
            eq_gain: 1.0,
            eq_last: Instant::now(),
            romanize: false,
            sync_offsets: HashMap::new(),
            autoplay: false,
            searching: false,
            scanning: false,
            hist_stack: Vec::new(),
            hist_suppress: false,
            hist_list: Vec::new(),
            hist_sel: 0,
            lyrics_rx,
            search_rx,
            scan_rx,
            lyrics_tx,
            search_tx,
            scan_tx,
            lyrics_scroll: 0,
            sleep_until: None,
            play_attempt_at: None,
        }
    }
}

impl App {
    pub fn attach_engine(&mut self, engine: Arc<Engine>) {
        self.engine = Some(engine);
    }

    pub fn state_word(&self) -> &'static str {
        if self.current.is_none() {
            "STOPPED"
        } else if self.paused {
            "PAUSED"
        } else {
            "PLAYING"
        }
    }

    // ---------------- playback ----------------

    pub fn play_track(&mut self, t: Track) {
        let Some(eng) = &self.engine else { return };
        let uri = t.uri.clone();
        // capture + clear the one-shot suppress flag FIRST so a failed play
        // (early return below) can never leave it stuck (review finding #1)
        let suppress = std::mem::take(&mut self.hist_suppress);
        if let Err(e) = eng.play(&uri) {
            self.status = format!("play error: {e}");
            return;
        }
        // history: remember the previous track for back-navigation (H)
        if let Some(prev) = self.current.take() {
            if !suppress && prev.uri != t.uri {
                self.hist_stack.push(prev);
                if self.hist_stack.len() > 100 {
                    self.hist_stack.remove(0);
                }
            }
        }
        self.current = Some(t.clone());
        self.paused = false;
        self.display_pos = 0.0;
        self.play_attempt_at = Some(Instant::now());
        self.last_path.clear();
        self.status = format!("▶ {} — {}", t.title, t.artist);
        // lyrics fetch in background
        let title = t.title.clone();
        let dur = t.duration;
        let tx = self.lyrics_tx.clone();
        std::thread::spawn(move || {
            let lyr = lyrics::fetch_lyrics(&title, dur);
            let _ = tx.send(lyr);
        });
        // stats + notification
        crate::db::record_play(&t);
        let _ = notify_rich(&t);
    }

    pub fn toggle_pause(&mut self) {
        if let Some(eng) = &self.engine {
            let _ = eng.toggle_pause();
            self.paused = !self.paused;
        }
    }

    /// Set absolute pause state (used by MPRIS Play/Pause for correctness).
    pub fn set_pause(&mut self, p: bool) {
        if let Some(eng) = &self.engine {
            let _ = eng.pause(p);
            self.paused = p;
        }
    }

    pub fn next_track(&mut self, manual: bool) {
        let n = self.queue.len();
        if n == 0 {
            return;
        }
        let next = match (self.queue_pos, self.repeat) {
            (Some(p), _) => {
                let mut np = p + 1;
                if np >= n {
                    if self.repeat == Repeat::All || self.shuffle {
                        np = 0
                    } else if manual {
                        np = 0
                    } else {
                        self.stop_after();
                        return;
                    }
                }
                np
            }
            (None, _) => 0,
        };
        if let Some(t) = self.queue.get(next).cloned() {
            self.queue_pos = Some(next);
            self.play_track(t);
        }
    }

    pub fn prev_track(&mut self) {
        let n = self.queue.len();
        if n == 0 {
            return;
        }
        let p = self.queue_pos.unwrap_or(0);
        let prev = if p == 0 { n - 1 } else { p - 1 };
        self.queue_pos = Some(prev);
        if let Some(t) = self.queue.get(prev).cloned() {
            self.play_track(t);
        }
    }

    fn stop_after(&mut self) {
        self.queue_pos = None;
        self.current = None;
        if let Some(eng) = &self.engine {
            let _ = eng.stop();
        }
        self.status = "queue ended".into();
    }

    pub fn add_to_queue(&mut self) {
        if let Some(t) = self.results.get(self.sel).cloned() {
            self.queue.push(t);
            self.status = format!("+ queued ({} total)", self.queue.len());
        }
    }

    pub fn remove_from_queue(&mut self) {
        if self.queue.is_empty() {
            return;
        }
        let i = self.qsel.min(self.queue.len() - 1);
        self.queue.remove(i);
        if let Some(p) = self.queue_pos {
            if i < p {
                self.queue_pos = Some(p - 1);
            } else if i == p {
                self.queue_pos = None;
            }
        }
        self.qsel = self.qsel.min(self.queue.len().saturating_sub(1));
    }

    pub fn cycle_repeat(&mut self) {
        self.repeat = match self.repeat {
            Repeat::Off => Repeat::All,
            Repeat::All => Repeat::One,
            Repeat::One => Repeat::Off,
        };
        if let Some(eng) = &self.engine {
            let _ = eng.set_repeat_one(self.repeat == Repeat::One);
            let _ = eng.set_repeat_all(self.repeat == Repeat::All);
        }
        self.status = format!("repeat: {:?}", self.repeat);
    }

    pub fn toggle_shuffle(&mut self) {
        self.shuffle = !self.shuffle;
        self.status = if self.shuffle { "shuffle ON".into() } else { "shuffle OFF".into() };
    }

    // ---------------- search ----------------

    pub fn run_search(&mut self) {
        let q = self.query.trim().to_string();
        if q.is_empty() {
            self.mode = Mode::Normal;
            return;
        }
        self.searching = true;
        self.status = format!("searching: {q} …");
        self.mode = Mode::Normal;
        let tx = self.search_tx.clone();
        std::thread::spawn(move || {
            let tracks = crate::sources::search_all(&q, 6);
            let _ = tx.send(tracks);
        });
    }

    // ---------------- per-frame poll ----------------

    pub fn poll(&mut self) {
        // advance animation frame
        self.music_visual.tick();
        // drive the DNA helix from RAW bass power (bypasses the smoothed
        // loudness envelope — double smoothing added ~100ms latency and
        // blunted transients, which is why the coil never looked beat-tight).
        // Bass bands carry the kick/808 punch; mid adds body.
        let bands = self.spectrum.snapshot();
        let bass: f32 = bands[..4].iter().sum::<f32>() / 4.0;
        let mid: f32 = bands[4..12].iter().sum::<f32>() / 8.0;
        // live-normalize so a moderate hit reaches ~0.8 (auto-gain for beats)
        let bmax = bands.iter().cloned().fold(0.0f32, f32::max).max(0.02);
        let norm = (0.8 / bmax).clamp(1.0, 6.0);
        let helix_e = ((bass * 1.6 + mid * 0.5) * norm).clamp(0.0, 1.0);
        self.music_visual.set_energy(helix_e);

        // ---- EQ engine (cava-style pipeline, rebuilt from scratch) ----
        //   raw → 4-frame integral (noise reduction) → Monstercat EMA 0.81
        //   → gravity fall 3.0/s (instant rise, fixed-rate drop) → peak cap
        // Every value traces to real audio. No synthetic wobble.
        let now = Instant::now();
        let dt = (now - self.eq_last).as_secs_f32().clamp(0.0, 0.25);
        self.eq_last = now;
        let bands = self.spectrum.snapshot();
        // auto-gain: track a slow max so quiet sinks still animate
        let live_max = bands.iter().cloned().fold(0.0f32, f32::max).max(0.02);
        let target_gain = (0.9 / live_max).clamp(1.0, 12.0);
        self.eq_gain += (target_gain - self.eq_gain) * 0.05;
        for i in 0..crate::viz::BANDS {
            let raw = (bands[i] * self.eq_gain).clamp(0.0, 1.0);
            // 1) integral filter: 4-frame ring average kills FFT jitter
            self.eq_hist[i][self.eq_hidx[i]] = raw;
            self.eq_hidx[i] = (self.eq_hidx[i] + 1) & 3;
            let filt = self.eq_hist[i].iter().sum::<f32>() / 4.0;
            // 2) Monstercat EMA: 0.81 → punchy attack, smooth decay
            self.eq_ema[i] = self.eq_ema[i] * 0.19 + filt * 0.81;
            // 3) gravity fall: instant rise, fixed-rate fall (3.0 heights/s)
            let ema = self.eq_ema[i];
            self.eq_disp[i] = if ema > self.eq_disp[i] {
                ema
            } else {
                (self.eq_disp[i] - 3.0 * dt).max(ema)
            };
            // 4) peak-hold cap: captures the hit, decays slower (1.5/s)
            self.eq_peak[i] = if self.eq_disp[i] > self.eq_peak[i] {
                self.eq_disp[i]
            } else {
                (self.eq_peak[i] - 1.5 * dt).max(self.eq_disp[i])
            };
        }
        // drain lyric results
        while let Ok(l) = self.lyrics_rx.try_recv() {
            match l {
                Some(l) => {
                    self.lyrics = Some(l);
                    self.lyrics_scroll = 0;
                }
                None => {
                    self.lyrics = None;
                    self.status = "no lyrics found".into();
                }
            }
        }
        // drain search results
        while let Ok(tracks) = self.search_rx.try_recv() {
            self.results = tracks;
            self.sel = 0;
            self.active_tab = 0;
            self.status = format!("{} results", self.results.len());
            self.searching = false;
            // `:play <q>` — auto-play the first result as soon as it lands
            if self.autoplay {
                self.autoplay = false;
                if !self.results.is_empty() {
                    self.enter_current();
                }
            }
        }
        // drain scan results
        while let Ok(n) = self.scan_rx.try_recv() {
            self.scanning = false;
            self.status = format!("library: {n} tracks indexed");
        }

        // engine state → display
        let mut track_ended = false;
        if let Some(eng) = &self.engine {
            let (now_playing, path, tpos, dur, paused, idle, vol) = {
                let st = eng.state.lock().unwrap();
                (
                    !st.paused && !st.idle,
                    st.path.clone(),
                    st.time_pos,
                    st.duration,
                    st.paused,
                    st.idle,
                    st.volume,
                )
            };
            // track started?
            if !self.was_playing && now_playing && path != self.last_path {
                self.last_path = path;
                self.paused = false;
                // playback begun: the switch-grace is over. A genuine end
                // (even of a sub-1.5s track) is now detectable immediately.
                self.play_attempt_at = None;
                // ensure our volume is applied (mpv resets to --volume on load
                // only; explicit push guards against stale/low values)
                if let Some(eng) = &self.engine {
                    let _ = eng.set_volume(self.volume.clamp(1, 100));
                }
            }
            // track ended? (idle transition while we had a current)
            // GRACE: mpv blips idle between loadfile calls — a new track
            // starting looks like "stopped playing" for ~100-400ms while the
            // old duration is still reported. Ignore end-detection within
            // the switch grace, or switching songs falsely triggers
            // "queue ended" (user's bug: second song wouldn't play).
            let switching = self
                .play_attempt_at
                .map(|at| at.elapsed().as_secs_f64() < 1.5)
                .unwrap_or(false);
            if should_treat_as_ended(self.was_playing, now_playing, self.current.is_some(), dur, switching, paused) {
                self.paused = true;
                track_ended = true;
            }
            self.was_playing = now_playing;
            let _ = idle;
            // Playback failure detection: a play was issued but mpv is still
            // idle after 4s → source is dead/rate-limited (e.g. JioSaavn 429).
            // Auto-skip to the next available track so the user always hears
            // something instead of silence.
            // NEVER fire while paused — the user pausing for >4s total after
            // a play would otherwise look like a dead source and skip the
            // track mid-pause (e2e pause-regression caught this: the 4s
            // auto-skip fired DURING a pause and switched songs).
            if let Some(at) = self.play_attempt_at {
                if !now_playing && !paused && Instant::now().duration_since(at).as_secs_f64() > 4.0 {
                    self.play_attempt_at = None;
                    if self.current.is_some() {
                        self.status = "source failed (429/dead) — skipping…".into();
                        // jump to next in results/queue
                        self.next_track(false);
                    }
                }
            }
            // any actual playback means the last play attempt succeeded —
            // clear the failure latch unconditionally (mpv can start a
            // replacement file WITHOUT an idle blip, so the rising-edge
            // branch below would miss it and the stale flag could later
            // misfire during a pause; e2e pause-regression, same bug).
            if now_playing {
                self.play_attempt_at = None;
            }
            // interpolate position
            let now = Instant::now();
            let dt = now.duration_since(self.last_poll).as_secs_f64();
            self.last_poll = now;
            if now_playing {
                self.display_pos = tpos + dt;
            } else {
                self.display_pos = tpos;
            }
            self.paused = paused;
            // Adopt mpv's volume ONLY when it reports a sane value.
            // mpv reports 0.0 while idle (no file / ao uninitialized) — if we
            // adopt that, the app stores 0 and pushes silence on the next load.
            if vol > 0 {
                self.volume = vol as i64;
            }
        }
        if track_ended {
            self.on_track_ended();
        }
        // sleep timer
        if let Some(until) = self.sleep_until {
            if Instant::now() >= until {
                self.sleep_until = None;
                if let Some(eng) = &self.engine {
                    let _ = eng.stop();
                }
                self.status = "sleep timer fired — stopped".into();
            }
        }
    }

    /// Read the cava-pipeline EQ state for the TUI renderer.
    /// Returns the per-band display height (0..1, gravity-fallen) and the
    /// peak-hold cap.
    pub fn eq_bars(&self) -> (&[f32; crate::viz::BANDS], &[f32; crate::viz::BANDS]) {
        (&self.eq_disp, &self.eq_peak)
    }

    fn on_track_ended(&mut self) {
        if self.repeat == Repeat::One {
            return; // mpv loops the file itself
        }
        self.next_track(false);
    }

    // ---------------- commands ----------------

    pub fn run_command(&mut self) {
        let c = self.cmd.trim().to_string();
        self.cmd.clear();
        self.mode = Mode::Normal;
        let mut parts = c.splitn(2, ' ');
        let cmd = parts.next().unwrap_or("").to_lowercase();
        let arg = parts.next().unwrap_or("").trim().to_string();
        match cmd.as_str() {
            "quit" | "q" => self.quit = true,
            "sleep" => match arg.parse::<u64>() {
                Ok(m) => {
                    self.sleep_until = Some(Instant::now() + Duration::from_secs(m * 60));
                    self.status = format!("sleep in {m} min");
                }
                Err(_) => self.status = "usage: :sleep <minutes>".into(),
            },
            "vol" => {
                if let Ok(v) = arg.parse::<i64>() {
                    self.volume = v.clamp(0, 100);
                    if let Some(eng) = &self.engine {
                        let _ = eng.set_volume(self.volume);
                    }
                }
            }
            "shuffle" => self.toggle_shuffle(),
            "repeat" => self.cycle_repeat(),
            "help" => self.mode = Mode::Help,
            "play" if !arg.is_empty() => {
                self.query = arg.clone();
                self.cmd.clear();
                self.mode = Mode::Normal; // leave command prompt so Enter plays
                self.active_tab = 0;
                self.sel = 0;
                self.autoplay = true; // play first result when search returns
                self.run_search();
            }
            "queue" => self.active_tab = 1,
            "stats" => self.active_tab = 3,
            "playlists" => self.active_tab = 4,
            "history" => {
                self.load_history();
                self.active_tab = 5;
            }
            "loop" => {
                self.repeat = match self.repeat {
                    Repeat::Off => Repeat::All,
                    Repeat::All => Repeat::One,
                    Repeat::One => Repeat::Off,
                };
                self.status = format!("repeat: {:?}", self.repeat);
            }
            "pl" if !arg.is_empty() => self.run_pl(&arg),
            "recs" => self.recs(),
            "scan" => {
                self.status = "scanning ~/Music …".into();
                self.scanning = true;
                let tx = self.scan_tx.clone();
                std::thread::spawn(move || {
                    let n = crate::db::scan_library(None);
                    let _ = tx.send(n);
                });
            }
            "clear" => {
                self.queue.clear();
                self.queue_pos = None;
                self.status = "queue cleared".into();
            }
            _ => self.status = format!("unknown command: {cmd} (try :help)"),
        }
    }

    /// `:pl <sub>` — playlist management.
    ///   save <name>  → persist current queue as a playlist
    ///   load <name>  → replace queue with the playlist
    ///   add <name>   → append current track to playlist
    ///   list         → show all playlists
    ///   delete <name>→ remove a playlist
    ///   now <name>   → save current queue AND load it (round-trip)
    fn run_pl(&mut self, arg: &str) {
        let mut it = arg.splitn(2, char::is_whitespace);
        let sub = it.next().unwrap_or("");
        let name = it.next().unwrap_or("").trim().to_string();
        match sub {
            "save" => {
                if name.is_empty() {
                    self.status = "usage: :pl save <name>".into();
                    return;
                }
                if self.queue.is_empty() {
                    self.status = "queue empty — nothing to save".into();
                    return;
                }
                let n = self.queue.len();
                match crate::db::save_playlist(&name, &self.queue) {
                    Ok(k) => self.status = format!("💾 playlist '{name}': {k}/{n} tracks saved"),
                    Err(e) => self.status = format!("playlist error: {e}"),
                }
            }
            "load" => {
                if name.is_empty() {
                    self.status = "usage: :pl load <name>".into();
                    return;
                }
                match crate::db::load_playlist(&name) {
                    Some(tracks) if !tracks.is_empty() => {
                        self.queue = tracks;
                        self.queue_pos = Some(0);
                        self.status = format!("▶ loaded playlist '{name}' ({} tracks)", self.queue.len());
                    }
                    _ => self.status = format!("playlist '{name}' empty or missing"),
                }
            }
            "add" => {
                if name.is_empty() {
                    self.status = "usage: :pl add <name>".into();
                    return;
                }
                let Some(cur) = self.current.clone() else {
                    self.status = "nothing playing".into();
                    return;
                };
                match crate::db::add_to_playlist(&name, &cur) {
                    Ok(n) => self.status = format!("+ added to '{name}' (track {n})"),
                    Err(e) => self.status = format!("playlist error: {e}"),
                }
            }
            "delete" | "rm" => {
                if name.is_empty() {
                    self.status = "usage: :pl delete <name>".into();
                    return;
                }
                match crate::db::delete_playlist(&name) {
                    Ok(()) => self.status = format!("🗑 deleted playlist '{name}'"),
                    Err(e) => self.status = format!("{e}"),
                }
            }
            "now" => {
                if name.is_empty() {
                    self.status = "usage: :pl now <name>".into();
                    return;
                }
                let _ = crate::db::save_playlist(&name, &self.queue);
                match crate::db::load_playlist(&name) {
                    Some(tracks) if !tracks.is_empty() => {
                        self.queue = tracks;
                        self.queue_pos = Some(0);
                        self.status = format!("💾+▶ '{name}' saved & loaded ({} tracks)", self.queue.len());
                    }
                    _ => self.status = "playlist error".into(),
                }
            }
            "list" => {
                let pls = crate::db::list_playlists();
                if pls.is_empty() {
                    self.status = "no playlists yet — :pl save <name>".into();
                } else {
                    self.status = format!(
                        "playlists: {}",
                        pls.iter().map(|(n, c, _)| format!("{n}({c})")).collect::<Vec<_>>().join(", ")
                    );
                }
            }
            _ => self.status = "usage: :pl save|load|add|delete|list|now <name>".into(),
        }
    }

    // ---------------- keys ----------------

    pub fn on_key(&mut self, key: crossterm::event::KeyEvent) {
        use crossterm::event::KeyCode::*;
        match self.mode {
            Mode::Search => match key.code {
                Char(c) => {
                    self.query.push(c);
                }
                Backspace => {
                    self.query.pop();
                }
                Esc => {
                    self.mode = Mode::Normal;
                }
                Enter => self.run_search(),
                _ => {}
            },
            Mode::Command => match key.code {
                Char(c) => {
                    self.cmd.push(c);
                }
                Backspace => {
                    self.cmd.pop();
                }
                Esc => {
                    self.cmd.clear();
                    self.mode = Mode::Normal;
                }
                Enter => self.run_command(),
                _ => {}
            },
            Mode::Help => match key.code {
                Char('q') | Esc => self.mode = Mode::Normal,
                _ => {}
            },
            Mode::Normal => match key.code {
                Char('q') => self.quit = true,
                Char(' ') => self.toggle_pause(),
                Char('/') => {
                    self.query.clear();
                    self.mode = Mode::Search;
                }
                Char(':') => {
                    self.cmd.clear();
                    self.mode = Mode::Command;
                }
                Char('?') => self.mode = Mode::Help,
                Char('j') | Down => self.move_sel(1),
                Char('k') | Up => self.move_sel(-1),
                Char('n') => self.next_track(true),
                Char('p') => self.prev_track(),
                Char('s') => self.toggle_shuffle(),
                Char('r') => self.cycle_repeat(),
                Char('a') => self.add_to_queue(),
                Char('d') => self.remove_from_queue(),
                Char('t') => {
                    self.romanize = !self.romanize;
                    self.status = if self.romanize { "tenglish: ON" } else { "tenglish: OFF" }.into();
                }
                Char('z') => self.shift_sync(-0.5),
                Char('x') => self.shift_sync(0.5),
                Char('>') => self.seek_rel(10.0),
                Char('<') => self.seek_rel(-10.0),
                Char('h') => {
                    self.load_history();
                    self.active_tab = 5; // History tab
                    self.hist_sel = 0;
                }
                Char('H') => self.back_track(),
                Char('R') => self.recs(),
                Char('1') => self.active_tab = 0,
                Char('2') => self.active_tab = 1,
                Char('3') => self.active_tab = 2,
                Char('4') => self.active_tab = 3,
                Char('5') => {
                    self.active_tab = 4; // Playlists
                }
                Char('6') => {
                    self.load_history();
                    self.active_tab = 5; // History
                    self.hist_sel = 0;
                }
                Left => self.seek_rel(-5.0),
                Right => self.seek_rel(5.0),
                Char('+') | Char('=') => self.volume += 5,
                Char('-') => self.volume -= 5,
                Enter => self.enter_current(),
                Tab => self.active_tab = (self.active_tab + 1) % TABS.len(),
                BackTab => self.active_tab = (self.active_tab + TABS.len() - 1) % TABS.len(),
                PageDown => {
                    self.lyrics_scroll = self.lyrics_scroll.saturating_add(4);
                }
                PageUp => {
                    self.lyrics_scroll = self.lyrics_scroll.saturating_sub(4);
                }
                _ => {}
            },
        }
        self.volume = self.volume.clamp(0, 100);
        if let Some(eng) = &self.engine {
            let _ = eng.set_volume(self.volume);
        }
    }

    fn move_sel(&mut self, d: i64) {
        let len = match self.active_tab {
            0 => self.results.len() as i64,
            1 => self.queue.len() as i64,
            5 => self.hist_list.len() as i64,
            _ => 0,
        };
        if len == 0 {
            return;
        }
        if self.active_tab == 0 {
            self.sel = ((self.sel as i64 + d).clamp(0, len - 1)) as usize;
        } else if self.active_tab == 1 {
            self.qsel = ((self.qsel as i64 + d).clamp(0, len - 1)) as usize;
        } else if self.active_tab == 5 {
            self.hist_sel = ((self.hist_sel as i64 + d).clamp(0, len - 1)) as usize;
        }
    }

    fn enter_current(&mut self) {
        match self.active_tab {
            0 => {
                if let Some(t) = self.results.get(self.sel).cloned() {
                    // play + put at front of queue
                    if self.queue.is_empty() {
                        self.queue.push(t.clone());
                        self.queue_pos = Some(0);
                    } else {
                        self.queue_pos = Some(0);
                        self.queue.insert(0, t.clone());
                    }
                    self.play_track(t);
                }
            }
            1 => {
                if let Some(t) = self.queue.get(self.qsel).cloned() {
                    self.queue_pos = Some(self.qsel);
                    self.play_track(t);
                }
            }
            5 => {
                // History tab: replay the selected track
                if let Some(t) = self.hist_list.get(self.hist_sel).cloned() {
                    if self.queue.is_empty() {
                        self.queue.push(t.clone());
                        self.queue_pos = Some(0);
                    }
                    self.play_track(t);
                }
            }
            _ => {}
        }
    }

    pub fn seek_rel(&mut self, d: f64) {
        if let Some(eng) = &self.engine {
            let _ = eng.seek_rel(d);
        }
    }

    /// Back-navigation: replay the previously played track (H key).
    /// Walks the in-memory stack (independent of the queue-based prev_track).
    pub fn back_track(&mut self) {
        if let Some(prev) = self.hist_stack.pop() {
            self.hist_suppress = true; // don't re-push the track we just left
            self.play_track(prev);
            self.status = format!("◀ back: {} — {}", self.current.as_ref().map(|t| t.title.clone()).unwrap_or_default(), self.current.as_ref().map(|t| t.artist.clone()).unwrap_or_default());
        } else {
            self.status = "no history".into();
        }
    }

    /// Refresh the History tab from the DB (newest first).
    pub fn load_history(&mut self) {
        self.hist_list = crate::db::recent_history(100).into_iter().map(|(t, _)| t).collect();
    }

    /// Recommendations: background search of the current track's keywords.
    pub fn recs(&mut self) {
        let Some(cur) = self.current.clone() else {
            self.status = "nothing playing — no recs".into();
            return;
        };
        let kw = format!("{} {}", keyword_of(&cur.title), keyword_of(&cur.artist));
        let kw = kw.trim();
        if kw.is_empty() {
            self.status = "no keywords for recs".into();
            return;
        }
        self.query = kw.to_string();
        self.status = format!("🔍 recs for: {}", cur.title);
        self.run_search();
    }

    fn shift_sync(&mut self, d: f64) {
        if self.current.is_none() {
            return;
        }
        let key = self.current.as_ref().unwrap().uri.clone();
        let e = self.sync_offsets.entry(key).or_insert(0.0);
        *e += d;
        self.status = format!("sync offset {}{:.1}s", if *e >= 0.0 { "+" } else { "" }, e);
    }

    pub fn sync_for_current(&self) -> f64 {
        self.current
            .as_ref()
            .and_then(|t| self.sync_offsets.get(&t.uri))
            .copied()
            .unwrap_or(0.0)
    }

    // ---- public helpers for the MPRIS server ----
    pub fn stop_after_public(&mut self) {
        self.stop_after();
    }
    pub fn seek_abs_public(&mut self, pos: f64) {
        if let Some(eng) = &self.engine {
            let _ = eng.seek(pos);
        }
    }
    pub fn set_volume_public(&mut self, v: i64) {
        self.volume = v.clamp(0, 100);
        if let Some(eng) = &self.engine {
            let _ = eng.set_volume(self.volume);
        }
    }
}

/// Pure decision: did the CURRENT track actually end, or is mpv just
/// transitioning between files? Extracted for unit testing (see tests).
/// `switching` = a play was issued <1.5s ago (loadfile blip window).
pub fn should_treat_as_ended(
    was_playing: bool,
    now_playing: bool,
    has_current: bool,
    dur: f64,
    switching: bool,
    paused: bool,
) -> bool {
    if !was_playing || now_playing || !has_current {
        return false;
    }
    if paused {
        return false; // user pressed space — mpv is paused, NOT idle/ended
    }
    if switching {
        return false; // user-initiated switch — don't fire "ended"
    }
    dur > 0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ended_detection() {
        // THE BUG: switching=T was treated as ended → "queue ended" on switch
        assert!(!should_treat_as_ended(true, false, true, 180.0, true, false));
        assert!(should_treat_as_ended(true, false, true, 180.0, false, false));
        assert!(!should_treat_as_ended(false, false, true, 180.0, false, false));
        assert!(!should_treat_as_ended(true, true, true, 180.0, false, false));
        assert!(!should_treat_as_ended(true, false, false, 180.0, false, false));
        assert!(!should_treat_as_ended(true, false, true, 0.0, false, false));
        // THE OTHER BUG: pause (space) must NEVER count as a track end
        assert!(!should_treat_as_ended(true, false, true, 180.0, false, true));
    }

    #[test]
    fn keyword_filter() {
        assert_eq!(keyword_of("Manohari (Official Video)"), "Manohari");
        assert_eq!(keyword_of("A.R. Rahman - Jai Ho"), "Rahman"); // 'A' too short
        assert_eq!(keyword_of("official"), ""); // all noise
        assert_eq!(keyword_of(""), "");
    }
}

/// Extract the first meaningful keyword from a title/artist string
/// (strips noise words like "official", "video", "lyrics", brackets, pipes).
pub(crate) fn keyword_of(s: &str) -> String {
    const NOISE: [&str; 14] = [
        "official", "video", "lyrics", "full", "song", "hd", "4k", "audio",
        "song", "music", "video", "lyric", "remix", "live",
    ];
    let mut best = String::new();
    for tok in s.split(|c: char| !c.is_alphanumeric()) {
        let tok = tok.trim();
        if tok.is_empty() || tok.len() < 2 || NOISE.contains(&tok.to_lowercase().as_str()) {
            continue;
        }
        if best.is_empty() {
            best = tok.to_string();
            break;
        }
    }
    best
}

/// Desktop notification on track change (notify-rust).
fn notify_rich(t: &Track) -> Result<(), ()> {
    use notify_rust::Notification;
    let mut n = Notification::new();
    n.summary(&format!("♪ {}", t.title))
        .body(&format!("{} — {}", t.artist, t.source))
        .timeout(notify_rust::Timeout::Milliseconds(4000));
    match n.show() {
        Ok(_) => Ok(()),
        Err(_) => Err(()),
    }
}
