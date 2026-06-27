//! Scene view widget: renders DrawCommands as Braille dot characters
//! (2×4 dots per character) with spherical shading and line drawing.

use chemrust_vis_core::viewport::DrawCommands;
use chemrust_vis_core::scene::RgbColor;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    widgets::Widget,
};

/// Map canvas dot coords to Braille position and bit.
/// 2×4 grid per char. Rows 0-2 → bits 0-2(left)/3-5(right), row 3 → bits 6(left)/7(right).
fn canvas_to_braille(x: f64, y: f64) -> (usize, usize, u32) {
    let col = (x / 2.0).floor() as usize;
    let row = (y / 4.0).floor() as usize;
    let xm = (x as usize) % 2;
    let ym = (y as usize) % 4;
    let dot = if ym < 3 { xm * 3 + ym } else { xm + 6 };
    (col, row, dot as u32)
}

pub fn rgb_to_ratatui(color: RgbColor) -> Color {
    Color::Rgb(color.0, color.1, color.2)
}

fn shade_color(color: Color, intensity: f64) -> Color {
    if let Color::Rgb(r, g, b) = color {
        Color::Rgb((r as f64 * intensity) as u8, (g as f64 * intensity) as u8, (b as f64 * intensity) as u8)
    } else { color }
}

struct BrailleGrid {
    chars: Vec<Vec<u32>>,
    colors: Vec<Vec<Option<Color>>>,
    cols: usize, rows: usize,
}

impl BrailleGrid {
    fn new(cols: usize, rows: usize) -> Self {
        BrailleGrid { chars: vec![vec![0x2800u32; cols]; rows], colors: vec![vec![None; cols]; rows], cols, rows }
    }
    fn set_dot(&mut self, x: f64, y: f64, color: Color) {
        let (col, row, dot) = canvas_to_braille(x, y);
        if col < self.cols && row < self.rows {
            self.chars[row][col] |= 1 << dot;
            if self.colors[row][col].is_none() { self.colors[row][col] = Some(color); }
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
                let hx = lx/ln; let hy = ly/ln; let hz = (lz/ln + 1.0)/2.0;
                let hn = f64::sqrt(hx*hx+hy*hy+hz*hz);
                let spec = ((nx*hx + ny*hy + z*hz)/hn).max(0.0).powi(16) * 0.6;
                let intensity = (0.25 + diffuse * 0.55 + spec).min(1.0);
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
        let dw = cc as f64 * 2.0; let dh = cr as f64 * 4.0;
        let mut grid = BrailleGrid::new(cc, cr);
        for line in &self.draw_commands.lines {
            grid.draw_line(line.x1, line.y1, line.x2, line.y2, Color::Gray);
        }
        for pt in &self.draw_commands.points {
            if pt.x < -pt.radius || pt.x > dw + pt.radius || pt.y < -pt.radius || pt.y > dh + pt.radius { continue; }
            grid.fill_circle(pt.x, pt.y, pt.radius, rgb_to_ratatui(pt.color));
        }
        for row in 0..cr.min(grid.rows) {
            for col in 0..cc.min(grid.cols) {
                let ch = char::from_u32(grid.chars[row][col]).unwrap_or(' ');
                let style = if let Some(c) = grid.colors[row][col] { Style::default().fg(c) } else { Style::default() };
                let x = area.x + col as u16; let y = area.y + row as u16;
                if let Some(cell) = buf.cell_mut((x, y)) { cell.set_char(ch).set_style(style); }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn braille_00() { let (c,r,d) = canvas_to_braille(0.,0.); assert_eq!((c,r,d),(0,0,0)); assert_eq!(0x2800|(1<<d),0x2801); }
    #[test] fn braille_10() { let (c,r,d) = canvas_to_braille(1.,0.); assert_eq!((c,r,d),(0,0,3)); assert_eq!(0x2800|(1<<d),0x2808); }
    #[test] fn braille_03_bottom() { let (_,_,d) = canvas_to_braille(0.,3.); assert_eq!(d,6); assert_eq!(0x2800|(1<<d),0x2840); }
    #[test] fn braille_two_dots() { let (c1,r1,d1)=canvas_to_braille(0.,0.); let (c2,r2,d2)=canvas_to_braille(1.,2.); assert_eq!((c1,r1),(c2,r2)); let comb=0x2800u32|(1<<d1)|(1<<d2); assert_eq!(comb,0x2801|0x20); }
    #[test] fn braille_floor() { let (c,r,d)=canvas_to_braille(3.7,8.2); assert_eq!(c,1); assert_eq!(r,2); assert_eq!(d,3); }
}
