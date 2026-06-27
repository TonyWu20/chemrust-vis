//! XYZ axis indicator widget: shows world coordinate axes projected
//! to screen space, updating with camera rotation.

use chemrust_vis_core::camera::Camera;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    widgets::Widget,
};

/// Widget showing XYZ axes as colored lines (R=+X, G=+Y, B=+Z).
pub struct AxisIndicator {
    /// Screen-space direction of each axis (unit vectors, canvas coords).
    pub x_dir: [f64; 2],
    pub y_dir: [f64; 2],
    pub z_dir: [f64; 2],
}

impl AxisIndicator {
    pub fn new(camera: &Camera) -> Self {
        let (x, y, z) = camera.axis_directions();
        AxisIndicator { x_dir: x, y_dir: y, z_dir: z }
    }
}

impl Widget for AxisIndicator {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let w = area.width as f64;
        let h = area.height as f64;
        if w < 3.0 || h < 3.0 { return; }

        // Center of the widget in character coordinates
        let cx = w / 2.0;
        let cy = h / 2.0;
        // Scale so the longest axis reaches the edge with some margin
        let max_len = [self.x_dir, self.y_dir, self.z_dir]
            .iter()
            .map(|d| (d[0].abs()).max(d[1].abs()))
            .fold(0.0f64, f64::max)
            .max(0.001);
        let scale = (w.min(h) / 2.0 - 1.0) / max_len;

        // Draw each axis: line from center outward
        let axes = [
            (self.x_dir, Color::Rgb(255, 80, 80), 'X'),   // red-ish
            (self.y_dir, Color::Rgb(80, 255, 80), 'Y'),   // green-ish
            (self.z_dir, Color::Rgb(80, 80, 255), 'Z'),   // blue-ish
        ];

        for (dir, color, label) in &axes {
            let ex = cx + dir[0] * scale;
            let ey = cy + dir[1] * scale;
            draw_line_on_buf(buf, area, cx, cy, ex, ey, *color);
            // Label at the end
            let lx = (area.x as f64 + ex) as u16;
            let ly = (area.y as f64 + ey) as u16;
            if lx < area.x + area.width && ly < area.y + area.height {
                if let Some(cell) = buf.cell_mut((lx, ly)) {
                    cell.set_char(*label).set_style(Style::default().fg(*color));
                }
            }
        }
    }
}

fn draw_line_on_buf(buf: &mut Buffer, area: Rect, x1: f64, y1: f64, x2: f64, y2: f64, color: Color) {
    let (mut x, mut y) = (x1 as i64, y1 as i64);
    let (x2i, y2i) = (x2 as i64, y2 as i64);
    let dx = (x2i - x).abs(); let dy = -(y2i - y).abs();
    let sx = if x < x2i { 1 } else { -1 };
    let sy = if y < y2i { 1 } else { -1 };
    let mut err = dx + dy;
    loop {
        let px = area.x as i64 + x;
        let py = area.y as i64 + y;
        if px >= area.x as i64 && px < (area.x + area.width) as i64
            && py >= area.y as i64 && py < (area.y + area.height) as i64
        {
            if let Some(cell) = buf.cell_mut((px as u16, py as u16)) {
                cell.set_char('▌').set_style(Style::default().fg(color));
            }
        }
        if x == x2i && y == y2i { break; }
        let e2 = 2 * err;
        if e2 >= dy { if x == x2i { break; } err += dy; x += sx; }
        if e2 <= dx { if y == y2i { break; } err += dx; y += sy; }
    }
}
