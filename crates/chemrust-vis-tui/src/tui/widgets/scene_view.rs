//! Scene view widget: renders DrawCommands as half-block characters
//! (▀▄█) giving 2 vertical pixels per character with solid fill.

use chemrust_vis_core::viewport::DrawCommands;
use chemrust_vis_core::scene::RgbColor;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    widgets::Widget,
};

pub fn rgb_to_ratatui(color: RgbColor) -> Color {
    Color::Rgb(color.0, color.1, color.2)
}

fn shade_color(color: Color, intensity: f64) -> Color {
    if let Color::Rgb(r, g, b) = color {
        Color::Rgb((r as f64 * intensity) as u8, (g as f64 * intensity) as u8, (b as f64 * intensity) as u8)
    } else { color }
}

/// Half-block grid: 2 vertical sub-pixels per character cell.
struct HalfBlockGrid {
    /// Top color for each cell (upper half).
    top: Vec<Vec<Option<Color>>>,
    /// Bottom color for each cell (lower half).
    bot: Vec<Vec<Option<Color>>>,
    cols: usize, rows: usize,
}

impl HalfBlockGrid {
    fn new(cols: usize, rows: usize) -> Self {
        HalfBlockGrid {
            top: vec![vec![None; cols]; rows],
            bot: vec![vec![None; cols]; rows],
            cols, rows,
        }
    }

    fn set_dot(&mut self, x: f64, y: f64, color: Color) {
        let col = (x / 2.0).floor() as usize;
        let row = (y / 2.0).floor() as usize;
        if col < self.cols && row < self.rows {
            if (y as usize) % 2 == 0 {
                if self.top[row][col].is_none() { self.top[row][col] = Some(color); }
            } else {
                if self.bot[row][col].is_none() { self.bot[row][col] = Some(color); }
            }
        }
    }

    fn fill_circle(&mut self, cx: f64, cy: f64, radius: f64, base_color: Color) {
        let r = radius.ceil() as i64;
        if r < 1 { self.set_dot(cx, cy, base_color); return; }
        for dy in -r..=r {
            for dx in -r..=r {
                let d2 = (dx as f64).powi(2) + (dy as f64).powi(2);
                if d2 > radius.powi(2) { continue; }
                let d = d2.sqrt() / radius;
                let z = (1.0 - d.powi(2)).max(0.0).sqrt();
                let nx = dx as f64 / radius;
                let ny = dy as f64 / radius;
                let lx: f64 = 0.5; let ly: f64 = -0.5; let lz: f64 = 0.707;
                let ln = f64::sqrt(lx*lx+ly*ly+lz*lz);
                let diffuse = ((nx*lx + ny*ly + z*lz)/ln).max(0.0);
                let hx = lx/ln; let hy = ly/ln; let hz = (lz/ln+1.0)/2.0;
                let hn = f64::sqrt(hx*hx+hy*hy+hz*hz);
                let spec = ((nx*hx+ny*hy+z*hz)/hn).max(0.0).powi(16)*0.6;
                let intensity = (0.25 + diffuse*0.55 + spec).min(1.0);
                self.set_dot(cx + dx as f64, cy + dy as f64, shade_color(base_color, intensity));
            }
        }
    }

    fn draw_line(&mut self, x1: f64, y1: f64, x2: f64, y2: f64, color: Color) {
        let (mut x, mut y) = (x1 as i64, y1 as i64);
        let (x2i, y2i) = (x2 as i64, y2 as i64);
        let dx = (x2i - x).abs(); let dy = -(y2i - y).abs();
        let sx = if x < x2i { 1 } else { -1 };
        let sy = if y < y2i { 1 } else { -1 };
        let mut err = dx + dy;
        loop {
            self.set_dot(x as f64, y as f64, color);
            if x == x2i && y == y2i { break; }
            let e2 = 2 * err;
            if e2 >= dy { if x == x2i { break; } err += dy; x += sx; }
            if e2 <= dx { if y == y2i { break; } err += dx; y += sy; }
        }
    }
}

pub struct SceneWidget<'a> { draw_commands: &'a DrawCommands }

impl<'a> SceneWidget<'a> {
    pub fn new(draw_commands: &'a DrawCommands) -> Self { SceneWidget { draw_commands } }
}

impl<'a> Widget for SceneWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let cc = area.width as usize; let cr = area.height as usize;
        if cc == 0 || cr == 0 { return; }
        let dw = cc as f64 * 2.0; let dh = cr as f64 * 2.0;
        let mut grid = HalfBlockGrid::new(cc, cr);

        for line in &self.draw_commands.lines {
            grid.draw_line(line.x1, line.y1, line.x2, line.y2, Color::Gray);
        }
        for pt in &self.draw_commands.points {
            if pt.x < -pt.radius || pt.x > dw + pt.radius || pt.y < -pt.radius || pt.y > dh + pt.radius { continue; }
            grid.fill_circle(pt.x, pt.y, pt.radius, rgb_to_ratatui(pt.color));
        }

        for row in 0..cr {
            for col in 0..cc {
                let top = grid.top[row][col];
                let bot = grid.bot[row][col];
                let (ch, fg, bg) = match (top, bot) {
                    (Some(t), Some(b)) => ('▀', t, Some(b)),
                    (Some(t), None) => ('▀', t, None),
                    (None, Some(b)) => ('▄', b, None),
                    (None, None) => (' ', Color::Reset, None),
                };
                let mut style = Style::default().fg(fg);
                if let Some(b) = bg { style = style.bg(b); }
                let x = area.x + col as u16;
                let y = area.y + row as u16;
                if let Some(cell) = buf.cell_mut((x, y)) {
                    cell.set_char(ch).set_style(style);
                }
            }
        }
    }
}
