//! KALKI database — SQLite: play history/stats + local library.
//! DB at ~/.local/share/kalki/kalki.db

use crate::sources::Track;
use rusqlite::{Connection, params};
use std::path::{Path, PathBuf};

fn db_path() -> PathBuf {
    // KALKI_DB_PATH lets tests point the DB at a temp file (unit tests,
    // selftest-units) without touching the real library.
    if let Ok(p) = std::env::var("KALKI_DB_PATH") {
        return PathBuf::from(p);
    }
    let base = dirs::data_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
    let dir = base.join("kalki");
    let _ = std::fs::create_dir_all(&dir);
    dir.join("kalki.db")
}

fn open() -> Connection {
    let conn = Connection::open(db_path()).expect("open kalki db");
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            uri TEXT, title TEXT, artist TEXT, source TEXT, played_at INTEGER
        );
        CREATE TABLE IF NOT EXISTS library (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            path TEXT UNIQUE, title TEXT, artist TEXT, album TEXT,
            duration REAL, mtime INTEGER
        );
        CREATE TABLE IF NOT EXISTS playlists (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT UNIQUE, created_at INTEGER
        );
        CREATE TABLE IF NOT EXISTS playlist_tracks (
            playlist_id INTEGER, pos INTEGER,
            track_id TEXT, title TEXT, artist TEXT, source TEXT, uri TEXT, duration REAL,
            PRIMARY KEY (playlist_id, pos)
        );
        CREATE INDEX IF NOT EXISTS idx_history_title ON history(title);
        CREATE INDEX IF NOT EXISTS idx_library_title ON library(title);
        CREATE INDEX IF NOT EXISTS idx_pt_pl ON playlist_tracks(playlist_id);",
    )
    .expect("init kalki db");
    // migrations for older DBs (ignore "duplicate column" errors)
    for (col, ty) in [("track_id", "TEXT"), ("duration", "REAL")] {
        let _ = conn.execute(
            &format!("ALTER TABLE history ADD COLUMN {col} {ty}"),
            [],
        );
    }
    conn
}

/// Record a play start (stats / scrobbler-style history).
pub fn record_play(t: &Track) {
    let conn = open();
    let _ = conn.execute(
        "INSERT INTO history (uri, title, artist, source, played_at, track_id, duration)
         VALUES (?1,?2,?3,?4,?5,?6,?7)",
        params![t.uri, t.title, t.artist, t.source, chrono_now(), t.id, t.duration],
    );
}

fn static_source(s: &str) -> &'static str {
    match s {
        "JioSaavn" => "JioSaavn",
        "YouTube" => "YouTube",
        "Deezer" => "Deezer",
        "SoundCloud" => "SoundCloud",
        "Archive" => "Archive",
        "Local" => "Local",
        _ => Box::leak(s.to_string().into_boxed_str()),
    }
}

/// Most recent plays (newest first) — for the History tab + back-navigation.
pub fn recent_history(n: usize) -> Vec<(Track, i64)> {
    let conn = open();
    let mut stmt = conn
        .prepare(
            "SELECT COALESCE(track_id,''), title, artist, source, uri, COALESCE(duration,0), played_at
             FROM history ORDER BY id DESC LIMIT ?1",
        )
        .unwrap();
    let rows = stmt
        .query_map(params![n as i64], |r| {
            let t = crate::sources::Track {
                id: r.get::<_, String>(0)?,
                title: r.get::<_, String>(1)?,
                artist: r.get::<_, String>(2)?,
                source: static_source(&r.get::<_, String>(3)?),
                uri: r.get::<_, String>(4)?,
                duration: r.get::<_, f64>(5)?,
                ..Default::default()
            };
            Ok((t, r.get::<_, i64>(6)?))
        })
        .unwrap();
    rows.filter_map(|r| r.ok()).collect()
}

/// Save the given tracks as a named playlist (replaces if exists).
pub fn save_playlist(name: &str, tracks: &[Track]) -> Result<usize, String> {
    let conn = open();
    let id: i64 = match conn.query_row(
        "SELECT id FROM playlists WHERE name = ?1",
        params![name],
        |r| r.get(0),
    ) {
        Ok(id) => {
            let _ = conn.execute("DELETE FROM playlist_tracks WHERE playlist_id = ?1", params![id]);
            id
        }
        Err(_) => {
            conn.execute(
                "INSERT INTO playlists (name, created_at) VALUES (?1, ?2)",
                params![name, chrono_now()],
            )
            .map_err(|e| e.to_string())?;
            conn.last_insert_rowid()
        }
    };
    let mut n = 0usize;
    for (pos, t) in tracks.iter().enumerate() {
        let ok = conn.execute(
            "INSERT OR REPLACE INTO playlist_tracks
             (playlist_id, pos, track_id, title, artist, source, uri, duration)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![id, pos as i64, t.id, t.title, t.artist, t.source, t.uri, t.duration],
        );
        if ok.is_ok() {
            n += 1;
        }
    }
    Ok(n)
}

/// Load a named playlist into a Vec<Track>.
pub fn load_playlist(name: &str) -> Option<Vec<Track>> {
    let conn = open();
    let id: i64 = conn
        .query_row("SELECT id FROM playlists WHERE name = ?1", params![name], |r| r.get(0))
        .ok()?;
    let mut stmt = conn
        .prepare(
            "SELECT track_id, title, artist, source, uri, duration FROM playlist_tracks
             WHERE playlist_id = ?1 ORDER BY pos",
        )
        .unwrap();
    let rows = stmt
        .query_map(params![id], |r| {
            Ok(Track {
                id: r.get::<_, String>(0)?,
                title: r.get::<_, String>(1)?,
                artist: r.get::<_, String>(2)?,
                source: static_source(&r.get::<_, String>(3)?),
                uri: r.get::<_, String>(4)?,
                duration: r.get::<_, f64>(5)?,
                ..Default::default()
            })
        })
        .unwrap();
    Some(rows.filter_map(|r| r.ok()).collect())
}

/// All playlists: (name, track_count, created_at).
pub fn list_playlists() -> Vec<(String, i64, i64)> {
    let conn = open();
    let mut stmt = conn
        .prepare(
            "SELECT p.name, COUNT(pt.pos), p.created_at FROM playlists p
             LEFT JOIN playlist_tracks pt ON pt.playlist_id = p.id
             GROUP BY p.id ORDER BY p.created_at DESC",
        )
        .unwrap();
    let rows = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap();
    rows.filter_map(|r| r.ok()).collect()
}

/// Append a track to a playlist (creating it if missing).
pub fn add_to_playlist(name: &str, t: &Track) -> Result<usize, String> {
    let conn = open();
    let id: i64 = match conn.query_row(
        "SELECT id FROM playlists WHERE name = ?1",
        params![name],
        |r| r.get(0),
    ) {
        Ok(id) => id,
        Err(_) => {
            conn.execute(
                "INSERT INTO playlists (name, created_at) VALUES (?1, ?2)",
                params![name, chrono_now()],
            )
            .map_err(|e| e.to_string())?;
            conn.last_insert_rowid()
        }
    };
    let pos: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(pos), -1) + 1 FROM playlist_tracks WHERE playlist_id = ?1",
            params![id],
            |r| r.get(0),
        )
        .unwrap_or(0);
    conn.execute(
        "INSERT OR REPLACE INTO playlist_tracks
         (playlist_id, pos, track_id, title, artist, source, uri, duration)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
        params![id, pos, t.id, t.title, t.artist, t.source, t.uri, t.duration],
    )
    .map_err(|e| e.to_string())?;
    Ok(pos as usize + 1)
}

/// Delete a named playlist.
pub fn delete_playlist(name: &str) -> Result<(), String> {
    let conn = open();
    if let Ok(id) = conn.query_row("SELECT id FROM playlists WHERE name = ?1", params![name], |r| r.get::<_, i64>(0)) {
        let _ = conn.execute("DELETE FROM playlist_tracks WHERE playlist_id = ?1", params![id]);
        let _ = conn.execute("DELETE FROM playlists WHERE id = ?1", params![id]);
        Ok(())
    } else {
        Err(format!("playlist not found: {name}"))
    }
}

pub fn top_tracks(n: usize) -> Vec<(String, String, i64)> {
    let conn = open();
    let mut stmt = conn
        .prepare("SELECT title, artist, COUNT(*) c FROM history WHERE title != '' GROUP BY title, artist ORDER BY c DESC, MAX(played_at) DESC LIMIT ?1")
        .unwrap();
    let rows = stmt
        .query_map(params![n as i64], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?))
        })
        .unwrap();
    rows.filter_map(|r| r.ok()).collect()
}

pub fn top_artists(n: usize) -> Vec<(String, i64)> {
    let conn = open();
    let mut stmt = conn
        .prepare("SELECT artist, COUNT(*) c FROM history WHERE artist != '' GROUP BY artist ORDER BY c DESC LIMIT ?1")
        .unwrap();
    let rows = stmt
        .query_map(params![n as i64], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
        .unwrap();
    rows.filter_map(|r| r.ok()).collect()
}

pub fn play_count() -> i64 {
    let conn = open();
    conn.query_row("SELECT COUNT(*) FROM history", [], |r| r.get(0))
        .unwrap_or(0)
}

pub fn library_tracks(n: usize) -> Vec<(String, String)> {
    let conn = open();
    let mut stmt = conn
        .prepare("SELECT title, artist FROM library ORDER BY title LIMIT ?1")
        .unwrap();
    let rows = stmt
        .query_map(params![n as i64], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .unwrap();
    rows.filter_map(|r| r.ok()).collect()
}

pub fn library_count() -> i64 {
    let conn = open();
    conn.query_row("SELECT COUNT(*) FROM library", [], |r| r.get(0))
        .unwrap_or(0)
}

/// Smart playlists as (name, track count) computed from the library.
pub fn smart_playlists() -> Vec<(String, i64)> {
    let conn = open();
    let mut out = Vec::new();
    let queries: &[(&str, &str)] = &[
        ("Recently Added", "SELECT COUNT(*) FROM library ORDER BY mtime DESC"),
        ("Long Tracks (6m+)", "SELECT COUNT(*) FROM library WHERE duration >= 360"),
        ("Short Tracks", "SELECT COUNT(*) FROM library WHERE duration > 0 AND duration <= 150"),
        ("Untitled Tracks", "SELECT COUNT(*) FROM library WHERE title = ''"),
    ];
    for (name, _sql) in queries {
        let n = conn
            .query_row(&format!("{_sql}"), [], |r| r.get::<_, i64>(0))
            .unwrap_or(0);
        out.push((name.to_string(), n));
    }
    let _ = &queries;
    out
}

/// Scan audio dirs (default ~/Music plus extra dirs) into the library table.
/// Returns number of tracks indexed.
pub fn scan_library(extra_dirs: Option<&[PathBuf]>) -> usize {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(music) = dirs::audio_dir() {
        dirs.push(music);
    }
    if let Some(home) = dirs::home_dir() {
        let m = home.join("Music");
        if !dirs.contains(&m) {
            dirs.push(m);
        }
    }
    if let Some(extra) = extra_dirs {
        dirs.extend(extra.iter().cloned());
    }
    let conn = open();
    let mut count = 0;
    for dir in dirs {
        if !dir.exists() {
            continue;
        }
        count += scan_dir(&conn, &dir);
    }
    count
}

fn scan_dir(conn: &Connection, dir: &Path) -> usize {
    let mut count = 0;
    let mut stack = vec![dir.to_path_buf()];
    let exts = ["mp3", "m4a", "flac", "ogg", "opus", "wav", "aac", "wma"];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            let ext = p
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();
            if !exts.contains(&ext.as_str()) {
                continue;
            }
            let mtime = std::fs::metadata(&p)
                .and_then(|m| m.modified())
                .map(|t| t.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0))
                .unwrap_or(0);
            // skip unchanged
            let path_str = p.to_string_lossy().to_string();
            let exists = conn
                .query_row(
                    "SELECT 1 FROM library WHERE path = ?1 AND mtime = ?2",
                    params![path_str, mtime],
                    |r| r.get::<_, i64>(0),
                )
                .is_ok();
            if exists {
                continue;
            }
            let (title, artist, album, dur) = probe(&p);
            let _ = conn.execute(
                "INSERT INTO library (path, title, artist, album, duration, mtime)
                 VALUES (?1,?2,?3,?4,?5,?6)
                 ON CONFLICT(path) DO UPDATE SET title=?2, artist=?3, album=?4, duration=?5, mtime=?6",
                params![path_str, title, artist, album, dur, mtime],
            );
            count += 1;
        }
    }
    count
}

/// Read tags with lofty (pure Rust). Falls back to filename on any error.
fn probe(p: &Path) -> (String, String, String, f64) {
    use lofty::prelude::*;
    let fname = p
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let Ok(tagged) = lofty::probe::Probe::open(p).and_then(|pr| pr.read()) else {
        return (fname, String::new(), String::new(), 0.0);
    };
    let title = tagged
        .primary_tag()
        .and_then(|t| t.title())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or(fname.clone());
    let artist = tagged
        .primary_tag()
        .and_then(|t| t.artist())
        .map(|s| s.to_string())
        .unwrap_or_default();
    let album = tagged
        .primary_tag()
        .and_then(|t| t.album())
        .map(|s| s.to_string())
        .unwrap_or_default();
    let dur = tagged.properties().duration().as_secs_f64();
    (title, artist, album, dur)
}

fn chrono_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
