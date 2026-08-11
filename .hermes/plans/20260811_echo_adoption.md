# Echo (brahmkshatriya/echo, 3445★) — Adoption Plan for kalki

Analyzed 2026-08-11 from the live repo (688 files; Kotlin/Compose multiplatform
build; extension-based architecture; key files read: ExtensionLoader.kt,
MediaState.kt, OfflineExtension.kt, di/App.kt, download task state machine).

Echo's core architectural ideas, mapped to OUR Rust TUI (app.rs / sources.rs /
engine.rs / lyrics.rs / db.rs / tui.rs):

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
1. EXTENSION INTERFACE  →  Rust `Source` trait + registry  (P0)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Echo: every provider implements `Extension` / `MusicExtension` /
`LyricsExtension` client contracts (search, songs, albums, lyrics, login);
extensions are DISCOVERED and loaded, UI never knows the concrete provider.

We already have the spirit in sources.rs (search_jiosaavn, search_youtube,
search_archive, search_soundcloud, search_all) but as free functions.
Refactor (small, ~2h):
  trait Source {
      fn name(&self) -> &'static str;
      fn search(&self, q: &str, limit: usize) -> Result<Vec<Track>>;
      fn lyrics(&self, title: &str, dur: f64) -> Option<Lyrics>; // default off
      fn resolve(&self, t: &Track) -> Result<String>;            // uri → playable
  }
  static REGISTRY: &[&dyn Source] = &[&JioSaavn, &YouTube, &Archive, &SoundCloud];
Benefit: search_all becomes a 3-line fold; adding a source (Deezer, Radio
Garden) is one new file + one registry entry — exactly echo's pluggability
without dlopen complexity. This is the highest-leverage refactor in the repo.

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
2. RICH MEDIA STATE  →  extend Track metadata  (P1)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Echo's MediaState: Loaded/Unloaded + isFollowed/isSaved/isLiked/isHidden/
followers/showRadio — the UI reacts to state, not raw lists.

Ours: Track { id, title, artist, album, source, uri, duration }.
Add (serde-friendly, defaults): popularity: Option<u32>, explicit: bool,
cover_url: Option<String> (JioSaavn already returns image URLs — we drop them).
Benefit: stats tab + queue view can show popularity; groundwork for a
"liked" playlist (db table + :like command). Cheap (~1h + db migration).

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
3. OFFLINE EXTENSION  →  upgrade ~/Music scan  (P1)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Echo's OfflineExtension + MediaStoreUtils: robust local-media discovery,
dedup by content id, metadata repair (missing title → filename fallback),
playlist CRUD.

Ours: db::scan_library(None) — a single pass. Upgrades:
  a) incremental scan (mtime tracking — skip unchanged files)  [P1, 1h]
  b) URI scheme `local://<id>` so local tracks round-trip through the same
     play_track path (Echo unifies local+streaming — we should too)
  c) filename → title fallback when tags are empty (echo does this)
Benefit: big local libraries index in seconds instead of minutes; the
"Local" source becomes a first-class tab.

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
4. DOWNLOAD TASK STATE MACHINE  →  `:dl <q>`  (P2)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Echo's download/ tasks: LoadingTask → DownloadingTask → MergingTask →
SaveToUnifiedTask with per-task state + DB rows + progress events. UI shows
per-track download chips.

For kalki: yt-dlp already does resolution+download in one step, so we don't
need the 4-task split — but the STATE MODEL is worth copying:
  enum DlState { Queued, Resolving, Downloading(pct), Merging, Done, Failed }
  DownloadStore: HashMap<TrackId, DlState> + spawned worker (yt-dlp -x
  --audio-format mp3 --output ~/Music/%(title)s.%(ext)s)
  Tab " Downloads " listing states + a live % bar (reuse loading_bar for
  indeterminate stages).
Benefit: offline-first listening without Android. ~half a day. This is the
single most user-visible feature echo's design suggests.

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
5. DI / CACHE  →  what we should NOT copy  (P0 decision)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Echo uses manual DI (di/App.kt) + a 50MB LRU file cache. We should NOT add
DI ceremony to a 690-line app (App struct already IS our DI — constructor
injection via attach_engine). A small LRU cover-art cache (jiosaavn images)
is worth it later (P2), but only if we adopt cover_url (#2).

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
ROADMAP
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
P0 (half a day, no behavior change):
  · sources.rs → trait Source + REGISTRY (file: src/sources.rs only)
P1 (1-2 days):
  · Track metadata + popularity/cover (sources.rs, db.rs, tui.rs stats tab)
  · incremental ~/Music scan + local:// URIs (db.rs, app.rs)
  · lyric source becomes an extension too (lyrics.rs → trait)
P2 (2-4 days):
  · :dl download store + tab (new src/download.rs, app.rs, tui.rs)
  · cover-art cache for the now-playing panel
NOT NOW:
  · plugin/dlopen, client-server split, accounts/login (Echo's heavy parts)

Source files on disk for reference: /tmp/echo_analysis/
