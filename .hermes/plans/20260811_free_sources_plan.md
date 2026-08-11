# Kalki — Resilience & Source Expansion Plan

---

## A) The Echo Model — How Users Get a "Massive Free Library" (5 Bullets)

1. **Zero-Host Philosophy** — Echo hosts no content, no API keys, no servers. It's a pure client. This means there's nothing to take down, no DMCA target, no cost. The app is just a shell.

2. **Extension Marketplace** — Users install community-built extensions (APK modules) from public repositories. Each extension implements a standard `MusicExtension` interface (search, stream, metadata). The app itself never changes; the ecosystem grows via third-party modules.

3. **Community Scrapers as Sources** — The "free library" comes from extensions that scrape or stream free tiers of commercial services: Spotify-alternative scrapers, Deezer/Tidal free tiers, Google Drive (user's own files). The content is *someone else's* — Echo just orchestrates.

4. **No Ads by Architecture** — F-Droid distribution means no ad SDKs. No content hosting means no revenue pressure. The app is ad-free because it's structurally incapable of showing ads.

5. **User-Managed Resilience** — If one extension dies, users install another. The app's resilience is *distributed* across the community. Echo itself never breaks; individual extensions might, but the platform survives.

**Key takeaway for kalki:** We don't need to *host* anything. We need *multiple independent acquisition paths* so no single service's death kills us.

---

## B) Resilience Upgrades for Our 4 Current Sources

### 1. JioSaavn (currently reliable)

| Upgrade | Mechanism |
|---------|-----------|
| **RC4 Key Rotation Detection** | JioSaavn periodically rotates the RC4 key used for decrypting stream URLs. Implement a background task that: (a) fetches a known song's API response, (b) attempts decryption with the current key, (c) if decryption fails, probes a list of known historical keys + scrapes the JioSaavn web player's JS bundle for the new key (regex-extract `var key = "..."` pattern). Store the working key in a config file. |
| **API Endpoint Fallback** | JioSaavn has multiple API hosts (`www.jiosaavn.com/api.php`, `api.savn.com`, `c.savncdn.com`). Maintain a list of 3–4 known-good endpoints. On HTTP 403/404/500, rotate to the next endpoint. Also support the unofficial `jiosaavn-api` community mirrors (e.g., `https://jiosaavn-api-*.vercel.app`) as a last-resort proxy. |

### 2. YouTube (currently bot-walled: HTTP 429)

| Upgrade | Mechanism |
|---------|-----------|
| **Cookies-from-Browser Extraction** | Use `yt-dlp`'s `--cookies-from-browser` flag. In kalki, shell out to `yt-dlp --cookies-from-browser firefox` (or chrome/brave) to get authenticated cookies. This bypasses the "Sign in to confirm you're not a bot" wall because the browser session is already verified. Store cookies to a file for reuse. |
| **Invidious/Piped API Fallback** | Maintain a list of public Invidious instances (e.g., `yewtu.be`, `inv.nadeko.net`) and Piped instances (e.g., `pipedapi.kavin.rocks`). On YouTube 429, query these APIs: `GET /api/v1/search?q=...` and `GET /api/v1/videos/{id}/streams`. These return direct stream URLs without bot detection. Rotate through the instance list on failure. |

### 3. SoundCloud (client_id auto-extracted)

| Upgrade | Mechanism |
|---------|-----------|
| **client_id Rotation Pool** | SoundCloud's `client_id` is embedded in their web app's JS bundle. Maintain a pool of 3–5 known-good client_ids (hardcoded + periodically refreshed). On HTTP 401/403, rotate to the next ID in the pool. Add a background refresher that fetches `https://soundcloud.com` HTML, extracts the JS bundle URL, and regex-scrapes the `client_id` parameter. |
| **API Endpoint Diversity** | SoundCloud has multiple API paths: `api-v2.soundcloud.com`, `api.soundcloud.com`, and the legacy `api-mobile.soundcloud.com`. On failure, try the alternate host. Also support the `soundcloud-dl` community proxy pattern (e.g., `https://api.scdl.workers.dev`) as a fallback. |

### 4. Internet Archive (public API — most stable)

| Upgrade | Mechanism |
|---------|-----------|
| **Advanced Search Query Builder** | Use the full `advancedsearch.php` API with `fl[]` (field list) and `sort[]` parameters. Add support for `mediatype:audio` + `collection:opensource_audio` + `format:MP3` filters to get only directly-streamable files. Cache search results locally (JSON) to avoid rate limits. |
| **CDN Endpoint Rotation** | Archive.org serves files from `archive.org/download/` but also has regional mirrors (`ia800000.us.archive.org`, `ia801000.us.archive.org`). On slow/failed download, try the `archive.org/download/` → `ia*.us.archive.org` redirect chain manually. Also support the `archive.org/metadata/{id}` API to get all file URLs at once. |

---

## C) 3 New Free Sources Worth Adding

### 1. Radio Browser API (radio-browser.info)

- **Acquisition:** Public API, no auth. `GET https://de1.api.radio-browser.info/json/stations/search?name={query}&limit=10&hidebroken=true`. Returns station metadata + stream URLs (MP3/AAC). 
- **Why it fits:** Terminal player friendly — streams are direct URLs, no DRM, no scraping. Perfect for "radio mode" in kalki.
- **Resilience:** Multiple API mirrors (`de1`, `nl1`, `at1` subdomains). Rotate on failure.

### 2. Free Music Archive (freemusicarchive.org)

- **Acquisition:** Public API at `https://freemusicarchive.org/api/tracks?q={query}&format=json` (legacy) or scrape the newer site's search results. Tracks are CC-licensed, direct MP3 URLs.
- **Why it fits:** 100% legal, no DRM, no bot walls. Great for "discovery" mode.
- **Resilience:** The API is occasionally flaky — add a fallback to scraping the HTML search page (`https://freemusicarchive.org/search?quicksearch={query}`) and extracting track URLs from the JSON-LD embedded in the page.

### 3. Deezer Free Tier (via unofficial API)

- **Acquisition:** Deezer's public API (`https://api.deezer.com/search?q={query}`) returns track metadata + a `preview` MP3 URL (30-second clip). For full tracks, use the community-maintained `deezer-dl` pattern: extract the track's `md5` + `track_token` from the API, then call `https://api.deezer.com/track/{id}/stream` with the token to get a full stream URL.
- **Why it fits:** Deezer's free tier is ad-supported on their app, but the *API* returns direct stream URLs. Since we're a terminal client (no ads), we get clean streams.
- **Resilience:** The `track_token` expires — implement a token refresh on each search. Fallback to the 30-second preview if full-stream fails.

---

## D) The "Never Breaks" Architecture (Rust, ~700-line codebase)

### Source Priority Tiers

```
TIER 1 (Primary — most reliable, legal):
 - Internet Archive (public API, never breaks)
 - Radio Browser (public API, never breaks)

TIER 2 (Secondary — reliable but may rate-limit):
 - JioSaavn (works today, needs key rotation)
 - Free Music Archive (CC-licensed, occasional API flakiness)

TIER 3 (Tertiary — may bot-wall or break):
 - YouTube (needs cookies or Invidious fallback)
 - SoundCloud (needs client_id rotation)
 - Deezer (needs token refresh)
```

### Per-Source Health Check (background task, runs every 5 min)

```rust
// Pseudocode structure (NOT actual code)
struct SourceHealth {
    source: SourceId,
    last_success: Instant,
    consecutive_failures: u32,
    status: HealthStatus, // Healthy | Degraded | Down
}

fn health_check(source: &SourceId) -> HealthStatus {
    // For each source, do a minimal probe:
    // - JioSaavn: GET a known song's API endpoint (expect 200)
    // - YouTube: HEAD request to youtube.com (expect 200, not 429)
    // - SoundCloud: GET api-v2.soundcloud.com (expect 200)
    // - Internet Archive: GET archive.org/metadata/{known_id} (expect 200)
    // - Radio Browser: GET de1.api.radio-browser.info/json/stats (expect 200)
    // - Deezer: GET api.deezer.com/search?q=test (expect 200)
    // - FMA: GET freemusicarchive.org (expect 200)
    
    // If 3+ consecutive failures → mark Down
    // If 1-2 failures → mark Degraded
    // Update a global HashMap<SourceId, SourceHealth>
}
```

### Automatic Fallback Chain in `search_all`

```rust
// search_all(query) logic:
// 1. Check health map — skip sources marked Down
// 2. Query sources in priority order: Tier 1 → Tier 2 → Tier 3
// 3. Collect results from each source (with a 3-second timeout per source)
// 4. If a source fails mid-query, mark it Degraded, continue to next
// 5. Merge results, deduplicate by (title, artist), sort by source tier
// 6. Return results + a "source_status" map for UI display
```

### Surfacing "Source X Down, Using Y" to the User

```rust
// In the TUI status bar (bottom line), show:
//   [IA ✓] [RB ✓] [JS ✓] [FMA ⚠] [YT ✗] [SC ✗] [DZ ⚠]
//   Where: ✓ = healthy, ⚠ = degraded, ✗ = down

// When a search completes, show a one-line notice:
//   "YouTube down (429) — used Invidious fallback instead"
//   "SoundCloud degraded — rotating client_id..."

// Store last_error per source in the health map:
//   source_errors: HashMap<SourceId, String> // e.g. "HTTP 429: bot wall"
```

### Implementation Notes for a ~700-line Codebase

| Component | Approx. Lines | Description |
|-----------|--------------|-------------|
| Source trait + registry | 80 | `trait Source { fn search(&self, q: &str) -> Vec<Track>; fn health_check(&self) -> HealthStatus; }` |
| Health monitor | 60 | Background thread, probes each source every 5 min |
| Fallback chain | 50 | `search_all()` orchestrator with timeout + skip logic |
| JioSaavn source | 100 | API call + RC4 decrypt + key rotation |
| YouTube source | 80 | yt-dlp call + Invidious fallback |
| SoundCloud source | 60 | client_id pool + rotation |
| Internet Archive source | 50 | Advanced search API |
| Radio Browser source | 40 | Simple GET + parse |
| FMA source | 40 | API + HTML fallback |
| Deezer source | 60 | API + token refresh |
| UI status display | 40 | Status bar + error notices |
| Config + persistence | 40 | Store keys, cookies, health state |

**Total: ~700 lines** — fits the constraint.

---

## Summary

The Echo model works because it's **architecturally decentralized** — no single point of failure. Kalki can achieve the same by:

1. **Never depending one source** — always have 2+ paths to any content.
2. **Health-checking everything** — know what's broken before the user hits it.
3. **Failing gracefully** — show "source X down, using Y" instead of a dead search.
4. **Rotating credentials** — keys, client_ids, cookies are all ephemeral; treat them as such.

The plan above gives kalki 7 sources (4 current + 3 new), each with a fallback mechanism, wrapped in a health-checked priority chain. No single service death can kill the player.