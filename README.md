# KALKI — Terminal Music Player

A fast, beautiful, terminal-native music player for Telugu, Hindi, and global music — built in Rust, driven by mpv.

![Rust](https://img.shields.io/badge/Rust-1.75%2B-orange) ![License](https://img.shields.io/badge/License-MIT-blue) ![Platform](https://img.shields.io/badge/Platform-Linux-lightgrey) ![Status](https://img.shields.io/badge/Status-Stable%20v1.0-green)

Search, play, and enjoy music from four live sources — **JioSaavn, YouTube, SoundCloud, and the Internet Archive** — without leaving your terminal. Includes a real-time spectrum EQ, an animated DNA helix visualizer, synced Telugu lyrics, MPRIS desktop integration, playlists, history, and recommendations.

## ✨ Features

- 🔍 **4-source search** — JioSaavn, YouTube, SoundCloud, Internet Archive (merged, deduped)
- ▶️ **Real playback** via mpv (no audio files stored — streams directly)
- 📊 **Live spectrum EQ** with Monstercat-style ballistics + peak hold
- 🧬 **Animated DNA helix visualizer** that expands and contracts with the beat
- 📜 **Synced lyrics** (Telugu/Hindi/English) with romanization for non-Latin scripts
- 🎚️ **Full playback control** — seek, pause, volume, repeat (off/all/one), shuffle
- 📝 **Playlists** — save/load/delete your queues
- 🕘 **History** — recently played + back-navigation (H)
- 💡 **Recommendations** (R) — one-key similar-song search
- 🎵 **MPRIS 2.1** integration — media keys and desktop players control kalki
- ⏳ **Live loading + progress bars** — gradient comet, shimmer, spinner
- 🎨 Blue-themed minimalist terminal aesthetic (transparent panels)

## 📦 Requirements

- Linux with PipeWire/PulseAudio/ALSA
- [mpv](https://mpv.io) (`mpv` on PATH)
- [ffmpeg](https://ffmpeg.org) (test tone generation)
- [yt-dlp](https://github.com/yt-dlp/yt-dlp) + [node](https://nodejs.org) (YouTube search)
- Rust 1.75+ (build only)

```bash
# Debian/Ubuntu/Pop!_OS
sudo apt install -y mpv ffmpeg yt-dlp nodejs
```

## 🔧 Install & Run

```bash
git clone https://github.com/<your-user>/kalki-music-player.git
cd kalki-music-player
cargo build --release
./target/release/kalki
```

Optional: add a `music` launcher to your PATH (one line):

```bash
ln -sf "$PWD/target/release/kalki" ~/.local/bin/music && music
```

## 🎮 Keys

| Key | Action |
|-----|--------|
| `/` | search |
| `j` / `k` | move selection |
| `Enter` | play selected |
| `a` | add to queue |
| `d` | remove from queue |
| `Space` | pause / resume |
| `←` / `→` | seek ±5s |
| `n` / `p` | next / previous |
| `r` | repeat: off → all → one |
| `s` | shuffle toggle |
| `+` / `-` | volume |
| `H` | go back to previous track |
| `R` | recommendations |
| `h` / `1`–`6` | tabs: history, results, queue, library, stats, playlists |
| `:` | commands (`:pl save <name>`, `:pl load <name>`, `:play <query>`, `:vol <n>`) |
| `?` | help |
| `q` | quit |

## 🧪 Tests

The project ships a full self-test battery — no mocking, real playback:

```bash
# unit tests
cargo test --release

# integration selftests (engine, lyrics, sources, EQ/helix, app, features, logic)
./target/release/kalki --selftest
./target/release/kalki --selftest-lyrics
./target/release/kalki --selftest-sources
./target/release/kalki --selftest-eq
./target/release/kalki --selftest-app
./target/release/kalki --selftest-features
./target/release/kalki --selftest-units

# full end-to-end: 20 real songs from live sources, every feature hammered
./target/release/kalki --selftest-e2e
```

Verified state (v1.0): `hermes verify` 8/8 phases green, `cargo test` 5/5, 20-song live e2e **20/20 tracks, 0 failures** — including the pause-regression suite (pausing never switches the track).

## 🛡️ Security

- No API keys or secrets required — all sources are public endpoints
- No shell interpolation of user input — every subprocess uses argument vectors; all URLs are percent-encoded
- SQLite access uses bound parameters exclusively (no SQL injection surface)
- Uninstall is fully reversible and scoped to its own directory

## ⚠️ Disclaimer

Streams from JioSaavn / YouTube / SoundCloud are unofficial public endpoints and may change or be rate-limited at any time. This project is for personal use; respect the platforms' terms of service. YouTube playback may require browser cookies (`yt-dlp --cookies-from-browser`) on bot-walled networks.

## 📄 License

MIT — see [LICENSE](LICENSE).

Built with ❤️ on Pop!_OS with mpv + ratatui + rust-viz.
