//! Scene view widget: renders `DrawCommands` as a grid of Braille-encoded
//! characters, with element-specific colors for atoms and line segments for
//! cell edges.

use chemrust_vis_core::viewport::DrawCommands;
#[allow(unused_imports)]
use chemrust_vis_core::scene::RgbColor;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    widgets::Widget,
};

/// Convert canvas dot coordinates to Braille character grid position and bit index.
///
/// Each Braille character (U+2800–U+28FF) represents a 2×4 dot grid:
/// - Dots 1-3 (bits 0-2): left column, rows 0-2 (top-to-bottom)
/// - Dots 4-6 (bits 3-5): right column, rows 0-2 (top-to-bottom)
/// - Dots 7-8 (bits 6-7): bottom row (row 3), left then right
///
/// Returns `(char_col, char_row, dot_index)` where dot_index is the bit position
/// within the Braille code point.
pub fn canvas_to_braille(canvas_x: f64, canvas_y: f64) -> (usize, usize, u32) {
    let char_col = (canvas_x / 2.0).floor() as usize;
    let char_row = (canvas_y / 4.0).floor() as usize;
    let x_mod = (canvas_x as usize) % 2;
    let y_mod = (canvas_y as usize) % 4;
    // Rows 0-2 map to bits 0-2 (left) and 3-5 (right).
    // Bottom row (y_mod=3) maps to bits 6 (left) and 7 (right).
    let dot_index = if y_mod < 3 {
        x_mod * 3 + y_mod
    } else {
        x_mod + 6
    };
    (char_col, char_row, dot_index as u32)
}

/// Convert an RgbColor to a ratatui Color.
pub fn rgb_to_ratatui(color: RgbColor) -> Color {
    Color::Rgb(color.0, color.1, color.2)
}

/// Shade a ratatui Color by an intensity factor (0.0 = black, 1.0 = full color).
fn shade_color(color: Color, intensity: f64) -> Color {
    if let Color::Rgb(r, g, b) = color {
        let sr = (r as f64 * intensity) as u8;
        let sg = (g as f64 * intensity) as u8;
        let sb = (b as f64 * intensity) as u8;
        Color::Rgb(sr, sg, sb)
    } else {
        color
    }
}

/// A grid of Braille characters with per-cell colors.
struct BrailleGrid {
    /// Unicode Braille code points for each character cell.
    chars: Vec<Vec<u32>>,
    /// Foreground color for each cell (first atom color that hits it).
    colors: Vec<Vec<Option<Color>>>,
    cols: usize,
    rows: usize,
}

impl BrailleGrid {
    fn new(cols: usize, rows: usize) -> Self {
        BrailleGrid {
            chars: vec![vec![0x2800u32; cols]; rows],
            colors: vec![vec![None; cols]; rows],
            cols,
            rows,
        }
    }

    /// Set a dot at canvas coordinates with the given color.
    fn set_dot(&mut self, canvas_x: f64, canvas_y: f64, color: Color) {
        let (col, row, dot) = canvas_to_braille(canvas_x, canvas_y);
        if col < self.cols && row < self.rows {
            self.chars[row][col] |= 1 << dot;
            if self.colors[row][col].is_none() {
                self.colors[row][col] = Some(color);
            }
        }
    }

    /// Draw a shaded sphere on the grid using limb darkening and specular highlight.
    ///
    /// Dots near the sphere center are bright (facing the viewer). Dots near the
    /// edge are darker (grazing angle). A specular highlight adds a 3D glossy look.
    fn fill_circle(&mut self, cx: f64, cy: f64, radius: f64, base_color: Color) {
        let r = radius.ceil() as i64;
        if r < 1 {
            self.set_dot(cx, cy, base_color);
            return;
        }
        for dy in -r..=r {
            for dx in -r..=r {
                let d2 = (dx as f64).powi(2) + (dy as f64).powi(2);
                let r2 = radius.powi(2);
                if d2 > r2 {
                    continue;
                }
                // Normalized distance from center (0 = center, 1 = edge)
                let d = d2.sqrt() / radius;
                // Z-component of sphere surface normal at this point
                let z = (1.0 - d.powi(2)).max(0.0).sqrt();
                // Surface normal (pointing outward): (nx, ny, nz)
                let nx = (dx as f64) / radius;
                let ny = (dy as f64) / radius;
                let nz = z;
                // Light direction (upper-left-front)
                let lx: f64 = 0.5;
                let ly: f64 = -0.5;
                let lz: f64 = 0.707;
                let l_norm = f64::sqrt(lx * lx + ly * ly + lz * lz);
                // Lambertian diffuse
                let n_dot_l = (nx * lx + ny * ly + nz * lz) / l_norm;
                let diffuse = n_dot_l.max(0.0);
                // Specular (Blinn-Phong): half-vector between light and view (0,0,1)
                let hx = lx / l_norm;
                let hy = ly / l_norm;
                let hz = (lz / l_norm + 1.0) / 2.0; // approximate half-vector
                let h_norm = (hx * hx + hy * hy + hz * hz).sqrt();
                let n_dot_h = (nx * hx + ny * hy + nz * hz) / h_norm;
                let specular = n_dot_h.max(0.0).powi(16) * 0.6;
                // Combined intensity: ambient + diffuse + specular
                let ambient = 0.25;
                let intensity = (ambient + diffuse * 0.55 + specular).min(1.0);

                // Modulate base color by intensity
                let shaded = shade_color(base_color, intensity);
                self.set_dot(cx + dx as f64, cy + dy as f64, shaded);
            }
        }
    }

    /// Draw a line segment on the grid using Bresenham's algorithm.
    fn draw_line(&mut self, x1: f64, y1: f64, x2: f64, y2: f64, color: Color) {
        let (mut x, mut y) = (x1 as i64, y1 as i64);
        let (x2i, y2i) = (x2 as i64, y2 as i64);
        let dx = (x2i - x).abs();
        let dy = -(y2i - y).abs();
        let sx = if x < x2i { 1 } else { -1 };
        let sy = if y < y2i { 1 } else { -1 };
        let mut err = dx + dy;

        loop {
            self.set_dot(x as f64, y as f64, color);
            if x == x2i && y == y2i {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                if x == x2i { break; }
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                if y == y2i { break; }
                err += dx;
                y += sy;
            }
        }
    }
}

/// Widget that renders a scene view from DrawCommands.
pub struct SceneWidget<'a> {
    draw_commands: &'a DrawCommands,
}

impl<'a> SceneWidget<'a> {
    pub fn new(draw_commands: &'a DrawCommands) -> Self {
        SceneWidget { draw_commands }
    }
}

impl<'a> Widget for SceneWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let char_cols = area.width as usize;
        let char_rows = area.height as usize;
        if char_cols == 0 || char_rows == 0 {
            return;
        }

        let dot_width = char_cols as f64 * 2.0;
        let dot_height = char_rows as f64 * 4.0;

        // Find bounding box of all draw commands
        let mut min_x = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        for pt in &self.draw_commands.points {
            min_x = min_x.min(pt.x);
            max_x = max_x.max(pt.x);
            min_y = min_y.min(pt.y);
            max_y = max_y.max(pt.y);
        }
        for line in &self.draw_commands.lines {
            min_x = min_x.min(line.x1).min(line.x2);
            max_x = max_x.max(line.x1).max(line.x2);
            min_y = min_y.min(line.y1).min(line.y2);
            max_y = max_y.max(line.y1).max(line.y2);
        }
        if !min_x.is_finite() {
            return;
        }
        let data_width = (max_x - min_x + 1.0).max(1.0);
        let data_height = (max_y - min_y + 1.0).max(1.0);

        // Scale to fit, preserving aspect ratio
        let fit_scale = (dot_width / data_width).min(dot_height / data_height);
        let offset_x = (dot_width - data_width * fit_scale) / 2.0;
        let offset_y = (dot_height - data_height * fit_scale) / 2.0;

        let mut grid = BrailleGrid::new(char_cols, char_rows);

        // Draw cell edges first (behind atoms)
        for line in &self.draw_commands.lines {
            let lx1 = (line.x1 - min_x) * fit_scale + offset_x;
            let ly1 = (line.y1 - min_y) * fit_scale + offset_y;
            let lx2 = (line.x2 - min_x) * fit_scale + offset_x;
            let ly2 = (line.y2 - min_y) * fit_scale + offset_y;
            grid.draw_line(lx1, ly1, lx2, ly2, Color::Gray);
        }

        // Draw atoms as filled circles (depth-sorted: farthest first, closest on top).
        // Radius is perspective-dependent: closer atoms appear larger.
        for pt in &self.draw_commands.points {
            let cx = (pt.x - min_x) * fit_scale + offset_x;
            let cy = (pt.y - min_y) * fit_scale + offset_y;
            let color = rgb_to_ratatui(pt.color);
            let radius = (pt.radius * fit_scale).max(0.5); // at least half a dot
            grid.fill_circle(cx, cy, radius, color);
        }

        // Render grid to buffer
        for row in 0..char_rows.min(grid.rows) {
            for col in 0..char_cols.min(grid.cols) {
                let ch = char::from_u32(grid.chars[row][col]).unwrap_or(' ');
                let style = if let Some(color) = grid.colors[row][col] {
                    Style::default().fg(color)
                } else {
                    Style::default()
                };
                let x = area.x + col as u16;
                let y = area.y + row as u16;
                if let Some(cell) = buf.cell_mut((x, y)) {
                    cell.set_char(ch).set_style(style);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn braille_dot_0_0() {
        let (col, row, dot) = canvas_to_braille(0.0, 0.0);
        assert_eq!(col, 0);
        assert_eq!(row, 0);
        assert_eq!(dot, 0);
        assert_eq!(0x2800 | (1 << dot), 0x2801);
    }

    #[test]
    fn braille_dot_1_0_right_column_top() {
        let (col, row, dot) = canvas_to_braille(1.0, 0.0);
        assert_eq!(col, 0);
        assert_eq!(row, 0);
        assert_eq!(dot, 3); // right column, top
        assert_eq!(0x2800 | (1 << dot), 0x2808);
    }

    #[test]
    fn braille_two_dots_same_cell() {
        let (c1, r1, d1) = canvas_to_braille(0.0, 0.0);
        let (c2, r2, d2) = canvas_to_braille(1.0, 2.0);
        assert_eq!(c1, c2);
        assert_eq!(r1, r2);
        let combined = 0x2800u32 | (1 << d1) | (1 << d2);
        assert_eq!(combined, 0x2801 | 0x20); // bits 0 and 5
    }

    #[test]
    fn braille_subpixel_flooring() {
        let (col, row, dot) = canvas_to_braille(3.7, 8.2);
        assert_eq!(col, 1); // floor(3.7/2) = 1
        assert_eq!(row, 2); // floor(8.2/4) = 2
        // x%2 = 1, y%4 = 0, dot = 1*3 + 0 = 3
        assert_eq!(dot, 3);
    }

    #[test]
    fn braille_bottom_row_correct_bit() {
        // Bottom row (y%4 == 3) maps to bits 6 (left) and 7 (right)
        let (_, _, dot_left) = canvas_to_braille(0.0, 3.0);
        assert_eq!(dot_left, 6); // left bottom → bit 6 (U+2840)
        assert_eq!(0x2800 | (1 << dot_left), 0x2840);

        let (_, _, dot_right) = canvas_to_braille(1.0, 3.0);
        assert_eq!(dot_right, 7); // right bottom → bit 7 (U+2880)
        assert_eq!(0x2800 | (1 << dot_right), 0x2880);

        // (0,3) and (1,0) should NOT collide
        assert_ne!(dot_left, 3); // should not collide with (1,0)
    }
}
