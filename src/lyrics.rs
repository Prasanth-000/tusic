//! KALKI lyrics — LRCLIB client + Indic script detection + tenglish romanize.
//! Ported from the verified legacy Python pipeline (lang.py + lyrics.py maps,
//! UCD-verified 2026-08-10). Telugu map is load-bearing: 0x0C29/0x0C3A+
//! unassigned — ఱ RRA=0x0C31, ఴ LLLA=0x0C34 shift the block.

use anyhow::Result;
use std::collections::HashMap;

// ---------- script detection (port of lang.py) ----------

pub const SCRIPT_TABLE: &[(&str, &[(u32, u32)], &str)] = &[
    ("Telugu", &[(0x0C00, 0x0C7F)], "te"),
    ("Tamil", &[(0x0B80, 0x0BFF)], "ta"),
    ("Kannada", &[(0x0C80, 0x0CFF)], "kn"),
    ("Malayalam", &[(0x0D00, 0x0D7F)], "ml"),
    ("Bengali", &[(0x0980, 0x09FF)], "bn"),
    ("Gurmukhi", &[(0x0A00, 0x0A7F)], "pa"),
    ("Gujarati", &[(0x0A80, 0x0AFF)], "gu"),
    ("Oriya", &[(0x0B00, 0x0B7F)], "or"),
    ("Sinhala", &[(0x0D80, 0x0DFF)], "si"),
    ("Devanagari", &[(0x0900, 0x097F), (0xA8E0, 0xA8FF)], "hi"),
    (
        "Hangul",
        &[(0xAC00, 0xD7A3), (0x1100, 0x11FF), (0x3130, 0x318F), (0xA960, 0xA97F), (0xD7B0, 0xD7FF)],
        "ko",
    ),
    ("Japanese", &[(0x3040, 0x309F), (0x30A0, 0x30FF)], "ja"),
    ("Han", &[(0x4E00, 0x9FFF), (0x3400, 0x4DBF)], "zh"),
    ("Arabic", &[(0x0600, 0x06FF), (0x0750, 0x077F), (0xFB50, 0xFDFF)], "ar"),
    ("Hebrew", &[(0x0590, 0x05FF)], "he"),
    ("Thai", &[(0x0E00, 0x0E7F)], "th"),
    ("Cyrillic", &[(0x0400, 0x04FF), (0x0500, 0x052F)], "ru"),
    ("Greek", &[(0x0370, 0x03FF)], "el"),
    (
        "Latin",
        &[(0x0041, 0x005A), (0x0061, 0x007A), (0x00C0, 0x024F)],
        "other",
    ),
];

pub fn script_of(cp: u32) -> Option<&'static str> {
    for (script, ranges, _) in SCRIPT_TABLE {
        for &(lo, hi) in *ranges {
            if lo <= cp && cp <= hi {
                return Some(script);
            }
            if cp < lo {
                break;
            }
        }
    }
    None
}

pub fn script_counts(text: &str) -> HashMap<&'static str, usize> {
    let mut counts: HashMap<&'static str, usize> = HashMap::new();
    for ch in text.chars() {
        if !ch.is_alphabetic() {
            continue;
        }
        if let Some(s) = script_of(ch as u32) {
            *counts.entry(s).or_insert(0) += 1;
        }
    }
    counts
}

const TITLE_KEYWORDS: &[(&str, &[&str])] = &[
    ("te", &["telugu", "tollywood", "\u{0c24}\u{0c46}\u{0c32}\u{0c41}\u{0c17}\u{0c41}"]),
    ("ta", &["tamil", "kollywood", "\u{0ba4}\u{0bae}\u{0bbf}\u{0bb4}\u{0bcd}"]),
    ("kn", &["kannada", "sandalwood", "\u{0c95}\u{0ca8}\u{0ccd}\u{0ca8}\u{0ca1}"]),
    ("hi", &["hindi", "bollywood", "\u{0939}\u{093f}\u{0928}\u{094d}\u{0926}\u{0940}"]),
    ("ne", &["nepali", "nepal", "\u{0928}\u{0947}\u{092a}\u{093e}\u{0932}\u{0940}", "\u{0928}\u{0947}\u{092a}\u{093e}\u{0932}"]),
    ("ko", &["korean", "k-pop", "kpop", "\u{d55c}\u{ad6d}", "\u{d55c}\u{ad6d}\u{c5b4}"]),
    ("ml", &["malayalam", "mollywood"]),
    ("bn", &["bengali", "bangla"]),
    ("pa", &["punjabi", "panjabi"]),
    ("ja", &["japanese", "j-pop", "jpop"]),
    ("ar", &["arabic"]),
    ("ru", &["russian"]),
];

const DEVANAGARI_LANGS: &[(&str, &[&str])] = &[
    ("ne", &["nepali", "nepal", "\u{0928}\u{0947}\u{092a}\u{093e}\u{0932}\u{0940}", "\u{0928}\u{0947}\u{092a}\u{093e}\u{0932}", "nepali song", "lok dohori"]),
    ("mr", &["marathi", "\u{092e}\u{0930}\u{093e}\u{0920}\u{0940}"]),
    ("hi", &["hindi", "\u{0939}\u{093f}\u{0928}\u{094d}\u{0926}\u{0940}", "bollywood"]),
];

pub fn title_lang(title: &str) -> Option<&'static str> {
    let t = title.to_lowercase();
    let mut best: Option<&'static str> = None;
    let mut best_len = 0;
    for (lang, kws) in TITLE_KEYWORDS {
        for kw in *kws {
            if t.contains(kw) && kw.len() > best_len {
                best = Some(lang);
                best_len = kw.len();
            }
        }
    }
    best
}

/// Returns (lang_code, script). Port of lang.py::detect_language.
pub fn detect_language(title: &str, lyrics: &str) -> (String, String) {
    let tlang = title_lang(title);
    let counts = script_counts(lyrics);
    let total_alpha: usize = counts.values().sum();
    let non_latin: HashMap<&str, usize> = counts
        .iter()
        .filter(|(k, _)| **k != "Latin")
        .map(|(k, v)| (*k, *v))
        .collect();

    let mut dominant: Option<&str> = None;
    if !non_latin.is_empty() {
        let sum: usize = non_latin.values().sum();
        if sum >= 3usize.max((0.10 * total_alpha as f64) as usize) {
            dominant = non_latin.iter().max_by_key(|(_, v)| **v).map(|(k, _)| *k);
        }
    } else if !counts.is_empty() {
        dominant = Some("Latin");
    }

    if let Some(d) = dominant {
        if d != "Latin" && non_latin.len() > 1 {
            let top = *non_latin.get(d).unwrap_or(&0);
            let rivals: Vec<&str> = non_latin
                .iter()
                .filter(|(_, v)| **v >= (0.8 * top as f64) as usize)
                .map(|(k, _)| *k)
                .collect();
            if rivals.len() > 1 {
                if let Some(tl) = tlang {
                    let tl_script = lang_to_script(tl);
                    if let Some(ts) = tl_script {
                        if rivals.contains(&ts) {
                            dominant = Some(ts);
                        }
                    }
                }
            }
        }
    }

    match dominant {
        None => {
            if let Some(tl) = tlang {
                return (tl.to_string(), lang_to_script(tl).unwrap_or("Latin").to_string());
            }
            ("other".into(), "Latin".into())
        }
        Some("Latin") => {
            if let Some(tl) = tlang {
                (tl.to_string(), lang_to_script(tl).unwrap_or("Latin").to_string())
            } else {
                ("other".into(), "Latin".into())
            }
        }
        Some("Devanagari") => {
            let low = title.to_lowercase();
            for (lang, kws) in DEVANAGARI_LANGS {
                if kws.iter().any(|k| low.contains(k)) {
                    return (lang.to_string(), "Devanagari".into());
                }
            }
            ("hi".into(), "Devanagari".into())
        }
        Some(other) => {
            let mut lang = script_table_lang(other);
            if let Some(tl) = tlang {
                if tl != lang && lang_to_script(tl) == Some(other) {
                    lang = tl;
                }
            }
            (lang.to_string(), other.to_string())
        }
    }
}

fn script_table_lang(script: &str) -> &'static str {
    SCRIPT_TABLE
        .iter()
        .find(|(s, _, _l)| *s == script)
        .map(|(_, _, l)| *l)
        .unwrap_or("other")
}

fn lang_to_script(lang: &str) -> Option<&'static str> {
    Some(match lang {
        "te" => "Telugu",
        "ta" => "Tamil",
        "kn" => "Kannada",
        "hi" | "ne" | "mr" => "Devanagari",
        "ko" => "Hangul",
        "ml" => "Malayalam",
        "bn" => "Bengali",
        "pa" => "Gurmukhi",
        "ja" => "Japanese",
        "ar" => "Arabic",
        "ru" => "Cyrillic",
        _ => return None,
    })
}

// ---------- tenglish / hinglish romanize (port of lyrics.py maps) ----------

const TE_VOWELS: &[(u32, &str)] = &[
    (0x0C05, "a"), (0x0C06, "aa"), (0x0C07, "i"), (0x0C08, "ee"),
    (0x0C09, "u"), (0x0C0A, "oo"), (0x0C0B, "ru"), (0x0C0E, "e"),
    (0x0C0F, "E"), (0x0C10, "ai"), (0x0C11, "o"), (0x0C12, "O"), (0x0C13, "au"),
];
const TE_SIGNS: &[(u32, &str)] = &[
    (0x0C3E, "aa"), (0x0C3F, "i"), (0x0C40, "ee"), (0x0C41, "u"), (0x0C42, "oo"),
    (0x0C43, "ru"), (0x0C44, "ru"), (0x0C46, "e"), (0x0C47, "E"), (0x0C48, "ai"),
    (0x0C4A, "o"), (0x0C4B, "O"), (0x0C4C, "au"),
];
const TE_CONS: &[(u32, &str)] = &[
    (0x0C15, "k"), (0x0C16, "kh"), (0x0C17, "g"), (0x0C18, "gh"), (0x0C19, "ng"),
    (0x0C1A, "ch"), (0x0C1B, "chh"), (0x0C1C, "j"), (0x0C1D, "jh"), (0x0C1E, "ny"),
    (0x0C1F, "T"), (0x0C20, "Th"), (0x0C21, "D"), (0x0C22, "Dh"), (0x0C23, "N"),
    (0x0C24, "t"), (0x0C25, "th"), (0x0C26, "d"), (0x0C27, "dh"), (0x0C28, "n"),
    (0x0C2A, "p"), (0x0C2B, "ph"), (0x0C2C, "b"), (0x0C2D, "bh"), (0x0C2E, "m"),
    (0x0C2F, "y"), (0x0C30, "r"), (0x0C31, "R"), (0x0C32, "l"), (0x0C33, "L"),
    (0x0C34, "zh"), (0x0C35, "v"), (0x0C36, "sh"), (0x0C37, "Sh"), (0x0C38, "s"),
    (0x0C39, "h"),
];
const TE_OTHER: &[(u32, &str)] = &[(0x0C01, "m"), (0x0C02, "m"), (0x0C03, "h")];
const TE_VIRAMA: u32 = 0x0C4D;

const DEV_VOWELS: &[(u32, &str)] = &[
    (0x0905, "a"), (0x0906, "aa"), (0x0907, "i"), (0x0908, "ee"), (0x0909, "u"),
    (0x090A, "oo"), (0x090B, "ri"), (0x090E, "e"), (0x090F, "e"), (0x0910, "ai"),
    (0x0913, "o"), (0x0914, "au"),
];
const DEV_SIGNS: &[(u32, &str)] = &[
    (0x093E, "aa"), (0x093F, "i"), (0x0940, "ee"), (0x0941, "u"), (0x0942, "oo"),
    (0x0943, "ri"), (0x0947, "e"), (0x0948, "ai"), (0x094B, "o"), (0x094C, "au"),
];
const DEV_CONS: &[(u32, &str)] = &[
    (0x0915, "k"), (0x0916, "kh"), (0x0917, "g"), (0x0918, "gh"), (0x0919, "ng"),
    (0x091A, "ch"), (0x091B, "chh"), (0x091C, "j"), (0x091D, "jh"), (0x091E, "ny"),
    (0x091F, "T"), (0x0920, "Th"), (0x0921, "D"), (0x0922, "Dh"), (0x0923, "N"),
    (0x0924, "t"), (0x0925, "th"), (0x0926, "d"), (0x0927, "dh"), (0x0928, "n"),
    (0x0929, "n"), (0x092A, "p"), (0x092B, "ph"), (0x092C, "b"), (0x092D, "bh"),
    (0x092E, "m"), (0x092F, "y"), (0x0930, "r"), (0x0931, "r"), (0x0932, "l"),
    (0x0933, "L"), (0x0934, "L"), (0x0935, "v"), (0x0936, "sh"), (0x0937, "Sh"),
    (0x0938, "s"), (0x0939, "h"),
];
const DEV_OTHER: &[(u32, &str)] = &[(0x0901, "m"), (0x0902, "m"), (0x0903, "h")];
const DEV_VIRAMA: u32 = 0x094D;

fn map_get(m: &'static [(u32, &'static str)], cp: u32) -> Option<&'static str> {
    m.iter().find(|(c, _)| *c == cp).map(|(_, s)| *s)
}

/// Telugu -> tenglish. Port of lyrics.py::telugu_to_latin (same assembly rules).
pub fn telugu_to_latin(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let n = chars.len();
    while i < n {
        let cp = chars[i] as u32;
        if let Some(c) = map_get(TE_CONS, cp) {
            out.push_str(c);
            if i + 1 < n {
                let ncp = chars[i + 1] as u32;
                if let Some(vs) = map_get(TE_SIGNS, ncp) {
                    out.push_str(vs);
                    i += 2;
                    continue;
                }
                if ncp == TE_VIRAMA {
                    i += 2;
                    continue;
                }
            }
            out.push('a');
            i += 1;
            continue;
        } else if let Some(v) = map_get(TE_VOWELS, cp) {
            out.push_str(v);
            i += 1;
            continue;
        } else if let Some(vs) = map_get(TE_SIGNS, cp) {
            out.push_str(vs);
            i += 1;
            continue;
        } else if cp == TE_VIRAMA {
            i += 1;
            continue;
        } else if let Some(o) = map_get(TE_OTHER, cp) {
            out.push_str(o);
            i += 1;
            continue;
        } else if (0x0C66..=0x0C6F).contains(&cp) {
            out.push(char::from_digit(cp - 0x0C66, 10).unwrap());
            i += 1;
            continue;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// Devanagari -> hinglish. Port of lyrics.py::devanagari_to_latin (word-final
/// schwa deleted, kept mid-word). Nukta (ड़ ज़) replaces the PRECEDING
/// consonant token via the nukta map — token-based assembly, never char pops
/// (maps emit multi-char strings like "kh").
pub fn devanagari_to_latin(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut tokens: Vec<String> = Vec::new();
    let mut last_cons_cp: Option<u32> = None;
    let mut i = 0;
    let n = chars.len();
    while i < n {
        let cp = chars[i] as u32;
        if cp == 0x20 {
            tokens.push(" ".into());
            last_cons_cp = None;
            i += 1;
            continue;
        }
        if cp == 0x093C {
            // nukta: rewrite the last consonant token
            if let (Some(lcp), Some(_)) = (last_cons_cp, tokens.last()) {
                if let Some(repl) = match lcp {
                    0x0915 => Some("q"), 0x0916 => Some("kh"), 0x0917 => Some("g"),
                    0x091C => Some("z"), 0x0921 => Some("D"), 0x0922 => Some("Dh"),
                    0x092B => Some("f"), 0x092F => Some("y"),
                    _ => None,
                } {
                    tokens.pop();
                    tokens.push(repl.to_string());
                }
            }
            last_cons_cp = None;
            i += 1;
            continue;
        }
        if let Some(c) = map_get(DEV_CONS, cp) {
            tokens.push(c.to_string());
            last_cons_cp = Some(cp);
            if i + 1 < n {
                let ncp = chars[i + 1] as u32;
                if let Some(vs) = map_get(DEV_SIGNS, ncp) {
                    tokens.push(vs.to_string());
                    last_cons_cp = None;
                    i += 2;
                    continue;
                }
                if ncp == DEV_VIRAMA {
                    last_cons_cp = None;
                    i += 2;
                    continue;
                }
            }
            // inherent 'a': keep mid-word, delete at word end
            let word_end = i + 1 >= n || chars[i + 1] == ' ';
            if !word_end {
                tokens.push("a".into());
            }
            i += 1;
            continue;
        } else if let Some(v) = map_get(DEV_VOWELS, cp) {
            tokens.push(v.to_string());
            last_cons_cp = None;
            i += 1;
            continue;
        } else if let Some(vs) = map_get(DEV_SIGNS, cp) {
            tokens.push(vs.to_string());
            last_cons_cp = None;
            i += 1;
            continue;
        } else if cp == DEV_VIRAMA {
            last_cons_cp = None;
            i += 1;
            continue;
        } else if let Some(o) = map_get(DEV_OTHER, cp) {
            tokens.push(o.to_string());
            last_cons_cp = None;
            i += 1;
            continue;
        } else {
            tokens.push(chars[i].to_string());
            last_cons_cp = None;
            i += 1;
        }
    }
    tokens.concat()
}

/// Romanize by detected script (tenglish for Telugu, hinglish for Devanagari).
pub fn romanize(text: &str) -> String {
    let (_, script) = detect_language("", text);
    match script.as_str() {
        "Telugu" => telugu_to_latin(text),
        "Devanagari" => devanagari_to_latin(text),
        _ => text.to_string(),
    }
}

// ---------- title cleaning (port of legacy clean_title) ----------

pub fn clean_title(title: &str) -> String {
    let t: String = title
        .split('|')
        .next()
        .unwrap_or(title)
        .trim_matches(|c| c == '"' || c == '\'' || c == ' ')
        .to_string();
    let t = regex_noop(&t);
    t
}

// lightweight replacement for the legacy regex pipeline
fn regex_noop(t: &str) -> String {
    let mut words: Vec<&str> = t.split_whitespace().collect();
    let drop = [
        "8k", "4k", "hd", "1080p", "2160p", "video", "song", "lyrical", "lyric",
        "full", "official", "video", "song", "lyrical",
    ];
    words.retain(|w| {
        let lw = w.to_lowercase();
        !drop.contains(&lw.as_str())
    });
    let joined = words.join(" ");
    // strip parenthesized segments: "(Telugu)", "(with Lyrics)"
    let mut out = String::new();
    let mut depth = 0;
    for c in joined.chars() {
        match c {
            '(' => depth += 1,
            ')' => {
                if depth > 0 {
                    depth -= 1
                }
            }
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out.split(" - ").next().unwrap_or(&out).trim().to_string()
}

// ---------- LRCLIB client ----------

#[derive(Debug, Clone)]
pub struct LrcCandidate {
    pub track_name: String,
    pub artist_name: String,
    pub duration: f64,
    pub synced: Option<String>,
    pub plain: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Lyrics {
    pub text: String,
    pub synced: bool,
    pub lines: Vec<(f64, String)>, // parsed synced lines
    pub lang: String,
    pub script: String,
}

/// Parse "[mm:ss.xx] line" LRC text into (time, line) pairs.
pub fn parse_lrc(text: &str) -> Vec<(f64, String)> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if !line.starts_with('[') {
            continue;
        }
        let mut rest = line;
        let mut ts: Option<f64> = None;
        loop {
            if !rest.starts_with('[') {
                break;
            }
            let end = match rest.find(']') {
                Some(e) => e,
                None => break,
            };
            let tag = &rest[1..end];
            rest = &rest[end + 1..];
            let parts: Vec<&str> = tag.split(':').collect();
            if parts.len() == 2 {
                let m: f64 = parts[0].parse().unwrap_or(-1.0);
                let s: f64 = parts[1].parse().unwrap_or(-1.0);
                if m >= 0.0 && s >= 0.0 {
                    ts = Some(m * 60.0 + s);
                }
            }
        }
        if let Some(t) = ts {
            if !rest.trim().is_empty() {
                out.push((t, rest.trim().to_string()));
            }
        }
    }
    out
}

/// Duration gate: |cand_dur - track_dur| <= max(10, 8%).
pub fn duration_ok(cand: f64, track: f64) -> bool {
    if track <= 0.0 {
        return true;
    }
    (cand - track).abs() <= 10.0f64.max(track * 0.08)
}

/// Fetch candidates from LRCLIB by track_name (with up to 3 tries on 429).
pub fn lrclib_search(track_name: &str) -> Result<Vec<LrcCandidate>> {
    let url = format!(
        "https://lrclib.net/api/search?track_name={}",
        urlencode(track_name)
    );
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()?;
    let mut attempt = 0;
    loop {
        attempt += 1;
        let resp = client.get(&url).send()?;
        if resp.status() == 429 && attempt < 3 {
            std::thread::sleep(std::time::Duration::from_millis(800 * attempt));
            continue;
        }
        let body = resp.error_for_status()?.text()?;
        let v: serde_json::Value = serde_json::from_str(&body)?;
        let mut out = Vec::new();
        if let Some(arr) = v.as_array() {
            for it in arr {
                let synced = it.get("syncedLyrics").and_then(|s| s.as_str()).map(String::from);
                let plain = it.get("plainLyrics").and_then(|s| s.as_str()).map(String::from);
                if synced.is_none() && plain.is_none() {
                    continue;
                }
                out.push(LrcCandidate {
                    track_name: it.get("trackName").and_then(|s| s.as_str()).unwrap_or("").to_string(),
                    artist_name: it.get("artistName").and_then(|s| s.as_str()).unwrap_or("").to_string(),
                    duration: it.get("duration").and_then(|d| d.as_f64()).unwrap_or(0.0),
                    synced,
                    plain,
                });
            }
        }
        return Ok(out);
    }
}

/// Artist hints from the title's " | " segments (up to 5, split on ,&/).
pub fn artist_hints(title: &str) -> Vec<String> {
    let mut hints = Vec::new();
    for seg in title.split('|').skip(1).take(5) {
        for part in seg.split([',', '&', '/']) {
            let p = part.trim();
            if !p.is_empty() && p.chars().count() <= 40 {
                hints.push(p.to_string());
            }
        }
    }
    hints
}

/// Pick the best candidate: clean-title or colon-stripped match, artist hints,
/// strict duration gate. Wrong lyrics are worse than none.
pub fn pick_candidate(
    cands: Vec<LrcCandidate>,
    title: &str,
    duration: f64,
) -> Option<LrcCandidate> {
    let clean = clean_title(title);
    let variants = [clean.clone(), clean.replace(':', "")];
    let hints = artist_hints(title);

    let mut best: Option<LrcCandidate> = None;
    let mut best_score = 0i32;
    for c in cands {
        if !duration_ok(c.duration, duration) {
            continue;
        }
        let ct = clean_title(&c.track_name);
        let mut score = 0i32;
        for v in &variants {
            let v = v.to_lowercase();
            if !v.is_empty() && (ct.to_lowercase() == v || ct.to_lowercase().contains(&v)) {
                score += 3;
                break;
            }
        }
        if hints.iter().any(|h| {
            h.to_lowercase() == c.artist_name.to_lowercase()
                || c.artist_name.to_lowercase().contains(&h.to_lowercase())
        }) {
            score += 2;
        }
        if score > best_score {
            best_score = score;
            best = Some(c);
        }
    }
    best
}

/// Full fetch: search + pick + parse. Returns None if nothing passes.
/// PITFALL (hit live): LRCLIB returns [] for raw YouTube titles with parens
/// and " - " segments — always query with the CLEANED title.
pub fn fetch_lyrics(title: &str, duration: f64) -> Option<Lyrics> {
    let query = clean_title(title);
    let cands = lrclib_search(&query).ok()?;
    let cand = pick_candidate(cands, title, duration)?;
    let text = cand.synced.clone().or(cand.plain)?;
    let synced = cand.synced.is_some();
    let lines = if synced { parse_lrc(&text) } else { Vec::new() };
    let (lang, script) = detect_language(title, &text);
    Some(Lyrics {
        text,
        synced,
        lines,
        lang,
        script,
    })
}

pub fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
