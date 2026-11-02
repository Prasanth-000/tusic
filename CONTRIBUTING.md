# Contributing to TUSIC

Thanks for considering a contribution! This is a personal project, but improvements are welcome.

## Ways to contribute

- **Bug reports** — Use the issue template, include steps to reproduce and `cargo build --release` output.
- **Feature ideas** — Open an issue first to discuss scope; PRs without discussion may be closed.
- **Code changes** — Fork, branch from `master`, keep commits focused, write tests for new logic.
- **Documentation** — README, inline comments, or the key-binding help screen.

## Development setup

```bash
git clone https://github.com/Prasanth-000/tusic.git
cd tusic
cargo build --release
./target/release/tusic
```

Run the test battery before submitting:

```bash
cargo test --release
./target/release/tusic --selftest-units
```

## Code style

- Rust 2021 edition, `cargo fmt` / `cargo clippy` clean.
- Keep the blue theme (theme.rs) — it's the project's visual identity.
- No external API keys; all sources must remain keyless public endpoints.
- Shell commands use argument vectors, never string interpolation.
- SQLite uses bound parameters exclusively.

## Commit messages

Brief subject, optional body. Example:

```
fix: pause auto-skip during long pauses

The 4s "source failed" watchdog could fire while the user had the track
paused. Now it refuses to skip when paused, and any playback clears the latch.
```

## License

By contributing, you agree your changes are licensed under MIT (the project's license).