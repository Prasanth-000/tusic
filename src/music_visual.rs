//! Animated 3D DNA-helix music visual for the right panel.
//!
//! Two strands of music symbols (♪ ♫ ♩ ♬) wind as a genuine 3D double helix
//! that REVOLVES in time (phase advances by omega*dt), with horizontal
//! parallax depth (front strands shift right, back shift left) and depth
//! shading (front = bright cyan, side = blue, back = slate). Rungs (base
//! pairs) connect the strands near mid-depth, thickening on beats.
//!
//! The helix EXPANDS and CONTRACTS with the music: amplitude is modulated by
//! a beat-punched energy envelope, so a bass hit snaps the coil open and it
//! eases back — visible breathing, not a flat scroll.
//!
//! Rendering note: we rasterize onto a per-cell grid and gap-fill vertically
//! so the strands stay CONTINUOUS even when amplitude is large (high energy).
//! Sampling per-column and thresholding would scatter dots at high amplitude
//! and make the art "fall apart" — the grid approach never does.

use crate::theme;
use ratatui::prelude::{Color, Line, Span, Style};
use std::time::Instant;

/// Animation state: continuous time clock + beat-punched energy.
pub struct MusicVisual {
    start: Instant,
    last: Instant,
    energy: f32,
}

impl Default for MusicVisual {
    fn default() -> Self {
        let now = Instant::now();
        MusicVisual {
            start: now,
            last: now,
            energy: 0.0,
        }
    }
}

impl MusicVisual {
    /// Advance one tick. Call from the app's poll loop (dt derived from real
    /// wall-clock time so motion is frame-rate independent).
    pub fn tick(&mut self) {
        self.last = Instant::now();
    }

    /// Feed the current loudness envelope (0..1) from the spectrum.
    /// Drives contract ↔ normal ↔ expansion. MILLISECOND-TIGHT (user spec):
    /// attack 0.97/poll → a kick snaps the coil to expansion in ~2 polls
    /// (~66ms); decay 0.45/poll → eases back to normal/contract in ~4 polls
    /// (~130ms). Instant punch, fast release.
    pub fn set_energy(&mut self, e: f32) {
        let e = e.clamp(0.0, 1.0);
        if e > self.energy {
            self.energy += (e - self.energy) * 0.97; // snap to expansion (~ms)
        } else {
            self.energy += (e - self.energy) * 0.45; // fast release (~130ms)
        }
    }

    /// Current beat-punched energy (0..1) — read by tests/debug.
    pub fn energy(&self) -> f32 {
        self.energy
    }

    /// Wall-clock seconds since the visual started — drives the loading
    /// bar shimmer (smooth, frame-rate independent).
    pub fn clock(&self) -> f64 {
        self.last.duration_since(self.start).as_secs_f64()
    }

    /// Render the DNA helix for the given area as styled Lines.
    pub fn render(&self, width: u16, height: u16) -> Vec<Line<'static>> {
        let w = width.max(1) as i64;
        let h = height.max(1) as i64;
        let notes = ['♪', '♫', '♩', '♬'];
        let cy = (h as f64) / 2.0;

        // ---- EXPAND/CONTRACT (user spec) ----
        //   contract = normal/2  |  normal = 0.32h  |  max = 2× normal
        // Piecewise on energy e ∈ [0,1]:
        //   e ≤ 0.5  → amp = normal * (0.5 + e)   [0.5× at silence, 1× at half]
        //   e > 0.5  → amp = normal * (2e)        [1× at half, 2× at full beat]
        // Clamped below the panel edge so the coil never clips.
        let e = (self.energy as f64).clamp(0.0, 1.0);
        let normal = 0.32 * h as f64;
        let max_amp = (2.0 * normal).min(cy - 1.0); // permanent max (2×)
        let amp = if e <= 0.5 {
            normal * (0.5 + e)
        } else {
            normal * (2.0 * e)
        }
        .min(max_amp);

        // one full twist every ~w/5 columns (w=60 → 12 cols/twist → ~5 full
        // turns across the panel — reads as a helix, not a wave)
        let period = (w as f64 / 5.0).max(10.0);
        let k = 2.0 * std::f64::consts::PI / period;

        // ---- 3D REVOLUTION: time-based phase, ~0.19 Hz (≈5.2s / turn) ----
        let t = self.last.duration_since(self.start).as_secs_f64();
        let omega = 1.2; // rad/s
        let phase0 = omega * t;

        // horizontal parallax: SUBTLE (≤2 columns) — enough for depth shading
        // to sell the 3D turn, small enough not to split the strands into an X.
        let parallax = (w as f64 * 0.02).clamp(1.0, 2.0);

        // ---- raster grid: glyph + color per cell ----
        let mut gch = vec![vec![' '; w as usize]; h as usize];
        let mut gco = vec![vec![theme::ACCENT; w as usize]; h as usize];

        let depth_color = |z: f64| -> Color {
            if z > 0.3 {
                theme::BRIGHT // front strand — full glow
            } else if z < -0.3 {
                theme::MID // back strand — visible, not blacked out (50% opacity feel)
            } else {
                theme::ACCENT // side — electric blue
            }
        };

        let glyph_at = |x: i64| -> char { notes[(((x + (t * 4.0) as i64).rem_euclid(4)) as usize) % notes.len()] };

        let plot_a = |gch: &mut Vec<Vec<char>>, gco: &mut Vec<Vec<Color>>, col: i64, row: i64, ch: char, co: Color| {
            if col >= 0 && col < w && row >= 0 && row < h {
                gch[row as usize][col as usize] = ch;
                gco[row as usize][col as usize] = co;
            }
        };

        // Bresenham line (x0,y0)→(x1,y1) — connecting consecutive strand
        // points with segments makes the sine read as a SMOOTH CURVE.
        // The old code drew single dots + vertical gap-fills: quantized to
        // rows, that produced jagged slopes and hard right angles where the
        // rungs met the strands → the "diamond" look the user called out.
        let line = |gch: &mut Vec<Vec<char>>, gco: &mut Vec<Vec<Color>>, x0: i64, y0: i64, x1: i64, y1: i64, ch: char, co: Color| {
            let mut x = x0;
            let mut y = y0;
            let dx = (x1 - x0).abs();
            let dy = -(y1 - y0).abs();
            let sx = if x0 < x1 { 1 } else { -1 };
            let sy = if y0 < y1 { 1 } else { -1 };
            let mut err = dx + dy;
            loop {
                plot_a(gch, gco, x, y, ch, co);
                if x == x1 && y == y1 {
                    break;
                }
                let e2 = 2 * err;
                if e2 >= dy {
                    err += dy;
                    x += sx;
                }
                if e2 <= dx {
                    err += dx;
                    y += sy;
                }
            }
        };

        let mut pts_a: Vec<(i64, i64)> = Vec::with_capacity(w as usize);
        let mut pts_b: Vec<(i64, i64)> = Vec::with_capacity(w as usize);
        let mut zs: Vec<f64> = Vec::with_capacity(w as usize);
        for x in 0..w {
            let xf = x as f64;
            let ang_a = k * xf + phase0;
            let za = ang_a.cos(); // +1 front, -1 back
            let ya = cy + amp * ang_a.sin();
            let yb = cy - amp * ang_a.sin(); // strand B = π out of phase
            let xa = xf + parallax * za;
            let xb = xf - parallax * za;
            pts_a.push((xa.round() as i64, ya.round() as i64));
            pts_b.push((xb.round() as i64, yb.round() as i64));
            zs.push(za);
        }

        for i in 0..w as usize - 1 {
            let za = zs[i];
            let co_a = depth_color(za);
            let ga = glyph_at(i as i64);
            line(&mut gch, &mut gco, pts_a[i].0, pts_a[i].1, pts_a[i + 1].0, pts_a[i + 1].1, ga, co_a);
            let co_b = depth_color(-za);
            let gb = glyph_at(i as i64 + 2);
            line(&mut gch, &mut gco, pts_b[i].0, pts_b[i].1, pts_b[i + 1].0, pts_b[i + 1].1, gb, co_b);
            // rung (base pair): every 4th column, only where the strands are
            // actually separated (>3 rows). Near the front/back crossings the
            // rung collapses to nothing anyway; drawing it there created the
            // hard vertical bars + X shapes = the "diamond" facets.
            if i % 4 == 0 && (pts_a[i].1 - pts_b[i].1).abs() > 3 {
                line(&mut gch, &mut gco, pts_a[i].0, pts_a[i].1, pts_b[i].0, pts_b[i].1, '·', theme::DIM);
            }
        }

        // ---- emit Lines ----
        let mut lines = Vec::with_capacity(h as usize);
        for r in 0..h as usize {
            let mut spans = Vec::with_capacity(w as usize);
            for c in 0..w as usize {
                let ch = gch[r][c];
                let co = gco[r][c];
                // bold every music glyph + rung → ~50% opacity boost: the
                // helix reads bright and present against the terminal bg
                let bold = ch != ' ';
                let style = if bold {
                    Style::default().fg(co).add_modifier(ratatui::style::Modifier::BOLD)
                } else {
                    Style::default()
                };
                spans.push(Span::styled(ch.to_string(), style));
            }
            lines.push(Line::from(spans));
        }
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render_at(w: u16, h: u16, e: f32) -> Vec<ratatui::prelude::Line<'static>> {
        let mut mv = MusicVisual::default();
        mv.tick();
        mv.set_energy(e);
        mv.render(w, h)
    }

    #[test]
    fn render_is_deterministic() {
        let a = render_at(80, 10, 0.5);
        let b = render_at(80, 10, 0.5);
        assert_eq!(a, b);
    }

    #[test]
    fn render_bounds_never_panic() {
        // sweep every sane panel size — Bresenham line() must stay in-bounds
        for w in 1..=120 {
            for h in 1..=16 {
                let _ = render_at(w, h, 0.05);
                let _ = render_at(w, h, 0.9);
            }
        }
    }

    #[test]
    fn strand_continuity() {
        // every column must carry at least one music glyph (no gaps → the
        // curve reads continuous, never scattered dots)
        for e in [0.05f32, 0.5, 0.9] {
            let lines = render_at(60, 12, e);
            for x in 0..60usize {
                let has_glyph = lines.iter().any(|l| {
                    l.spans
                        .get(x)
                        .map(|s| !s.content.trim().is_empty())
                        .unwrap_or(false)
                });
                assert!(has_glyph, "column {x} empty at e={e} — strand gap");
            }
        }
    }
}

