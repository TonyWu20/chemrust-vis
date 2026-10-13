# ADR-0002: Kitty Graphics Protocol as an Optional Pixel Renderer

**Status**: Accepted
**Date**: 2026-10-13
**Deciders**: TonyWu20

## Context

ADR-0001 rejected the sixel/kitty graphics protocols for interactive
rendering. The stated reason was portability: the protocol needs terminal
support that plain SSH/tmux sessions may lack, and chemrust-vis must work
in a basic terminal. That constraint still holds. But the situation has
changed in two ways.

1. The terminals that implement the kitty graphics protocol are now
   mainstream. kitty, WezTerm, and Ghostty all implement it. The spec
   lists several more.
2. The rejection cost real rendering quality. The half-block renderer
   shows 2 dots per character with one color per half-cell. A pixel
   renderer shows a filled, shaded sphere with 24-bit color and smooth
   1-pixel lines. For a 3D structure viewer, that is a large step in
   fidelity.

The pixel renderer already exists in this codebase. `sixel.rs` rasterizes
`DrawCommands` to an RGB buffer with shaded spheres and lines, and the
`--sixel` one-shot mode proves the pipeline. The missing piece was the
kitty encoder and the interactive integration.

## Decision

Support the kitty graphics protocol as an **optional** scene renderer.

- New module `crates/chemrust-vis-tui/src/tui/kitty.rs`:
  - Hand-rolled base64 and chunked escape encoder (no new dependencies).
    Raw RGB (`f=24`) is base64 encoded and sent in chunks of at most
    4096 bytes, per the spec's remote-client scheme.
  - Placement request places the image at the top-left of the scene
    area, scaled to `cols` x `scene_rows` cells, with `z=-1` (below the
    text layer) and `C=1` (no cursor movement). The status bar and axis
    indicator are text and render above the image.
  - Terminal detection from `KITTY_PID`, `TERM_PROGRAM`, and `TERM`.
  - `KittyRenderer` re-transmits the image only when the camera,
    viewport, or terminal size changes; otherwise the placement persists.
- CLI: `--renderer auto|halfblock|kitty` (default `auto` = detect).
  `--kitty` renders one frame and exits, mirroring `--sixel`.
- The half-block renderer remains the universal fallback. Plain
  terminals and tmux without pass-through keep the current behavior.
  Unsupported terminals ignore APC codes, so a forced kitty render degrades
  to text UI without garbage output.

## Consequences

### Positive

- **Better rendering where the terminal supports it.** Filled shaded
  spheres, 24-bit color, sub-character line resolution.
- **No new dependencies.** The encoder is about two hundred lines of
  plain Rust on top of the existing pixel primitives in `sixel.rs`.
- **Portability preserved.** Auto-detection plus the half-block fallback
  keeps the basic-terminal guarantee from ADR-0001.
- **Cheap updates.** Only camera or size changes re-transmit image data.
  A static view sends one image per session.

### Negative

- **Detection is a heuristic.** WezTerm gates the protocol behind a
  config option that env vars cannot see. A user in that setup must use
  `--renderer kitty` to force it.
- **One image per frame in terminal memory.** Re-transmitting an id
  replaces the stored image, but a rapid camera sweep creates and frees
  many images. Kitty's storage quota handles eviction.
- **Fixed cell aspect assumption.** The pixel buffer uses 2 px wide by
  4 px tall per cell. A terminal with a very different cell aspect gets
  a slight letterbox.

### Neutral / design implications

- The scene pipeline stays renderer-agnostic: `Viewport` produces
  `DrawCommands`, and each renderer (half-block, sixel, kitty) consumes
  them. A future SVG or PNG export reuses the same commands.
- `App` now writes escapes directly to the backend after the text pass.
  The backend emits an absolute `MoveTo` before every text write, so raw
  escape injection does not desynchronize the cursor.

## Alternatives considered

### Sixel interactive rendering

Sixel works in xterm, foot, and iTerm2, and tmux can pass it through.
It is not the chosen path because tmux pass-through is inconsistent and
the sixel encoding (6-pixel subbands, palette limits) gives less control
than the kitty protocol for shaded spheres. The one-shot `--sixel`
preview path remains.

### `ratatui-image` crate

`ratatui-image` renders image files through the kitty or sixel protocol
into a ratatui `Buffer`. It takes a decoded `image::DynamicImage`, which
pulls a heavy decode dependency for a use case that here is "rasterize
our own `DrawCommands` every frame." Its Unicode-placeholder integration
also needs a newer protocol version than the plain transmit-plus-place
flow used here.

### PNG-encode and transmit with `f=100`

Sending PNG data needs a PNG encoder (a new dependency) for a small
bandwidth saving. The pixel buffer is under 2 MB per frame at a large
terminal. Raw RGB is simpler and needs no encoder.

## References

- Kitty graphics protocol: https://sw.kovidgoyal.net/kitty/graphics-protocol/
- Terminals implementing the protocol: https://sw.kovidgoyal.net/kitty/graphics-protocol/#other-terminals-that-have-implemented-the-graphics-protocol
- ADR-0001 (partially superseded): ./0001-braille-canvas-rendering.md
- Encoder: `crates/chemrust-vis-tui/src/tui/kitty.rs`

## Accepted limits

On 2026-10-13 the decider accepted the limits in the "Negative"
section above:

- Environment detection cannot see the WezTerm `enable_kitty_graphics`
  config gate. Force the renderer with `--renderer kitty` in that setup.
- Under tmux, the image shows only when `allow-passthrough` is on and
  the outer terminal supports the protocol. Otherwise the text UI runs
  unchanged.
- The pixel buffer assumes a 1:2 cell aspect (2x4 px per cell). Terminals
  with unusual cell aspects show a slight letterbox.
