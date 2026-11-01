//! Shared bitmap lettering for dynamic text, sampled like the pixel-art panel.

use nih_plug_egui::egui::{self, pos2, vec2, Color32, Pos2, Rect, TextureHandle};

pub(super) struct PixelFont(TextureHandle);

impl PixelFont {
    pub fn new(ctx: &egui::Context) -> Self {
        let mut image = egui::ColorImage::new([96 * 6, 8], Color32::TRANSPARENT);
        for ascii in 32u8..128 {
            for (y, row) in glyph(ascii as char).iter().enumerate() {
                for x in 0..5 {
                    if row & (1 << (4 - x)) != 0 {
                        image[((ascii as usize - 32) * 6 + x, y)] = Color32::WHITE;
                    }
                }
            }
        }
        Self(ctx.load_texture("tapeworm_pixel_font", image, egui::TextureOptions::NEAREST))
    }

    pub fn width(text: &str, pixel: f32) -> f32 {
        (text.chars().count() as f32 * 6.0 - 1.0).max(0.0) * pixel
    }

    pub fn draw(
        &self,
        painter: &egui::Painter,
        text: &str,
        origin: Pos2,
        pixel: f32,
        color: Color32,
    ) {
        self.draw_cells(painter, text, origin, vec2(pixel, pixel), color);
    }

    /// Condensed cells match the narrow lettering on the selector buttons.
    pub fn draw_cells(
        &self,
        painter: &egui::Painter,
        text: &str,
        origin: Pos2,
        cell: egui::Vec2,
        color: Color32,
    ) {
        for (i, character) in text.chars().enumerate() {
            let ascii = character as usize;
            let index = if (32..128).contains(&ascii) {
                ascii - 32
            } else {
                31
            };
            let uv = Rect::from_min_max(
                pos2((index * 6) as f32 / 576.0, 0.0),
                pos2((index * 6 + 5) as f32 / 576.0, 7.0 / 8.0),
            );
            let rect = Rect::from_min_size(
                origin + vec2(i as f32 * 6.0 * cell.x, 0.0),
                vec2(5.0 * cell.x, 7.0 * cell.y),
            );
            painter.image(self.0.id(), rect, uv, color);
        }
    }
}

/// Word wrap in bitmap cells, including long words; never truncate a hint.
pub(super) fn wrap_text(text: &str, max_columns: usize) -> Vec<String> {
    let max_columns = max_columns.max(1);
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && line.len() + 1 + word.len() > max_columns {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        for ch in word.chars() {
            if line.len() == max_columns {
                lines.push(std::mem::take(&mut line));
            }
            line.push(ch.to_ascii_uppercase());
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

// Five columns and seven rows, including the slashed zero used by the artwork.
fn glyph(ch: char) -> [u8; 7] {
    // Keep the physical unit dB correctly cased in knob readouts.
    if ch == 'd' {
        return [1, 1, 15, 17, 17, 17, 15];
    }
    if ch == 'z' {
        return [0, 0, 31, 2, 4, 8, 31];
    }
    match ch.to_ascii_uppercase() {
        'A' => [14, 17, 17, 31, 17, 17, 17],
        'B' => [30, 17, 17, 30, 17, 17, 30],
        'C' => [14, 17, 16, 16, 16, 17, 14],
        'D' => [30, 17, 17, 17, 17, 17, 30],
        'E' => [31, 16, 16, 30, 16, 16, 31],
        'F' => [31, 16, 16, 30, 16, 16, 16],
        'G' => [14, 17, 16, 23, 17, 17, 15],
        'H' => [17, 17, 17, 31, 17, 17, 17],
        'I' => [31, 4, 4, 4, 4, 4, 31],
        'J' => [7, 2, 2, 2, 18, 18, 12],
        'K' => [17, 18, 20, 24, 20, 18, 17],
        'L' => [16, 16, 16, 16, 16, 16, 31],
        'M' => [17, 27, 21, 21, 17, 17, 17],
        'N' => [17, 25, 21, 19, 17, 17, 17],
        'O' => [14, 17, 17, 17, 17, 17, 14],
        'P' => [30, 17, 17, 30, 16, 16, 16],
        'Q' => [14, 17, 17, 17, 21, 18, 13],
        'R' => [30, 17, 17, 30, 20, 18, 17],
        'S' => [15, 16, 16, 14, 1, 1, 30],
        'T' => [31, 4, 4, 4, 4, 4, 4],
        'U' => [17, 17, 17, 17, 17, 17, 14],
        'V' => [17, 17, 17, 17, 17, 10, 4],
        'W' => [17, 17, 17, 21, 21, 21, 10],
        'X' => [17, 17, 10, 4, 10, 17, 17],
        'Y' => [17, 17, 10, 4, 4, 4, 4],
        'Z' => [31, 1, 2, 4, 8, 16, 31],
        '0' => [14, 17, 19, 21, 25, 17, 14],
        '1' => [4, 12, 4, 4, 4, 4, 14],
        '2' => [14, 17, 1, 2, 4, 8, 31],
        '3' => [30, 1, 1, 14, 1, 1, 30],
        '4' => [2, 6, 10, 18, 31, 2, 2],
        '5' => [31, 16, 16, 30, 1, 1, 30],
        '6' => [14, 16, 16, 30, 17, 17, 14],
        '7' => [31, 1, 2, 4, 8, 8, 8],
        '8' => [14, 17, 17, 14, 17, 17, 14],
        '9' => [14, 17, 17, 15, 1, 1, 14],
        '.' => [0, 0, 0, 0, 0, 6, 6],
        ',' => [0, 0, 0, 0, 6, 6, 4],
        ':' => [0, 6, 6, 0, 6, 6, 0],
        '-' => [0, 0, 0, 31, 0, 0, 0],
        '+' => [0, 4, 4, 31, 4, 4, 0],
        '/' => [1, 2, 2, 4, 8, 8, 16],
        '%' => [25, 25, 2, 4, 8, 19, 19],
        '(' => [2, 4, 8, 8, 8, 4, 2],
        ')' => [8, 4, 2, 2, 2, 4, 8],
        '&' => [12, 18, 20, 8, 21, 18, 13],
        '\'' => [4, 4, 8, 0, 0, 0, 0],
        ' ' => [0; 7],
        _ => [14, 17, 1, 2, 4, 0, 4],
    }
}
