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
//!    `i=<id>,f=24,s=<w>,v=<h>,m=1`. Later chunks carry only `m`.
//! 3. A placement request `a=p,i=<id>` displays the stored image at the
//!    current cursor position, scaled to a `c` column x `r` row
//!    rectangle. `z=-1` places the image below the text layer so the
//!    status bar and axis indicator render on top of it. `C=1` keeps the
//!    cursor in place after placement.

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
pub fn detect_kitty(
    term_program: Option<&str>,
    term: Option<&str>,
    has_kitty_pid: bool,
) -> bool {
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

// ── Base64 ──────────────────────────────────────────────────────────────────

/// Standard base64 alphabet.
const B64_ALPHABET: [u8; 64] =
    *b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

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
/// - First escape: `\x1b_Gi=<id>,f=24,s=<w>,v=<h>,m=<1|0>;<chunk>\x1b\`
/// - Middle escapes: `\x1b_Gm=<1>;<chunk>\x1b\`
/// - Final escape: `\x1b_Gm=<0>;<chunk>\x1b\`
pub fn encode_transmission(bytes: &[u8], width: u32, height: u32, image_id: u32) -> Vec<u8> {
    let b64 = base64_encode(bytes);
    let total = b64.len().div_ceil(MAX_CHUNK).max(1);
    let mut out = Vec::with_capacity(b64.len() + 64 * total + 64);
    for (i, chunk) in b64.as_bytes().chunks(MAX_CHUNK).enumerate() {
        let m = if i + 1 < total { 1 } else { 0 };
        if i == 0 {
            out.extend_from_slice(
                format!("\x1b_Gi={image_id},f=24,s={width},v={height},m={m};").as_bytes(),
            );
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
pub fn encode_placement(image_id: u32, cols: u16, rows: u16) -> Vec<u8> {
    format!("\x1b_Ga=p,i={image_id},c={cols},r={rows},z=-1,C=1\x1b\\").into_bytes()
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

// ── Renderer state ──────────────────────────────────────────────────────────

/// Interactive renderer: produces the full escape sequence for one frame.
pub struct KittyRenderer {
    image_id: u32,
}

impl KittyRenderer {
    pub fn new() -> Self {
        KittyRenderer { image_id: IMAGE_ID }
    }

    /// Build the transmit + placement sequence for one frame.
    ///
    /// The caller writes the bytes after moving the terminal cursor to
    /// the top-left corner of the scene area.
    pub fn frame(
        &self,
        cmds: &DrawCommands,
        px_w: usize,
        px_h: usize,
        cols: u16,
        rows: u16,
    ) -> Vec<u8> {
        let pixels = rasterize(cmds, px_w, px_h);
        let mut out =
            encode_transmission_rgb(&pixels, px_w as u32, px_h as u32, self.image_id);
        out.extend(encode_placement(self.image_id, cols, rows));
        out
    }
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
        assert_eq!(ctrl, "i=7,f=24,s=1,v=1,m=0");
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
        assert_eq!(escs[0].0, "i=1,f=24,s=1333,v=3,m=1");
        assert_eq!(escs[1].0, "m=0");
        assert_eq!(escs[0].1.len(), MAX_CHUNK, "non-final chunk fills MAX_CHUNK");
        assert_eq!(escs[0].1.len() % 4, 0, "non-final chunks are 4-byte aligned");
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
        assert_eq!(seq, b"\x1b_Ga=p,i=3,c=80,r=23,z=-1,C=1\x1b\\");
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
        assert!(!detect_kitty(Some("iTerm.app"), Some("xterm-256color"), false));
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
        let r = KittyRenderer::new();
        let cmds = DrawCommands { points: vec![], lines: vec![] };
        let seq = r.frame(&cmds, 2, 2, 10, 5);
        let escs = split_escapes(&seq);
        assert_eq!(escs.len(), 2, "single-chunk transmission + placement");
        assert!(escs[1].0.starts_with("a=p,i=1,"), "second escape is a placement");
        assert!(escs[1].0.contains("c=10"), "placement width");
        assert!(escs[1].0.contains("r=5"), "placement height");
    }
}
