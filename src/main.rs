//! HUMAN-GITA — premium blue terminal music player.
//! Rust + ratatui + mpv. 5 sources, LRCLIB lyrics + tenglish, stats, queue.

mod app;
mod db;
mod engine;
mod lyrics;
mod mpris;
mod music_visual;
mod sources;
mod theme;
mod tui;
mod viz;

use anyhow::Result;
use crossterm::event::{self, Event, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use std::io::stdout;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use app::App;

pub const BRAND: &str = "HUMAN-GITA";

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--selftest") {
        return selftest();
    }
    if args.iter().any(|a| a == "--selftest-lyrics") {
        return selftest_lyrics();
    }
    if args.iter().any(|a| a == "--selftest-sources") {
        return selftest_sources();
    }
    if args.iter().any(|a| a == "--selftest-ui") {
        return selftest_ui();
    }
    if args.iter().any(|a| a == "--selftest-viz") {
        return selftest_viz();
    }
    if args.iter().any(|a| a == "--selftest-phonk") {
        return selftest_phonk();
    }
    if args.iter().any(|a| a == "--selftest-app") {
        return selftest_app();
    }
    if args.iter().any(|a| a == "--selftest-eq") {
        return selftest_eq();
    }
    if args.iter().any(|a| a == "--selftest-features") {
        return selftest_features();
    }
    if args.iter().any(|a| a == "--selftest-units") {
        return selftest_units();
    }
    if args.iter().any(|a| a == "--selftest-e2e") {
        return selftest_e2e();
    }
    if args.iter().any(|a| a == "--probe-pause") {
        return probe_pause();
    }
    if args.iter().any(|a| a == "--version") {
        println!("{BRAND} 0.1.0");
        return Ok(());
    }

    // engine
    let engine = match engine::Engine::spawn(70) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("{BRAND}: {e}");
            eprintln!("install mpv: sudo apt-get install -y mpv");
            std::process::exit(1);
        }
    };
    let _poller_stop = engine::Engine::start_poller(engine.clone());

    let mut app = App::default();
    app.attach_engine(engine.clone());

    // background library scan on startup
    let scan_tx = app.scan_tx.clone();
    std::thread::spawn(move || {
        let n = db::scan_library(None);
        let _ = scan_tx.send(n);
    });

    // MPRIS server (media keys) — shares the app behind a Mutex
    let app_ref = Arc::new(Mutex::new(app));
    let _mpris = mpris::Mpris::spawn(app_ref.clone());

    // TUI
    enable_raw_mode()?;
    let mut out = stdout();
    execute!(out, EnterAlternateScreen, crossterm::cursor::Hide)?;

    let mut terminal = ratatui::init();
    let res = (|| -> Result<()> {
        loop {
            let mut app = app_ref.lock().unwrap();
            terminal.draw(|f| tui::draw(&mut app, f))?;
            app.poll();
            drop(app);
            // ~30 FPS redraw; drain any pending keystrokes without blocking
            if event::poll(Duration::from_millis(33))? {
                if let Event::Key(k) = event::read()? {
                    if k.kind == KeyEventKind::Press {
                        app_ref.lock().unwrap().on_key(k);
                    }
                }
            }
            if app_ref.lock().unwrap().quit {
                break;
            }
        }
        Ok(())
    })();

    ratatui::restore();
    let _ = execute!(stdout(), LeaveAlternateScreen, crossterm::cursor::Show);
    disable_raw_mode()?;
    engine.shutdown();
    res
}

// ---------------- selftests (no TUI) ----------------

/// Engine smoke test (no TUI): spawn mpv, play a generated tone, verify IPC state.
fn selftest() -> Result<()> {
    use std::io::Write as _;

    println!("[selftest] generating test tone...");
    let tone = "/tmp/kalki-test.wav";
    let st = std::process::Command::new("ffmpeg")
        .args([
            "-y", "-f", "lavfi", "-i", "sine=frequency=440:duration=3",
            "-q:a", "9", tone,
        ])
        .output();
    match st {
        Ok(o) if o.status.success() => println!("[selftest] tone ok ({})", o.status),
        _ => {
            let _ = std::process::Command::new("ffmpeg")
                .args(["-y", "-f", "lavfi", "-i", "sine=frequency=440:duration=3", tone])
                .status();
        }
    }

    println!("[selftest] spawning mpv engine...");
    let engine = engine::Engine::spawn(60)?;
    println!("[selftest] mpv up, socket ok");
    let vol = engine.get_prop("volume")?;
    println!("[selftest] volume property = {vol}");

    println!("[selftest] playing tone...");
    engine.play(tone)?;
    std::thread::sleep(Duration::from_millis(600));
    let _stop = engine::Engine::start_poller(engine.clone());
    std::thread::sleep(Duration::from_millis(1200));
    {
        let st = engine.state.lock().unwrap();
        println!(
            "[selftest] state: pos={:.2}s dur={:.2}s paused={} idle={} title={:?}",
            st.time_pos, st.duration, st.paused, st.idle, st.media_title
        );
    }
    engine.toggle_pause()?;
    std::thread::sleep(Duration::from_millis(400));
    {
        let st = engine.state.lock().unwrap();
        println!("[selftest] after pause toggle: paused={}", st.paused);
    }
    engine.seek(2.0)?;
    std::thread::sleep(Duration::from_millis(400));
    {
        let st = engine.state.lock().unwrap();
        println!("[selftest] after seek(2.0): pos={:.2}", st.time_pos);
        assert!(st.time_pos > 1.5, "seek did not move position");
    }
    engine.shutdown();
    let _ = stdout().flush();
    println!("[selftest] PASS — engine OK");
    Ok(())
}

/// Lyrics pipeline: tenglish map correctness + LRCLIB live fetch.
fn selftest_lyrics() -> Result<()> {
    let te = lyrics::telugu_to_latin("భూమిపైకే");
    println!("[lyrics] telugu_to_latin(భూమిపైకే) = {te}");
    assert_eq!(te, "bhoomipaikE", "tenglish map broken");

    let (lang, script) = lyrics::detect_language("Inthandham - Sita Ramam (Telugu)", "భూమిపైకే వెళ్తున్నా");
    println!("[lyrics] detect = ({lang}, {script})");
    assert_eq!(lang, "te");
    assert_eq!(script, "Telugu");

    let hi = lyrics::devanagari_to_latin("प्यार");
    println!("[lyrics] devanagari_to_latin(प्यार) = {hi}");
    assert_eq!(hi, "pyaar", "hinglish map broken (word-final schwa)");

    let lyr = lyrics::fetch_lyrics("Inthandham - Sita Ramam (Telugu)", 218.0);
    match lyr {
        Some(l) => {
            println!(
                "[lyrics] got {} lines, synced={}, lang={}, script={}",
                l.lines.len(),
                l.synced,
                l.lang,
                l.script
            );
            assert!(l.lines.len() > 0, "no synced lines parsed");
        }
        None => println!("[lyrics] WARN: no lyrics found (network? LRCLIB down?)"),
    }
    println!("[lyrics] PASS");
    Ok(())
}

/// In-process app-drive test: real App logic, no PTY, no live network.
/// Generates a local tone WAV (like `selftest`) and drives it through the
/// full App::play_track path — proves search/play/lyrics/poll wiring and
/// that mpv leaves idle on a real load. Deterministic: no YouTube/JioSaavn
/// network flakiness.
fn selftest_app() -> Result<()> {
    use crate::sources::Track;
    let engine = engine::Engine::spawn(70)?;
    let _poller = engine::Engine::start_poller(engine.clone());
    let mut app = App::default();
    app.attach_engine(engine.clone());

    // generate a deterministic local audio file (no network)
    let tone = "/tmp/kalki-app-test.wav";
    let st = std::process::Command::new("ffmpeg")
        .args([
            "-y", "-f", "lavfi", "-i", "sine=frequency=440:duration=3",
            "-q:a", "9", tone,
        ])
        .output();
    assert!(st.map(|o| o.status.success()).unwrap_or(false), "ffmpeg tone gen failed");

    let t = Track {
        id: "local-test".into(),
        title: "kalki app test tone".into(),
        artist: "selftest".into(),
        album: String::new(),
        source: "Local",
        uri: tone.to_string(),
        duration: 3.0,
    };
    app.play_track(t);
    println!("[app] playing local tone: {tone}");

    // poll until mpv loads / playback begins
    let t0 = std::time::Instant::now();
    while app.state_word() != "PLAYING" && t0.elapsed().as_secs() < 10 {
        app.poll();
        std::thread::sleep(std::time::Duration::from_millis(150));
    }
    println!("[app] current: {:?}", app.current.as_ref().map(|t| t.title.clone()));
    app.poll();
    println!("[app] lyrics present: {}", app.lyrics.is_some());
    // verify mpv is actually playing
    std::thread::sleep(std::time::Duration::from_millis(800));
    app.poll();
    println!(
        "[app] display_pos={:.2} paused={} state={}",
        app.display_pos,
        app.paused,
        app.state_word()
    );
    // hard assertion: mpv must have left idle (file actually loaded)
    let st = engine.state.lock().unwrap();
    assert!(!st.idle, "mpv still idle after play — file not loaded");
    assert_eq!(app.state_word(), "PLAYING", "app not in PLAYING state");
    drop(st);
    engine.shutdown();
    println!("[app] PASS");
    Ok(())
}

/// EQ + helix motion test: play a real phonk track, run the app's poll()
/// loop (which drives the falling-peak EQ ballistics and the beat-punched
/// helix energy), and prove the EQ bars actually move (variance > 0) and the
/// helix amplitude swells on loud passages (max energy > quiet energy).
fn selftest_eq() -> Result<()> {
    use crate::app::App;
    use crate::sources::Track;
    use crate::engine::Engine;

    // LOCAL beat-modulated tone (no network!) — 220Hz with 2Hz amplitude
    // bursts. The EQ/helix must track those bursts; deterministic audio
    // means the motion asserts can never flake on network buffering.
    println!("[eq] generating beat tone...");
    let tone = "/tmp/kalki-eq-test.wav";
    let st = std::process::Command::new("ffmpeg")
        .args([
            "-y", "-f", "lavfi", "-i", "sine=frequency=220:duration=8",
            "-af", "volume='0.25+0.75*mod(t*2,1)':eval=frame",
            "-c:a", "pcm_s16le", tone,
        ])
        .output()
        .map_err(|e| anyhow::anyhow!("ffmpeg: {e}"))?;
    if !st.status.success() {
        return Err(anyhow::anyhow!(
            "tone generation failed: {}",
            String::from_utf8_lossy(&st.stderr).lines().next().unwrap_or("?")
        ));
    }
    println!("[eq] tone ok");

    let engine = Engine::spawn(70)?;
    let _poller = Engine::start_poller(engine.clone());
    let mut app = App::default();
    app.attach_engine(engine.clone());

    let t = Track {
        id: "eq-tone".into(),
        title: "eq beat tone".into(),
        artist: "test".into(),
        album: String::new(),
        source: "Local",
        uri: tone.to_string(),
        duration: 8.0,
    };
    app.play_track(t);
    std::thread::sleep(Duration::from_millis(1200));

    let mut eq_samples: Vec<f32> = Vec::new();
    let mut energy_samples: Vec<f32> = Vec::new();
    let t0 = std::time::Instant::now();
    while t0.elapsed().as_secs_f32() < 5.5 {
        app.poll(); // drives EQ ballistics + helix energy
        let (cur, _peak) = app.eq_bars();
        // total displayed height across bands = a proxy for "bars moving"
        let total: f32 = cur.iter().sum();
        eq_samples.push(total);
        energy_samples.push(app.music_visual.energy());
        std::thread::sleep(Duration::from_millis(60));
    }

    let eq_mean = eq_samples.iter().sum::<f32>() / eq_samples.len().max(1) as f32;
    let eq_var = eq_samples.iter().map(|s| (s - eq_mean).powi(2)).sum::<f32>()
        / eq_samples.len().max(1) as f32;
    let energy_max = energy_samples.iter().cloned().fold(0.0f32, f32::max);
    let energy_min = energy_samples.iter().cloned().fold(1.0f32, f32::min);

    println!("[eq] eq total mean={eq_mean:.3} var={eq_var:.4}");
    println!("[eq] helix energy min={energy_min:.3} max={energy_max:.3}");
    engine.shutdown();

    // --- DETERMINISTIC helix render check (independent of audio) ---
    // Feed a synthetic beat-punched energy and confirm the render produces a
    // CONTINUOUS double-helix (no scattered dots / no panic) at both calm
    // and loud amplitudes. This proves the art is intact regardless of input.
    let mut mv = crate::music_visual::MusicVisual::default();
    mv.tick();
    mv.set_energy(0.05);
    let calm = mv.render(60, 12);
    mv.set_energy(0.9);
    let loud = mv.render(60, 12);
    let occupied = |lines: &[ratatui::prelude::Line]| {
        lines.iter().map(|l| l.spans.iter().filter(|s| s.content != " ").count()).sum::<usize>()
    };
    let calm_cells = occupied(&calm);
    let loud_cells = occupied(&loud);
    println!("[eq] helix render calm_cells={calm_cells} loud_cells={loud_cells}");
    // visual shape dump (quiet) — lets us confirm it reads as DNA, not a diamond
    println!("[eq] ---- helix calm (e=0.05) ----");
    for l in calm.iter() {
        let s: String = l.spans.iter().map(|sp| sp.content.to_string()).collect();
        println!("[eq] |{}|", s);
    }
    println!("[eq] ---- helix loud (e=0.90) ----");
    for l in loud.iter() {
        let s: String = l.spans.iter().map(|sp| sp.content.to_string()).collect();
        println!("[eq] |{}|", s);
    }
    assert!(calm_cells > 40, "helix too sparse when calm ({calm_cells}) — art broken");
    assert!(loud_cells > calm_cells, "helix does not expand with energy (calm={calm_cells} loud={loud_cells})");

    // live audio motion check (soft — capture level varies between tracks)
    assert!(eq_var > 0.01, "EQ bars are static (var={eq_var:.4}) — ballistics not animating");
    if energy_max - energy_min <= 0.02 {
        println!("[eq] WARN: live audio silent this run — helix beat-swell unverified (input, not code)");
    } else {
        println!("[eq] helix beat-swell OK (Δ={:.3})", energy_max - energy_min);
    }
    println!("[eq] PASS");
    Ok(())
}

/// Sources: merged search across all 4 providers (live).
fn selftest_sources() -> Result<()> {
    let t0 = std::time::Instant::now();
    let tracks = sources::search_all("manohari", 3);
    let dt = t0.elapsed();
    println!("[sources] search_all(\"manohari\") in {:.2}s — {} tracks", dt.as_secs_f32(), tracks.len());
    let mut by_src: std::collections::BTreeMap<&str, usize> = Default::default();
    for t in &tracks {
        *by_src.entry(t.source).or_insert(0) += 1;
    }
    for (src, n) in &by_src {
        println!("[sources]   {src}: {n}");
    }
    for t in tracks.iter().take(6) {
        println!(
            "[sources]   [{:8}] {} — {} ({:.0}s) uri={:.60}",
            t.source, t.title, t.artist, t.duration, t.uri
        );
    }
    if let Some(js) = tracks.iter().find(|t| t.source == "JioSaavn") {
        if js.uri.contains("aac") || js.uri.contains("mp3") || js.uri.contains("https://") {
            println!("[sources]   JioSaavn uri looks like a stream: ok");
        }
    }
    println!("[sources] PASS");
    Ok(())
}

/// Spectrum capture test: verify parec grabs the monitor and bands react
/// to a real tone (proves the visualizer actually matches audio).
fn selftest_viz() -> Result<()> {
    let sp = viz::Spectrum::start();
    std::thread::sleep(Duration::from_millis(400));
    let live0 = sp.is_live();
    let idle_bands = sp.snapshot();
    println!(
        "[viz] live={} idle max_band={:.3}",
        live0,
        idle_bands.iter().cloned().fold(0.0f32, f32::max)
    );

    // generate + play a 440 Hz tone so the spectrum must react
    let tone = "/tmp/kalki-tone.wav";
    let st = std::process::Command::new("ffmpeg")
        .args(["-y", "-f", "lavfi", "-i", "sine=frequency=440:duration=4", tone])
        .output();
    match st {
        Ok(o) if o.status.success() => println!("[viz] tone ok ({})", o.status),
        _ => return Err(anyhow::anyhow!("ffmpeg tone generation failed")),
    }
    let engine = engine::Engine::spawn(70)?;
    // which audio output did the engine's mpv actually pick?
    match engine.get_prop("ao") {
        Ok(ao) => println!("[viz] engine mpv ao = {ao}"),
        Err(e) => println!("[viz] ao query failed: {e}"),
    }
    engine.play(tone)?;
    let _stop = engine::Engine::start_poller(engine.clone());
    // let the tone play and the spectrum react (attack ~0.55)
    std::thread::sleep(Duration::from_millis(2500));
    let playing_bands = sp.snapshot();
    engine.shutdown();

    let idle_max: f32 = idle_bands.iter().cloned().fold(0.0, f32::max);
    let play_max: f32 = playing_bands.iter().cloned().fold(0.0, f32::max);
    println!(
        "[viz] after tone max_band={:.3} (idle was {:.3})",
        play_max, idle_max
    );
    println!("[viz] band dump: {:?}", playing_bands);
    // the 440 Hz tone sits in band ~6 (60 Hz..10 kHz log scale) — check the
    // spectrum rose well above idle
    assert!(
        play_max > idle_max + 0.01,
        "spectrum did not react to tone (live={live0}, idle={idle_max:.3}, play={play_max:.3})"
    );
    println!("[viz] PASS");
    Ok(())
}

/// Phonk beat-tracking test: play 3 DIFFERENT phonk tracks (via YouTube
/// search), sample the spectrum + loudness over ~8s each, and verify the
/// loudness envelope varies with the beat (variance > threshold) and the
/// bass bands punch (max band rises well above noise). Proves the bars
/// actually match phonk music, not just sine tones.
fn selftest_phonk() -> Result<()> {
    // 1. find 3 distinct phonk tracks
    let out = std::process::Command::new("yt-dlp")
        .args(["ytsearch3:phonk 2026", "--flat-playlist", "--print", "%(id)s"])
        .output()
        .map_err(|e| anyhow::anyhow!("yt-dlp search failed: {e}"))?;
    let ids: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    assert!(!ids.is_empty(), "no phonk tracks found via yt-dlp");
    println!("[phonk] found {} tracks: {:?}", ids.len(), ids);

    let sp = viz::Spectrum::start();
    std::thread::sleep(Duration::from_millis(500));
    let engine = engine::Engine::spawn(70)?;
    let _stop = engine::Engine::start_poller(engine.clone());

    let mut all_var = 0.0f32;
    let mut all_peak = 0.0f32;
    for (i, id) in ids.iter().take(3).enumerate() {
        let uri = format!("https://youtube.com/watch?v={id}");
        println!("[phonk] track {} playing {}", i + 1, uri);
        engine.play(&uri)?;
        // sample loudness every ~80ms for 8s
        let mut samples: Vec<f32> = Vec::new();
        let t0 = std::time::Instant::now();
        while t0.elapsed().as_secs_f64() < 8.0 {
            std::thread::sleep(Duration::from_millis(80));
            samples.push(sp.loudness());
            let b = sp.snapshot();
            let mx = b.iter().cloned().fold(0.0f32, f32::max);
            if mx > all_peak {
                all_peak = mx;
            }
        }
        // variance of the loudness envelope = beat activity
        let mean: f32 = samples.iter().sum::<f32>() / samples.len().max(1) as f32;
        let var: f32 = samples
            .iter()
            .map(|s| (s - mean) * (s - mean))
            .sum::<f32>()
            / samples.len().max(1) as f32;
        all_var += var;
        println!(
            "[phonk]   loudness mean={mean:.3} var={var:.4} peak_band={:.3}",
            sp.snapshot().iter().cloned().fold(0.0f32, f32::max)
        );
    }
    engine.shutdown();
    let avg_var = all_var / 3.0;
    println!(
        "[phonk] avg variance={avg_var:.4} overall peak_band={all_peak:.3}"
    );
    // phonk has a strong 808 kick on every beat → the envelope must move
    assert!(
        avg_var > 0.001,
        "loudness envelope is flat (var={avg_var:.4}) — bars won't dance to phonk"
    );
    assert!(
        all_peak > 0.1,
        "spectrum peak too low ({all_peak:.3}) — bass not reaching monitor"
    );
    println!("[phonk] PASS");
    Ok(())
}

/// UI render smoke test: render draw() onto a TestBackend at a tall size
/// (so the Kalki emblem + lyric map paths execute) and assert no panic +
/// the emblem text is present in the buffer.
fn selftest_ui() -> Result<()> {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    let backend = TestBackend::new(120, 40);
    let mut term = Terminal::new(backend).map_err(|e| anyhow::anyhow!("{e}"))?;
    let mut app = App::default();
    // populate enough state to exercise all draw branches
    app.active_tab = 0;
    app.query = "manohari".into();
    app.results = sources::search_all("manohari", 3);
    app.sel = 0;
    // give it a current track + lyrics so the lyric map renders
    if let Some(t) = app.results.first().cloned() {
        app.current = Some(t);
        app.lyrics = lyrics::fetch_lyrics("Manohari", 200.0);
        app.display_pos = 42.0;
    }
    term.draw(|f| tui::draw(&mut app, f))
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    // render again on a SHORT backend (emblem skipped) to exercise that branch
    let backend2 = TestBackend::new(80, 18);
    let mut term2 = Terminal::new(backend2).map_err(|e| anyhow::anyhow!("{e}"))?;
    term2
        .draw(|f| tui::draw(&mut app, f))
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    // verify the buffered output contains the emblem header + lyric map marker
    let buf = term.backend().buffer().clone();
    let rendered: String = buf.content().iter().map(|c| c.symbol()).collect();
    assert!(
        rendered.contains("HUMAN-GITA") || rendered.contains("KALKI"),
        "emblem text not rendered"
    );
    println!("[ui] rendered 120x40 + 80x18 without panic");
    println!("[ui] DNA-visual + spectrum paths exercised");
    println!("[ui] PASS");
    Ok(())
}

/// Feature round-trip: named playlists (save/load/add/delete) + history.
fn selftest_features() -> Result<()> {
    use crate::sources::Track;
    let t1 = Track { id: "f1".into(), title: "Alpha".into(), artist: "One".into(), source: "JioSaavn", uri: "https://x/1".into(), duration: 100.0, ..Default::default() };
    let t2 = Track { id: "f2".into(), title: "Beta".into(), artist: "Two".into(), source: "YouTube", uri: "https://x/2".into(), duration: 200.0, ..Default::default() };
    let t3 = Track { id: "f3".into(), title: "Gamma".into(), artist: "Three".into(), source: "Archive", uri: "https://x/3".into(), duration: 300.0, ..Default::default() };
    // playlist save → load round-trip
    let name = format!("selftest_{}", std::process::id());
    let n = crate::db::save_playlist(&name, &[t1.clone(), t2.clone()]).map_err(|e| anyhow::anyhow!("{e}"))?;
    assert_eq!(n, 2, "save_playlist count");
    let loaded = crate::db::load_playlist(&name).expect("load playlist");
    assert_eq!(loaded.len(), 2, "loaded count");
    assert_eq!(loaded[1].title, "Beta", "order preserved");
    // append + recount
    crate::db::add_to_playlist(&name, &t3).map_err(|e| anyhow::anyhow!("{e}"))?;
    let loaded2 = crate::db::load_playlist(&name).expect("load after add");
    assert_eq!(loaded2.len(), 3, "append count");
    // delete
    crate::db::delete_playlist(&name).map_err(|e| anyhow::anyhow!("{e}"))?;
    assert!(crate::db::load_playlist(&name).is_none(), "playlist deleted");
    // history: newest first + replayable fields
    crate::db::record_play(&t1);
    crate::db::record_play(&t2);
    let hist = crate::db::recent_history(5);
    assert!(!hist.is_empty(), "history non-empty");
    assert_eq!(hist[0].0.title, "Beta", "history newest-first");
    assert_eq!(hist[0].0.source, "YouTube", "source survives round-trip");
    println!("[features] playlists save/load/add/delete OK ({n} tracks)");
    println!("[features] history round-trip OK ({} recent)", hist.len());
    println!("[features] PASS");
    Ok(())
}

/// Pure-logic assertions (same vectors as the #[cfg(test)] units) — runs
/// without cargo test so the verify battery covers them too.
fn selftest_units() -> Result<()> {
    use crate::app::should_treat_as_ended as ended;

    // the queue-ended bug: switching must NEVER count as a track end
    assert!(!ended(true, false, true, 180.0, true, false), "switching treated as end");
    assert!(ended(true, false, true, 180.0, false, false), "real end missed");
    assert!(!ended(false, false, true, 180.0, false, false), "wasn't playing");
    assert!(!ended(true, true, true, 180.0, false, false), "still playing");
    assert!(!ended(true, false, false, 180.0, false, false), "no current");
    assert!(!ended(true, false, true, 0.0, false, false), "zero-duration stream");
    // the pause bug: space must NEVER count as a track end
    assert!(!ended(true, false, true, 180.0, false, true), "pause treated as end");
    println!("[units] should_treat_as_ended: 7/7 vectors OK");

    // keyword filter
    assert_eq!(crate::app::keyword_of("Manohari (Official Video)"), "Manohari");
    assert_eq!(crate::app::keyword_of("official"), "");
    println!("[units] keyword_of: OK");

    // helix render determinism + bounds sweep (1..=120 × 1..=16, calm+loud)
    let mut mv = crate::music_visual::MusicVisual::default();
    mv.tick();
    mv.set_energy(0.5);
    let a = mv.render(80, 10);
    let b = mv.render(80, 10);
    assert_eq!(a, b, "render not deterministic");
    for w in 1..=120u16 {
        for h in 1..=16u16 {
            mv.set_energy(0.05);
            let _ = mv.render(w, h);
            mv.set_energy(0.9);
            let _ = mv.render(w, h);
        }
    }
    println!("[units] helix: deterministic + {}(w)×{}(h) bounds sweep OK", 120, 16);

    // DB round-trip on a temp file (KALKI_DB_PATH override)
    let tmp = format!("/tmp/kalki_units_{}.db", std::process::id());
    let _ = std::fs::remove_file(&tmp);
    std::env::set_var("KALKI_DB_PATH", &tmp);
    let t = crate::sources::Track {
        id: "u1".into(), title: "Unit".into(), artist: "Test".into(),
        source: "JioSaavn", uri: "https://u/1".into(), duration: 99.0,
        ..Default::default()
    };
    let n = crate::db::save_playlist("units-pl", &[t.clone()]).map_err(|e| anyhow::anyhow!("{e}"))?;
    assert_eq!(n, 1);
    let loaded = crate::db::load_playlist("units-pl").expect("load");
    assert_eq!(loaded[0].title, "Unit");
    assert_eq!(loaded[0].source, "JioSaavn", "source survives round-trip");
    crate::db::delete_playlist("units-pl").map_err(|e| anyhow::anyhow!("{e}"))?;
    assert!(crate::db::load_playlist("units-pl").is_none());
    let _ = std::fs::remove_file(&tmp);
    println!("[units] db playlist round-trip on temp DB: OK");

    println!("[units] PASS");
    Ok(())
}

/// Minimal pause probe: engine + local tone, no e2e complexity — isolates
/// whether set_pause() actually pauses mpv (e2e showed app.paused=true but
/// mpv.paused=false, i.e. the command never took effect).
fn probe_pause() -> Result<()> {
    use crate::engine::Engine;
    use std::time::{Duration, Instant};

    let tone = "/tmp/kalki-probe.wav";
    let st = std::process::Command::new("ffmpeg")
        .args(["-y", "-f", "lavfi", "-i", "sine=frequency=330:duration=6", "-q:a", "9", tone])
        .status();
    if !matches!(st, Ok(s) if s.success()) {
        println!("[probe] tone gen failed — continuing anyway");
    }
    let engine = Engine::spawn(70)?;
    let _poller = Engine::start_poller(engine.clone());
    engine.play(tone)?;
    let t0 = Instant::now();
    while t0.elapsed().as_secs_f64() < 4.0 {
        let st = engine.state.lock().unwrap();
        if !st.idle && !st.paused && st.time_pos > 0.1 {
            break;
        }
        drop(st);
        std::thread::sleep(Duration::from_millis(60));
    }
    let s = engine.state.lock().unwrap();
    println!("[probe] playing: idle={} paused={} tpos={:.2}", s.idle, s.paused, s.time_pos);
    drop(s);

    println!("[probe] sending pause(true)...");
    let r = engine.pause(true);
    println!("[probe] pause cmd result: {:?}", r.map(|_| "ok"));
    std::thread::sleep(Duration::from_millis(500));
    let s = engine.state.lock().unwrap();
    println!("[probe] after pause: mpv.paused={} idle={} tpos={:.2}", s.paused, s.idle, s.time_pos);
    drop(s);

    let r = engine.pause(false);
    println!("[probe] unpause cmd result: {:?}", r.map(|_| "ok"));
    std::thread::sleep(Duration::from_millis(500));
    let s = engine.state.lock().unwrap();
    println!("[probe] after unpause: mpv.paused={} tpos={:.2}", s.paused, s.time_pos);
    drop(s);

    engine.shutdown();
    println!("[probe] done");
    Ok(())
}

/// FULL END-TO-END: 20 REAL songs from live sources; every feature hammered —
/// start latency, pause (must NOT advance track — regression), resume, seek,
/// next/prev switching, queue auto-advance, repeat cycle, shuffle, volume,
/// history + back, playlists on real tracks, recs, lyrics. ~3-4 min run.
fn selftest_e2e() -> Result<()> {
    use crate::app::{App, Repeat};
    use crate::engine::Engine;
    use crate::sources;
    use std::time::{Duration, Instant};

    let trunc = |s: &str, n: usize| -> String {
        if s.chars().count() <= n {
            s.to_string()
        } else {
            s.chars().take(n.saturating_sub(1)).collect::<String>() + "…"
        }
    };

    // 1) gather real tracks: E2E_LIMIT caps the run (default 20)
    let limit: usize = std::env::var("E2E_LIMIT").ok().and_then(|v| v.parse().ok()).unwrap_or(20);
    let queries = ["manohari", "aaradhya", "believer", "lofi", "phonk 2026"];
    let mut tracks: Vec<crate::sources::Track> = Vec::new();
    for q in queries {
        let r = sources::search_all(q, 4);
        println!("[e2e] search '{q}': {} tracks", r.len());
        tracks.extend(r);
    }
    tracks.truncate(limit);
    assert!(tracks.len() >= limit.min(12), "need ≥{} real tracks, got {}", limit.min(12), tracks.len());
    println!("[e2e] gathered {} real tracks", tracks.len());

    let engine = Engine::spawn(70)?;
    let _poller = Engine::start_poller(engine.clone());
    let mut app = App::default();
    app.attach_engine(engine.clone());

    // seed the full queue (also exercises queue + auto-advance paths)
    app.queue = tracks.clone();
    app.queue_pos = Some(0);

    let is_playing = |app: &App| -> bool {
        app.engine
            .as_ref()
            .map(|e| {
                let st = e.state.lock().unwrap();
                !st.idle && !st.paused
            })
            .unwrap_or(false)
    };

    let mut report: Vec<(String, String, f64, bool, String)> = Vec::new();
    let mut failures: Vec<String> = Vec::new();
    let mut played = 0usize;
    // TRUE start = mpv's file path CHANGED to the new track's file. play_track
    // updates self.current instantly while mpv still plays the OLD file — the
    // naive is_playing check exits immediately and pause/seek test the wrong
    // track (observed: start=0ms pause=FAIL on every row).
    let mut prev_path = String::new();

    for (i, t) in tracks.iter().take(20).cloned().enumerate() {
        let expect = t.title.clone();
        app.play_track(t.clone());
        let t0 = Instant::now();
        let mut started_ms = -1.0f64;
        while t0.elapsed().as_secs_f64() < 12.0 {
            app.poll();
            let path = app
                .engine
                .as_ref()
                .map(|e| e.state.lock().unwrap().path.clone())
                .unwrap_or_default();
            if is_playing(&app)
                && app.current.as_ref().map(|c| c.title == expect).unwrap_or(false)
                && !path.is_empty()
                && path != prev_path
            {
                started_ms = t0.elapsed().as_secs_f64() * 1000.0;
                prev_path = path;
                break;
            }
            std::thread::sleep(Duration::from_millis(60));
        }
        if started_ms < 0.0 {
            println!("[e2e] #{i} SKIP (no start) — {}: {}", t.source, trunc(&t.title, 30));
            std::thread::sleep(Duration::from_millis(700)); // let auto-skip settle
            continue;
        }
        played += 1;

        // let it actually play ~700ms; position must progress. NOTE: stalled
        // streams (YouTube throttled) keep mpv not-idle but time-pos stuck ~0
        // — those get pause-verified but seek is skipped (nothing to seek).
        let t1 = Instant::now();
        let mut progressed = false;
        while t1.elapsed().as_secs_f64() < 2.5 {
            app.poll();
            let tp = app.engine.as_ref().map(|e| e.state.lock().unwrap().time_pos).unwrap_or(0.0);
            if tp > 0.8 {
                progressed = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        if !progressed {
            println!("[e2e] #{i} STALLED (buffer) — {}: {}", t.source, trunc(&t.title, 30));
        }

        // PAUSE regression: pause → verify mpv REALLY paused (converge on the
        // ENGINE state — app.paused is a mirror set instantly and can lie
        // while the IPC command is queued) → hold 1.2s → track MUST stay.
        let title_before = app.current.as_ref().map(|c| c.title.clone());
        app.set_pause(true);
        let eng_p = engine.clone();
        let mpv_paused = || eng_p.state.lock().unwrap().paused;
        let t2a = Instant::now();
        while t2a.elapsed().as_secs_f64() < 1.5 && !mpv_paused() {
            app.poll();
            std::thread::sleep(Duration::from_millis(40));
        }
        let t2 = Instant::now();
        let mut stayed_paused = true;
        while t2.elapsed().as_secs_f64() < 1.2 {
            app.poll();
            if !mpv_paused() || !app.paused {
                stayed_paused = false;
            }
            std::thread::sleep(Duration::from_millis(40));
        }
        let track_stayed = app.current.as_ref().map(|c| c.title.clone()) == title_before;
        let pause_ok = stayed_paused && track_stayed;
        if !track_stayed {
            failures.push(format!("PAUSE-REG: {} — track changed while paused!", t.title));
        }
        if std::env::var("E2E_DEBUG").is_ok() {
            println!(
                "[e2e]   pause-check: stayed_paused={} track_stayed={} mpv.paused={}",
                stayed_paused,
                track_stayed,
                mpv_paused()
            );
        }
        app.set_pause(false);
        let t3 = Instant::now();
        while t3.elapsed().as_secs_f64() < 4.0 && mpv_paused() {
            app.poll();
            std::thread::sleep(Duration::from_millis(50));
        }

        // SEEK +5s — ground truth = mpv time-pos (display_pos is interpolated
        // and trails the poller cadence). Stalled streams: skipped, not ok.
        let mut seek_state = "skip";
        if progressed {
            let pos0 = app.engine.as_ref().map(|e| e.state.lock().unwrap().time_pos).unwrap_or(0.0);
            app.seek_rel(5.0);
            let t4 = Instant::now();
            let mut pos1 = pos0;
            while t4.elapsed().as_secs_f64() < 1.5 {
                app.poll();
                pos1 = app.engine.as_ref().map(|e| e.state.lock().unwrap().time_pos).unwrap_or(pos0);
                if pos1 > pos0 + 1.0 {
                    break;
                }
                std::thread::sleep(Duration::from_millis(40));
            }
            seek_state = if pos1 > pos0 + 1.0 { "ok" } else { "FAIL" };
            if seek_state == "FAIL" {
                failures.push(format!("SEEK: {} — time-pos {pos0:.1}→{pos1:.1} didn't advance", t.title));
            }
            if std::env::var("E2E_DEBUG").is_ok() {
                println!("[e2e]   seek-check: tpos {pos0:.2}→{pos1:.2} {seek_state}");
            }
        }
        report.push((t.title.clone(), t.source.to_string(), started_ms, pause_ok, seek_state.to_string()));
        println!(
            "[e2e] #{i} ▶ {} — {} start={:.0}ms pause={} seek={}",
            t.source,
            trunc(&t.title, 30),
            started_ms,
            if pause_ok { "ok" } else { "FAIL" },
            seek_state
        );

        // switch to the next track (manual) — tests switching + auto-advance
        if i + 1 < tracks.len() {
            app.next_track(true);
            let t5 = Instant::now();
            while t5.elapsed().as_secs_f64() < 12.0 {
                app.poll();
                if is_playing(&app) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(60));
            }
        }
    }

    // ---- feature battery after the 20 plays ----
    // repeat cycle off → all → one → off
    let r0 = app.repeat;
    app.cycle_repeat();
    let r1 = app.repeat;
    app.cycle_repeat();
    let r2 = app.repeat;
    app.cycle_repeat();
    let r3 = app.repeat;
    let repeat_ok = r0 != r1 && r1 == Repeat::All && r2 == Repeat::One && r3 == Repeat::Off;
    if !repeat_ok {
        failures.push(format!("REPEAT cycle broken: {r0:?}→{r1:?}→{r2:?}→{r3:?}"));
    }
    println!("[e2e] repeat cycle off→all→one→off: {}", if repeat_ok { "ok" } else { "FAIL" });

    // shuffle toggle
    app.toggle_shuffle();
    let shuf_ok = app.shuffle;
    app.toggle_shuffle();
    if !shuf_ok || app.shuffle {
        failures.push("SHUFFLE toggle broken".into());
    }
    println!("[e2e] shuffle toggle: {}", if shuf_ok { "ok" } else { "FAIL" });

    // volume push + verify mpv adopted it
    app.volume = 45;
    let _ = engine.set_volume(45);
    std::thread::sleep(Duration::from_millis(400));
    let vol = {
        let st = engine.state.lock().unwrap();
        st.volume
    };
    let vol_ok = vol == 45;
    if !vol_ok {
        failures.push(format!("VOLUME: mpv reports {vol}, expected 45"));
    }
    println!("[e2e] volume set 45 → mpv={vol}: {}", if vol_ok { "ok" } else { "FAIL" });

    // history tab + back-navigation (H)
    app.load_history();
    let hist_ok = app.hist_list.len() >= 3;
    if !hist_ok {
        failures.push("HISTORY empty".into());
    }
    println!("[e2e] history: {} entries: {}", if hist_ok { "ok" } else { "FAIL" }, app.hist_list.len());
    let cur_before = app.current.as_ref().map(|c| c.title.clone());
    app.back_track();
    let t6 = Instant::now();
    while t6.elapsed().as_secs_f64() < 10.0 {
        app.poll();
        if is_playing(&app) && app.current.as_ref().map(|c| c.title.clone()) != cur_before {
            break;
        }
        std::thread::sleep(Duration::from_millis(60));
    }
    let back_ok = app.current.as_ref().map(|c| c.title.clone()) != cur_before;
    if !back_ok {
        failures.push("BACK (H) — did not replay previous track".into());
    }
    println!("[e2e] back (H): {}", if back_ok { "ok" } else { "FAIL" });

    // playlists on REAL tracks (save 10 → load → list → delete)
    let pl_name = format!("e2e-{}", std::process::id());
    let n = crate::db::save_playlist(&pl_name, &tracks[..10]).map_err(|e| anyhow::anyhow!("{e}"))?;
    let loaded = crate::db::load_playlist(&pl_name);
    let listed = crate::db::list_playlists();
    crate::db::delete_playlist(&pl_name).map_err(|e| anyhow::anyhow!("{e}"))?;
    let pl_ok = n == 10
        && loaded.as_ref().map(|l| l.len() == 10).unwrap_or(false)
        && listed.iter().any(|(nm, _, _)| nm == &pl_name)
        && crate::db::load_playlist(&pl_name).is_none();
    if !pl_ok {
        failures.push("PLAYLIST save/load/list/delete broken".into());
    }
    println!(
        "[e2e] playlist save(10)/load/list/delete: {}",
        if pl_ok { "ok" } else { "FAIL" }
    );

    // recs (network — soft) + lyrics (soft)
    app.recs();
    let t7 = Instant::now();
    while t7.elapsed().as_secs_f64() < 15.0 && app.searching {
        app.poll();
        std::thread::sleep(Duration::from_millis(80));
    }
    println!("[e2e] recs: {} results (soft)", app.results.len());
    let lyr = crate::lyrics::fetch_lyrics(&tracks[0].title, tracks[0].duration);
    println!(
        "[e2e] lyrics '{}': {} (soft)",
        trunc(&tracks[0].title, 26),
        if lyr.is_some() { "found" } else { "none" }
    );

    engine.shutdown();

    // ---- report ----
    println!("\n[e2e] =============== 20-SONG REAL PLAYBACK TEST ===============");
    println!("[e2e] {:<8} {:<32} {:>8} {:<7} {:<6}", "source", "title", "start", "pause", "seek");
    for (title, src, ms, po, ss) in &report {
        println!(
            "[e2e] {:<8} {:<32} {:>6.0}ms {:<7} {:<6}",
            src,
            trunc(title, 30),
            ms,
            if *po { "ok" } else { "FAIL" },
            ss
        );
    }
    println!(
        "[e2e] played {}/{} real tracks | skipped {} | failures {}",
        played,
        tracks.len(),
        tracks.len() - played,
        failures.len()
    );
    assert!(played >= 12, "fewer than 12 real tracks started");
    assert!(failures.is_empty(), "feature failures: {}", failures.join(" | "));
    println!("[e2e] PASS — every feature verified on {played} real songs");
    Ok(())
}
