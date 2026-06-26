# ADR-0001: Braille-Dot Canvas Widget for 3D Terminal Rendering

**Status**: Accepted
**Date**: 2026-06-26
**Deciders**: TonyWu20

## Context

chemrust-vis must render 3D chemical structures (atoms as points, bonds as lines,
cell boundaries as boxes) inside a terminal emulator. The primary use case is
remote viewing over SSH in tmux — meaning the rendering must work in a basic
terminal without graphics protocol support.

Four rendering strategies were evaluated:

| Strategy | Resolution | Portability | Bond/line support | Looks like 3D? |
|----------|-----------|-------------|-------------------|----------------|
| Half-block Unicode (▀▄) | ~2× char grid | Universal | Manual line drawing | Moderate |
| Sixel / Kitty graphics | Pixel-level | Terminal-specific | Native | Yes |
| ASCII wireframe | Char grid | Universal | Native (line chars) | Wireframe only |
| **Ratatui canvas + Braille** | 2×4 dot grid per char | Universal | Canvas line drawing | Good for point clouds |

## Decision

**Use ratatui's built-in `Canvas` widget with Braille dot markers for atom
positions, combined with canvas line-drawing for bonds and cell boundaries.**

The `Canvas` widget maps a logical 2D coordinate space (with Braille dots giving
2×4 sub-character resolution) onto terminal cells. Atoms are rendered as labeled
Braille dot clusters at their projected 2D positions. Bonds between nearby atom
pairs are rendered as line segments on the same canvas. The cell box (when a
periodic cell exists) is rendered as axis-aligned line segments after projection.

Camera manipulation (rotation via nalgebra `Isometry3`, projection via
orthographic/perspective matrix) transforms 3D world coordinates → 2D canvas
coordinates. The canvas is re-rendered each frame on input events.

## Consequences

### Positive

- **Universal portability** — Works in any Unicode-capable terminal (SSH, tmux,
  screen, local). No graphics protocol negotiation needed.
- **Single dependency chain** — ratatui already provides this widget. No
  additional rendering crate.
- **Ratatui ecosystem** — Native integration with ratatui's event loop,
  layout system, and widget composition (scene view + status bar + inspector
  panel in one terminal).
- **Sub-character precision** — Braille dots provide 2 horizontal × 4 vertical
  addressable points per character cell, giving effective resolution of
  `(2 × cols) × (4 × rows)` for atom positioning.
- **Natural for point data** — Chemical structures are fundamentally point
  clouds (atoms) with connections (bonds), which maps naturally to dot + line
  rendering.

### Negative

- **No filled surfaces** — Braille dots are discrete; there's no way to render
  filled polygons or shaded surfaces. Isosurfaces, charge density maps, and
  volumetric data cannot be rendered with this approach.
- **Limited color per cell** — Each terminal cell has one foreground and one
  background color. Atoms sharing the same character cell (when projected close
  together) may visually merge.
- **Depth occlusion is manual** — The canvas is a 2D framebuffer with no
  built-in Z-buffer. Atom overlap must be handled by sorting back-to-front
  before drawing (painter's algorithm) or by skipping occluded atoms.
- **Lower visual fidelity** — Compared to sixel/kitty graphics, Braille dots
  look like a dot matrix, not smooth spheres.

### Neutral / design implications

- The `Viewport` abstraction must own the projection math (nalgebra) and produce
  a list of 2D draw commands (point at (x,y) with color, line from (x1,y1) to
  (x2,y2) with color) that the canvas widget consumes.
- Atom picking (click-to-select) requires reverse-projecting from 2D canvas
  coordinates back to 3D world coordinates (ray casting through the projection).
  The Braille dot resolution means picking accuracy is ±1 character cell, which
  is acceptable for atom selection (atoms are not that dense in chemical
  structures).
- Future: if higher-fidelity rendering is needed (e.g., for publication-quality
  screenshots), the scene model should be renderer-agnostic — the same
  `Scene` produces draw commands that could be consumed by a sixel or SVG
  renderer without changing the scene construction logic.

## Alternatives considered

### Half-block Unicode characters

Higher vertical resolution (2 per cell vs Braille's 4) for atom dots, but
no built-in canvas widget in ratatui for half-block plotting. Would require
custom widget implementation. Braille's 2×4 dot matrix per character cell
actually gives more addressable points for positioning atoms.

### Sixel / Kitty graphics protocol

Would produce photorealistic 3D renders in terminal. Rejected because:
1. Not available in basic SSH/tmux sessions without protocol forwarding.
2. Requires terminal capability detection and fallback paths.
3. Adds complexity (shader-like rendering code, image encoding) disproportionate
   to the project's scope.

### ASCII wireframe only

Simplest implementation. Rejected because atom visualization as labeled dots
(not just vertices) is essential for chemical understanding — users need to
see which element is where, and dots with element labels are more informative
than bare wireframe vertices.

## References

- ratatui Canvas widget: https://docs.rs/ratatui/latest/ratatui/widgets/canvas/index.html
- Braille Patterns Unicode block: U+2800–U+28FF
- chemrust-geometry Structure type: `chemrust-geometry/src/structure.rs:10-38`
