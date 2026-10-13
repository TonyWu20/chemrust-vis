//! Sixel encoder: converts an RGB pixel buffer to sixel escape sequences.

/// Encode an RGB pixel buffer as a sixel escape sequence.
pub fn encode_sixel(buf: &[[u8; 3]], width: usize, height: usize) -> String {
    assert!(height % 6 == 0, "sixel height must be multiple of 6");
    let sixel_rows = height / 6;
    let mut out = String::new();
    use std::fmt::Write;

    let _ = write!(out, "\x1bP0;0;0q");

    // Palette of unique colors. Sixel defines a color with
    // `#<n>;0;<r>;<g>;<b>` where the components are 0-100 percents
    // (the DEC spec range; WezTerm parses them as percents). In
    // 1-bit mode the defined color is not the foreground until a
    // `#<n>` select command is sent, so each definition is
    // followed by a select.
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
                let lum = 0.299 * rgb[0] as f64 + 0.587 * rgb[1] as f64 + 0.114 * rgb[2] as f64;
                if lum > 20.0 {
                    sixel |= 1 << dy;
                    on = true;
                    // The representative color of this column is the
                    // lowest on pixel, so assign only when on.
                    color = rgb;
                }
            }
            if !on { continue; }

            let ci = palette.iter().position(|c| *c == color).unwrap_or_else(|| {
                palette.push(color);
                palette.len() - 1
            }) as i32;

            if ci != last_color_idx && (ci as usize) < 256 {
                // Define the palette entry in 0-100 percent RGB, then
                // select it as the foreground color.
                let pct = |v: u8| (v as u32 * 100 / 255).min(100);
                let _ = write!(out, "#{};0;{};{};{}#{}", ci, pct(color[0]), pct(color[1]), pct(color[2]), ci);
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

pub(crate) fn fill_circle_px(buf: &mut [[u8; 3]], w: usize, h: usize, cx: f64, cy: f64, r: f64, base: [u8; 3]) {
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

pub(crate) fn draw_line_px(buf: &mut [[u8; 3]], w: usize, h: usize, x1: f64, y1: f64, x2: f64, y2: f64, color: [u8; 3]) {
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

// ── Cell pixel size ────────────────────────────────────────────────────────

/// Parse an xterm window-ops pixel-size report.
///
/// The `CSI 8 t` request (text area size in pixels) is answered with
/// `ESC[4;<height_px>;<width_px>t`. Returns (width_px, height_px).
pub fn parse_pixel_size_report(s: &str) -> Option<(u32, u32)> {
    let i = s.find("\x1b[4;")?;
    let fields: Vec<&str> = s[i + 4..].split(';').take(2).collect();
    let height: u32 = fields.first()?.parse().ok()?;
    // The width field carries the `t` final byte (`...960t`).
    let width: u32 = fields
        .get(1)?
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .ok()?;
    if width == 0 || height == 0 {
        return None;
    }
    Some((width, height))
}

/// Parse a `--sixel-px` value of the form `<W>x<H>`.
pub fn parse_px_cell(s: &str) -> Option<(u32, u32)> {
    let (w, h) = s.split_once('x')?;
    let w: u32 = w.trim().parse().ok()?;
    let h: u32 = h.trim().parse().ok()?;
    if w == 0 || h == 0 {
        return None;
    }
    Some((w, h))
}

/// Query the terminal for its text area pixel size via the xterm
/// `CSI 8 t` extension, and derive pixels per cell.
///
/// xterm, iTerm2, and foot answer by default. WezTerm answers only
/// when its `resize_reporting` config option is enabled. Must run
/// with stdin in raw mode (the response has no newline). Returns
/// pixels per cell, or None when the terminal does not report. The
/// caller falls back to 12x24 in that case.
#[cfg(unix)]
pub fn query_cell_pixels(cols: u16, rows: u16) -> Option<(u32, u32)> {
    use std::io::Write;
    use std::os::unix::io::AsRawFd;
    if cols == 0 || rows == 0 {
        return None;
    }
    let mut stdout = std::io::stdout();
    if stdout.write_all(b"\x1b[8t").is_err() {
        return None;
    }
    let _ = stdout.flush();
    let fd = std::io::stdin().as_raw_fd();
    // Make the fd non-blocking for the read so a terminal that does
    // not report times out. The original flags are restored.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 {
        return None;
    }
    let _ = unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) };
    let result = read_pixel_report(fd);
    let _ = unsafe { libc::fcntl(fd, libc::F_SETFL, flags) };
    // Convert the text-area pixel size to pixels per cell.
    result.map(|(w_px, h_px)| (w_px / cols as u32, h_px / rows as u32))
}

/// Read stdin (non-blocking) until a pixel-size report arrives or
/// 300 ms elapse, then parse it.
#[cfg(unix)]
fn read_pixel_report(fd: libc::c_int) -> Option<(u32, u32)> {
    use std::time::{Duration, Instant};
    let mut acc: Vec<u8> = Vec::with_capacity(64);
    let mut buf = [0u8; 32];
    let deadline = Instant::now() + Duration::from_millis(300);
    let mut pollfd = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    loop {
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        // Poll in small slices. A zero return ends the wait: the
        // report either arrived (parsed below) or did not come.
        let timeout_ms = (deadline - now).as_millis().min(100) as i32;
        let n = unsafe { libc::poll(&mut pollfd, 1, timeout_ms) };
        if n < 0 {
            break;
        }
        if n == 0 {
            break;
        }
        let r = unsafe {
            libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len())
        };
        if r < 0 {
            let e = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
            if e == libc::EAGAIN || e == libc::EWOULDBLOCK {
                continue;
            }
            break;
        }
        if r == 0 {
            break;
        }
        acc.extend_from_slice(&buf[..r as usize]);
        let text = String::from_utf8_lossy(&acc);
        if parse_pixel_size_report(&text).is_some() || acc.len() > 256 {
            break;
        }
    }
    let text = String::from_utf8_lossy(&acc);
    let (w_px, h_px) = parse_pixel_size_report(&text)?;
    Some((w_px, h_px))
}

/// Non-unix stub: the pixel-size report is not available.
#[cfg(not(unix))]
pub fn query_cell_pixels(_cols: u16, _rows: u16) -> Option<(u32, u32)> {
    None
}

// ── Terminal detection ─────────────────────────────────────────────────────

/// Detect whether the current terminal renders sixel, from the
/// environment.
///
/// Known sixel-capable signals: `TERM_PROGRAM=iTerm.app`, `TERM`
/// starting with `xterm` (the real xterm), `TERM=foot`, and tmux
/// sessions (pass-through, which depends on the outer terminal).
/// Terminals on the kitty-protocol side (kitty, WezTerm, Ghostty) do
/// not implement sixel and are rejected first; `xterm-ghostty`
/// starts with `xterm`, so the order matters.
///
/// This is a heuristic, in the same class as `kitty::detect_kitty`.
pub fn terminal_supports_sixel() -> bool {
    let term_program = std::env::var("TERM_PROGRAM").ok();
    let term = std::env::var("TERM").ok();
    let in_tmux = std::env::var_os("TMUX").is_some();
    detect_sixel(term_program.as_deref(), term.as_deref(), in_tmux)
}

/// Pure detection logic, split out for testing without touching env vars.
pub fn detect_sixel(
    term_program: Option<&str>,
    term: Option<&str>,
    in_tmux: bool,
) -> bool {
    // Terminals that only implement the kitty graphics protocol
    // (kitty, Ghostty) do not render sixel. WezTerm implements both.
    // Check this first: `xterm-ghostty` starts with `xterm` and
    // would otherwise look like the xterm sixel case.
    let term_program = term_program.map(|s| s.to_ascii_lowercase());
    let term = term.map(|s| s.to_ascii_lowercase());
    let kitty_only = matches!(
        term_program.as_deref(),
        Some("kitty") | Some("ghostty")
    ) || term.as_deref().is_some_and(|t| {
        t.contains("ghostty") || t == "kitty" || t.contains("xterm-kitty")
    });
    if kitty_only {
        return false;
    }
    if let Some(tp) = term_program {
        if tp == "iterm.app" || tp == "wezterm" {
            return true;
        }
    }
    if let Some(t) = term {
        let t = t.to_ascii_lowercase();
        if t.starts_with("xterm") || t == "foot" {
            return true;
        }
    }
    in_tmux
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_sixel_uses_percent_colors_and_selects_them() {
        // 6x6 px: one gray band on top, black below.
        let mut buf = vec![[0u8; 3]; 36];
        for px in buf.iter_mut().take(6) {
            *px = [160, 160, 160];
        }
        let data = encode_sixel(&buf, 6, 6);
        // The first palette entry is index 0. Define gray 160 as a
        // 0-100 percent value (62), then select it as the foreground.
        // Without the select, WezTerm keeps its default green
        // foreground.
        assert!(data.contains("#0;0;62;62;62#0"), "percent define + select");
        assert!(!data.contains(";2;"), "raw 0-255 form is gone");
        assert_eq!(data.matches('-').count(), 1, "one newline per row");
    }

    #[test]
    fn parse_pixel_size_report_values() {
        assert_eq!(parse_pixel_size_report("\x1b[4;576;960t"), Some((960, 576)));
        assert_eq!(parse_pixel_size_report("\x1b[4;400;800tjunk"), Some((800, 400)));
        assert_eq!(parse_pixel_size_report("nothing"), None);
        assert_eq!(parse_pixel_size_report("\x1b[4;;t"), None);
    }

    #[test]
    fn parse_px_cell_values() {
        assert_eq!(parse_px_cell("12x24"), Some((12, 24)));
        assert_eq!(parse_px_cell("24x48"), Some((24, 48)));
        assert_eq!(parse_px_cell("banana"), None);
        assert_eq!(parse_px_cell("0x24"), None);
    }

    #[test]
    fn detect_sixel_env_signals() {
        // Known sixel-capable terminals.
        assert!(detect_sixel(Some("iTerm.app"), Some("xterm-256color"), false));
        assert!(detect_sixel(None, Some("xterm"), false));
        assert!(detect_sixel(None, Some("foot"), false));
        assert!(detect_sixel(None, Some("xterm-256color"), true), "tmux pass-through");
        // Kitty-protocol-only terminals: no sixel, including xterm-ghostty.
        assert!(!detect_sixel(Some("ghostty"), Some("xterm-ghostty"), false));
        assert!(!detect_sixel(Some("kitty"), Some("kitty"), false));
        assert!(!detect_sixel(None, Some("xterm-ghostty"), false));
        // WezTerm implements sixel.
        assert!(detect_sixel(Some("WezTerm"), None, false));
        // Unknown plain terminal, no tmux: no sixel signal.
        assert!(!detect_sixel(Some("Alacritty"), Some("alacritty"), false));
        assert!(!detect_sixel(None, None, false));
    }
}
