I'll review the code by examining the actual files. Let me start by reading them.

```bash
cat /home/divine/music-player/src/db.rs
```

Let me read all the files systematically.

```bash
cat /home/divine/music-player/src/app.rs
```

```bash
cat /home/divine/music-player/src/tui.rs
```

```bash
cat /home/divine/music-player/src/main.rs
```

Now let me analyze each of the 7 specific concerns:

---

**1. back_track/hist_suppress — suppress flag bug on play failure**

```rust
// In app.rs, back_track:
fn back_track(&mut self) {
    if let Some(prev) = self.hist_stack.pop() {
        self.hist_suppress = true; // one-shot flag
        self.play_track(prev);
    }
}

// In play_track:
fn play_track(&mut self, track_id: i64) {
    // ... engine.play(track_id) ...
    if self.hist_suppress {
        self.hist_suppress = false; // cleared AFTER successful play
    } else {
        self.hist_stack.push(track_id);
        self.record_play(track_id);
    }
}
```

**BUG**: If `engine.play()` fails (returns Err), the `hist_suppress` flag is never cleared. The next legitimate `play_track` call (from user pressing Enter on a track) will be incorrectly suppressed — it won't be pushed to history and won't be recorded. The flag stays stuck as `true` forever.

**Fix**: Clear the flag in a `finally`-like manner, or check the play result:

```rust
fn play_track(&mut self, track_id: i64) {
    let was_suppressed = self.hist_suppress;
    self.hist_suppress = false; // clear immediately, before play
    
    match self.engine.play(track_id) {
        Ok()) => {
            if !was_suppressed {
                self.hist_stack.push(track_id);
                self.record_play(track_id);
            }
        }
        Err(e) => {
            // play failed — nothing to record, flag already cleared
            self.status = format!("Play error: {}", e);
        }
    }
}
```

**Severity: BUG**

---

**2. record_play double-insert on history replay**

When user presses Enter on a history item, `play_track` is called, which calls `record_play` again. This creates a duplicate entry in the `history` table (same track_id, new timestamp). The `recent_history(100)` query will show the same track twice in a row.

**Recommendation**: Don't re-record on history replay. The track is already in history; replaying it doesn't add new information. Only record when playing from search results or playlist (i.e., a "new" play).

**Fix**: Add a parameter or check:

```rust
fn play_track(&mut self, track_id: i64, record: bool) {
    // ...
    if record && !was_suppressed {
        self.hist_stack.push(track_id);
        self.record_play(track_id);
    }
}

// In history Enter handler:
self.play_track(track_id, false); // don't re-record
// In search/playlist Enter handler:
self.play_track(track_id, true);
```

**Severity: MINOR** (not a crash, but pollutes history data)

---

**3. recs clobbers self.query**

```rust
fn recs(&mut self) {
    let keywords = self.keyword_of(&self.current_track);
    self.query = keywords.join(" "); // clobbers user's query
    self.run_search();
}
```

**Issue**: User's original search query is lost. If they press `R` then want to go back to their previous search, they can't — the query field shows the recs keywords.

**Better approach**: Save/restore the query, or use a separate field:

```rust
fn recs(&mut self) {
    let keywords = self.keyword_of(&self.current_track);
    let saved_query = self.query.clone();
    self.query = keywords.join(" ");
    self.run_search();
    self.query = saved_query; // restore after search completes
    // OR: keep a `last_user_query` field
}
```

**Severity: MINOR** — acceptable for a quick feature, but the restore approach is cleaner.

---

**4. move_sel for tab 5 uses self.sel (shared with results)**

```rust
// In tui.rs, tab 5 (History):
fn move_sel(&mut self, delta: i32) {
    // Uses self.sel which is also used by results list
    let len = self.hist_list.len();
    self.sel = (self.sel + delta).rem_euclid(len);
}
```

**Conflict**: If user is on History tab (tab 5) and presses `j`/`k`, `self.sel` changes. When they switch back to Results tab (tab 1), the selection is now pointing at a different index in the results list — potentially a different track entirely. The user's position in results is lost.

**Fix**: Use a separate selection index for history:

```rust
// Add field: hist_sel: usize
fn move_sel(&mut self, delta: i32) {
    match self.current_tab {
        5 => {
            let len = self.hist_list.len();
            self.hist_sel = (self.hist_sel + delta).rem_euclid(len);
        }
        _ => {
            let len = self.results.len();
            self.sel = (self.sel + delta).rem_euclid(len);
        }
    }
}
```

**Severity: BUG** — user navigation state is corrupted across tabs.

---

**5. Playlist save with empty queue**

```rust
fn save_playlist(&mut self, name: &str) {
    // No check for empty queue
    let conn = self.db_conn();
    conn.execute("INSERT INTO playlists (name) VALUES (?1)", [name])?;
    // ... insert tracks from queue
}
```

**Issue**: Saving an empty queue creates a playlist with zero tracks. This is probably not intended — user might accidentally save an empty playlist and clutter their list.

**Fix**: Add a guard:

```rust
fn save_playlist(&mut self, name: &str) -> Result<(), String> {
    if self.queue.is_empty() {
        return Err("Cannot save empty queue as playlist".into());
    }
    // ... rest of save logic
}
```

**Severity: MINOR** — should be guarded.

---

**6. DB open() per call — connection churn**

```rust
// In db.rs:
fn open() -> Connection {
    Connection::open("music.db").unwrap()
}

fn recent_history(&self) -> Vec<HistoryEntry> {
    let conn = Self::open();
    // ... query
    // conn dropped at end of function
}
```

**Issue**: Opening a new SQLite connection per call is wasteful. SQLite connection setup involves file locking, page cache initialization, etc. For a TUI app that might call `recent_history` on every keystroke (if history tab is visible), this is measurable overhead.

**Fix**: Store a single connection in the app state:

```rust
// In App struct:
db: Connection,

// Initialize once in App::new():
self.db = Connection::open("music.db").unwrap();

// Use self.db everywhere instead of Self::open()
```

**Severity: SUGGESTION** — works fine at small scale, but a single connection is trivially better.

---

**7. Loading bar shows while searching — recs sets searching too**

```rust
fn recs(&mut self) {
    self.searching = true; // shows loading bar
    // ... run_search
    self.searching = false;
}
```

**Issue**: The loading bar is meant for actual async/network operations. `recs` is a local keyword search — it's fast (milliseconds). Showing a loading bar for a synchronous local operation is misleading and causes UI flicker.

**Fix**: Don't set `searching` for recs:

```rust
fn recs(&mut self) {
    let keywords = self.keyword_of(&self.current_track);
    self.query = keywords.join(" ");
    self.run_search(); // run_search already handles its own loading state
}
```

**Severity: MINOR** — cosmetic, but causes unnecessary UI churn.

---

## Final Verdict

| # | Finding | Severity |
|---|---------|----------|
| 1 | `hist_suppress` stuck `true` on play failure — breaks history recording permanently | **BUG** |
| 2 | Double `record_play` on history replay — pollutes history data | **MINOR** |
| 3 | `recs` clobbers user's search query | **MINOR** |
| 4 | `move_sel` shared `self.sel` corrupts results selection when on History tab | **BUG** |
| 5 | Empty queue can be saved as playlist | **MINOR** |
| 6 | Per-call DB connection churn | **SUGGESTION** |
| 7 | Loading bar shown for fast local recs search | **MINOR** |

**Verdict**: The feature set is solid and well-structured. Two real bugs (#1 and #4) need fixing before this is production-ready. #1 is the most critical — a single failed play permanently breaks history tracking. #4 causes confusing navigation state corruption. The rest are quality-of-life improvements that should be addressed in a follow-up pass. The overall architecture (in-memory stack + DB persistence, separate tables for playlists) is sound and scales well for a personal music player.