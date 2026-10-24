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
- **Terminal responses are suppressed with `q=1`.** The spec replies
  `ESC_G i=<id>;OK` to requests that carry an image id. In a
  raw-stdin event loop, those response bytes return on stdin and the
  event reader parses them as key events. The first implementation
  left `=` bound to zoom: the `=` of every response mutated the
  camera, marked the image dirty, and triggered the re-transmit that
  produced the next response. The loop zoomed the view out until the
  program exited, and the leftover bytes leaked into the shell
  (reported on Ghostty). All transmits and placements now set
  `q=1`, `=` is not a key binding, and a `TerminalGuard` in `main.rs`
  restores the terminal and drains stdin on every exit path.
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
- **Terminals that ignore `q=1`** can still write responses back. The
  response characters are inert: no key binding maps them to camera
  actions, and `=` is deliberately unbound. A lone ESC byte from a
  response can still read as the quit key under the crossterm parser;
  that is a parser behavior, not an app-level defect.

### Neutral / design implications

- The scene pipeline stays renderer-agnostic: `Viewport` produces
  `DrawCommands`, and each renderer (half-block, sixel, kitty) consumes
  them. A future SVG or PNG export reuses the same commands.
- `App` now writes escapes directly to the backend after the text pass.
  The backend emits an absolute `MoveTo` before every text write, so raw
  escape injection does not desynchronize the cursor.

## Alternatives considered

### Sixel interactive rendering

Sixel works in xterm, foot, iTerm2, and WezTerm, and tmux can pass it
through. It is not the chosen path for interactive rendering because
tmux pass-through is inconsistent and the sixel encoding (6-pixel
subbands, palette limits) gives less control than the kitty protocol
for shaded spheres. The one-shot `--sixel` preview path existed until
2026-10-14 and was then removed (third amendment below). Sixel stays
a candidate for a future one-shot or image-export path.

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

## Amendment (2026-10-14)

The Ghostty incident (view zoomed out and the program exited with
`i=1;OK` bytes leaking into the shell) was a response feedback loop,
fixed the same day: `q=1` on every transmit and placement, `=`
unbound, and a `TerminalGuard` that restores the terminal and drains
stdin on all exit paths. The decisions in this ADR are unchanged;
this section records the amendment so unsuppressed responses are not
re-introduced.

## Amendment (2026-10-14, second)

The `--sixel` one-shot render in WezTerm showed every atom in green and
at half the expected size. Both defects were in `sixel.rs`:

- **Green atoms.** The encoder defined palette colors with color system
  2 and 0-255 byte components, and never sent a color select.
  WezTerm's sixel decoder parses `#` color definitions as 0-100
  percent, and in 1-bit mode the defined color is not the foreground
  until a `#<n>` select is sent. Its default foreground is pure green,
  which is what every lit pixel showed. The encoder now defines colors
  with system 0 and 0-100 percent components, and each definition is
  immediately followed by a select. It also assigns the column color
  only from on pixels (the old code took the last pixel of the column,
  which is dark for a thin circle edge).
- **Half size.** Sixel has no size fields: the terminal maps image
  pixels to cells at its own font cell size. The one-shot rendered at a
  fixed 12x24 px per cell while WezTerm's cells were larger, so the
  image covered fewer cells. `--sixel` now queries the terminal for
  its text-area pixel size via the xterm `CSI 8 t` extension (xterm,
  iTerm2, and foot answer by default; WezTerm answers when its
  `resize_reporting` option is on), derives pixels per cell, and
  falls back to 12x24 when the terminal does not answer.
  `--sixel-px <W>x<H>` forces the value. The query runs in raw mode
  and reads stdin non-blocking for at most 300 ms.

The kitty renderer is unchanged: it places the image in a cell
rectangle, so its 2x4 px-per-cell assumption only affects content
framing, not placement.

## Amendment (2026-10-14, third)

Despite the color and size fixes in the second amendment, the
decider judged the WezTerm sixel render still off and asked for
sixel support to be removed for now. This amendment records that
removal.

- Removed: the `--sixel` and `--sixel-px` flags and their code paths,
  the sixel encoder (`encode_sixel`), the terminal sixel detection
  (`terminal_supports_sixel`, `detect_sixel`), and the `CSI 8 t`
  pixel-size query (`query_cell_pixels`, `parse_pixel_size_report`,
  `parse_px_cell`). The `libc` dependency added for the query went
  with it.
- Kept: `render_to_pixels`, `fill_circle_px`, and `draw_line_px` in
  `sixel.rs`, because the kitty renderer builds its image on those
  primitives. The `--kitty` one-shot and the interactive kitty
  renderer are unchanged.
- The sixel encoder design (0-100 percent color system 0 definitions
  with an immediate select, pixel-size query, 12x24 fallback) is
  recorded in the second amendment and this ADR's alternatives, so a
  revisit can start from there.

## Amendment (2026-10-21, fourth)

### The `kitty-graphics-protocol` crate was evaluated and rejected

On 2026-10-21 the decider asked whether `kitty-graphics-protocol`
0.1.3 replaces the hand-rolled encoder in `kitty.rs`. The evaluation
found:

- The crate covers only the transport layer (command builder,
  chunked base64). Detection, rasterization, and re-transmit state
  stay hand-rolled either way.
- The builder output differs from our escapes: an explicit `a=t` on
  transmits, a trailing `;` on payload-free placements, `a=d,d=a`
  for delete-all. All forms are spec-valid. This protocol already
  caused a production incident (the Ghostty feedback loop), so the
  diff needs real-terminal re-verification.
- The convenience functions use `a=T` (transmit and display at the
  cursor) with `quiet(2)`. That changes the flow semantics.
- A spec bug was verified: `query_window_size` sends `CSI 14 t`
  (screen origin) but parses the `CSI 16 t` (text-area pixel)
  response.
- The crate adds `base64`, `thiserror`, and `libc` to the tree,
  against the "no new dependencies" benefit recorded above. It is
  0.1.x, and its docs.rs build failed.

**Decision: keep the hand-rolled encoder.** No crate dependency was
added. The encoder, its tests, and the byte-stability guarantee
stay. A stale comment in `main.rs` was also fixed: it claimed the
one-shot renders at 2x4 px per cell while the code uses 12x24.

### Native pixel resolution for the interactive renderer

The interactive renderer used a fixed 2x4 px-per-cell grid. The
one-shot `--kitty` used 12x24 px per cell. The interactive view
carried 37x fewer pixels at 80x24.

**Decision:** the interactive renderer now queries the terminal's
native text-area pixel size via the `TIOCGWINSZ` pty ioctl on
stdout (`ws_xpixel`, `ws_ypixel`). The image spans the full width.
Its height is proportional to the scene rows. Both dimensions are
then scaled by `NATIVE_FRAME_SCALE` (0.5x linear) to keep
re-transmits fast during camera drags. When the query fails or the
result is too small, the renderer falls back to the 2x4 px-per-cell
grid. The mouse vertical scale uses the px-per-row of the last
transmitted frame.

Consequences:

- Re-introduces `libc` as a Unix-only target dependency. The third
  amendment removed `libc` with the sixel stdin-query path. This
  path reads no stdin and needs no raw mode.
- Supersedes the "Fixed cell aspect assumption" negative item and
  the 1:2-aspect "Accepted limit". The 1:2 assumption now applies
  only to the fallback grid.
- Re-transmit cost is bounded by the scale factor. At full scale,
  an 80x24 terminal reporting a 960x480 text area transmits
  ~1.7 MB of base64 per re-transmit, which dropped the interactive
  frame rate. At the 0.5x scale the same window transmits ~430 KB.
  The fallback grid transmits ~59 KB. A static view still sends
  one image per session.
- The status bar shows the last transmitted kitty frame size
  (for example `kitty 240×460px`), so the active grid is visible
  on screen. The value is computed on the first draw pass and
  refreshed whenever a new frame is built.
- Verification: a forced-pty test ran the TUI under a pty with a
  known pixel size and checked the transmitted `s`/`v` fields and
  the status bar text. A 480x960 px pty transmitted `s=240,v=460`
  (the 0.5x of 480x920) and showed `kitty 240×460px`. Zero-pixel
  ptys and pipes fell back to 2x4 as designed.

## Amendment (2026-10-24, fifth)

Transmission media and frame content.

- Pixel data now travels over a session-dependent medium. A local
  session on a known kitty-protocol terminal uses POSIX shared
  memory (`t=s`): kitty through `KITTY_PID`, WezTerm and Ghostty
  through `TERM_PROGRAM` / `TERM`. SSH sessions (`SSH_CLIENT` or
  `SSH_TTY` set) use plain base64, because the terminal reads the
  segment locally while the app writes it on the remote host.
  Unknown local terminals also use plain base64.
- `--kitty-compress` selects the zlib-compressed base64 path
  (`o=z`, `S=` carries the raw size). `--no-kitty-shm` forces plain
  base64. Both apply on top of the medium choice; an explicit
  choice wins.
- Shared memory is created with the `shared_memory` crate under the
  name `/chemrust-vis-tui-<pid>-<seq>` (one leading slash, no
  other slashes, 0600 permissions). The crate unlinks the segment
  when its handle is the owner. The renderer therefore drops the
  handle as a non-owner (`set_owner(false)`) so the segment
  survives until the terminal unlinks it after reading, as the
  spec assigns.
- A failed segment create (for example a full `/dev/shm`) drops
  the session to plain base64 for the rest of the run. The status
  bar shows the effective medium, so the downgrade is visible.
- The 1x1 black clear tile always travels base64. Three bytes do
  not justify a segment.

The animation protocol (`a=f` / `a=a` / `a=c`) is not used.
Rationale: the renderer replaces its content every frame and skips
re-transmit on static views, so the terminal-side frame cache buys
nothing. The pixel economy the animation path offers comes from
sending only the changed regions, which plain placements achieve
portably:

- When the native pixel mapping is available, the frame crops to
  the scene content's cell rectangle: the bounding box of the
  drawn points and lines, padded one pixel and rounded outward to
  cell boundaries. A region that covers 95% or more of the canvas
  falls back to the full canvas (`CROP_AREA_LIMIT`).
- The previous rectangle clears with a one-pixel black tile
  (image id 2, transmitted once) stretched over the old
  rectangle. The clear runs when the new rectangle does not fully
  contain the old one. A full-canvas image is opaque black in its
  content-free areas, so a region-to-full transition also clears.
- An empty scene sends nothing. It clears its live rectangle and
  stops.
- A terminal resize or a frame-pixel-size change remaps the cell
  grid. The renderer deletes all placements and restarts its
  region state.

The status bar readout gains the medium: `kitty 480×230px shm`,
`kitty 480×230px b64`, `kitty 480×230px b64+z`.

Consequences:

- Adds two dependencies to the TUI crate: `shared_memory` (0.12,
  POSIX shared memory wrapper) and `flate2` (zlib, pure-Rust
  miniz backend).
- The base64 path is the universal fallback. `q=1` stays on every
  transmit and placement, per the Ghostty feedback-loop lesson.
- `o=z` is opt-in. kitty implements it. WezTerm and Ghostty are
  unverified, so the default SSH medium stays plain base64.
- Payload sizes at a 480x960 px pty, cropped region: ~150 B on the
  wire over shared memory (the segment name), ~205 KB base64, ~18 KB
  zlib base64. The uncropped 0.5x frame transmits ~430 KB
  base64.

Verification (forced pty, `TERM_PROGRAM=WezTerm`, 480x960 px):
25 shared-memory frames produced 25 named segments and 3 black-tile
clears during orbit keys. The base64 capture measured 205 KB and
the `--kitty-compress` capture 18 KB. An `SSH_CLIENT` environment
selected base64. An unknown terminal selected base64, and the
auto path fell back to half-block. 97 unit tests pass.

## Accepted limits

On 2026-10-13 the decider accepted the limits in the "Negative"
section above:

- Environment detection cannot see the WezTerm `enable_kitty_graphics`
  config gate. Force the renderer with `--renderer kitty` in that setup.
- Under tmux, the image shows only when `allow-passthrough` is on and
  the outer terminal supports the protocol. Otherwise the text UI runs
  unchanged.
- The kitty pixel buffer assumes a 1:2 cell aspect (2x4 px per cell).
  Terminals with unusual cell aspects show a slight letterbox. (Superseded
  by the fourth amendment: 2x4 is now the fallback grid; the renderer
  prefers the terminal's reported pixel size.) The
  `--sixel` one-shot path sizes its buffer from the queried pixel
  size instead (amendment above).

## Amendment 6 (2026-10-24): Ghostty corruption report, `--kitty-full`

Report: on Ghostty (macOS, local and over SSH), the interactive renderer
looks correct on startup, then after a few rotation steps the view turns
black and does not recover. Both transmission mediums (shared memory
local, base64 over SSH) show the same corruption, so the medium is not
the cause; the shared content-box delta path is the common factor.

Diagnosis so far:

- The emitted escape stream is well-formed and geometrically
  consistent (audited byte by byte from a forced pty).
- A software simulation of the terminal image store composites the
  stream to a clean molecule on black: no corruption.
- The shared-memory segments carry the full pixel data (153 KB at the
  test window).
- Suspected root cause: a terminal-side bug interacting with the
  delta design. Ghostty issues #6711 (same-image-ID data replace not
  honored, fixed in 1.4.0) and #6710 (image stacking) are exactly the
  operations the delta path relies on: replacing the live image data
  with different pixel dimensions every frame, accumulating
  placements, and a 1x1 black tile stretched to clear the old
  rectangle. The reporter's Ghostty version is unconfirmed; a build
  earlier than 1.4.0 exhibits #6711.

Changes:

- New flag `--kitty-full`: forces the full-canvas transmission path.
  One image, one fixed placement at the top-left of the scene, and
  in-place data replacement afterwards (the same dimensions every
  frame, the safest replace). No content-box crop, no black clear,
  no moving placements. The most conservative kitty mode and the
  isolation test.
- The full-canvas path places once and then replaces the image data
  in place instead of re-placing every frame.
- Empty-scene clearing uses the delete-all action, so a live
  full-canvas image also clears.
- The delta path no longer re-transmits under a fixed content id.
  Each frame places its content under a fresh monotonic image id
  (base 100), so a stored image is never replaced with different
  pixel dimensions while its placement is live. Old content images
  stay covered by the black clear and are evicted by the terminal.
- The black clear is now an 8x8 image (sent once, then placed
  stretched over the old rectangle) instead of a 1x1 tile, keeping
  the scale a normal terminal operation.

Reporter confirmed the Ghostty version is 1.3.1, the newest stable
release. So updating is not a fix; the robustness work above is the
app-side response.

Open:

- Confirm on 1.3.1 whether the rebuilt delta path still corrupts.
- If it does, `--kitty-full` is the fully robust fallback (single
  image, no accumulation). If even that corrupts, report to the
  Ghostty project: a single-image in-place replace should not break.
- Optional follow-up: a bounded delta that periodically sends
  delete-all to cap image-stack growth, if a terminal's eviction is
  too slow.

Decision (after 1.3.1 testing): the full-canvas path is now the
default. The reporter confirmed the rebuilt delta no longer paints
black over the atoms, but Ghostty does not evict old images, so the
delta path's per-frame content images accumulate without bound. The
full-canvas path stores a single image and runs smoothly. The
transmission paths are therefore:

- Default (no flag): full-canvas, one image, in-place data replace.
- `--kitty-full`: the same default, made explicit.
- `--kitty-delta`: opt-in content-box crop. Saves bandwidth on
  terminals that evict old images (kitty, WezTerm). On Ghostty it
  accumulates images; avoid it there.

CLI regrouping (same date): the kitty flags were reorganized into
orthogonal axes to remove the `--kitty` one-shot vs `--kitty-*`
interactive collision. The current surface is:

- `--once`: one-shot single-frame render and exit.
- `--kitty-mode <full|delta>`: transmission strategy (default full).
- `--kitty-medium <auto|shm|base64|zlib>`: transmission medium
  (default auto).
- `--renderer <auto|kitty|halfblock>`: protocol selection.

The old names `--kitty`, `--kitty-full`, `--kitty-delta`,
`--kitty-compress`, and `--no-kitty-shm` remain as hidden deprecated
aliases that map onto the new flags.
