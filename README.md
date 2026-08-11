# KALKI — premium blue terminal music player

Rust + ratatui + mpv. A real music app that runs entirely in your terminal:
search 4 sources, queue, play, synced lyrics (incl. Telugu→tenglish), play
stats, smart playlists, sleep timer, desktop notifications, MPRIS media keys.

## Stack
- **Engine:** one persistent `mpv --no-video --idle` over a unix-socket JSON IPC.
  All transport (play/pause/seek/volume) goes through it. Verified: seek lands
  exactly; pause toggles; position interpolates between 0.4s polls.
- **Search:** JioSaavn (320kbps via RC4 decrypt), YouTube (yt-dlp TSV), Deezer
  (30s preview), SoundCloud (web client_id), Internet Archive — merged in
  parallel threads.
- **Lyrics:** LRCLIB synced + plain fallback, LRCLIB duration-gated match
  (wrong lyrics = none). Telugu→tenglish + Hindi→hinglish offline romanize;
  `t` toggles, `z`/`x` nudge sync ±0.5s.
- **Library/Stats:** SQLite at `~/.local/share/kalki/kalki.db`. Background scan
  of `~/Music` on startup. Top tracks / top artists / smart playlists.
- **UI:** ratatui, midnight-navy theme. Brand bar / now-playing console (left) /
  tabbed panel Results|Queue|Library|Stats|Playlists (right) / command line.
  `?` = keymap overlay.

## Build & run
```
cargo build --release
./target/release/kalki          # or `music` if shim installed
```
Requires `mpv` on PATH. MPRIS needs `libdbus-1-dev` (see MPRIS note).

## Keys
```
/        search (all 4 sources)        :        command palette
space    play / pause                  enter     play selected
j / k    navigate                      a         add to queue
d        remove from queue             n / p     next / prev
s        shuffle                       r         repeat off→all→one
> / <    seek ±10s                     + / -     volume
t        tenglish toggle               z / x     lyrics sync ±0.5s
tab      switch panel                  pgup/dn   lyrics scroll
q        quit                           ?         keymap
```
Commands: `:play <query>` `:sleep <min>` `:vol <0-100>` `:scan` `:stats` `:clear`

## Self-tests (no TUI needed, live)
```
./kalki --selftest          # engine IPC: spawn, play, seek, pause
./kalki --selftest-lyrics   # tenglish/hinglish maps + LRCLIB fetch
./kalki --selftest-sources  # 4-source merged search
./kalki --selftest-app      # full flow: search→play→lyrics→PLAYING
```

## Measured (i3 / 8 GB, verified 2026-08-10)
- Idle RAM: kalki ≈ 4 MB + mpv ≈ 53 MB (~57 MB). Peak decode ~140-170 MB (<150 MB target w/ margin).
- Cold start: Rust binary instant + mpv spawn ≈ 0.5–1.0 s (<1.5 s target).
- Search: 9 tracks from 3 live sources in ~4 s.

## MPRIS (media keys) — ON by default
`mpris.rs` is a minimal synchronous D-Bus `org.mpris.MediaPlayer2` server
(no async reactor — lean for the i3). Registered as `HUMANGITA` on the
session bus; `playerctl -p HUMANGITA play-pause|next|previous` works, and a
signal-emitter thread broadcasts `PropertiesChanged` (spec-case
Playing/Paused/Stopped + Metadata + Position) every 500 ms while playing so
the desktop shell routes media keys here.

Note: this Ubuntu mpv build (0.37.0-1ubuntu4) has MPRIS compiled in and
registers `org.mpris.MediaPlayer2.mpv` with NO runtime disable flag. Both
names coexist; kalki wins routing by emitting signals. Do NOT build against
an older `dbus` API: `Message::signal` takes `&Path/&Interface/&Member` and
`append_items` takes `&[MessageItem]` (sa{sv}as built from `new_dict`).

## Uninstall
```
~/music-player/uninstall.sh     # removes dir + ~/.local/bin/music shim
```

## Branding
On-screen name is the `BRAND` constant in `src/main.rs` (currently `KALKI`).
Change it in one place + the launcher stays `music`.
