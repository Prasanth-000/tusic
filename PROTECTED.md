# TUSIC — PROTECTED STABLE BASELINE

Status: STABLE — verified 2026-08-11. DO NOT DELETE OR REWRITE THIS BASELINE.

## Verification state (all green on baseline commit)
- build + 7 integration tests: 8/8 PASS
- cargo test: 5/5 unit tests PASS
- 20-song real-playback e2e: 20/20 tracks played, 0 failures, 0 skipped
  (pause regression: 20/20 ok — pause never switches the track)

## How this baseline is protected
- Git repo at ~/music-player, commit c2127b5, tag: `stable-v1`
- Archive: /media/divine/Data/Archives/kalki-music-player-stable-v1-20260811.tar.gz
- Full restore (one command):
  tar xzf /media/divine/Data/Archives/kalki-music-player-stable-v1-20260811.tar.gz -C ~
  (then: cd ~/music-player && cargo build --release)
- Restore from git (one command):
  cd ~/music-player && git checkout stable-v1

## Working agreement
- Future work IMPROVES on top of this baseline — never deletes or rewrites it.
- New changes are committed as NEW commits/tags (stable-v2, ...); the baseline
  stays recoverable forever.
- The user's daily driver: `music` (shim → ~/music-player/target/release/tusic).
