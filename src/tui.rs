//! KALKI drawing — pure render functions over app state.

use crate::app::{App, Mode, Repeat};
use crate::theme;
use crate::{lyrics, sources::Track, viz};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Tabs, Wrap};

/// Build a real-time spectrum strip (1 row) from live audio bands.
/// Each band maps to a block char; the current playhead is a ◆ marker.
fn spectrum_strip(bands: &[f32; viz::BANDS], dur: f64, pos: f64, w: usize) -> String {
    use viz::BANDS;
    if w == 0 {
        return String::new();
    }
    let ramp = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let maxv = bands.iter().cloned().fold(0.0f32, f32::max).max(0.001);
    let mut out = String::with_capacity(w);
    for i in 0..w {
        let band = (i as f32 / w as f32 * BANDS as f32) as usize % BANDS;
        let v = (bands[band] / maxv).clamp(0.0, 1.0);
        let idx = ((v * 7.0).round() as usize).clamp(0, 7);
        let play_b = if dur > 0.0 {
            ((pos / dur) * (w as f64)).floor() as usize
        } else {
            usize::MAX
        };
        if i == play_b && pos > 0.0 {
            out.push('◆');
        } else {
            out.push(ramp[idx]);
        }
    }
    out
}

/// Map 16 FFT bands onto `cols` display columns (resize-safe).
/// cols <= 16: adjacent bands are AVERAGED into groups (no dropped bars).
/// cols >  16: linear interpolation between band centers (smooth fill).
fn map_bands(disp: &[f32; viz::BANDS], peak: &[f32; viz::BANDS], cols: usize) -> Vec<(f32, f32)> {
    use viz::BANDS;
    let n = BANDS as f64;
    (0..cols)
        .map(|c| {
            let c0 = c as f64 / cols as f64 * n; // start band (fractional)
            let c1 = (c + 1) as f64 / cols as f64 * n;
            if cols <= BANDS {
                let s = c0.floor() as usize;
                let e = (c1.ceil() as usize).min(BANDS).max(s + 1);
                let mut d = 0.0f64;
                let mut p = 0.0f64;
                for i in s..e {
                    d += disp[i] as f64;
                    p += peak[i] as f64;
                }
                let k = (e - s) as f64;
                (d / k, p / k)
            } else {
                // interpolate: band center at (i + 0.5)/n * cols
                let x = c as f64 + 0.5; // column center in column space
                let bi = x / cols as f64 * n - 0.5; // band-space position
                let i0 = bi.floor() as isize;
                let frac = bi - i0 as f64;
                let g = |i: isize| -> (f64, f64) {
                    if i < 0 {
                        (disp[0] as f64, peak[0] as f64)
                    } else if i >= BANDS as isize {
                        (disp[BANDS - 1] as f64, peak[BANDS - 1] as f64)
                    } else {
                        (disp[i as usize] as f64, peak[i as usize] as f64)
                    }
                };
                let (d0, p0) = g(i0);
                let (d1, p1) = g(i0 + 1);
                (
                    d0 + (d1 - d0) * frac,
                    p0 + (p1 - p0) * frac,
                )
            }
        })
        .map(|(d, p)| (d.clamp(0.0, 1.0) as f32, p.clamp(0.0, 1.0) as f32))
        .collect()
}

/// Render a vertical equalizer from the cava-pipeline state (0..1 per band).
/// FRACTIONAL bar tops: the top row uses a partial gradient char (▁..█) from
/// the continuous height, so motion is perfectly smooth — a bar hovering at
/// 3.42 rows shows 3 full █ + a ▄, never a jump between 3 and 4 rows.
/// A thin peak-hold cap '▔' rides just above the bar at a FLOORED row
/// (stable, no jitter). No synthetic wobble — all values trace to audio.
fn eq_render(disp: &[f32; viz::BANDS], peak: &[f32; viz::BANDS], rows: usize, cols: usize) -> Vec<String> {
    let grad = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let bars = map_bands(disp, peak, cols);
    let mut out = vec![String::with_capacity(cols); rows];
    for (_x, &(v, pv)) in bars.iter().enumerate() {
        let h_f = (v * rows as f32).clamp(0.0, rows as f32); // continuous height
        let h_i = h_f.floor() as usize; // full rows below the fractional top
        let frac = h_f - h_i as f32;
        let top_char = if frac <= 0.02 {
            ' '
        } else {
            grad[((frac * 8.0).round() as usize).min(8)]
        };
        let p_i = (pv * rows as f32).clamp(0.0, rows as f32).floor() as usize;
        for r in 0..rows {
            let from_bottom = rows - 1 - r; // row 0 = bottom
            let ch = if from_bottom < h_i {
                '█'
            } else if from_bottom == h_i {
                top_char
            } else if from_bottom == p_i && pv > 0.03 {
                '▔'
            } else {
                ' '
            };
            out[r].push(ch);
        }
    }
    out
}

pub const TABS: [&str; 6] = ["1 Results", "2 Queue", "3 Library", "4 Stats", "5 Playlists", "6 History"];

fn fmt_time(s: f64) -> String {
    if !s.is_finite() || s < 0.0 {
        return "0:00".into();
    }
    let t = s as u64;
    format!("{}:{:02}", t / 60, t % 60)
}

fn track_line(t: &Track, selected: bool) -> Line<'static> {
    let dur = if t.duration > 0.0 {
        format!(" ({:.0}s)", t.duration)
    } else {
        String::new()
    };
    let src_color = match t.source {
        "JioSaavn" => theme::BRIGHT,
        "YouTube" => theme::ACCENT,
        "Deezer" => theme::WARN,
        "SoundCloud" => theme::ACCENT,
        _ => theme::DIM,
    };
    let text = format!("{}  {:8}  {}{dur}", if selected { "▶" } else { " " }, t.source, t.title);
    let artist = if t.artist.is_empty() {
        String::new()
    } else {
        format!(" — {}", t.artist)
    };
    Line::from(vec![
        Span::styled(text, Style::default().fg(theme::TEXT)),
        Span::styled(artist, Style::default().fg(theme::DIM)),
        Span::styled(
            format!("  [{}]", t.source),
            Style::default().fg(src_color),
        ),
    ])
}

pub fn draw(app: &mut App, f: &mut Frame) {
    let area = f.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1), Constraint::Length(1)])
        .split(area);
    draw_brand_bar(app, f, chunks[0]);
    draw_middle(app, f, chunks[1]);
    draw_bottom(app, f, chunks[2]);
    if app.mode == Mode::Help {
        draw_help(f, area);
    }
}

fn draw_brand_bar(app: &App, f: &mut Frame, area: Rect) {
    let state = app.state_word();
    let state_color = match state {
        "PLAYING" => theme::BRIGHT,
        "PAUSED" => theme::WARN,
        _ => theme::DIM,
    };
    let mut icons = String::new();
    if app.shuffle {
        icons.push_str(" [SHUF]");
    }
    match app.repeat {
        crate::app::Repeat::All => icons.push_str(" [REP-ALL]"),
        crate::app::Repeat::One => icons.push_str(" [REP-1]"),
        _ => {}
    }
    if let Some(until) = app.sleep_until {
        let left = until.saturating_duration_since(std::time::Instant::now());
        icons.push_str(&format!(" [SLEEP {}m]", left.as_secs() / 60));
    }
    let line = Line::from(vec![
        Span::styled(
            format!("♪ {}", crate::BRAND),
            Style::default().fg(theme::ACCENT).bold(),
        ),
        Span::raw("   "),
        Span::styled(state, Style::default().fg(state_color)),
        Span::styled(icons, Style::default().fg(theme::DIM)),
    ]);
    f.render_widget(Paragraph::new(line).style(Style::default().bg(theme::BG)), area);
}

fn draw_middle(app: &mut App, f: &mut Frame, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(38), Constraint::Percentage(62)])
        .split(area);
    draw_now_playing(app, f, chunks[0]);
    draw_panel(app, f, chunks[1]);
}

fn draw_now_playing(app: &App, f: &mut Frame, area: Rect) {
    let panel = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::BORDER))
        .title(Span::styled(" NOW PLAYING ", Style::default().fg(theme::ACCENT)))
        .style(Style::default().bg(theme::PANEL));
    let inner = panel.inner(area);
    f.render_widget(panel, area);

    let title = app.current.as_ref().map(|t| t.title.clone()).unwrap_or_else(|| "nothing playing".into());
    let artist = app.current.as_ref().map(|t| t.artist.clone()).unwrap_or_default();
    let album = app.current.as_ref().map(|t| t.album.clone()).unwrap_or_default();
    let pos = app.display_pos;
    let dur = app
        .current
        .as_ref()
        .map(|t| t.duration)
        .unwrap_or_else(|| app.engine.as_ref().map(|e| e.state.lock().unwrap().duration).unwrap_or(0.0));

    let pct = if dur > 0.0 { (pos / dur).clamp(0.0, 1.0) } else { 0.0 };
    let w = inner.width as usize;
    let bar_w = w.saturating_sub(2).max(4);
    // COOL progress bar: gradient blue→cyan with a glowing head at the play
    // position + a live shimmer sweeping the played segment (matches the
    // loading-bar aesthetic — no more static "─ line with a ball").
    let t_bar = app.music_visual.clock();
    let bar_spans = progress_bar(bar_w, pct, t_bar);

    // ---- FIXED layout: header(6) + bordered EQ box + strip + lyrics ----
    // The old code pushed everything into ONE wrapping paragraph, so a
    // wrapped title or variable EQ height shifted the whole block in Y.
    // Now each section owns a fixed Rect — the EQ box NEVER moves.
    let mode_icon = match app.repeat {
        Repeat::Off => "🔁 OFF",
        Repeat::All => "🔁 ALL",
        Repeat::One => "🔂 ONE",
    };
    let shuffle_icon = if app.shuffle { "🔀" } else { "" };
    let header = vec![
        Line::from(Span::styled(
            title,
            Style::default().fg(theme::TEXT).bold(),
        )),
        Line::from(Span::styled(artist, Style::default().fg(theme::DIM))),
        Line::from(Span::styled(album, Style::default().fg(theme::DIM))),
        Line::raw(""),
        Line::from(bar_spans),
        Line::from(vec![
            Span::styled(
                format!("{} / {}", fmt_time(pos), fmt_time(dur)),
                Style::default().fg(theme::BRIGHT),
            ),
            Span::styled(format!("   {mode_icon} {shuffle_icon}"), Style::default().fg(theme::WARN)),
        ]),
    ];
    let header_h = 6u16;
    f.render_widget(
        Paragraph::new(header).wrap(Wrap { trim: false }),
        Rect::new(inner.x, inner.y, inner.width, header_h),
    );

    // ---- EQ in a standard bordered box, bars animating inside ----
    let (disp, peak) = app.eq_bars();
    let map_w = (inner.width as usize).saturating_sub(2).max(8);
    let eq_rows = (inner.height as usize).saturating_sub(12).clamp(4, 14) as u16;
    let eq_box = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::BORDER))
        .title(Span::styled(" EQ ", Style::default().fg(theme::DIM)));
    let eq_area = Rect::new(inner.x, inner.y + header_h, inner.width, eq_rows + 2);
    f.render_widget(eq_box.clone(), eq_area);
    let eq_inner = eq_box.inner(eq_area);
    let eq_lines: Vec<Line> = if app.spectrum.is_live() {
        eq_render(disp, peak, eq_inner.height as usize, eq_inner.width as usize)
            .into_iter()
            .map(|s| Line::from(Span::styled(s, Style::default().fg(theme::ACCENT))))
            .collect()
    } else {
        vec![Line::raw(""); eq_inner.height as usize]
    };
    f.render_widget(Paragraph::new(eq_lines), eq_inner);

    // ---- strip below the EQ box ----
    let strip_y = inner.y + header_h + eq_rows + 2;
    let strip_line = if app.spectrum.is_live() {
        Line::from(Span::styled(
            format!("♪ {}", spectrum_strip(disp, dur, pos, map_w)),
            Style::default().fg(theme::BRIGHT),
        ))
    } else {
        Line::from(Span::styled(
            format!("♪ {}", "▁".repeat(map_w)),
            Style::default().fg(theme::DIM),
        ))
    };
    f.render_widget(
        Paragraph::new(strip_line),
        Rect::new(inner.x, strip_y, inner.width, 1),
    );

    // ---- lyrics window (owns the remaining space) ----
    let lyrics_area = Rect::new(
        inner.x,
        strip_y + 1,
        inner.width,
        inner.y + inner.height - strip_y - 1,
    );
    let mut ly: Vec<Line> = Vec::new();
    if let Some(l) = &app.lyrics {
        let offset = app.sync_for_current();
        let display_lines: Vec<(f64, String)> = if l.synced {
            // window centered on current line, return (time, text) for highlighting
            let cur = l
                .lines
                .iter()
                .filter(|(t, _)| *t <= pos + offset)
                .map(|(t, _)| *t)
                .last();
            let cur_idx = cur
                .and_then(|t| l.lines.iter().position(|(lt, _)| *lt == t))
                .unwrap_or(0);
            let window = 3usize.max(lyrics_area.height as usize / 2);
            let start = cur_idx.saturating_sub(window);
            l.lines
                .iter()
                .skip(start)
                .take(window * 2 + 1)
                .map(|(t, s)| (*t, s.clone()))
                .collect()
        } else {
            l.text
                .lines()
                .enumerate()
                .map(|(i, s)| (i as f64 * 3.0, s.to_string())) // fallback spacing
                .collect()
        };
        ly.push(Line::raw(""));
        ly.push(Line::from(Span::styled(
            if l.synced {
                format!("LYRICS [{}] (t: tenglish, z/x: sync)", l.lang)
            } else {
                format!("LYRICS [{}] plain (no sync)", l.lang)
            },
            Style::default().fg(theme::DIM),
        )));
        // current line index for highlight
        let cur_time = pos + offset;
        let cur_idx = if l.synced {
            display_lines
                .iter()
                .position(|(t, _)| *t > cur_time)
                .map(|i| i.saturating_sub(1))
                .unwrap_or(display_lines.len().saturating_sub(1))
        } else {
            (cur_time / 3.0).floor() as usize
        };
        for (i, (_t, s)) in display_lines.into_iter().enumerate() {
            let shown = if app.romanize {
                lyrics::romanize(&s)
            } else {
                s
            };
            let style = if i == cur_idx {
                Style::default().fg(theme::BRIGHT).bold() // highlight current line
            } else {
                Style::default().fg(theme::TEXT)
            };
            ly.push(Line::from(Span::styled(shown, style)));
        }
    }
    f.render_widget(Paragraph::new(ly).wrap(Wrap { trim: false }), lyrics_area);
}

fn draw_panel(app: &mut App, f: &mut Frame, area: Rect) {
    let tabs = Tabs::new(TABS.to_vec())
        .select(app.active_tab)
        .style(Style::default().fg(theme::DIM))
        .highlight_style(Style::default().fg(theme::BG).bg(theme::ACCENT).bold())
        .divider("  ");
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::BORDER))
        .title(Span::styled(" LIBRARY ", Style::default().fg(theme::BRIGHT)));
    let inner = block.inner(area);
    f.render_widget(block, area);
    f.render_widget(tabs, Rect::new(inner.x, inner.y, inner.width, 1));
    // reserve bottom space for the animated music visual (taller = bigger DNA)
    let visual_h = if inner.height >= 26 {
        16
    } else if inner.height >= 18 {
        12
    } else {
        0
    };
    let body = Rect::new(
        inner.x,
        inner.y + 1,
        inner.width,
        inner.height.saturating_sub(1 + visual_h),
    );
    if visual_h > 0 {
        let vis_area = Rect::new(
            inner.x,
            inner.y + 1 + body.height,
            inner.width,
            visual_h,
        );
        // render the animated music visual
        let visual_lines = app.music_visual.render(vis_area.width, vis_area.height);
        f.render_widget(Paragraph::new(visual_lines), vis_area);
    }

    match app.active_tab {
        0 => draw_results(app, f, body),
        1 => draw_queue(app, f, body),
        2 => draw_library(app, f, body),
        3 => draw_stats(app, f, body),
        4 => draw_playlists(app, f, body),
        5 => draw_history(app, f, body),
        _ => {}
    }
}

fn draw_results(app: &App, f: &mut Frame, area: Rect) {
    if app.results.is_empty() {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "no results — press / to search",
                Style::default().fg(theme::DIM),
            ))),
            area,
        );
        return;
    }
    let (start, vis) = window(app.sel, app.results.len(), area.height as usize);
    let items: Vec<ListItem> = app.results[start..start + vis]
        .iter()
        .enumerate()
        .map(|(i, t)| ListItem::new(track_line(t, start + i == app.sel)))
        .collect();
    let list = List::new(items).highlight_style(Style::default().fg(theme::BG).bg(theme::ACCENT));
    // selection moves WITH app.sel (was pinned at 0 → the blue bar never
    // moved; user's "static blue bar" complaint)
    let sel_in_list = app.sel.saturating_sub(start).min(vis.saturating_sub(1));
    f.render_stateful_widget(list, area, &mut ratatui::widgets::ListState::default().with_selected(Some(sel_in_list)));
}

fn draw_queue(app: &App, f: &mut Frame, area: Rect) {
    if app.queue.is_empty() {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "queue is empty — select a result and press a",
                Style::default().fg(theme::DIM),
            ))),
            area,
        );
        return;
    }
    let (start, vis) = window(app.qsel, app.queue.len(), area.height as usize);
    let items: Vec<ListItem> = app.queue[start..start + vis]
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let idx = start + i;
            let mut line = track_line(t, idx == app.qsel);
            let mut spans = line.spans.clone();
            let now = app.queue_pos == Some(idx);
            let prefix = if now { "▶ " } else { "  " };
            spans.insert(
                0,
                Span::styled(prefix, Style::default().fg(theme::BRIGHT)),
            );
            line = Line::from(spans);
            ListItem::new(line)
        })
        .collect();
    let list = List::new(items).highlight_style(Style::default().fg(theme::BG).bg(theme::ACCENT));
    let sel_in_list = app.qsel.saturating_sub(start).min(vis.saturating_sub(1));
    f.render_stateful_widget(list, area, &mut ratatui::widgets::ListState::default().with_selected(Some(sel_in_list)));
}

fn draw_library(_app: &App, f: &mut Frame, area: Rect) {
    let tracks = crate::db::library_tracks(200);
    if tracks.is_empty() {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "library empty — run :scan to index ~/Music",
                Style::default().fg(theme::DIM),
            ))),
            area,
        );
        return;
    }
    let items: Vec<ListItem> = tracks
        .iter()
        .take(area.height as usize)
        .map(|(title, artist)| {
            ListItem::new(Line::from(vec![
                Span::styled(title, Style::default().fg(theme::TEXT)),
                Span::styled(format!(" — {artist}"), Style::default().fg(theme::DIM)),
            ]))
        })
        .collect();
    f.render_widget(List::new(items), area);
}

fn draw_stats(_app: &App, f: &mut Frame, area: Rect) {
    let total = crate::db::library_count();
    let plays = crate::db::play_count();
    let top = crate::db::top_tracks(10);
    let mut lines = vec![Line::from(Span::styled(
        "YOUR STATS",
        Style::default().fg(theme::ACCENT).bold(),
    ))];
    lines.push(Line::from(vec![
        Span::styled(
            format!("{total} tracks in library   "),
            Style::default().fg(theme::TEXT),
        ),
        Span::styled(format!("{plays} plays logged"), Style::default().fg(theme::WARN)),
    ]));
    lines.push(Line::raw(""));
    if top.is_empty() {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "no plays recorded yet — play something!",
                Style::default().fg(theme::DIM),
            ))),
            area,
        );
        return;
    }
    let mut lines = vec![Line::from(Span::styled(
        "TOP TRACKS",
        Style::default().fg(theme::ACCENT).bold(),
    ))];
    for (i, (title, artist, n)) in top.iter().enumerate() {
        lines.push(Line::from(vec![
            Span::styled(
                format!("{:2}. ", i + 1),
                Style::default().fg(theme::BRIGHT),
            ),
            Span::styled(title, Style::default().fg(theme::TEXT)),
            Span::styled(format!(" — {artist}"), Style::default().fg(theme::DIM)),
            Span::styled(format!("  ×{n}"), Style::default().fg(theme::WARN)),
        ]));
    }
    let artists = crate::db::top_artists(5);
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "TOP ARTISTS",
        Style::default().fg(theme::ACCENT).bold(),
    )));
    for (i, (a, n)) in artists.iter().enumerate() {
        lines.push(Line::from(vec![
            Span::styled(
                format!("{:2}. ", i + 1),
                Style::default().fg(theme::BRIGHT),
            ),
            Span::styled(a, Style::default().fg(theme::TEXT)),
            Span::styled(format!("  ×{n}"), Style::default().fg(theme::WARN)),
        ]));
    }
    f.render_widget(Paragraph::new(lines), area);
}

fn draw_playlists(_app: &App, f: &mut Frame, area: Rect) {
    let mut lines: Vec<Line> = vec![Line::from(Span::styled(
        "YOUR PLAYLISTS  (:pl save|load|add|delete|list|now <name>)",
        Style::default().fg(theme::ACCENT).bold(),
    ))];
    let named = crate::db::list_playlists();
    if named.is_empty() {
        lines.push(Line::from(Span::styled(
            "  none yet — :pl save <name>  (saves the current queue)",
            Style::default().fg(theme::DIM),
        )));
    }
    for (name, count, _created) in named.iter() {
        lines.push(Line::from(vec![
            Span::styled("♪ ", Style::default().fg(theme::BRIGHT)),
            Span::styled(name, Style::default().fg(theme::TEXT)),
            Span::styled(format!("  ({count} tracks)"), Style::default().fg(theme::DIM)),
        ]));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "SMART PLAYLISTS (from stats)",
        Style::default().fg(theme::ACCENT).bold(),
    )));
    let pls = crate::db::smart_playlists();
    if pls.is_empty() {
        lines.push(Line::from(Span::styled(
            "  none — :scan first",
            Style::default().fg(theme::DIM),
        )));
    }
    for (name, count) in pls.iter() {
        lines.push(Line::from(vec![
            Span::styled("✦ ", Style::default().fg(theme::BRIGHT)),
            Span::styled(name, Style::default().fg(theme::TEXT)),
            Span::styled(format!("  ({count} tracks)"), Style::default().fg(theme::DIM)),
        ]));
    }
    f.render_widget(Paragraph::new(lines), area);
}

/// History tab: most recently played tracks (newest first), Enter replays.
fn draw_history(app: &App, f: &mut Frame, area: Rect) {
    if app.hist_list.is_empty() {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "no history yet — play something!",
                Style::default().fg(theme::DIM),
            ))),
            area,
        );
        return;
    }
    let items: Vec<ListItem> = app
        .hist_list
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let prefix = if i == app.hist_sel { "▶ " } else { "  " };
            ListItem::new(Line::from(vec![
                Span::styled(prefix, Style::default().fg(theme::BRIGHT)),
                Span::styled(&t.title, Style::default().fg(theme::TEXT)),
                Span::styled(format!(" — {}", t.artist), Style::default().fg(theme::DIM)),
                Span::styled(format!("  [{}]", t.source), Style::default().fg(theme::WARN)),
            ]))
        })
        .collect();
    f.render_widget(List::new(items).highlight_symbol(""), area);
}

fn window(sel: usize, len: usize, height: usize) -> (usize, usize) {
    let vis = height.max(1);
    if len <= vis {
        return (0, len);
    }
    let start = sel.saturating_sub(vis / 2).min(len - vis);
    (start, vis)
}

/// COOL seek progress bar: the played segment is a blue→cyan gradient with a
/// bright head cell at the play position and a live shimmer sweeping through
/// it — alive like the loading bar, not a static "─ line with a ball".
fn progress_bar(width: usize, pct: f64, t: f64) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::with_capacity(width);
    if width == 0 {
        return spans;
    }
    let filled = ((pct * width as f64) as usize).min(width);
    let (tb, tg, tr) = (0x1E, 0x5A, 0xC8); // deep blue
    let (hb, hg, hr) = (0xBF, 0xF0, 0xFF); // cyan-white
    let shim = ((t * 2.0) % 1.0) * filled.max(1) as f64; // sweeping highlight
    for x in 0..width {
        if x >= filled {
            spans.push(Span::styled(
                "─".to_string(),
                Style::default().fg(theme::DIM),
            ));
            continue;
        }
        let k = if filled > 1 {
            x as f64 / (filled - 1) as f64
        } else {
            1.0
        };
        let dist = (x as f64 - shim).abs();
        let glow = if dist < 2.0 { (1.0 - dist / 2.0) * 0.45 } else { 0.0 };
        let k2 = (k + glow).clamp(0.0, 1.0);
        let (b, g, r) = (
            (tb as f64 + (hb - tb) as f64 * k2) as u8,
            (tg as f64 + (hg - tg) as f64 * k2) as u8,
            (tr as f64 + (hr - tr) as f64 * k2) as u8,
        );
        let ch = if x == filled - 1 && filled < width { '█' } else { '━' };
        spans.push(Span::styled(
            ch.to_string(),
            Style::default().fg(Color::Rgb(b, g, r)).bold(),
        ));
    }
    spans
}

/// COOL indeterminate loading bar: a gradient comet (bright head fading
/// through blue→cyan tail) slides across the track, fronted by a rotating
/// braille spinner. Wall-clock driven → perfectly smooth at any frame rate.
fn loading_bar(width: usize, t: f64) -> Vec<Span<'static>> {
    const SPIN: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
    let mut spans: Vec<Span<'static>> = Vec::with_capacity(width + 6);
    if width == 0 {
        return spans;
    }
    let spin = SPIN[(t * 10.0) as usize % SPIN.len()];
    spans.push(Span::styled(
        spin.to_string(),
        Style::default().fg(theme::BRIGHT).bold(),
    ));
    spans.push(Span::styled(
        " [".to_string(),
        Style::default().fg(theme::DIM),
    ));
    let seg = ((width as f64) * 0.30).max(3.0) as usize; // comet length
    let span = (width + seg) as f64;
    let pos = ((t * 0.85) % 1.0) * span - seg as f64; // head position (-seg..w)
    let fade = [' ', '▁', '▂', '▄', '▆', '█'];
    // gradient anchors: tail = deep blue, head = bright cyan-white
    let (tb, tg, tr) = (0x1E, 0x5A, 0xC8); // deep blue
    let (hb, hg, hr) = (0xBF, 0xF0, 0xFF); // near-white cyan
    for x in 0..width {
        let d = (x as f64 - pos) / seg as f64; // 0 at head → 1 at tail
        if d < 0.0 || d >= 1.0 {
            spans.push(Span::raw(" "));
            continue;
        }
        let fi = ((1.0 - d) * (fade.len() - 1) as f64).round() as usize;
        let fi = fi.min(fade.len() - 1);
        let k = (1.0 - d).clamp(0.0, 1.0); // 1 at head → 0 at tail
        let (b, g, r) = (
            (tb as f64 + (hb - tb) as f64 * k) as u8,
            (tg as f64 + (hg - tg) as f64 * k) as u8,
            (tr as f64 + (hr - tr) as f64 * k) as u8,
        );
        spans.push(Span::styled(
            fade[fi].to_string(),
            Style::default().fg(Color::Rgb(b, g, r)).bold(),
        ));
    }
    spans.push(Span::styled(
        "]".to_string(),
        Style::default().fg(theme::DIM),
    ));
    spans
}

fn draw_bottom(app: &App, f: &mut Frame, area: Rect) {
    let (prefix, content): (String, Vec<Span<'static>>) = match app.mode {
        Mode::Search => ("/".into(), vec![Span::raw(app.query.clone())]),
        Mode::Command => (":".into(), vec![Span::raw(app.cmd.clone())]),
        Mode::Help => ("".into(), vec![Span::raw("help — press any key".to_string())]),
        Mode::Normal => {
            let mut c = vec![Span::styled(
                app.status.clone(),
                Style::default().fg(theme::TEXT),
            )];
            if app.searching || app.scanning {
                // live smooth loading bar right after the status text
                let t = app.music_visual.clock();
                c.push(Span::raw("  "));
                c.extend(loading_bar(18, t));
            }
            ("".into(), c)
        }
    };
    let mut line_spans = vec![Span::styled(
        prefix,
        Style::default().fg(theme::ACCENT).bold(),
    )];
    line_spans.extend(content);
    f.render_widget(
        Paragraph::new(Line::from(line_spans)).style(Style::default().bg(theme::PANEL)),
        area,
    );
}

fn draw_help(f: &mut Frame, area: Rect) {
    let w = 58u16.min(area.width.saturating_sub(4));
    let h = 24u16.min(area.height.saturating_sub(4));
    let x = area.x + (area.width - w) / 2;
    let y = area.y + (area.height - h) / 2;
    let box_area = Rect::new(x, y, w, h);
    let keys = [
        ("space", "play / pause"),
        ("enter", "play selected"),
        ("n / p", "next / prev track"),
        ("H", "back — replay previous track (history)"),
        ("h", "History tab"),
        ("R", "recommendations (from current track)"),
        ("← / →", "seek ±5s"),
        ("/", "search (all 4 sources)"),
        (":", "command palette"),
        ("j / k", "navigate"),
        ("a", "add to queue"),
        ("d", "remove from queue"),
        ("s", "toggle shuffle"),
        ("r", "loop: off → all → one"),
        ("> / <", "seek ±10s"),
        ("+ / -", "volume"),
        ("t", "tenglish (Telugu → Latin)"),
        ("z / x", "lyrics sync ±0.5s"),
        ("tab", "switch panel"),
        ("pgup/pgdn", "lyrics scroll"),
        ("q", "quit"),
    ];
    let mut lines = vec![Line::from(Span::styled(
        format!(" {} KEYS ", crate::BRAND),
        Style::default().fg(theme::ACCENT).bold(),
    ))];
    for (k, v) in keys {
        lines.push(Line::from(vec![
            Span::styled(format!("  {k:<8} "), Style::default().fg(theme::BRIGHT)),
            Span::styled(v, Style::default().fg(theme::TEXT)),
        ]));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "  commands: :play <q>  :sleep <min>  :vol <0-100>  :stats  :clear",
        Style::default().fg(theme::DIM),
    )));
    let p = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme::ACCENT))
                .style(Style::default().bg(theme::PANEL)),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(Clear, box_area);
    f.render_widget(p, box_area);
}
