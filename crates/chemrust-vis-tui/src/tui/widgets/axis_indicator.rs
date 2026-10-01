//! XYZ axis indicator widget: shows world axes projected to screen.

use chemrust_vis_core::camera::Camera;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    widgets::Widget,
};

pub struct AxisIndicator {
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
        let w = area.width as i64;
        let h = area.height as i64;
        if w < 4 || h < 4 { return; }

        // Origin at bottom-left of widget
        let ox = area.x as i64 + 1;
        let oy = area.y as i64 + h - 2;

        // Scale to widget size
        let max_len = [self.x_dir, self.y_dir, self.z_dir]
            .iter()
            .map(|d| d[0].abs().max(d[1].abs()))
            .fold(0.0f64, f64::max)
            .max(0.001);
        let scale = (w.min(h) - 2) as f64 / max_len;

        let axes: [([f64; 2], Color, char); 3] = [
            (self.x_dir, Color::Rgb(255, 50, 50), 'X'),
            (self.y_dir, Color::Rgb(50, 255, 50), 'Y'),
            (self.z_dir, Color::Rgb(80, 140, 255), 'Z'),
        ];

        for (dir, color, label) in &axes {
            let ex = (ox as f64 + dir[0] * scale) as i64;
            let ey = (oy as f64 - dir[1] * scale) as i64; // screen Y is inverted from world Y

            // Draw line from origin to tip
            bresenham(ox, oy, ex, ey, |x, y| {
                let ux = x as u16;
                let uy = y as u16;
                if ux >= area.x && ux < area.x + area.width
                    && uy >= area.y && uy < area.y + area.height
                {
                    if let Some(cell) = buf.cell_mut((ux, uy)) {
                        cell.set_char('·').set_style(Style::default().fg(*color));
                    }
                }
            });

            // Label at tip
            let lx = ex.max(area.x as i64).min((area.x + area.width - 1) as i64) as u16;
            let ly = ey.max(area.y as i64).min((area.y + area.height - 1) as i64) as u16;
            if let Some(cell) = buf.cell_mut((lx, ly)) {
                cell.set_char(*label).set_style(Style::default().fg(*color).bg(Color::Black));
            }
        }

        // Origin dot
        if let Some(cell) = buf.cell_mut((ox as u16, oy as u16)) {
            cell.set_char('○').set_style(Style::default().fg(Color::White));
        }
    }
}

fn bresenham(mut x: i64, mut y: i64, x2: i64, y2: i64, mut plot: impl FnMut(i64, i64)) {
    let dx = (x2 - x).abs();
    let dy = -(y2 - y).abs();
    let sx = if x < x2 { 1 } else { -1 };
    let sy = if y < y2 { 1 } else { -1 };
    let mut err = dx + dy;
    loop {
        plot(x, y);
        if x == x2 && y == y2 { break; }
        let e2 = 2 * err;
        if e2 >= dy { if x == x2 { break; } err += dy; x += sx; }
        if e2 <= dx { if y == y2 { break; } err += dx; y += sy; }
    }
}
