//! Scene view widget: renders `DrawCommands` as a grid of Unicode
//! quadrant block characters (2×2 sub-pixels per character), with
//! spherical shading for atoms and line segments for cell edges.

use chemrust_vis_core::viewport::DrawCommands;
use chemrust_vis_core::scene::RgbColor;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    widgets::Widget,
};

/// Quadrant block lookup: index bits are (ul, ur, ll, lr).
/// ul = bit 3 (8), ur = bit 2 (4), ll = bit 1 (2), lr = bit 0 (1).
const QUADRANT: [char; 16] = [
    ' ', '▗', '▝', '▐', // 0-3
    '▖', '▄', '▞', '▟', // 4-7
    '▘', '▚', '▀', '▜', // 8-11
    '▌', '▙', '▛', '█', // 12-15
];

/// Map canvas dot coordinates to block character grid position and quadrant bit.
///
/// Each character cell is 2×2 dots:
/// - (0,0) = upper-left quadrant (bit 8)
/// - (1,0) = upper-right quadrant (bit 4)
/// - (0,1) = lower-left quadrant (bit 2)
/// - (1,1) = lower-right quadrant (bit 1)
fn canvas_to_block(canvas_x: f64, canvas_y: f64) -> (usize, usize, u8) {
    let col = (canvas_x / 2.0).floor() as usize;
    let row = (canvas_y / 2.0).floor() as usize;
    let qx = (canvas_x as usize) % 2; // 0 = left, 1 = right
    let qy = (canvas_y as usize) % 2; // 0 = top, 1 = bottom
    let bit = match (qx, qy) {
        (0, 0) => 8,  // upper-left
        (1, 0) => 4,  // upper-right
        (0, 1) => 2,  // lower-left
        (1, 1) => 1,  // lower-right
        _ => unreachable!(),
    };
    (col, row, bit)
}

/// Convert an RgbColor to a ratatui Color.
pub fn rgb_to_ratatui(color: RgbColor) -> Color {
    Color::Rgb(color.0, color.1, color.2)
}

/// Shade a ratatui Color by an intensity factor (0.0 = black, 1.0 = full color).
fn shade_color(color: Color, intensity: f64) -> Color {
    if let Color::Rgb(r, g, b) = color {
        Color::Rgb(
            (r as f64 * intensity) as u8,
            (g as f64 * intensity) as u8,
            (b as f64 * intensity) as u8,
        )
    } else {
        color
    }
}

/// A grid of block characters with per-cell colors.
struct BlockGrid {
    /// Bitmask of which quadrants are filled in each cell.
    bits: Vec<Vec<u8>>,
    /// Foreground color for each cell.
    colors: Vec<Vec<Option<Color>>>,
    cols: usize,
    rows: usize,
}

impl BlockGrid {
    fn new(cols: usize, rows: usize) -> Self {
        BlockGrid {
            bits: vec![vec![0u8; cols]; rows],
            colors: vec![vec![None; cols]; rows],
            cols,
            rows,
        }
    }

    fn set_dot(&mut self, x: f64, y: f64, color: Color) {
        let (col, row, bit) = canvas_to_block(x, y);
        if col < self.cols && row < self.rows {
            self.bits[row][col] |= bit;
            if self.colors[row][col].is_none() {
                self.colors[row][col] = Some(color);
            }
        }
    }

    /// Draw a shaded sphere at (cx, cy) with Blinn-Phong shading.
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
                let d = d2.sqrt() / radius;
                let z = (1.0 - d.powi(2)).max(0.0).sqrt();
                let nx = dx as f64 / radius;
                let ny = dy as f64 / radius;
                let nz = z;
                // Light from upper-left-front
                let lx: f64 = 0.5;
                let ly: f64 = -0.5;
                let lz: f64 = 0.707;
                let l_norm = f64::sqrt(lx * lx + ly * ly + lz * lz);
                let n_dot_l = (nx * lx + ny * ly + nz * lz) / l_norm;
                let diffuse = n_dot_l.max(0.0);
                // Specular highlight
                let hx = lx / l_norm;
                let hy = ly / l_norm;
                let hz = (lz / l_norm + 1.0) / 2.0;
                let h_norm = f64::sqrt(hx * hx + hy * hy + hz * hz);
                let n_dot_h = (nx * hx + ny * hy + nz * hz) / h_norm;
                let specular = n_dot_h.max(0.0).powi(16) * 0.6;
                let ambient = 0.25;
                let intensity = (ambient + diffuse * 0.55 + specular).min(1.0);
                self.set_dot(cx + dx as f64, cy + dy as f64, shade_color(base_color, intensity));
            }
        }
    }

    /// Bresenham line drawing.
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

        // Viewport outputs dot coordinates matching char_cols*2 × char_rows*2.
        let dot_width = char_cols as f64 * 2.0;
        let dot_height = char_rows as f64 * 2.0;

        let mut grid = BlockGrid::new(char_cols, char_rows);

        // Cell edges behind atoms
        for line in &self.draw_commands.lines {
            grid.draw_line(line.x1, line.y1, line.x2, line.y2, Color::Gray);
        }

        // Atoms as shaded spheres (depth-sorted: farthest first, closest on top)
        for pt in &self.draw_commands.points {
            if pt.x < -pt.radius || pt.x > dot_width + pt.radius
                || pt.y < -pt.radius || pt.y > dot_height + pt.radius
            {
                continue;
            }
            grid.fill_circle(pt.x, pt.y, pt.radius, rgb_to_ratatui(pt.color));
        }

        // Render grid to buffer
        for row in 0..char_rows.min(grid.rows) {
            for col in 0..char_cols.min(grid.cols) {
                let ch = QUADRANT[grid.bits[row][col] as usize];
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
    fn block_ul_quadrant() {
        let (col, row, bit) = canvas_to_block(0.0, 0.0);
        assert_eq!(col, 0);
        assert_eq!(row, 0);
        assert_eq!(bit, 8); // upper-left
    }

    #[test]
    fn block_ur_quadrant() {
        let (col, row, bit) = canvas_to_block(1.0, 0.0);
        assert_eq!(col, 0);
        assert_eq!(row, 0);
        assert_eq!(bit, 4); // upper-right
    }

    #[test]
    fn block_ll_quadrant() {
        let (_, _, bit) = canvas_to_block(0.0, 1.0);
        assert_eq!(bit, 2); // lower-left
    }

    #[test]
    fn block_lr_quadrant() {
        let (_, _, bit) = canvas_to_block(1.0, 1.0);
        assert_eq!(bit, 1); // lower-right
    }

    #[test]
    fn block_full_square() {
        let (c1, r1, b1) = canvas_to_block(0.0, 0.0);
        let (c2, r2, b2) = canvas_to_block(1.0, 0.0);
        let (c3, r3, b3) = canvas_to_block(0.0, 1.0);
        let (c4, r4, b4) = canvas_to_block(1.0, 1.0);
        assert_eq!((c1, r1), (0, 0));
        assert_eq!((c2, r2), (0, 0));
        assert_eq!((c3, r3), (0, 0));
        assert_eq!((c4, r4), (0, 0));
        let combined = b1 | b2 | b3 | b4;
        assert_eq!(combined, 15); // all 4 bits
        assert_eq!(QUADRANT[combined as usize], '█');
    }

    #[test]
    fn block_subpixel_flooring() {
        let (col, row, _) = canvas_to_block(3.7, 8.2);
        assert_eq!(col, 1); // floor(3.7/2) = 1
        assert_eq!(row, 4); // floor(8.2/2) = 4
    }
}
