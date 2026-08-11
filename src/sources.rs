//! KALKI sources — 5 live music sources:
//!  YouTube (yt-dlp, reliable full audio), SoundCloud (free full tracks),
//!  Deezer (30s preview), Internet Archive, JioSaavn (Indian catalog; its
//!  preview CDN is often 429-rate-limited, so treat as metadata + fallback).
//! The app auto-skips to the next source on playback failure.

use anyhow::Result;
use serde_json::Value;

#[derive(Debug, Clone, Default)]
pub struct Track {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration: f64,
    pub source: &'static str, // "JioSaavn" | "YouTube" | "Deezer" | "SoundCloud" | "Archive"
    pub uri: String,          // what mpv plays
}

const UA: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:128.0) Gecko/20100101 Firefox/128.0";

fn client() -> Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .user_agent(UA)
        .build()?)
}

// ---------------- JioSaavn ----------------

/// RC4 decrypt for JioSaavn encrypted_media_url.
/// The wire value is base64 (may contain + / =), NOT hex.
pub fn jiosaavn_decrypt(enc: &str) -> Result<String> {
    // strip whitespace, then base64-decode (standard alphabet, tolerant of
    // missing padding).
    let s = enc.trim();
    let mut b64 = s.replace(['\n', '\r', ' '], "");
    // add padding if needed
    while b64.len() % 4 != 0 {
        b64.push('=');
    }
    let bytes = match base64_decode(&b64) {
        Some(b) if !b.is_empty() => b,
        _ => return Ok(enc.to_string()), // not base64 → return as-is (fallback)
    };
    let key: Vec<u8> = b"38346591".to_vec();
    // RC4 KSA
    let mut sbox: [u8; 256] = [0; 256];
    for (i, v) in sbox.iter_mut().enumerate() {
        *v = i as u8;
    }
    let mut j: usize = 0;
    for i in 0..256 {
        j = (j + sbox[i] as usize + key[i % key.len()] as usize) % 256;
        sbox.swap(i, j);
    }
    // PRGA
    let mut i: usize = 0;
    j = 0;
    let mut out = Vec::with_capacity(bytes.len());
    for &b in &bytes {
        i = (i + 1) % 256;
        j = (j + sbox[i] as usize) % 256;
        sbox.swap(i, j);
        out.push(b ^ sbox[(sbox[i] as usize + sbox[j] as usize) % 256]);
    }
    String::from_utf8(out).map_err(|_| anyhow::anyhow!("decrypt produced non-utf8"))
}

/// Minimal standard-base64 decoder (no external dep).
fn base64_decode(s: &str) -> Option<Vec<u8>> {
    const TAB: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut buf: u32 = 0;
    let mut bits = 0;
    let mut out = Vec::new();
    for c in s.bytes() {
        if c == b'=' {
            break;
        }
        let v = TAB.iter().position(|&x| x == c)? as u32;
        buf = (buf << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    Some(out)
}

pub fn search_jiosaavn(query: &str, limit: usize) -> Result<Vec<Track>> {
    let url = format!(
        "https://www.jiosaavn.com/api.php?__call=search.getResults&q={}&_format=json&_marker=0",
        crate::lyrics::urlencode(query)
    );
    let c = client()?;
    let body = c.get(&url).send()?.error_for_status()?.text()?;
    let v: Value = serde_json::from_str(&body)?;
    let mut out = Vec::new();
    if let Some(arr) = v.get("results").and_then(|r| r.as_array()) {
        for it in arr.iter().take(limit) {
            let title = it.get("song").and_then(|s| s.as_str()).unwrap_or("").to_string();
            if title.is_empty() {
                continue;
            }
            let artists = it.get("primary_artists").and_then(|s| s.as_str()).unwrap_or("").to_string();
            let album = it.get("album").and_then(|s| s.as_str()).unwrap_or("").to_string();
            let dur: f64 = it
                .get("duration")
                .and_then(|d| d.as_str())
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.0);
            let preview = it.get("media_preview_url").and_then(|s| s.as_str()).unwrap_or("").to_string();
            let enc = it.get("encrypted_media_url").and_then(|s| s.as_str()).unwrap_or("").to_string();
            // prefer 320kbps decrypted, fall back to preview
            let uri = if !enc.is_empty() {
                jiosaavn_decrypt(&enc).unwrap_or_else(|_| preview.clone())
            } else {
                preview.clone()
            };
            if uri.is_empty() {
                continue;
            }
            out.push(Track {
                id: it.get("id").and_then(|s| s.as_str()).unwrap_or("").to_string(),
                title,
                artist: artists,
                album,
                duration: dur,
                source: "JioSaavn",
                uri,
            });
        }
    }
    Ok(out)
}

// ---------------- YouTube (yt-dlp TSV) ----------------

pub fn search_youtube(query: &str, limit: usize) -> Result<Vec<Track>> {
    let out = std::process::Command::new("yt-dlp")
        .args([
            "--js-runtimes",
            "node",
            "--flat-playlist",
            "--print",
            "%(id)s\t%(title)s\t%(duration)s\t%(channel)s",
            &format!("ytsearch{limit}:{query}"),
        ])
        .output()?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut tracks = Vec::new();
    for line in stdout.lines() {
        let mut f = line.split('\t');
        let id = f.next().unwrap_or("");
        let title = f.next().unwrap_or("");
        if id.is_empty() || title.is_empty() {
            continue;
        }
        let dur: f64 = f
            .next()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0);
        let channel = f.next().unwrap_or("").to_string();
        tracks.push(Track {
            id: id.to_string(),
            title: title.to_string(),
            artist: channel,
            album: String::new(),
            duration: dur,
            source: "YouTube",
            uri: format!("https://youtube.com/watch?v={id}"),
        });
    }
    if tracks.is_empty() {
        // retry without --flat-playlist (some videos report no duration)
        let out = std::process::Command::new("yt-dlp")
            .args(["--js-runtimes", "node", "--print", "%(id)s\t%(title)s\t%(duration)s\t%(channel)s", &format!("ytsearch{limit}:{query}")])
            .output()?;
        let stdout = String::from_utf8_lossy(&out.stdout);
        for line in stdout.lines() {
            let mut f = line.split('\t');
            let id = f.next().unwrap_or("");
            let title = f.next().unwrap_or("");
            if id.is_empty() || title.is_empty() {
                continue;
            }
            let dur: f64 = f.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
            let channel = f.next().unwrap_or("").to_string();
            tracks.push(Track {
                id: id.to_string(),
                title: title.to_string(),
                artist: channel,
                album: String::new(),
                duration: dur,
                source: "YouTube",
                uri: format!("https://youtube.com/watch?v={id}"),
            });
        }
    }
    Ok(tracks)
}

// ---------------- Deezer (30s previews) ----------------

pub fn search_deezer(query: &str, limit: usize) -> Result<Vec<Track>> {
    let url = format!(
        "https://api.deezer.com/search?q={}&limit={}",
        crate::lyrics::urlencode(query),
        limit
    );
    let c = client()?;
    let v: Value = c.get(&url).send()?.error_for_status()?.json()?;
    let mut out = Vec::new();
    if let Some(arr) = v.get("data").and_then(|d| d.as_array()) {
        for it in arr {
            let title = it.get("title").and_then(|s| s.as_str()).unwrap_or("").to_string();
            let artist = it
                .get("artist")
                .and_then(|a| a.get("name"))
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string();
            let album = it
                .get("album")
                .and_then(|a| a.get("title"))
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string();
            let dur = it.get("duration").and_then(|d| d.as_f64()).unwrap_or(0.0);
            let preview = it.get("preview").and_then(|s| s.as_str()).unwrap_or("").to_string();
            if preview.is_empty() {
                continue;
            }
            out.push(Track {
                id: it.get("id").and_then(|i| i.as_u64()).map(|i| i.to_string()).unwrap_or_default(),
                title,
                artist,
                album,
                duration: dur,
                source: "Deezer",
                uri: preview,
            });
        }
    }
    Ok(out)
}

// ---------------- Internet Archive ----------------

pub fn search_archive(query: &str, limit: usize) -> Result<Vec<Track>> {
    let url = format!(
        "https://archive.org/advancedsearch.php?q={}+AND+mediatype%3Aaudio&fl[]=identifier&fl[]=title&fl[]=creator&rows={}&output=json",
        crate::lyrics::urlencode(query),
        limit
    );
    let c = client()?;
    let v: Value = c.get(&url).send()?.error_for_status()?.json()?;
    let mut out = Vec::new();
    if let Some(docs) = v.pointer("/response/docs").and_then(|d| d.as_array()) {
        for doc in docs {
            let id = doc.get("identifier").and_then(|s| s.as_str()).unwrap_or("").to_string();
            let title = doc.get("title").and_then(|s| s.as_str()).unwrap_or("").to_string();
            if id.is_empty() || title.is_empty() {
                continue;
            }
            let creator = doc
                .get("creator")
                .and_then(|c| match c {
                    Value::String(s) => Some(s.clone()),
                    Value::Array(a) => a.first().and_then(|x| x.as_str()).map(String::from),
                    _ => None,
                })
                .unwrap_or_default();
            // resolve first audio file via metadata endpoint
            let uri = match archive_resolve(&id) {
                Ok(u) => u,
                Err(_) => continue,
            };
            out.push(Track {
                id,
                title,
                artist: creator,
                album: String::new(),
                duration: 0.0,
                source: "Archive",
                uri,
            });
        }
    }
    Ok(out)
}

fn archive_resolve(id: &str) -> Result<String> {
    let c = client()?;
    let url = format!("https://archive.org/metadata/{id}");
    let v: Value = c.get(&url).send()?.error_for_status()?.json()?;
    if let Some(files) = v.get("files").and_then(|f| f.as_array()) {
        for f in files {
            let name = f.get("name").and_then(|s| s.as_str()).unwrap_or("");
            let ftype = f.get("format").and_then(|s| s.as_str()).unwrap_or("");
            let is_audio = ftype.to_lowercase().contains("mp3")
                || ftype.to_lowercase().contains("ogg")
                || name.ends_with(".mp3")
                || name.ends_with(".ogg")
                || name.ends_with(".flac");
            if is_audio && !name.ends_with(".jpg") && !name.ends_with(".png") {
                return Ok(format!("https://archive.org/download/{id}/{}", crate::lyrics::urlencode(name)));
            }
        }
    }
    Err(anyhow::anyhow!("no audio file in {id}"))
}

// ---------------- SoundCloud ----------------
// Free full-track streaming via the public web client_id (unofficial but
// stable). We resolve a client_id from the web app bundle on first use and
// cache it. Stream URL comes from /tracks/{id}/streams?client_id=.

fn soundcloud_client_id(c: &reqwest::blocking::Client) -> Option<String> {
    // Extract the public web client_id from soundcloud.com's HTML bundle.
    // It appears as client_id="..." in the inline script. Multiple fallbacks.
    if let Ok(html) = c.get("https://soundcloud.com").send().and_then(|r| r.text()) {
        // primary: client_id="..." (JSON-ish inline)
        if let Some(idx) = html.find("client_id=\"") {
            let rest = &html[idx + 11..];
            if let Some(end) = rest.find('"') {
                let cid = &rest[..end];
                if !cid.is_empty() && cid.len() >= 8 && cid.len() <= 64 {
                    return Some(cid.to_string());
                }
            }
        }
        // secondary: client_id:"..." (JS object literal)
        if let Some(idx) = html.find("client_id:") {
            let rest = &html[idx + 10..];
            // skip whitespace/quote
            let rest = rest.trim_start_matches([' ', '"', '\'']);
            if let Some(end) = rest.find(['"', '\'', ',', '}', ' ']) {
                let cid = &rest[..end];
                if !cid.is_empty() && cid.len() >= 8 {
                    return Some(cid.to_string());
                }
            }
        }
    }
    None
}

pub fn search_soundcloud(query: &str, limit: usize) -> Result<Vec<Track>> {
    let c = client()?;
    // Cache the client_id across calls (it rotates infrequently; re-fetching
    // every search risks rate-limits / HTMLbot walls).
    static CID: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    let cid = CID
        .get_or_init(|| soundcloud_client_id(&c))
        .clone()
        .ok_or_else(|| anyhow::anyhow!("soundcloud: could not resolve client_id"))?;
    let q = crate::lyrics::urlencode(query);
    let url = format!(
        "https://api-v2.soundcloud.com/search/tracks?q={q}&client_id={cid}&limit={limit}&linked_partitioning=1",
    );
    let v: Value = c.get(&url).send()?.error_for_status()?.json()?;
    let mut out = Vec::new();
    if let Some(collection) = v.get("collection").and_then(|a| a.as_array()) {
        for it in collection {
            let id = it.get("id").and_then(|i| i.as_u64()).unwrap_or(0);
            if id == 0 {
                continue;
            }
            let title = it.get("title").and_then(|s| s.as_str()).unwrap_or("").to_string();
            let artist = it
                .get("user")
                .and_then(|u| u.get("username"))
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string();
            let duration = it.get("duration").and_then(|d| d.as_f64()).unwrap_or(0.0) / 1000.0;
            let stream_url = format!(
                "https://api-v2.soundcloud.com/tracks/{id}/streams?client_id={cid}"
            );
            // resolve the actual mp3 url (may be relative //host/path)
            let uri = match c.get(&stream_url).send().and_then(|r| r.json::<Value>()) {
                Ok(s) => {
                    let raw = s
                        .get("http_mp3_128_url")
                        .or_else(|| s.get("hls_mp3_128_url"))
                        .and_then(|u| u.as_str())
                        .unwrap_or("")
                        .to_string();
                    if raw.starts_with("//") {
                        format!("https:{raw}")
                    } else if raw.starts_with('/') {
                        format!("https://api-v2.soundcloud.com{raw}")
                    } else {
                        raw
                    }
                }
                Err(_) => String::new(),
            };
            if uri.is_empty() {
                continue;
            }
            out.push(Track {
                id: id.to_string(),
                title,
                artist,
                album: String::new(),
                duration,
                source: "SoundCloud",
                uri,
            });
        }
    }
    Ok(out)
}

// ---------------- merged search ----------------

/// Run all 5 sources in parallel threads, return merged tracks.
/// Order is deliberate: YouTube first (reliable full-audio via yt-dlp),
/// then SoundCloud/Deezer/Archive, JioSaavn LAST. JioSaavn's preview CDN is
/// now rate-limited (HTTP 429) from many IPs, so its audio frequently fails;
/// the app's auto-skip-on-failure handles that, but we lead with sources that
/// actually play. JioSaavn still contributes Indian-catalog titles + lyric
/// matching.
pub fn search_all(query: &str, per_source: usize) -> Vec<Track> {
    let q1 = query.to_string();
    let q2 = query.to_string();
    let q3 = query.to_string();
    let q4 = query.to_string();
    let q5 = query.to_string();
    let handles: Vec<std::thread::JoinHandle<Vec<Track>>> = vec![
        std::thread::spawn(move || search_youtube(&q1, per_source).unwrap_or_default()),
        std::thread::spawn(move || search_soundcloud(&q2, per_source).unwrap_or_default()),
        std::thread::spawn(move || search_deezer(&q3, per_source).unwrap_or_default()),
        std::thread::spawn(move || search_archive(&q4, per_source).unwrap_or_default()),
        std::thread::spawn(move || search_jiosaavn(&q5, per_source).unwrap_or_default()),
    ];
    let mut merged = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for h in handles {
        for t in h.join().unwrap_or_default() {
            // dedup by id if present, else by (title, artist)
            let key = if t.id.is_empty() {
                format!("{}|{}", t.title, t.artist)
            } else {
                t.id.clone()
            };
            if seen.insert(key) {
                merged.push(t);
            }
        }
    }
    merged
}
