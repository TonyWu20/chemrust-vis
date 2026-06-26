# Phase 1: Load + Render + Navigate

**Date:** 2026-06-26
**Status:** Draft

## Goals

1. **Structure loading from CASTEP `.cell` files.** Parse `.cell` files using
   `castep-cell-io` and construct `chemrust-geometry::Structure` instances.
   This gives us real structure data to visualize immediately, matching the
   format chemrust-geometry already targets in its examples (`cu111_co.rs`).
   Effort: medium.

2. **3D scene construction from a Structure.** Extract atom Cartesian positions,
   compute the periodic cell box (8 corners, 12 edges), and build a `Scene`
   that the renderer consumes. The scene is renderer-agnostic: it produces
   abstract draw commands (points + lines) that any backend could render.
   Effort: medium.

3. **Braille-dot terminal rendering.** Project the 3D scene to 2D canvas
   coordinates using nalgebra camera matrices, then render atoms as labeled
   Braille-dot clusters and the cell box as line segments on a ratatui
   `Canvas` widget. Atoms are color-coded by element. Depth-sort atoms
   back-to-front (painter's algorithm) for correct occlusion. Effort: large.

4. **Full keyboard camera controls.** Orbit rotation (wasd / arrow keys),
   zoom (+/- keys), and pan (shift+wasd / hjkl). Camera uses spherical
   coordinates around a target point, with orthographic projection. All
   controls update the camera and trigger immediate redraw. Effort: medium.

5. **Property-based correctness tests.** Verify the projection pipeline with
   `proptest`: round-trip invariance (project → unproject returns the same
   3D point), symmetry preservation (rotating by known angles produces
   symmetric screen coordinates), and known-point validation against
   hand-calculated projections. Effort: small.

## Scope Boundaries

**In scope:**
- Parse CASTEP `.cell` files into `Structure` via `castep-cell-io`
- Construct `Scene` from `Structure`: atom positions (Cartesian) + cell box geometry
- Camera model: spherical coordinates, look-at matrix, orthographic projection
- Ratatui `Canvas` widget rendering with Braille dots for atoms, lines for cell edges
- Atom color-coding by element symbol
- Keyboard input handling (crossterm events)
- Orbit camera: rotate θ/φ with wasd/arrows
- Zoom: change camera distance with +/- keys
- Pan: shift camera target with shift+wasd or hjkl
- Status bar showing: file name, atom count, camera angles
- CLI argument: `chemrust-vis-tui <file.cell>` to launch with a structure
- Property-based tests on the projection/transform math

**Out of scope:**
- Mouse event handling (Phase 2)
- Atom picking / selection (Phase 2)
- Bond detection and rendering (Phase 2)
- Atom inspector panel (Phase 2)
- MCP server mode (Phase 3)
- Format export (Phase 4)
- File format writing of any kind
- Perspective projection toggle (deferred, implement orthographic first)
- Loading formats other than CASTEP `.cell` (XYZ, CIF deferred)
- Color scheme customization (hard-code a reasonable palette)
- tmux-specific testing (confirm it works, but no special tmux handling)

## Design Notes

### Camera model

- Spherical coordinates: `(theta, phi, radius)` around a `target: Point3<f64>`.
- Camera position: `target + radius * (sin(phi)*cos(theta), sin(phi)*sin(theta), cos(phi))`.
- View matrix: `nalgebra::Isometry3::look_at_rh(&pos, &target, &up)`.
- Projection: orthographic, with scale determined by `radius` (closer = narrower
  field of view). Scale factor = `terminal_height / (2 * radius)`.
- Default view: `theta=30°`, `phi=45°`, `radius` computed to fit the structure's
  bounding box.
- Pan moves `target` in the camera's local right/up directions.
- Zoom changes `radius` (clamp to prevent going inside the structure).

### Scene construction

The `Scene` struct is the bridge between `Structure` and the renderer:

```rust
pub struct Scene {
    /// Atom positions in Cartesian coordinates (Å).
    pub atoms: Vec<AtomDrawData>,
    /// Cell box: list of line segments (pairs of corner indices).
    pub cell_edges: Vec<([f64; 3], [f64; 3])>,
}

pub struct AtomDrawData {
    pub position: [f64; 3],
    pub element: ElementSymbol,
    pub label: Option<String>,
    pub color: ratatui::style::Color,
}
```

Construction from `Structure`:
- Call `structure.cart_coords()` to get Cartesian positions.
- For cell: extract the 8 corners of the parallelepiped from `LatticeVectors`
  (origin + each combination of a, b, c vectors), then define the 12 edges.
- `Structure` with `cell: None` (molecules) simply has an empty `cell_edges`.
- Apply any pending transform via `structure.apply()` before extracting.

### Viewport rendering pipeline

```
Scene ──▶ World space ──▶ View space ──▶ Clip space ──▶ NDC ──▶ Canvas coords
            (identity)     (view matrix)  (proj matrix)  (/w)     (scale+bias)
```

1. **View transform**: `view_matrix * world_point` — camera-relative coords.
2. **Projection**: orthographic, scale factor maps world units to NDC.
3. **NDC → canvas**: `canvas_x = (ndc_x + 1) / 2 * canvas_width`,
   `canvas_y = (1 - ndc_y) / 2 * canvas_height`.
4. **Depth sort**: sort atoms by view-space Z (back to front) before drawing.
5. **Braille mapping**: for each atom at canvas `(x, y)`, set the appropriate
   Braille dot in the character cell at `(x/2, y/4)`.
6. **Line drawing**: Bresenham on canvas coordinates for cell edges.

### Why orthographic first

Orthographic projection is simpler (no perspective divide edge cases at z≤0),
more useful for crystal structures (parallel lines stay parallel — important
for seeing lattice symmetry), and easier to test (projection is linear).

### Loading: castep-cell-io integration

The conversion from `CellDocument` to `Structure` follows the pattern in
chemrust-geometry's `cu111_co.rs` example:
- Extract lattice vectors from `LatticeCart`
- Extract species and fractional coordinates from `PositionsFrac`
- Build `Structure::new(...)` with `pbc = [true, true, true]` for 3D periodic

The loader lives in `chemrust-vis-core` (not the TUI binary), behind a
feature-gated module: `chemrust-vis-core = { features = ["castep-loader"] }`.
This keeps the core crate usable without CASTEP dependencies.

### Dependency graph for Phase 1

```
chemrust-vis-core (lib)
├── chemrust-geometry (path)
├── nalgebra 0.33
├── ratatui (canvas types)
├── castep-cell-io (optional, feature = "castep-loader")
└── proptest (dev)

chemrust-vis-tui (bin)
├── chemrust-vis-core (path, features = ["castep-loader"])
├── ratatui (full, with crossterm backend)
├── crossterm (input events, raw mode)
├── anyhow
└── clap (CLI args)
```

## Deferred Items Absorbed

None — this is the first phase. No deferred improvements from prior work exist.

## Domain Terms

None refined — the existing CONTEXT.md glossary is adequate for Phase 1.
All terms used here (`Structure`, `LatticeVectors`, `Scene`, `Viewport`,
`Camera`, `Projection`, `FracCoord`) are already defined in CONTEXT.md §2.
