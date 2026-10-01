//! Sixel encoder: converts an RGB pixel buffer to sixel escape sequences.

/// Encode an RGB pixel buffer as a sixel escape sequence.
pub fn encode_sixel(buf: &[[u8; 3]], width: usize, height: usize) -> String {
    assert!(height % 6 == 0, "sixel height must be multiple of 6");
    let sixel_rows = height / 6;
    let mut out = String::new();
    use std::fmt::Write;

    let _ = write!(out, "\x1bP0;0;0q");

    // Collect unique colors → palette
    let mut palette: Vec<[u8; 3]> = Vec::new();
    let mut last_color_idx: i32 = -1;

    for row in 0..sixel_rows {
        let y0 = row * 6;
        for x in 0..width {
            let mut sixel: u8 = 0;
            let mut color = [0u8; 3];
            let mut on = false;
            for dy in 0..6 {
                let rgb = buf[(y0 + dy) * width + x];
                color = rgb;
                let lum = 0.299 * rgb[0] as f64 + 0.587 * rgb[1] as f64 + 0.114 * rgb[2] as f64;
                if lum > 20.0 { sixel |= 1 << dy; on = true; }
            }
            if !on { continue; }

            let ci = palette.iter().position(|c| *c == color).unwrap_or_else(|| {
                palette.push(color);
                palette.len() - 1
            }) as i32;

            if ci != last_color_idx && (ci as usize) < 256 {
                // Color definition: palette index + RGB
                let _ = write!(out, "#{};2;{};{};{}", ci, color[0], color[1], color[2]);
                last_color_idx = ci;
            }
            out.push((0x3F + sixel) as u8 as char);
        }
        out.push('-');
    }
    out.push_str("\x1b\\");
    out
}

use chemrust_vis_core::viewport::DrawCommands;

pub fn render_to_pixels(cmds: &DrawCommands, px_w: usize, px_h: usize) -> (usize, usize, Vec<[u8; 3]>) {
    let height = (px_h / 6) * 6;
    let width = px_w;
    let mut buf = vec![[0u8; 3]; width * height];

    // Find the 2D bounding box of all draw commands
    let mut min_y = f64::INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    let mut min_x = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    for pt in &cmds.points {
        min_x = min_x.min(pt.x - pt.radius);
        max_x = max_x.max(pt.x + pt.radius);
        min_y = min_y.min(pt.y - pt.radius);
        max_y = max_y.max(pt.y + pt.radius);
    }
    for line in &cmds.lines {
        min_x = min_x.min(line.x1).min(line.x2);
        max_x = max_x.max(line.x1).max(line.x2);
        min_y = min_y.min(line.y1).min(line.y2);
        max_y = max_y.max(line.y1).max(line.y2);
    }
    if !min_x.is_finite() { return (width, height, buf); }

    // Center the content: compute offset to center the bounding box in the pixel buffer
    let content_w = max_x - min_x;
    let content_h = max_y - min_y;
    let margin = 0.9; // fill 90% of buffer
    let scale = ((width as f64 * margin) / content_w).min((height as f64 * margin) / content_h);
    let off_x = (width as f64 - content_w * scale) / 2.0 - min_x * scale;
    let off_y = (height as f64 - content_h * scale) / 2.0 - min_y * scale;

    for line in &cmds.lines {
        draw_line_px(&mut buf, width, height,
            line.x1 * scale + off_x, line.y1 * scale + off_y,
            line.x2 * scale + off_x, line.y2 * scale + off_y, [160;3]);
    }
    for pt in &cmds.points {
        fill_circle_px(&mut buf, width, height,
            pt.x * scale + off_x, pt.y * scale + off_y,
            pt.radius * scale,
            [pt.color.0, pt.color.1, pt.color.2]);
    }
    (width, height, buf)
}

fn fill_circle_px(buf: &mut [[u8; 3]], w: usize, h: usize, cx: f64, cy: f64, r: f64, base: [u8; 3]) {
    let ri = r.ceil() as i64;
    for dy in -ri..=ri {
        let py = cy as i64 + dy;
        if py < 0 || py >= h as i64 { continue; }
        for dx in -ri..=ri {
            let px = cx as i64 + dx;
            if px < 0 || px >= w as i64 { continue; }
            let d2 = (dx as f64).powi(2) + (dy as f64).powi(2);
            if d2 > r.powi(2) { continue; }
            let d = d2.sqrt() / r;
            let z = (1.0 - d.powi(2)).max(0.0).sqrt();
            let nx = dx as f64 / r; let ny = dy as f64 / r;
            let lx: f64 = 0.5; let ly: f64 = -0.5; let lz: f64 = 0.707;
            let ln = f64::sqrt(lx*lx+ly*ly+lz*lz);
            let dif = ((nx*lx + ny*ly + z*lz)/ln).max(0.0);
            let hx = lx/ln; let hy = ly/ln; let hz = (lz/ln+1.0)/2.0;
            let hn = f64::sqrt(hx*hx+hy*hy+hz*hz);
            let sp = ((nx*hx+ny*hy+z*hz)/hn).max(0.0).powi(16)*0.6;
            let int = (0.25 + dif*0.55 + sp).min(1.0);
            let idx = (py as usize) * w + (px as usize);
            buf[idx][0] = buf[idx][0].max((base[0] as f64 * int) as u8);
            buf[idx][1] = buf[idx][1].max((base[1] as f64 * int) as u8);
            buf[idx][2] = buf[idx][2].max((base[2] as f64 * int) as u8);
        }
    }
}

fn draw_line_px(buf: &mut [[u8; 3]], w: usize, h: usize, x1: f64, y1: f64, x2: f64, y2: f64, color: [u8; 3]) {
    let (mut x, mut y) = (x1 as i64, y1 as i64);
    let (x2i, y2i) = (x2 as i64, y2 as i64);
    let dx = (x2i - x).abs(); let dy = -(y2i - y).abs();
    let sx = if x < x2i { 1 } else { -1 }; let sy = if y < y2i { 1 } else { -1 };
    let mut err = dx + dy;
    loop {
        if x >= 0 && x < w as i64 && y >= 0 && y < h as i64 {
            buf[(y as usize) * w + (x as usize)] = color;
        }
        if x == x2i && y == y2i { break; }
        let e2 = 2 * err;
        if e2 >= dy { if x == x2i { break; } err += dy; x += sx; }
        if e2 <= dx { if y == y2i { break; } err += dx; y += sy; }
    }
}
