# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.0.0] - 2026-08-11

### Added
- Initial release: TUSIC terminal music player
- 4-source search: JioSaavn, YouTube, SoundCloud, Internet Archive
- Real-time spectrum EQ with Monstercat ballistics
- Animated DNA helix visualizer (Bresenham curves, energy-driven)
- Synced lyrics with Telugu/Hindi/English romanization
- MPRIS 2.1 desktop integration (media keys, player controls)
- Playlists (save/load/delete), history with back-navigation
- Recommendations (one-key similar-song search)
- Shuffle, repeat (off/all/one), seek, volume
- Comprehensive test suite: unit, integration, 20-song real e2e

### Fixed
- Pause regression: spacebar pause no longer switches tracks
- MPRIS metadata panic on startup
- End-detection false positives on track switch
- Queue selection highlight stuck at first item
- DNA helix visual diamond-edge artifact

## [Unreleased]

### Planned
- Library scan UI polish
- Invidious fallback for YouTube bot-wall
- Per-band helix amplitude mapping