//! Kitty graphics protocol renderer: pixel-level 3D scene rendering.
//!
//! Encodes an RGB pixel buffer as kitty graphics protocol escape sequences
//! (APC `\x1b_G`). Supported by kitty, WezTerm, and Ghostty. Terminals
//! that do not implement the protocol ignore APC codes, so the escapes are
//! harmless there; the renderer falls back to half-block characters.
//!
//! Protocol reference:
//! https://sw.kovidgoyal.net/kitty/graphics-protocol/
//!
//! Encoding scheme (remote-client variant of the spec):
//! 1. Raw RGB (f=24) is base64 encoded and chunked into pieces of at most
//!    4096 bytes; all chunks but the last have a size that is a multiple
//!    of 4.
//! 2. The first chunk carries the full control data:
//!    `i=<id>,f=24,s=<w>,v=<h>,q=1,m=1`. Later chunks carry only `m`.
//! 3. A placement request `a=p,i=<id>` displays the stored image at the
//!    current cursor position, scaled to a `c` column x `r` row
//!    rectangle. `z=-1` places the image below the text layer so the
//!    status bar and axis indicator render on top of it. `C=1` keeps the
//!    cursor in place after placement.
//! 4. `q=1` suppresses the terminal's OK responses. This matters for a
//!    raw-stdin event loop: a terminal that answers requests writes
//!    `ESC_G i=<id>;OK` back on stdin, and the crossterm event reader
//!    parses those bytes as key events. The `=` character of the
//!    response used to hit the zoom key binding, which mutated the
//!    camera, marked the image dirty, and triggered a re-transmit that
//!    produced the next response: a self-sustaining feedback loop that
//!    zoomed the view out until the program exited, with the leftover
//!    response bytes leaking into the shell. See ADR-0002.

use chemrust_vis_core::viewport::DrawCommands;

use crate::tui::sixel;

/// Maximum base64 chunk size per spec. Must be a multiple of 4.
pub const MAX_CHUNK: usize = 4096;

/// Image id used by this renderer.
///
/// A fixed id is safe: the app runs in the alternate screen buffer, where
/// the terminal isolates its image store, and re-transmitting data for an
/// id replaces the stored image (all old placements are deleted first).
pub const IMAGE_ID: u32 = 1;

/// Image id for the 1x1 black clear tile (content-box delta path).
pub const BLACK_TILE_ID: u32 = 2;

/// The pixel-data transmission medium for frame images.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Medium {
    /// POSIX shared memory (`t=s`). Local sessions only: the terminal
    /// must read the segment on the same host.
    Shm,
    /// Raw base64 direct data (`t=d`, the spec default).
    Base64,
    /// Base64 with a zlib-compressed payload (`o=z`). For network
    /// sessions where the pty carries the bytes over a link.
    Base64Zlib,
}

impl Medium {
    /// Short tag for the status bar readout.
    pub fn tag(self) -> &'static str {
        match self {
            Medium::Shm => "shm",
            Medium::Base64 => "b64",
            Medium::Base64Zlib => "b64+z",
        }
    }
}

// ── Terminal detection ─────────────────────────────────────────────────────

/// Detect whether the current terminal implements the kitty graphics
/// protocol, from the environment.
///
/// Signals used:
/// - `KITTY_PID` is set: only kitty sets it.
/// - `TERM_PROGRAM` in {kitty, WezTerm, ghostty} (case-insensitive).
/// - `TERM` containing `ghostty` or `wezterm` (case-insensitive), which
///   covers Ghostty's `xterm-ghostty` terminfo and WezTerm's `TERM`
///   value.
///
/// This is a heuristic. WezTerm gates the protocol behind a config option
/// (`enable_kitty_graphics`) that env vars cannot see. Use
/// `--renderer kitty` to force the renderer and `--renderer braille` to
/// force the fallback.
pub fn terminal_supports_kitty() -> bool {
    let term_program = std::env::var("TERM_PROGRAM").ok();
    let term = std::env::var("TERM").ok();
    let has_kitty_pid = std::env::var_os("KITTY_PID").is_some();
    detect_kitty(term_program.as_deref(), term.as_deref(), has_kitty_pid)
}

/// Pure detection logic, split out for testing without touching env vars.
pub fn detect_kitty(term_program: Option<&str>, term: Option<&str>, has_kitty_pid: bool) -> bool {
    if has_kitty_pid {
        return true;
    }
    if let Some(tp) = term_program {
        if matches!(
            tp.to_ascii_lowercase().as_str(),
            "kitty" | "wezterm" | "ghostty"
        ) {
            return true;
        }
    }
    if let Some(t) = term {
        let t = t.to_ascii_lowercase();
        if t.contains("ghostty") || t.contains("wezterm") {
            return true;
        }
    }
    false
}

/// Choose the transmission medium for a session.
///
/// Explicit choices win: `compress` selects the zlib base64 path,
/// `allow_shm = false` forces plain base64. Otherwise:
/// - an SSH session gets plain base64. Shared memory does not cross
///   hosts: the terminal reads the segment locally, the app writes it
///   on the remote side.
/// - a local session on a known kitty-protocol terminal (kitty,
///   WezTerm, Ghostty) gets shared memory.
/// - anything else gets plain base64.
pub fn select_medium(
    compress: bool,
    allow_shm: bool,
    ssh: bool,
    local_known_terminal: bool,
) -> Medium {
    if compress {
        return Medium::Base64Zlib;
    }
    if !allow_shm {
        return Medium::Base64;
    }
    if ssh || !local_known_terminal {
        return Medium::Base64;
    }
    Medium::Shm
}

/// Read the environment and pick a medium.
///
/// `SSH_CLIENT` / `SSH_TTY` mark a remote shell. `KITTY_PID` is set
/// only by kitty; the `TERM_PROGRAM` / `TERM` signals cover WezTerm
/// and Ghostty.
pub fn select_medium_env(compress: bool, allow_shm: bool) -> Medium {
    let ssh = std::env::var_os("SSH_CLIENT").is_some() || std::env::var_os("SSH_TTY").is_some();
    let has_kitty_pid = std::env::var_os("KITTY_PID").is_some();
    let term_program = std::env::var("TERM_PROGRAM").ok();
    let term = std::env::var("TERM").ok();
    let local_known =
        has_kitty_pid || detect_kitty(term_program.as_deref(), term.as_deref(), false);
    select_medium(compress, allow_shm, ssh, local_known)
}

// ── Native pixel size ────────────────────────────────────────────

/// Query the terminal's text-area pixel size.
///
/// Read via the pty `TIOCGWINSZ` ioctl on the stdout file descriptor
/// (`ws_xpixel`, `ws_ypixel`). kitty, WezTerm, Ghostty, and xterm
/// fill these fields. Terminals that leave them zero, and stdout that
/// is not a tty, yield `None`; the caller then falls back to the 2x4
/// px-per-cell grid.
#[cfg(unix)]
pub fn terminal_pixel_size() -> Option<(u32, u32)> {
    use libc::{ioctl, winsize, STDOUT_FILENO, TIOCGWINSZ};

    let mut ws: winsize = unsafe { std::mem::zeroed() };
    let ok = unsafe { ioctl(STDOUT_FILENO, TIOCGWINSZ, &mut ws) };
    if ok < 0 {
        return None;
    }
    let w = u32::from(ws.ws_xpixel);
    let h = u32::from(ws.ws_ypixel);
    if w == 0 || h == 0 {
        return None;
    }
    Some((w, h))
}

/// The native pixel size is unavailable on non-Unix platforms.
#[cfg(not(unix))]
pub fn terminal_pixel_size() -> Option<(u32, u32)> {
    None
}

/// Linear scale of the terminal's native text-area resolution for
/// the interactive kitty image.
///
/// Full-scale frames are too heavy for camera drags: an 80x24 window
/// with a 960x480 px text area transmits ~1.7 MB of base64 per
/// re-transmit. At 0.5x the same window transmits ~430 KB. The
/// terminal scales the image to the placement rectangle, so display
/// size is unchanged; only the source resolution drops.
pub const NATIVE_FRAME_SCALE: f64 = 0.5;

/// Compute the pixel buffer size for one kitty frame.
///
/// When `native` reports the text area in pixels, the image spans the
/// full width and its height is proportional to `scene_rows` (the
/// status bar row is not covered), then scaled by
/// [`NATIVE_FRAME_SCALE`]. This matches the placement rectangle's
/// aspect, so no letterbox is needed. When the query fails or the
/// result is too small to be useful, the 2x4 px-per-cell grid is used
/// instead.
pub fn frame_pixel_size(
    cols: u16,
    rows: u16,
    scene_rows: u16,
    native: Option<(u32, u32)>,
) -> (usize, usize) {
    let fallback = (cols as usize * 2, scene_rows as usize * 4);
    let Some((w, h)) = native else {
        return fallback;
    };
    if rows == 0 {
        return fallback;
    }
    let cell_h = h / (rows as u32);
    if cell_h < 2 {
        return fallback;
    }
    let px_w = (f64::from(w) * NATIVE_FRAME_SCALE).round() as usize;
    let px_h = ((f64::from(cell_h) * f64::from(scene_rows)) * NATIVE_FRAME_SCALE).round() as usize;
    if px_w < 8 || px_h < 4 {
        return fallback;
    }
    (px_w, px_h)
}

// ── Base64 ──────────────────────────────────────────────────────────────────

/// Standard base64 alphabet.
const B64_ALPHABET: [u8; 64] = *b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Base64 encode a byte slice (standard alphabet, `=` padding).
///
/// Hand-rolled to keep the TUI crate dependency-free. The payload is a
/// single frame of raw RGB, a few megabytes at most.
pub fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(B64_ALPHABET[(n >> 18) as usize] as char);
        out.push(B64_ALPHABET[((n >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            out.push(B64_ALPHABET[((n >> 6) & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(B64_ALPHABET[(n & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

// ── Escape sequence builders ───────────────────────────────────────────────

/// Encode an RGB pixel buffer as chunked kitty transmission escapes.
///
/// The sequence stores the image under `image_id`. It does not display
/// it; follow it with [`encode_placement`].
pub fn encode_transmission_rgb(
    pixels: &[[u8; 3]],
    width: u32,
    height: u32,
    image_id: u32,
) -> Vec<u8> {
    let bytes: Vec<u8> = pixels.iter().flat_map(|p| p.iter().copied()).collect();
    encode_transmission(&bytes, width, height, image_id)
}

/// Encode raw RGB bytes as chunked kitty transmission escapes.
///
/// Per spec (remote client, chunked):
/// - First escape: `\x1b_Gi=<id>,f=24,s=<w>,v=<h>,q=1,m=<1|0>;<chunk>\x1b\`
/// - Middle escapes: `\x1b_Gm=<1>;<chunk>\x1b\`
/// - Final escape: `\x1b_Gm=<0>;<chunk>\x1b\`
///
/// `q=1` suppresses the terminal's OK response to the stored image, so
/// no protocol bytes are written back on stdin (see the module docs).
pub fn encode_transmission(bytes: &[u8], width: u32, height: u32, image_id: u32) -> Vec<u8> {
    let b64 = base64_encode(bytes);
    let control = format!("i={image_id},f=24,s={width},v={height},q=1");
    encode_control_with_payload(&control, &b64)
}

/// Encode raw RGB bytes as a base64 transmission with a
/// zlib-compressed payload (`o=z`).
///
/// `S=` carries the original (uncompressed) byte size, per spec. The
/// terminal inflates before the base64 decode. `q=1` as always.
pub fn encode_transmission_zlib(
    pixels: &[[u8; 3]],
    width: u32,
    height: u32,
    image_id: u32,
) -> Vec<u8> {
    use flate2::write::ZlibEncoder;
    use flate2::Compression;

    let raw: Vec<u8> = pixels.iter().flat_map(|p| p.iter().copied()).collect();
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::new(3));
    std::io::Write::write_all(&mut encoder, &raw).expect("zlib encoder");
    let compressed = encoder.finish().expect("zlib finish");

    let control = format!(
        "i={image_id},f=24,s={width},v={height},o=z,q=1,S={}",
        raw.len()
    );
    let b64 = base64_encode(&compressed);
    encode_control_with_payload(&control, &b64)
}

/// Chunk a base64 payload under a control-data string.
///
/// The first chunk carries the control data plus `m=1` (or `m=0` when
/// it is the only chunk); later chunks carry only `m`. Shared by the
/// raw-base64 and zlib-base64 encoders.
fn encode_control_with_payload(control: &str, b64: &str) -> Vec<u8> {
    let total = b64.len().div_ceil(MAX_CHUNK).max(1);
    let mut out = Vec::with_capacity(b64.len() + 64 * total + 64);
    for (i, chunk) in b64.as_bytes().chunks(MAX_CHUNK).enumerate() {
        let m = if i + 1 < total { 1 } else { 0 };
        if i == 0 {
            out.extend_from_slice(format!("\x1b_G{control},m={m};").as_bytes());
        } else {
            out.extend_from_slice(format!("\x1b_Gm={m};").as_bytes());
        }
        out.extend_from_slice(chunk);
        out.extend_from_slice(b"\x1b\\");
    }
    out
}

/// Encode a placement request: display image `image_id` at the current
/// cursor position, scaled to `cols` x `rows` cells, below the text
/// layer, without moving the cursor.
///
/// `q=1` suppresses the placement response (`i=<id>,p=<pid>;OK`),
/// which would otherwise be written back on stdin and parsed by the
/// crossterm event reader as key events (see the module docs).
pub fn encode_placement(image_id: u32, cols: u16, rows: u16) -> Vec<u8> {
    format!("\x1b_Ga=p,i={image_id},c={cols},r={rows},z=-1,C=1,q=1\x1b\\").into_bytes()
}

/// Encode a shared-memory transmission escape (`t=s`).
///
/// Creates a POSIX shared memory segment named
/// `/chemrust-vis-tui-<pid>-<seq>` (spec: one leading slash, no
/// other slashes) and fills it with the raw RGB bytes. The escape
/// carries the segment name base64-encoded; `S=` is the byte size.
///
/// The terminal reads the segment and unlinks it. The creator must
/// not unlink, so the handle is dropped as a non-owner: the drop
/// unmaps and closes, but the segment survives until the terminal
/// unlinks it.
pub fn encode_transmission_shm(
    pixels: &[[u8; 3]],
    width: u32,
    height: u32,
    image_id: u32,
    seq: u64,
) -> Result<Vec<u8>, shared_memory::ShmemError> {
    let bytes: Vec<u8> = pixels.iter().flat_map(|p| p.iter().copied()).collect();
    let name = format!("/chemrust-vis-tui-{}-{}", std::process::id(), seq);
    let mut shm = shared_memory::ShmemConf::new()
        .os_id(&name)
        .size(bytes.len())
        .create()?;
    // The terminal unlinks after reading; a creator unlink on drop
    // could race the terminal's map.
    shm.set_owner(false);
    unsafe {
        std::slice::from_raw_parts_mut(shm.as_ptr(), bytes.len()).copy_from_slice(&bytes);
    }
    let os_id = shm.get_os_id().to_string();
    let payload = base64_encode(os_id.as_bytes());
    let mut out = format!(
        "\x1b_Gi={image_id},f=24,s={width},v={height},q=1,t=s,S={};{payload}\x1b\\",
        bytes.len()
    )
    .into_bytes();
    // The handle unmaps on drop; the segment itself persists for
    // the terminal to read and unlink.
    drop(shm);
    Ok(out)
}

/// Encode the 1x1 black clear tile (image id [`BLACK_TILE_ID`]).
///
/// The terminal scales the 3-byte image to the placement rectangle,
/// so one tile clears a region of any size. Always sent base64: the
/// bytes do not justify shared memory.
pub fn encode_black_tile() -> Vec<u8> {
    encode_transmission_rgb(&[[0, 0, 0]], 1, 1, BLACK_TILE_ID)
}

/// Cursor move to cell (col, row), 0-based as in crossterm.
///
/// Placements happen at the cursor position, so region placements
/// move the cursor first.
pub fn encode_move_to(col: u16, row: u16) -> Vec<u8> {
    format!("\x1b[{};{}H", row + 1, col + 1).into_bytes()
}

/// Encode the delete action that removes all visible placements.
///
/// Sent on exit. Leaving the alternate screen also clears its images
/// (per spec), so this is belt and braces.
pub fn encode_delete_all() -> Vec<u8> {
    b"\x1b_Ga=d\x1b\\".to_vec()
}

// ── Rasterizer ──────────────────────────────────────────────────────────────

/// Rasterize draw commands into an RGB pixel buffer of `px_w` x `px_h`.
///
/// Coordinates in `cmds` are in the same units as the pixel buffer
/// (the caller sizes the viewport in pixel units, so 1 dot = 1 pixel).
/// Lines render first, then points in depth-sorted order.
pub fn rasterize(cmds: &DrawCommands, px_w: usize, px_h: usize) -> Vec<[u8; 3]> {
    let mut buf = vec![[0u8; 3]; px_w * px_h];
    for line in &cmds.lines {
        sixel::draw_line_px(
            &mut buf,
            px_w,
            px_h,
            line.x1,
            line.y1,
            line.x2,
            line.y2,
            [140, 140, 140],
        );
    }
    for pt in &cmds.points {
        // Clamp the radius: a 1 pixel sphere is not visible, and the
        // viewport's minimum radius of 1 dot unit is meant for text
        // renderers, not pixels.
        let r = pt.radius.max(2.0);
        sixel::fill_circle_px(
            &mut buf,
            px_w,
            px_h,
            pt.x,
            pt.y,
            r,
            [pt.color.0, pt.color.1, pt.color.2],
        );
    }
    buf
}

/// Rasterize draw commands into an RGB buffer of `rw` x `rh`, with
/// canvas coordinates translated by (-ox, -oy).
///
/// The content-box delta path: only the region's pixels are touched,
/// so the cost is proportional to the region area, not the canvas.
pub fn rasterize_region(
    cmds: &DrawCommands,
    ox: f64,
    oy: f64,
    rw: usize,
    rh: usize,
) -> Vec<[u8; 3]> {
    let mut buf = vec![[0u8; 3]; rw * rh];
    for line in &cmds.lines {
        sixel::draw_line_px(
            &mut buf,
            rw,
            rh,
            line.x1 - ox,
            line.y1 - oy,
            line.x2 - ox,
            line.y2 - oy,
            [140, 140, 140],
        );
    }
    for pt in &cmds.points {
        let r = pt.radius.max(2.0);
        sixel::fill_circle_px(
            &mut buf,
            rw,
            rh,
            pt.x - ox,
            pt.y - oy,
            r,
            [pt.color.0, pt.color.1, pt.color.2],
        );
    }
    buf
}

/// The content bounding box in canvas pixel coordinates, padded by 1
/// pixel and clamped to the canvas.
///
/// Points contribute their shaded radius, lines their endpoints.
/// `None` when there is no content.
pub fn content_bbox(cmds: &DrawCommands, px_w: u32, px_h: u32) -> Option<(i32, i32, i32, i32)> {
    let (mut min_x, mut max_x) = (f64::INFINITY, f64::NEG_INFINITY);
    let (mut min_y, mut max_y) = (f64::INFINITY, f64::NEG_INFINITY);
    for pt in &cmds.points {
        let r = pt.radius.max(2.0);
        min_x = min_x.min(pt.x - r);
        max_x = max_x.max(pt.x + r);
        min_y = min_y.min(pt.y - r);
        max_y = max_y.max(pt.y + r);
    }
    for line in &cmds.lines {
        min_x = min_x.min(line.x1).min(line.x2);
        max_x = max_x.max(line.x1).max(line.x2);
        min_y = min_y.min(line.y1).min(line.y2);
        max_y = max_y.max(line.y1).max(line.y2);
    }
    if !min_x.is_finite() {
        return None;
    }
    let x0 = (min_x - 1.0).max(0.0).floor() as i32;
    let x1 = (max_x + 1.0).min(f64::from(px_w)).ceil() as i32;
    let y0 = (min_y - 1.0).max(0.0).floor() as i32;
    let y1 = (max_y + 1.0).min(f64::from(px_h)).ceil() as i32;
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    Some((x0, y0, x1 - x0, y1 - y0))
}

/// Map a content region in canvas pixels to a cell rectangle,
/// rounded outward.
///
/// The canvas of `px_w` x `px_h` pixels maps onto `cols` x
/// `scene_rows` cells. Returns (c0, r0, c, r).
pub fn region_to_cells(
    ox: i32,
    oy: i32,
    rw: i32,
    rh: i32,
    px_w: u32,
    px_h: u32,
    cols: u16,
    scene_rows: u16,
) -> (u16, u16, u16, u16) {
    let ppc = f64::from(px_w) / f64::from(cols.max(1));
    let ppr = f64::from(px_h) / f64::from(scene_rows.max(1));
    let c0 = ((f64::from(ox) / ppc).floor().max(0.0)) as u16;
    let c1 = ((f64::from(ox + rw) / ppc).ceil().min(f64::from(cols))) as u16;
    let r0 = ((f64::from(oy) / ppr).floor().max(0.0)) as u16;
    let r1 = ((f64::from(oy + rh) / ppr).ceil().min(f64::from(scene_rows))) as u16;
    let c = c1.saturating_sub(c0).max(1);
    let r = r1.saturating_sub(r0).max(1);
    (c0, r0, c, r)
}

/// Whether the old cell rectangle needs a black-tile clear before
/// placing the new one: when the new rectangle does not fully
/// contain the old one.
///
/// Rectangles are (c0, r0, c, r).
pub fn should_clear(new: (u16, u16, u16, u16), old: (u16, u16, u16, u16)) -> bool {
    let (nc0, nr0, nc, nr) = new;
    let (oc0, or0, oc, orr) = old;
    !(nc0 <= oc0 && nr0 <= or0 && nc0 + nc >= oc0 + oc && nr0 + nr >= or0 + orr)
}

// ── Renderer state ──────────────────────────────────────────────────────────

/// Interactive renderer: produces the full escape sequence for one frame.
pub struct KittyRenderer {
    image_id: u32,
    medium: Medium,
    /// A shared-memory create failed this session; the effective
    /// medium dropped to plain base64.
    shm_failed: bool,
    /// The 1x1 black tile was transmitted this session.
    black_sent: bool,
    /// The cell rectangle (c0, r0, c, r) of the last placed content
    /// image, or of the full canvas when the last frame was full.
    last_rect: Option<(u16, u16, u16, u16)>,
    /// Sequence number for shared-memory segment names.
    shm_seq: u64,
}

/// The content region covers this share of the canvas before the
/// crop path gives up and sends the full canvas instead.
const CROP_AREA_LIMIT: u32 = 95;

impl KittyRenderer {
    pub fn new() -> Self {
        KittyRenderer {
            image_id: IMAGE_ID,
            medium: Medium::Base64,
            shm_failed: false,
            black_sent: false,
            last_rect: None,
            shm_seq: 0,
        }
    }

    /// Create a renderer that transmits pixel data over `medium`.
    pub fn with_medium(medium: Medium) -> Self {
        KittyRenderer {
            medium,
            ..Self::new()
        }
    }

    /// The medium in effect for status-bar display.
    pub fn medium_tag(&self) -> &'static str {
        self.effective_medium().tag()
    }

    /// Drop placement state after a delete-all (terminal resize,
    /// frame-size change). The black tile is re-sent on demand.
    pub fn reset_placement_state(&mut self) {
        self.last_rect = None;
        self.black_sent = false;
    }

    fn effective_medium(&self) -> Medium {
        if self.medium == Medium::Shm && self.shm_failed {
            Medium::Base64
        } else {
            self.medium
        }
    }

    /// Build the frame sequence: an optional black-tile clear of the
    /// old cell rectangle, the content transmission, and the
    /// placement.
    ///
    /// The caller writes the bytes with the cursor at the top-left of
    /// the scene area. `crop_ok` enables the content-box delta path:
    /// rasterize only the content's cell rectangle and clear the old
    /// one with the black tile. It needs the native pixel mapping
    /// (cell sizes in pixels are known); without it the full canvas
    /// is sent.
    ///
    /// An empty scene with no live placement produces an empty
    /// sequence: nothing changes, nothing is sent.
    pub fn frame(
        &mut self,
        cmds: &DrawCommands,
        px_w: usize,
        px_h: usize,
        cols: u16,
        scene_rows: u16,
        crop_ok: bool,
    ) -> Vec<u8> {
        let full = (0u16, 0u16, cols, scene_rows);
        let plan = if crop_ok {
            match content_bbox(cmds, px_w as u32, px_h as u32) {
                Some((ox, oy, rw, rh)) => {
                    let rect =
                        region_to_cells(ox, oy, rw, rh, px_w as u32, px_h as u32, cols, scene_rows);
                    let area = u32::from(rect.2) * u32::from(rect.3);
                    let full_area = u32::from(cols) * u32::from(scene_rows);
                    if full_area > 0 && area * 100 >= full_area * CROP_AREA_LIMIT {
                        FramePlan::Full
                    } else {
                        FramePlan::Region(rect)
                    }
                }
                None => FramePlan::Empty,
            }
        } else {
            FramePlan::Full
        };

        let mut out = Vec::new();
        match plan {
            FramePlan::Empty => {
                if self.last_rect.is_some() {
                    out.extend(self.clear_escapes());
                    self.last_rect = None;
                }
            }
            FramePlan::Full => {
                // The full-canvas image is opaque black in the
                // content-free areas: it covers any old rectangle.
                // The clear still runs on a region-to-full
                // transition: the old placement re-renders the
                // replaced image data until the new placement lands.
                if self.last_rect.is_some() && self.last_rect != Some(full) {
                    out.extend(self.clear_escapes());
                }
                let pixels = rasterize(cmds, px_w, px_h);
                self.transmit(&mut out, &pixels, px_w, px_h);
                out.extend(encode_placement(self.image_id, cols, scene_rows));
                self.last_rect = Some(full);
            }
            FramePlan::Region(rect) => {
                if let Some(old) = self.last_rect {
                    if should_clear(rect, old) {
                        out.extend(self.clear_escapes());
                    }
                }
                let (c0, r0, c, r) = rect;
                let ppc = px_w as f64 / f64::from(cols.max(1));
                let ppr = px_h as f64 / f64::from(scene_rows.max(1));
                let rw = (f64::from(c) * ppc).round().max(1.0) as usize;
                let rh = (f64::from(r) * ppr).round().max(1.0) as usize;
                let ox = f64::from(c0) * ppc;
                let oy = f64::from(r0) * ppr;
                let pixels = rasterize_region(cmds, ox, oy, rw, rh);
                self.transmit(&mut out, &pixels, rw, rh);
                out.extend(encode_move_to(c0, r0));
                out.extend(encode_placement(self.image_id, c, r));
                self.last_rect = Some(rect);
            }
        }
        out
    }

    /// Black-tile clear of the last placed rectangle: the tile
    /// transmission (once) and its placement over the old rectangle.
    fn clear_escapes(&mut self) -> Vec<u8> {
        let (c0, r0, c, r) = self.last_rect.expect("called with a live rectangle");
        let mut out = Vec::new();
        if !self.black_sent {
            out.extend(encode_black_tile());
            self.black_sent = true;
        }
        out.extend(encode_move_to(c0, r0));
        out.extend(encode_placement(BLACK_TILE_ID, c, r));
        out
    }

    /// Append the pixel-data transmission in the effective medium.
    ///
    /// A shared-memory create failure downgrades the session to
    /// plain base64 for the rest of the run.
    fn transmit(&mut self, out: &mut Vec<u8>, pixels: &[[u8; 3]], w: usize, h: usize) {
        match self.effective_medium() {
            Medium::Shm => {
                self.shm_seq += 1;
                match encode_transmission_shm(
                    pixels,
                    w as u32,
                    h as u32,
                    self.image_id,
                    self.shm_seq,
                ) {
                    Ok(seq) => out.extend(seq),
                    Err(_) => {
                        self.shm_failed = true;
                        let raw: Vec<u8> = pixels.iter().flat_map(|p| p.iter().copied()).collect();
                        out.extend(encode_transmission(&raw, w as u32, h as u32, self.image_id));
                    }
                }
            }
            Medium::Base64 => {
                let raw: Vec<u8> = pixels.iter().flat_map(|p| p.iter().copied()).collect();
                out.extend(encode_transmission(&raw, w as u32, h as u32, self.image_id));
            }
            Medium::Base64Zlib => {
                out.extend(encode_transmission_zlib(
                    pixels,
                    w as u32,
                    h as u32,
                    self.image_id,
                ));
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum FramePlan {
    /// No content: clear the live rectangle if any, send nothing.
    Empty,
    /// Rasterize and place the full canvas.
    Full,
    /// Rasterize and place the content's cell rectangle.
    Region((u16, u16, u16, u16)),
}

impl Default for KittyRenderer {
    fn default() -> Self {
        Self::new()
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Reference decoder, for round-trip checks only.
    fn base64_decode(s: &str) -> Option<Vec<u8>> {
        fn val(c: u8) -> Option<u8> {
            B64_ALPHABET.iter().position(|&a| a == c).map(|i| i as u8)
        }
        let chars: Vec<u8> = s.bytes().filter(|&b| b != b'=').collect();
        if chars.iter().any(|&b| val(b).is_none()) {
            return None;
        }
        let mut bits: u32 = 0;
        let mut nbits = 0;
        let mut out = Vec::new();
        for &c in &chars {
            bits = (bits << 6) | val(c)? as u32;
            nbits += 6;
            if nbits >= 8 {
                nbits -= 8;
                out.push((bits >> nbits) as u8);
                bits &= (1u32 << nbits) - 1;
            }
        }
        Some(out)
    }

    /// Split an encoded sequence into (control, payload) pairs per escape.
    /// Each escape ends with `\x1b\`.
    fn split_escapes(seq: &[u8]) -> Vec<(String, String)> {
        let mut out = Vec::new();
        let mut rest = seq;
        while let Some(idx) = rest.windows(2).position(|w| w == b"\x1b\\") {
            let text = String::from_utf8(rest[..idx + 2].to_vec()).unwrap();
            let start = text.find("_G").unwrap() + 2;
            let body = &text[start..text.len() - 2];
            match body.find(';') {
                Some(sep) => out.push((body[..sep].to_string(), body[sep + 1..].to_string())),
                None => out.push((body.to_string(), String::new())),
            }
            rest = &rest[idx + 2..];
        }
        out
    }

    #[test]
    fn base64_known_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"A"), "QQ==");
        assert_eq!(base64_encode(b"AB"), "QUI=");
        assert_eq!(base64_encode(b"ABC"), "QUJD");
        assert_eq!(base64_encode(b"0123456789"), "MDEyMzQ1Njc4OQ==");
    }

    #[test]
    fn base64_round_trips() {
        let data: Vec<u8> = (0..517u32).map(|i| (i * 7) as u8).collect();
        let enc = base64_encode(&data);
        assert_eq!(base64_decode(&enc), Some(data));
    }

    #[test]
    fn transmission_single_chunk() {
        let bytes = vec![1u8, 2, 3, 4];
        let seq = encode_transmission(&bytes, 1, 1, 7);
        let escs = split_escapes(&seq);
        assert_eq!(escs.len(), 1);
        let (ctrl, payload) = &escs[0];
        assert_eq!(ctrl, "i=7,f=24,s=1,v=1,q=1,m=0");
        assert_eq!(base64_decode(payload), Some(vec![1, 2, 3, 4]));
    }

    #[test]
    fn transmission_multi_chunk_layout() {
        // 4000 bytes of RGB -> 5336 base64 chars -> 2 chunks
        // (4096 + 1240).
        let bytes: Vec<u8> = (0..4000u32).map(|i| (i % 251) as u8).collect();
        let seq = encode_transmission(&bytes, 1333, 3, 1);
        let escs = split_escapes(&seq);
        assert_eq!(escs.len(), 2);
        assert_eq!(escs[0].0, "i=1,f=24,s=1333,v=3,q=1,m=1");
        assert_eq!(escs[1].0, "m=0");
        assert_eq!(
            escs[0].1.len(),
            MAX_CHUNK,
            "non-final chunk fills MAX_CHUNK"
        );
        assert_eq!(
            escs[0].1.len() % 4,
            0,
            "non-final chunks are 4-byte aligned"
        );
        assert_eq!(escs[1].1.len(), 1240);
        // Concatenated payload decodes back to the original bytes.
        let joined = format!("{}{}", escs[0].1, escs[1].1);
        assert_eq!(base64_decode(&joined), Some(bytes));
    }

    #[test]
    fn transmission_big_payload_chunk_count() {
        // 40000 bytes -> 53336 base64 chars -> 14 chunks:
        // 13 full (4096) + 1 of 88.
        let bytes = vec![9u8; 40000];
        let seq = encode_transmission(&bytes, 400, 33, 1);
        let escs = split_escapes(&seq);
        assert_eq!(escs.len(), 14);
        for esc in &escs[..13] {
            assert!(esc.0.ends_with("m=1"), "non-final chunks say more");
            assert_eq!(esc.1.len(), MAX_CHUNK);
        }
        assert!(escs[13].0.ends_with("m=0"), "final chunk says done");
        assert_eq!(escs[13].1.len(), 88);
        let joined: String = escs.iter().map(|e| e.1.as_str()).collect();
        assert_eq!(base64_decode(&joined), Some(bytes));
    }

    #[test]
    fn transmission_exact_chunk_boundary() {
        // 3072 bytes -> exactly 4096 base64 chars -> 1 chunk of 4096.
        let bytes = vec![0u8; 3072];
        let seq = encode_transmission(&bytes, 1024, 3, 1);
        let escs = split_escapes(&seq);
        assert_eq!(escs.len(), 1);
        assert!(escs[0].0.ends_with("m=0"), "single chunk says done");
        assert_eq!(escs[0].1.len(), 4096);
    }

    #[test]
    fn placement_sequence_bytes() {
        let seq = encode_placement(3, 80, 23);
        assert_eq!(seq, b"\x1b_Ga=p,i=3,c=80,r=23,z=-1,C=1,q=1\x1b\\");
    }

    #[test]
    fn transmission_suppresses_terminal_responses() {
        // q=1 on the first chunk: the terminal must not write the
        // `i=<id>;OK` response back on stdin (feedback-loop guard).
        let seq = encode_transmission(&[0u8; 3], 1, 1, 1);
        let escs = split_escapes(&seq);
        assert!(escs[0].0.contains("q=1"), "first chunk sets q=1");
        assert!(escs[0].0.starts_with("i=1,"), "first chunk carries the id");
    }

    #[test]
    fn placement_suppresses_terminal_responses() {
        let seq = String::from_utf8(encode_placement(1, 80, 24)).unwrap();
        assert!(seq.contains("q=1"), "placement sets q=1");
    }

    #[test]
    fn delete_all_sequence_bytes() {
        assert_eq!(encode_delete_all(), b"\x1b_Ga=d\x1b\\");
    }

    #[test]
    fn detect_kitty_env_signals() {
        assert!(detect_kitty(None, None, true), "KITTY_PID wins");
        assert!(detect_kitty(Some("kitty"), None, false));
        assert!(detect_kitty(Some("WezTerm"), None, false));
        assert!(detect_kitty(Some("GHOSTTY"), None, false));
        assert!(detect_kitty(None, Some("xterm-ghostty"), false));
        assert!(detect_kitty(None, Some("WezTerm"), false));
        assert!(!detect_kitty(
            Some("iTerm.app"),
            Some("xterm-256color"),
            false
        ));
        assert!(!detect_kitty(None, Some("xterm-256color"), false));
        assert!(!detect_kitty(None, None, false));
    }

    #[test]
    fn rasterize_fills_circle_and_lines() {
        use chemrust_vis_core::scene::RgbColor;
        use chemrust_vis_core::viewport::{DrawCommands, DrawLine, DrawPoint};
        let cmds = DrawCommands {
            points: vec![DrawPoint {
                x: 5.0,
                y: 5.0,
                z_view: 0.0,
                radius: 2.0,
                color: RgbColor(255, 0, 0),
                atom_index: 0,
            }],
            lines: vec![DrawLine {
                x1: 0.0,
                y1: 0.0,
                x2: 9.0,
                y2: 0.0,
            }],
        };
        let buf = rasterize(&cmds, 10, 10);
        // Sphere center: shaded red (bright red, no green/blue).
        assert!(buf[5 * 10 + 5][0] > 100, "sphere center is red");
        assert_eq!(buf[5 * 10 + 5][1], 0);
        assert_eq!(buf[5 * 10 + 5][2], 0);
        // A line pixel is exactly the line gray.
        assert_eq!(buf[0 * 10 + 4], [140, 140, 140]);
        // A corner far from everything is black.
        assert_eq!(buf[9 * 10 + 9], [0, 0, 0]);
    }

    #[test]
    fn frame_is_transmission_plus_placement() {
        let mut r = KittyRenderer::new();
        let cmds = DrawCommands {
            points: vec![],
            lines: vec![],
        };
        let seq = r.frame(&cmds, 2, 2, 10, 5, false);
        let escs = split_escapes(&seq);
        assert_eq!(escs.len(), 2, "single-chunk transmission + placement");
        assert!(
            escs[1].0.starts_with("a=p,i=1,"),
            "second escape is a placement"
        );
        assert!(escs[1].0.contains("c=10"), "placement width");
        assert!(escs[1].0.contains("r=5"), "placement height");
    }

    fn test_point(x: f64, y: f64, r: f64) -> DrawCommands {
        use chemrust_vis_core::scene::RgbColor;
        use chemrust_vis_core::viewport::DrawPoint;
        DrawCommands {
            points: vec![DrawPoint {
                x,
                y,
                z_view: 0.0,
                radius: r,
                color: RgbColor(255, 0, 0),
                atom_index: 0,
            }],
            lines: vec![],
        }
    }

    #[test]
    fn medium_tags() {
        assert_eq!(Medium::Shm.tag(), "shm");
        assert_eq!(Medium::Base64.tag(), "b64");
        assert_eq!(Medium::Base64Zlib.tag(), "b64+z");
    }

    #[test]
    fn select_medium_rules() {
        // Explicit choices.
        assert_eq!(select_medium(true, true, false, true), Medium::Base64Zlib);
        assert_eq!(select_medium(false, false, false, true), Medium::Base64);
        assert_eq!(select_medium(true, false, true, false), Medium::Base64Zlib);
        // SSH: no shared memory across hosts.
        assert_eq!(select_medium(false, true, true, true), Medium::Base64);
        // Local known terminal: shared memory.
        assert_eq!(select_medium(false, true, false, true), Medium::Shm);
        // Local unknown terminal: base64.
        assert_eq!(select_medium(false, true, false, false), Medium::Base64);
    }

    #[test]
    fn content_bbox_point_and_line() {
        use chemrust_vis_core::scene::RgbColor;
        use chemrust_vis_core::viewport::{DrawCommands, DrawLine, DrawPoint};
        let cmds = DrawCommands {
            points: vec![DrawPoint {
                x: 50.0,
                y: 40.0,
                z_view: 0.0,
                radius: 5.0,
                color: RgbColor(255, 0, 0),
                atom_index: 0,
            }],
            lines: vec![DrawLine {
                x1: 10.0,
                y1: 10.0,
                x2: 90.0,
                y2: 10.0,
            }],
        };
        // Point: 50/40 +- 5, plus the 1 pixel pad. Line: 10..90,
        // 10..10, plus pad. Union: x 9..91, y 9..46.
        assert_eq!(content_bbox(&cmds, 200, 100), Some((9, 9, 82, 37)));
        // Clamping at the canvas edge.
        let edge = DrawCommands {
            points: vec![DrawPoint {
                x: 200.0,
                y: 0.0,
                z_view: 0.0,
                radius: 10.0,
                color: RgbColor(255, 0, 0),
                atom_index: 0,
            }],
            lines: vec![],
        };
        assert_eq!(content_bbox(&edge, 200, 100), Some((189, 0, 11, 11)));
    }

    #[test]
    fn content_bbox_empty() {
        let cmds = DrawCommands {
            points: vec![],
            lines: vec![],
        };
        assert_eq!(content_bbox(&cmds, 100, 100), None);
    }

    #[test]
    fn region_to_cells_rounds_outward() {
        // A 100x50 canvas on 10x5 cells: 10 px per cell and row.
        assert_eq!(region_to_cells(11, 11, 8, 8, 100, 50, 10, 5), (1, 1, 1, 1));
        // Touching a cell boundary at x=10 rounds into cell 1.
        assert_eq!(region_to_cells(10, 0, 1, 50, 100, 50, 10, 5), (1, 0, 1, 5));
        // A full-canvas region maps to the full grid.
        assert_eq!(
            region_to_cells(0, 0, 100, 50, 100, 50, 10, 5),
            (0, 0, 10, 5)
        );
    }

    #[test]
    fn should_clear_containment() {
        // Old inside new: no clear.
        assert!(!should_clear((2, 2, 8, 8), (4, 4, 4, 4)));
        // New inside old: clear.
        assert!(should_clear((4, 4, 4, 4), (2, 2, 8, 8)));
        // Disjoint: clear.
        assert!(should_clear((0, 0, 20, 10), (40, 10, 5, 5)));
        // The full canvas contains any old rectangle: no clear.
        assert!(!should_clear((0, 0, 80, 24), (40, 10, 5, 5)));
        // Identical: no clear.
        assert!(!should_clear((3, 2, 5, 5), (3, 2, 5, 5)));
    }

    #[test]
    fn rasterize_region_offsets_coords() {
        let cmds = test_point(10.0, 8.0, 2.0);
        let buf = rasterize_region(&cmds, 8.0, 4.0, 12, 10);
        // The sphere center lands at buffer (2, 4).
        assert!(buf[4 * 12 + 2][0] > 100, "center is red");
        // A far corner stays black.
        assert_eq!(buf[0 * 12 + 0], [0, 0, 0]);
    }

    #[test]
    fn black_tile_bytes() {
        let seq = encode_black_tile();
        let escs = split_escapes(&seq);
        assert_eq!(escs.len(), 1);
        assert_eq!(escs[0].0, "i=2,f=24,s=1,v=1,q=1,m=0");
        assert_eq!(base64_decode(&escs[0].1), Some(vec![0, 0, 0]));
    }

    #[test]
    fn move_to_bytes() {
        assert_eq!(encode_move_to(3, 5), b"\x1b[6;4H");
    }

    #[test]
    fn transmission_zlib_round_trip() {
        use flate2::read::ZlibDecoder;
        use std::io::Read;
        let pixels: Vec<[u8; 3]> = (0..500u32).map(|i| [(i % 256) as u8, 0, 0]).collect();
        let seq = encode_transmission_zlib(&pixels, 500, 1, 1);
        let escs = split_escapes(&seq);
        assert!(escs[0].0.contains("o=z"), "control has o=z");
        assert!(escs[0].0.contains("S=1500"), "S is the raw byte count");
        assert!(escs[0].0.contains("q=1"), "responses stay suppressed");
        let joined: String = escs.iter().map(|e| e.1.as_str()).collect();
        eprintln!("escs: {} joined len: {}", escs.len(), joined.len());
        let expected_b64 = {
            use flate2::write::ZlibEncoder;
            use flate2::Compression;
            let raw: Vec<u8> = pixels.iter().flat_map(|p| p.iter().copied()).collect();
            let mut enc = ZlibEncoder::new(Vec::new(), Compression::new(3));
            std::io::Write::write_all(&mut enc, &raw).unwrap();
            let comp = enc.finish().unwrap();
            eprintln!("compressed len: {}", comp.len());
            base64_encode(&comp)
        };
        eprintln!("joined == expected: {}", joined == expected_b64);
        eprintln!("first 40 joined: {}", &joined[..joined.len().min(40)]);
        eprintln!(
            "first 40 expected: {}",
            &expected_b64[..expected_b64.len().min(40)]
        );
        let compressed = base64_decode(&joined).expect("payload decodes");
        let mut dec = ZlibDecoder::new(compressed.as_slice());
        let mut out = Vec::new();
        dec.read_to_end(&mut out).expect("zlib inflate");
        let raw: Vec<u8> = pixels.iter().flat_map(|p| p.iter().copied()).collect();
        assert_eq!(out, raw, "zlib payload round-trips to the raw RGB");
    }

    #[test]
    fn frame_region_plan_clears_old_rect() {
        let mut r = KittyRenderer::new();
        let seq_a = r.frame(&test_point(10.0, 10.0, 2.0), 200, 100, 10, 5, true);
        let escs_a = split_escapes(&seq_a);
        assert!(
            escs_a.iter().all(|e| !e.0.starts_with("i=2,")),
            "first frame has no black tile"
        );
        // Move the content: the old rectangle must be cleared first.
        let seq_b = r.frame(&test_point(150.0, 80.0, 2.0), 200, 100, 10, 5, true);
        let escs_b = split_escapes(&seq_b);
        assert!(escs_b[0].0.starts_with("i=2,"), "tile transmitted first");
        assert!(
            escs_b.iter().any(|e| e.0.starts_with("a=p,i=2,")),
            "tile placed over the old rectangle"
        );
        assert!(
            escs_b.iter().any(|e| e.0.starts_with("i=1,")),
            "content transmitted"
        );
        // Same region again: contained, nothing new is sent.
        let seq_c = r.frame(&test_point(150.0, 80.0, 2.0), 200, 100, 10, 5, true);
        let escs_c = split_escapes(&seq_c);
        assert!(
            escs_c.iter().all(|e| !e.0.starts_with("i=2,")),
            "tile is sent once"
        );
        assert_eq!(escs_c.len(), 2, "content transmission + placement only");
    }

    #[test]
    fn frame_empty_scene_clears_live_rect() {
        let mut r = KittyRenderer::new();
        let _ = r.frame(&test_point(10.0, 10.0, 2.0), 200, 100, 10, 5, true);
        let empty = DrawCommands {
            points: vec![],
            lines: vec![],
        };
        let seq = r.frame(&empty, 200, 100, 10, 5, true);
        let escs = split_escapes(&seq);
        assert!(escs.iter().any(|e| e.0.starts_with("i=2,")), "tile sent");
        assert!(
            escs.iter().any(|e| e.0.starts_with("a=p,i=2,")),
            "tile placed over the old rectangle"
        );
        assert!(
            escs.iter().all(|e| !e.0.starts_with("i=1,")),
            "no content transmission"
        );
        // Second empty frame: the rectangle is gone, nothing is sent.
        let seq2 = r.frame(&empty, 200, 100, 10, 5, true);
        assert!(seq2.is_empty(), "no sequence for a second empty frame");
    }

    #[test]
    fn frame_full_plan_after_region_clears_full_canvas() {
        let mut r = KittyRenderer::new();
        let _ = r.frame(&test_point(10.0, 10.0, 2.0), 200, 100, 10, 5, true);
        // A dense scene forces the full-canvas plan; the live region
        // must be covered, which the opaque full canvas does on its
        // own, but the transition still clears the old rectangle.
        let dense = test_point(100.0, 50.0, 90.0);
        let seq = r.frame(&dense, 200, 100, 10, 5, true);
        let escs = split_escapes(&seq);
        assert!(
            escs.iter().any(|e| e.0.starts_with("a=p,i=2,")),
            "black tile covers the old region"
        );
        assert!(
            escs.iter().any(|e| e.0.contains("c=10")),
            "full-canvas placement"
        );
    }

    #[test]
    fn frame_pixel_size_falls_back_to_2x4() {
        assert_eq!(frame_pixel_size(80, 24, 23, None), (160, 92));
        assert_eq!(frame_pixel_size(80, 24, 23, Some((0, 0))), (160, 92));
        // A too-small width is rejected.
        assert_eq!(frame_pixel_size(80, 24, 23, Some((4, 480))), (160, 92));
        // 1 px rows: a broken report, rejected.
        assert_eq!(frame_pixel_size(80, 24, 23, Some((960, 24))), (160, 92));
    }

    #[test]
    fn frame_pixel_size_uses_native_pixels_scaled() {
        // An 80x24 text area at 12x20 px cells: width 960, scene
        // height = 20 px * 23 rows = 460. At the 0.5x native scale:
        // (480, 230).
        assert_eq!(frame_pixel_size(80, 24, 23, Some((960, 480))), (480, 230));
        assert_eq!(frame_pixel_size(80, 5, 4, Some((600, 200))), (300, 80));
    }

    #[test]
    fn frame_pixel_size_scaled_result_still_sane() {
        // A native size that scales down to a too-small image falls
        // back to the 2x4 grid. 8x4 px is rejected before scaling.
        assert_eq!(frame_pixel_size(80, 24, 23, Some((8, 48))), (160, 92));
    }
}
