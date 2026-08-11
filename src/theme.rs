//! KALKI premium blue palette (truecolor).

use ratatui::style::Color;

pub const BG: Color = Color::Reset; // transparent — inherit terminal bg
pub const PANEL: Color = Color::Reset; // transparent panel (just borders)
pub const BORDER: Color = Color::Rgb(0x1E, 0x2A, 0x5A); // border blue
pub const ACCENT: Color = Color::Rgb(0x3B, 0x82, 0xF6); // electric blue
pub const BRIGHT: Color = Color::Rgb(0x22, 0xD3, 0xEE); // cyan highlight
pub const TEXT: Color = Color::Rgb(0xE2, 0xE8, 0xF0); // near-white
pub const MID: Color = Color::Rgb(0x9A, 0xB0, 0xC8); // mid slate — back strand
pub const DIM: Color = Color::Rgb(0x64, 0x74, 0x8B); // slate dim
pub const WARN: Color = Color::Rgb(0xF5, 0x9E, 0x0B); // amber
