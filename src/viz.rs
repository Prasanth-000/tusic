//! Real audio spectrum visualizer — captures the PipeWire monitor (what you
//! actually hear) via `parec`, runs a radix-2 FFT over a Hann window, and
//! maps bins into 16 log-spaced bands using dB scaling so transients
//! (phonk 808 kicks, snares) punch through and decay fast — the bars visibly
//! follow the beat. Also exposes a smoothed `loudness` envelope that drives
//! the DNA helix expansion on the right panel.

use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub const BANDS: usize = 16;

/// Shared spectrum state: 16 band levels (0..1) + loudness envelope + liveness.
#[derive(Clone)]
pub struct Spectrum {
    pub bands: Arc<Mutex<[f32; BANDS]>>,
    pub loudness: Arc<Mutex<f32>>,
    pub live: Arc<AtomicBool>,
}

impl Default for Spectrum {
    fn default() -> Self {
        Self::start()
    }
}

impl Spectrum {
    /// Spawn the capture thread. Returns immediately; the thread exits
    /// quietly if parec/monitor is unavailable (live=false, bands idle).
    pub fn start() -> Self {
        let bands = Arc::new(Mutex::new([0.0f32; BANDS]));
        let loudness = Arc::new(Mutex::new(0.0f32));
        let live = Arc::new(AtomicBool::new(false));
        let (b2, l2, lv2) = (bands.clone(), loudness.clone(), live.clone());
        std::thread::spawn(move || capture_loop(b2, l2, lv2));
        Spectrum { bands, loudness, live }
    }

    /// Snapshot of the 16 band levels.
    pub fn snapshot(&self) -> [f32; BANDS] {
        *self.bands.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Current loudness envelope (0..1), smoothed fast — drives visual pumps.
    pub fn loudness(&self) -> f32 {
        *self.loudness.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn is_live(&self) -> bool {
        self.live.load(Ordering::Relaxed)
    }
}

/// Find the default monitor source name via pactl.
fn monitor_source() -> Option<String> {
    let out = Command::new("pactl")
        .args(["list", "short", "sources"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines()
        .map(|l| l.split('\t').nth(1).unwrap_or("").to_string())
        .find(|name| name.contains(".monitor"))
}

// ---------------- radix-2 FFT (in-place, 1024 points) ----------------

fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2usize;
    while len <= n {
        let ang = -2.0 * std::f32::consts::PI / len as f32;
        let (wr0, wi0) = (ang.cos(), ang.sin());
        let mut i = 0usize;
        while i < n {
            let mut wr = 1.0f32;
            let mut wi = 0.0f32;
            for k in 0..len / 2 {
                let (u_r, u_i) = (re[i + k], im[i + k]);
                let (v_r, v_i) = (
                    re[i + k + len / 2] * wr - im[i + k + len / 2] * wi,
                    re[i + k + len / 2] * wi + im[i + k + len / 2] * wr,
                );
                re[i + k] = u_r + v_r;
                im[i + k] = u_i + v_i;
                re[i + k + len / 2] = u_r - v_r;
                im[i + k + len / 2] = u_i - v_i;
                let nwr = wr * wr0 - wi * wi0;
                wi = wr * wi0 + wi * wr0;
                wr = nwr;
            }
            i += len;
        }
        len <<= 1;
    }
}

const N: usize = 1024; // FFT size; 1024 @ 22050 Hz → 21.5 Hz/bin, 46 ms window
const RATE: f32 = 22050.0;

fn capture_loop(bands: Arc<Mutex<[f32; BANDS]>>, loudness: Arc<Mutex<f32>>, live: Arc<AtomicBool>) {
    let Some(src) = monitor_source() else { return };
    let child = match Command::new("parec")
        .args([
            "--device",
            &src,
            "--format=s16le",
            "--rate=22050",
            "--channels=1",
            "--raw",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return,
    };
    let mut stdout: Child = child;
    let Some(mut pipe) = stdout.stdout.take() else { return };
    live.store(true, Ordering::Relaxed);

    // log-spaced band edges: 30 Hz .. 12 kHz (16 bands) — captures 808 sub
    let mut edges = [0usize; BANDS + 1];
    for (i, e) in edges.iter_mut().enumerate() {
        let f = 30.0f32 * (12_000.0f32 / 30.0f32).powf(i as f32 / BANDS as f32);
        *e = ((f / RATE) * N as f32).floor() as usize;
    }

    // Hann window
    let mut win = vec![0.0f32; N];
    for (i, w) in win.iter_mut().enumerate() {
        *w = 0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / (N as f32 - 1.0)).cos());
    }

    let mut buf = vec![0u8; N * 2];
    let mut re = vec![0.0f32; N];
    let mut im = vec![0.0f32; N];
    let mut cur = [0.0f32; BANDS];
    let mut lvl = 0.0f32;
    // Hann coherent gain ≈ 0.5 → normalize by N*0.5
    let norm = 1.0 / (N as f32 * 0.5);
    // dB floor: everything below -60 dBFS → 0
    let floor_db = 60.0f32;

    loop {
        let mut filled = 0usize;
        while filled < buf.len() {
            match pipe.read(&mut buf[filled..]) {
                Ok(0) => {
                    live.store(false, Ordering::Relaxed);
                    return;
                }
                Ok(n) => filled += n,
                Err(_) => {
                    live.store(false, Ordering::Relaxed);
                    return;
                }
            }
        }
        // s16le mono → f32, apply Hann
        for (i, chunk) in buf.chunks_exact(2).enumerate() {
            let raw = i16::from_le_bytes([chunk[0], chunk[1]]);
            re[i] = raw as f32 / 32768.0 * win[i];
            im[i] = 0.0;
        }
        fft(&mut re, &mut im);

        // per-band dB magnitude: 20*log10(|X|*norm), mapped -60..0 dB → 0..1
        let mut mags = [0.0f32; BANDS];
        let mut total = 0.0f32;
        for b in 0..BANDS {
            let lo = edges[b];
            let hi = edges[b + 1].min(N / 2);
            let mut peak = 0.0f32;
            for k in lo..hi {
                let m = (re[k] * re[k] + im[k] * im[k]).sqrt() * norm;
                peak = peak.max(m);
            }
            let db = 20.0 * (peak.max(1e-6)).log10(); // dBFS, negative
            let v = ((db + floor_db) / floor_db).clamp(0.0, 1.0);
            mags[b] = v;
            total += v;
        }
        // loudness envelope: mean band level, fast attack, quick decay
        let mean = total / BANDS as f32;
        if mean > lvl {
            lvl += (mean - lvl) * 0.65;
        } else {
            lvl += (mean - lvl) * 0.25;
        }

        // smoothing per band: fast attack, FAST decay so beats punch & fall
        for b in 0..BANDS {
            if mags[b] > cur[b] {
                cur[b] += (mags[b] - cur[b]) * 0.70;
            } else {
                cur[b] += (mags[b] - cur[b]) * 0.35;
            }
        }
        {
            let mut g = bands.lock().unwrap_or_else(|p| p.into_inner());
            *g = cur;
        }
        {
            let mut g = loudness.lock().unwrap_or_else(|p| p.into_inner());
            *g = lvl;
        }
    }
}
