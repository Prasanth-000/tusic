# HUMAN-GITA — Project History & Master Reference

> PERMANENT RECORD. This document survives all memory compression. Read it before
> any future upgrade work. Lives in: git history, GitHub, HDD archive.
> Last updated: 2026-08-11 (project completion, v1.0 stable).

---

## 1. What this project is

HUMAN-GITA — a terminal-native music player for Telugu, Hindi, English & global music.
Rust + ratatui + mpv. Streams from 4 live sources (JioSaavn, YouTube, SoundCloud,
Internet Archive). No API keys, no audio files stored.

- Repo: https://github.com/Prasanth-000/human-gita (PUBLIC, MIT)
- Local: ~/music-player (git, tag `stable-v1`)
- Launcher: `music` → ~/.local/bin/music → target/release/human-gita
- HDD archive: /media/divine/Data/Archives/kalki-music-player-stable-v1-20260811.tar.gz

## 2. Architecture (src/)

| file | role |
|------|------|
| main.rs | entry, selftest battery (incl. `--selftest-e2e`), romanize fallbacks |
| app.rs | App state, poll loop, keybindings, queue/history/back/recs, ended-detection |
| engine.rs | persistent mpv over unix-socket JSON IPC (SOCK=/tmp/kalki.sock), poller thread |
| tui.rs | ratatui rendering: NOW PLAYING, EQ box, DNA helix panel, tabs, bottom bar |
| music_visual.rs | 3D double-helix DNA visual (Bresenham curves, energy-driven amplitude) |
| viz.rs | cava-pipeline spectrum (FFT) → EQ bands |
| sources.rs | 4-source search + JioSaavn RC4 decrypt + SoundCloud/Archive resolvers |
| lyrics.rs | LRCLIB synced lyrics, Telugu/Hindi→Latin romanization, urlencode |
| db.rs | SQLite: history, playlists (bound params only, KALKI_DB_PATH override) |
| mpris.rs | MPRIS 2.1 server (dbus-crossroads), 500ms PropertiesChanged emitter |
| theme.rs | blue palette (ACCENT #3B82F6, BRIGHT #22D3EE, BORDER, DIM...) |

## 3. Key bindings (user-facing)

/ search · j/k move · Enter play · a add-queue · d remove-queue · Space pause ·
←/→ seek ±5s · n/p next/prev · r repeat off→all→one · s shuffle · +/- volume ·
H back-track · R recs · h/1-6 tabs · : commands (`:pl save/load <name>`, `:play <q>`,
`:vol <n>`) · ? help · q quit

## 4. Every bug found & fixed (the debugging chronicle)

1. **MPRIS EmptyArray panic** — emitter thread panicked at startup (no track loaded):
   `new_dict(empty)` → guarded, Metadata only emitted with a current track.
2. **selftest-eq network flake** — played a live YouTube "phonk" track → flaky asserts.
   Rewrote to a LOCAL ffmpeg-generated 2Hz beat-modulated tone (deterministic, no net).
3. **"Queue ended" on track switch** — mpv blips idle between loadfile calls while the
   OLD duration is still reported → false end-detection. Fix: switch-grace — ignore
   end-detection within 1.5s of a play attempt; extracted pure fn
   `should_treat_as_ended(was_playing, now_playing, has_current, dur, switching, paused)`.
4. **Pause redirects to another song (user's spacebar bug)** — TWO root causes:
   a) pause flips mpv to paused → old end-detection read it as "stopped playing"
      → added `paused` param (pause NEVER counts as end).
   b) the 4s "source failed" auto-skip fired DURING a pause because play_attempt_at
      was never cleared when mpv swaps files without an idle blip → auto-skip now
      refuses while paused, and any now_playing clears the latch.
   Both caught by the e2e pause-regression suite (pause 1.2s → track must stay).
5. **Static blue selection bar in results/queue** — ListState pinned at `Some(0)`
   forever → now `sel - start`, moves with j/k.
6. **Seek bar was a static "─ line with ball"** → gradient blue→cyan progress bar with
   glowing head + live shimmer (progress_bar in tui.rs).
7. **DNA helix looked like a diamond (sharp edges)** — single dots + vertical rungs
   created jagged slopes and hard right angles. Fix: Bresenham segments between
   consecutive strand points (smooth curves) + rungs only where strands separate >3
   rows, every 4th column.
8. **Tabs clipped / unreachable** — 6 long labels overflowed the panel → numbered
   labels "1 Results".."6 History" + direct number-key jumps.

## 5. Verification state (the proof)

- `hermes verify` — recipe in .hermes/environment.json: build + 7 selftests, all PASS.
- `cargo test --release` — 5 unit tests: ended_detection (7 vectors incl. both bugs),
  keyword_filter, render determinism, 120×16 bounds sweep, strand continuity.
- `--selftest-units` — same vectors runnable without cargo test (in verify battery).
- `--selftest-e2e` — 20 REAL songs from live sources: play/start latency, pause
  regression (must NOT switch), resume, seek, next/prev, queue auto-advance, repeat
  cycle, shuffle, volume (mpv-verified), history + back, playlists on real tracks,
  recs, lyrics. Last full run: 20/20 played, 0 failures, 0 skipped.
  Debug env: E2E_LIMIT=n caps tracks, E2E_DEBUG=1 prints check internals.
- Security: cargo-audit 0 vulnerabilities/0 CVEs (300 crates); secrets-scan clean;
  no shell interpolation (argv vectors everywhere, all URLs urlencode'd); SQLite
  bound params; uninstall.sh scoped to its own dir.

## 6. Testing lessons (avoid re-learning these)

- `play_track()` updates `app.current` INSTANTLY while mpv still plays the OLD file —
  gate "track started" on mpv `state.path` CHANGE.
- `app.paused` is an optimistic mirror (set before the IPC command lands) — converge
  on engine `st.paused` for assertions.
- mpv swaps files WITHOUT an idle blip → no rising edge; clear play_attempt_at on any
  now_playing.
- YouTube streams often stall (bot-wall) with time-pos stuck ~0 → treat as "stalled",
  skip seek asserts; archive.org can 503; JioSaavn rate-limits (4s auto-skip covers).
- `pkill -f "target/release/kalki"` matches its own shell → use `[t]arget/...` trick.
- Loading bars/seek bars: wall-clock driven (music_visual.clock()) so motion is
  frame-rate independent.
- UI lists: ListState must be seeded from the real selection, never Some(0).

## 7. How to resume / upgrade (months later)

1. `cd ~/music-player && git log --oneline` — baseline is commit f510af9, tag stable-v1.
2. `git status` clean → work on a new branch: `git checkout -b upgrade-v2`.
3. Build + run: `cargo build --release && ./target/release/human-gita`.
4. After changes: `cargo test --release`, then `./target/release/human-gita --selftest-units`,
   then `--selftest-e2e` (real songs, ~4 min). Keep the battery green.
5. Commit, tag `stable-v2`, push, and if the repo changed: bump the archive on the HDD.
6. The `music` shim and the verify recipe (environment.json) reference the binary name
   `human-gita` — keep them in sync if the binary is ever renamed.

## 8. Known limits (future enhancement seeds)

- YouTube: bot-wall on some networks → cookies-from-browser or Invidious fallback.
- JioSaavn: RC4 key rotates (monitor); 429s handled by auto-skip.
- No library scan UI polish (scanning flag exists, ~/Music index partial).
- EQ: Monstercat ballistics; helix: single amplitude channel (no per-band mapping yet).
- Windows/macOS: untested (mpv IPC should work; PipeWire env logic is Linux-only).
