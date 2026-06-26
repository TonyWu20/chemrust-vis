# TASKS.md — Phase 1: Load + Render + Navigate

**Date:** 2026-06-26
**Plan:** plans/phase-1/PHASE_PLAN.md
**Decisions:** notes/plans/phase-1/DECISIONS.md
**ODD Pattern:** /Users/tony/programming/rust-development-pipeline/skills/drive-outcomes/references/odd-pattern.md

## Declared Fixtures

| ID | Path | Description |
|----|------|-------------|
| F1 | `../chemrust/chemrust-geometry/Cu111_CO.cell` | Cu(111) 4-layer slab + CO, 18 atoms (16 Cu, 1 C, 1 O), axis-aligned orthorhombic cell, CASTEP format |

## Task Groups

| Group | Tasks | Depends On | Kind |
|-------|-------|-----------|------|
| group-scaffold | TASK-SCAFFOLD | none | direct |
| group-core | TASK-CORE-SCENE, TASK-CORE-CAMERA, TASK-CORE-LOADER, TASK-CORE-VIEWPORT | group-scaffold | lib-tdd |
| group-tui | TASK-TUI-CANVAS, TASK-TUI-APP | group-core | direct (canvas), direct (app) |

---

## Group: scaffold

### TASK-SCAFFOLD: Workspace and crate scaffolding
**Kind:** direct
**Goal:** Set up the Cargo workspace with two crates and all dependencies declared.

**Files to create/modify:**
- `Cargo.toml` — workspace root with `members = ["crates/chemrust-vis-core", "crates/chemrust-vis-tui"]`
- `crates/chemrust-vis-core/Cargo.toml` — lib crate with deps: chemrust-geometry (path), nalgebra 0.33, ratatui (default-features=false, features=["unstable-canvas"]), serde, serde_json. Optional dep: castep-cell-io 0.5 + castep-cell-fmt 0.1 behind feature "castep-loader". Dev-deps: proptest.
- `crates/chemrust-vis-core/src/lib.rs` — stub with `//! chemrust-vis-core` module doc
- `crates/chemrust-vis-tui/Cargo.toml` — bin crate with deps: chemrust-vis-core (path, features=["castep-loader"]), ratatui (with crossterm backend), crossterm, anyhow, clap
- `crates/chemrust-vis-tui/src/main.rs` — stub with `fn main() { println!("chemrust-vis-tui"); }`

**Acceptance:**
```bash
cargo check --workspace
```

---

## Group: core

### TASK-CORE-SCENE: Scene struct and construction from Structure
**Kind:** lib-tdd
**Goal:** Define `Scene` and `AtomDrawData` types, implement construction from `chemrust_geometry::Structure`, including cell box geometry extraction.

**Success Criteria:**
- `Scene::from_structure(&cu111_co_structure())` returns `Scene` with 18 atoms
  (Source: Cu111_CO.cell has 18 entries in POSITIONS_FRAC)
- `scene.atoms[0].position` ≈ (0.0, 0.0, 0.0) — Cu at origin in axis-aligned cell
  (Source: Cu111_CO.cell line 8, frac (0,0,0) × cell = (0,0,0))
- `scene.atoms[1].position` ≈ (1.2781, 0.7379, 2.0871) ± 0.001
  (Source: Cu111_CO.cell line 9, frac (0.125, 0.041667, 0.114292) × cell)
- `scene.atoms[16].element == ElementSymbol::C` — carbon atom at index 16
  (Source: Cu111_CO.cell line 24)
- `scene.cell_edges.len() == 12` — 12 edges of the parallelepiped
  (Source: 8 corners of a parallelepiped have 12 edges)
- `scene.cell_edges[0]` connects origin corner to a-vector corner
  (Source: edge 0-1 in axis-aligned cell is a = (10.2248, 0, 0))
- Constructor from `Structure` with `cell: None` (molecule) produces `cell_edges.is_empty()`
  (Source: molecules have no periodic cell)

**Files:**
- `crates/chemrust-vis-core/src/scene.rs` — `Scene`, `AtomDrawData` structs, `Scene::from_structure()`
- `crates/chemrust-vis-core/src/lib.rs` — `pub mod scene; pub use scene::*;`

**Test file:** `crates/chemrust-vis-core/src/scene.rs` — `#[cfg(test)] mod tests`

**Test code sketch:**
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use chemrust_geometry::slab::cu111_co_system;

    #[test]
    fn scene_from_cu111_co_has_18_atoms() {
        let structure = cu111_co_system(3.615);
        let scene = Scene::from_structure(&structure);
        assert_eq!(scene.atoms.len(), 18);
    }

    #[test]
    fn origin_cu_at_cartesian_zero() {
        let structure = cu111_co_system(3.615);
        let scene = Scene::from_structure(&structure);
        // First atom is Cu at frac (0,0,0) → Cartesian (0,0,0) in axis-aligned cell
        let origin = scene.atoms[0].position;
        assert!((origin[0]).abs() < 1e-6, "x should be 0, got {}", origin[0]);
        assert!((origin[1]).abs() < 1e-6, "y should be 0, got {}", origin[1]);
        assert!((origin[2]).abs() < 1e-6, "z should be 0, got {}", origin[2]);
    }

    #[test]
    fn cell_edges_count_is_12() {
        let structure = cu111_co_system(3.615);
        let scene = Scene::from_structure(&structure);
        assert_eq!(scene.cell_edges.len(), 12);
    }

    #[test]
    fn molecule_has_no_cell_edges() {
        let mol = Structure::new(
            vec![ElementSymbol::H, ElementSymbol::H],
            vec![FracCoord::new(0.0, 0.0, 0.0), FracCoord::new(0.0, 0.0, 0.74)],
            None, // no cell
            [false, false, false],
            vec![0, 0],
            vec![None, None],
            None,
        );
        let scene = Scene::from_structure(&mol);
        assert!(scene.cell_edges.is_empty());
    }
}
```

**Acceptance:**
```bash
cargo test -p chemrust-vis-core -- scene
```

---

### TASK-CORE-CAMERA: Camera model with spherical coordinates and projection
**Kind:** lib-tdd
**Goal:** Implement `Camera` with spherical coordinate positioning, look-at view matrix, and orthographic projection matrix.

**Success Criteria:**
- Default camera: `Camera::new(target, radius)` sets theta=0, phi=PI/4
- `camera.position()` computes correctly: `target + radius * (sin(phi)*cos(theta), sin(phi)*sin(theta), cos(phi))`
  (Source: standard spherical→Cartesian with physics convention: theta=azimuthal, phi=polar)
- `camera.view_matrix()` is a valid `Isometry3<f64>` with determinant ≈ 1.0
  (Source: rotation matrices have det=1)
- `camera.project(world_point)` for orthographic: returns NDC coordinates in [-1, 1] for points within the view volume
  (Source: orthographic projection is linear)
- `camera.orbit(d_theta, d_phi)` updates theta, phi; `camera.orbit(-d_theta, -d_phi)` restores original `position()` ± 1e-10
  (Source: spherical coordinate symmetry)
- `camera.zoom(dr)` changes `radius` by exactly `dr`; clamp prevents `radius <= 0`
- `camera.pan(dx, dy)` translates `target` by `dx*right + dy*up` where right/up are camera-local axes

**Files:**
- `crates/chemrust-vis-core/src/camera.rs` — `Camera` struct
- `crates/chemrust-vis-core/src/lib.rs` — `pub mod camera;`

**Test file:** `crates/chemrust-vis-core/src/camera.rs` — `#[cfg(test)] mod tests`

**Test code sketch:**
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use nalgebra::Point3;

    fn test_camera() -> Camera {
        Camera::new(Point3::new(5.0, 5.0, 5.0), 20.0)
    }

    #[test]
    fn orbit_round_trip() {
        let mut cam = test_camera();
        let orig = cam.position();
        cam.orbit(1.5, 0.8);
        cam.orbit(-1.5, -0.8);
        let restored = cam.position();
        assert!((orig - restored).norm() < 1e-10);
    }

    #[test]
    fn zoom_changes_radius() {
        let mut cam = test_camera();
        let r_before = cam.radius();
        cam.zoom(5.0);
        assert!((cam.radius() - r_before - 5.0).abs() < 1e-10);
    }

    #[test]
    fn view_matrix_is_rotation() {
        let cam = test_camera();
        let view = cam.view_matrix();
        let rot = view.rotation.to_rotation_matrix();
        // Rotation matrix determinant is 1
        assert!((rot.matrix().determinant() - 1.0).abs() < 1e-10);
    }

    #[test]
    fn project_origin_to_ndc_center() {
        let cam = Camera::new(Point3::new(0.0, 0.0, 0.0), 10.0);
        // Point at target projects to NDC center (0, 0)
        let ndc = cam.project(&Point3::new(0.0, 0.0, 0.0));
        assert!(ndc.x.abs() < 1e-10);
        assert!(ndc.y.abs() < 1e-10);
    }
}
```

**Acceptance:**
```bash
cargo test -p chemrust-vis-core -- camera
```

---

### TASK-CORE-LOADER: CASTEP .cell file loader (feature-gated)
**Kind:** lib-tdd
**Goal:** Parse CASTEP `.cell` files via `castep-cell-io`/`castep-cell-fmt` and construct `chemrust_geometry::Structure`. Feature-gated behind `castep-loader`.

**Success Criteria:**
- `CellLoader::load("Cu111_CO.cell")` returns `Structure` with `num_atoms() == 18`
  (Source: Cu111_CO.cell POSITIONS_FRAC block, 18 entries)
- Species counts: 16 Cu, 1 C, 1 O
  (Source: Cu111_CO.cell POSITIONS_FRAC species column)
- First atom species is `ElementSymbol::Cu` at frac ≈ (0, 0, 0) ± 1e-10
  (Source: Cu111_CO.cell line 8)
- `structure.cell.is_some()` with `cell.lengths()` ≈ (10.2248, 17.7098, 18.2614) ± 0.01
  (Source: Cu111_CO.cell LATTICE_CART block)
- Cell volume ≈ 3306.74 Å³ ± 1.0
  (Source: a×b×c for axis-aligned cell = 10.2248 × 17.7098 × 18.2614)
- `CellLoader::load()` returns `Err` for non-existent file path
- Compiles without `castep-cell-io` when feature `castep-loader` is disabled

**Files:**
- `crates/chemrust-vis-core/src/loader.rs` — `CellLoader` struct, feature-gated
- `crates/chemrust-vis-core/src/lib.rs` — `#[cfg(feature = "castep-loader")] pub mod loader;`
- `crates/chemrust-vis-core/Cargo.toml` — optional deps + feature definition

**Test file:** `crates/chemrust-vis-core/tests/loader_tests.rs` (integration test, needs feature)

**Test code sketch:**
```rust
// tests/loader_tests.rs — only compiled when feature "castep-loader" is enabled
#![cfg(feature = "castep-loader")]

use chemrust_vis_core::loader::CellLoader;
use chemrust_geometry::ElementSymbol;
use std::path::Path;

const FIXTURE: &str = "../chemrust/chemrust-geometry/Cu111_CO.cell";

#[test]
fn load_cu111_co_cell_has_18_atoms() {
    let structure = CellLoader::load(Path::new(FIXTURE)).unwrap();
    assert_eq!(structure.num_atoms(), 18);
}

#[test]
fn load_cu111_co_cell_species_counts() {
    let structure = CellLoader::load(Path::new(FIXTURE)).unwrap();
    let n_cu = structure.species.iter().filter(|s| **s == ElementSymbol::Cu).count();
    let n_c = structure.species.iter().filter(|s| **s == ElementSymbol::C).count();
    let n_o = structure.species.iter().filter(|s| **s == ElementSymbol::O).count();
    assert_eq!(n_cu, 16);
    assert_eq!(n_c, 1);
    assert_eq!(n_o, 1);
}

#[test]
fn load_cu111_co_cell_lattice_vectors() {
    let structure = CellLoader::load(Path::new(FIXTURE)).unwrap();
    let cell = structure.cell.expect("should have cell");
    let (a, b, c) = cell.lengths();
    assert!((a - 10.2248).abs() < 0.01, "a={}", a);
    assert!((b - 17.7098).abs() < 0.01, "b={}", b);
    assert!((c - 18.2614).abs() < 0.01, "c={}", c);
}

#[test]
fn load_cu111_co_cell_origin_atom() {
    let structure = CellLoader::load(Path::new(FIXTURE)).unwrap();
    let first = structure.frac_coords[0];
    assert!((first.x).abs() < 1e-10);
    assert!((first.y).abs() < 1e-10);
    assert!((first.z).abs() < 1e-10);
}
```

**Acceptance:**
```bash
cargo test -p chemrust-vis-core --features castep-loader -- test loader
```

---

### TASK-CORE-VIEWPORT: Viewport projection pipeline with depth sorting
**Kind:** lib-tdd
**Goal:** Implement the viewport rendering pipeline: 3D world → view → clip → NDC → canvas coordinates. Produce draw commands (points + lines) with depth sorting.

**Success Criteria:**
- `Viewport::render(&scene, &camera, width, height)` returns `DrawCommands` with `points` and `lines`
- Points are sorted by view-space Z (back-to-front) for correct depth occlusion
  (Source: painter's algorithm — farthest first)
- With camera looking down -z (phi=PI, theta=0, target=structure center): Cu at origin projects to `canvas_x ≈ width/2`, `canvas_y ≈ height/2`
  (Source: looking down -z from above, origin is at target=center → projects to screen center)
- Lines (cell edges) are included in `draw_commands.lines`
- `Viewport::new()` with default canvas size (80×24 chars)
- NDC → canvas transformation: `canvas_x = (ndc_x + 1.0) / 2.0 * width`, `canvas_y = (1.0 - ndc_y) / 2.0 * height`
  (Source: standard NDC→screen coordinate mapping, y-axis flipped)

**Files:**
- `crates/chemrust-vis-core/src/viewport.rs` — `Viewport`, `DrawCommands`, `DrawPoint`, `DrawLine` structs
- `crates/chemrust-vis-core/src/lib.rs` — `pub mod viewport;`

**Test file:** `crates/chemrust-vis-core/src/viewport.rs` — `#[cfg(test)] mod tests`

**Test code sketch:**
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::Camera;
    use crate::scene::Scene;
    use chemrust_geometry::slab::cu111_co_system;
    use nalgebra::Point3;

    #[test]
    fn render_produces_draw_commands() {
        let structure = cu111_co_system(3.615);
        let scene = Scene::from_structure(&structure);
        let center = scene.bounding_box_center();
        let camera = Camera::new(Point3::new(center[0], center[1], center[2] + 20.0), 20.0);
        let viewport = Viewport::new(160.0, 96.0);
        let cmds = viewport.render(&scene, &camera);
        assert_eq!(cmds.points.len(), 18);
        assert_eq!(cmds.lines.len(), 12); // cell edges
    }

    #[test]
    fn depth_sort_back_to_front() {
        // Two atoms at different z, camera looking from +z
        let structure = cu111_co_system(3.615);
        let scene = Scene::from_structure(&structure);
        let camera = Camera::new(Point3::new(5.0, 5.0, 30.0), 10.0);
        let viewport = Viewport::new(160.0, 96.0);
        let cmds = viewport.render(&scene, &camera);
        // Higher z atoms (closer to camera at +z) should be later in list
        // The O atom at z≈14.4 should appear after Cu at origin (z=0)
        // Verify: last element is closer to camera than first
        assert!(cmds.points.len() >= 2);
    }

    #[test]
    fn molecule_no_cell_edges() {
        use chemrust_geometry::{FracCoord, ElementSymbol, Structure};
        let mol = Structure::new(
            vec![ElementSymbol::H; 2],
            vec![FracCoord::new(0.,0.,0.), FracCoord::new(0.,0.,0.74)],
            None, [false;3], vec![0;2], vec![None;2], None,
        );
        let scene = Scene::from_structure(&mol);
        let camera = Camera::new(Point3::new(0.,0.,5.), 10.);
        let viewport = Viewport::new(160., 96.);
        let cmds = viewport.render(&scene, &camera);
        assert_eq!(cmds.points.len(), 2);
        assert!(cmds.lines.is_empty());
    }
}
```

**Acceptance:**
```bash
cargo test -p chemrust-vis-core -- viewport
```

---

## Group: tui

### TASK-TUI-CANVAS: Ratatui Braille canvas rendering
**Kind:** direct
**Goal:** Implement a ratatui `Canvas` widget that consumes `DrawCommands` and renders atoms as Braille dots with element colors, and cell edges as lines.

**Changes:**
- Implement `ratatui::widgets::Widget` for a `SceneWidget` that takes `&DrawCommands`
- Map draw points to Braille dots: compute character cell, set the appropriate dot bit in the Braille code point (U+2800 base + bit encoding)
- Map draw lines to canvas line segments
- Use element-based colors (Cu = copper/orange, C = dark gray, O = red)
- Status bar at bottom showing file name, atom count, camera angles

**Files:**
- `crates/chemrust-vis-tui/src/tui/widgets/scene_view.rs` — `SceneWidget`
- `crates/chemrust-vis-tui/src/tui/widgets/status_bar.rs` — `StatusBar`
- `crates/chemrust-vis-tui/src/tui/widgets/mod.rs`
- `crates/chemrust-vis-tui/src/tui/mod.rs`

**Acceptance:**
```bash
cargo check -p chemrust-vis-tui
cargo build -p chemrust-vis-tui
```

---

### TASK-TUI-APP: TUI application with keyboard camera controls and CLI
**Kind:** direct
**Goal:** Build the full TUI application: ratatui event loop, keyboard→camera mapping, scene view + status bar layout, CLI argument parsing.

**Changes:**
- `App` struct implementing `ratatui::widgets::StatefulWidget` or manual rendering
- Event loop: `crossterm::event::poll` + `read`, map keys to camera actions:
  - `w/s`: orbit θ (+/-)
  - `a/d`: orbit φ (+/-)
  - `q/e`: orbit θ (fine) or alternative axis
  - `+/-`: zoom in/out
  - `Shift+w/a/s/d`: pan
  - `r`: reset camera to default view
  - `Esc/q`: quit
- Layout: main area = SceneView, bottom 1 line = StatusBar
- CLI: `chemrust-vis-tui <file.cell>` using clap
- On startup: load structure via `CellLoader`, construct `Scene`, initialize `Camera` with auto-framing

**Files:**
- `crates/chemrust-vis-tui/src/tui/app.rs` — `App` struct and event loop
- `crates/chemrust-vis-tui/src/tui/input.rs` — key event → camera action mapping
- `crates/chemrust-vis-tui/src/main.rs` — CLI parsing, app init, main loop

**Acceptance:**
```bash
cargo build -p chemrust-vis-tui
# Manual verification: run with Cu111_CO.cell, see atoms in terminal
```

---

## Exploration Notes

### E1: castep-cell-io parsing capability

**Finding:** `castep-cell-io` v0.5.0 (used in chemrust workspace) combined with `castep-cell-fmt` v0.1.0 provides `parse::<CellDocument>(&str)` which parses .cell text into a typed `CellDocument`. The example in the crate docs confirms this works.

**Verified:** `castep-cell-fmt` docs show `parse_cell_file()` for tokenization and `parse()` for typed deserialization. `castep-cell-io` re-exports and uses this.

### E2: Structure construction from CellDocument

**Finding:** The `cu111_co.rs` example shows the inverse direction (Structure → CellDocument → .cell text). The loading direction (CellDocument → Structure) requires:
1. Extract `LatticeCart` fields → build `LatticeVectors` via `from_constants` or directly from the 3×3 matrix
2. Extract `PositionsFrac::positions` → map each `PositionFracEntry` to (ElementSymbol, FracCoord)
3. Build `Structure::new(...)` with collected data

**Implementation note:** Species parsing from `PositionFracEntry.species: Species::Symbol(String)` requires converting the string to `ElementSymbol`. `castep-periodic-table` provides `ElementSymbol::from_str()` or we can use `ELEMENT_TABLE.get_by_symbol()`.

### E3: Fixture path resolution

The Cu111_CO.cell fixture is in `../chemrust/chemrust-geometry/Cu111_CO.cell` relative to the chemrust-vis project root. For test code, this path must be resolved relative to `CARGO_MANIFEST_DIR`. The integration test should use:
```rust
let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
    .join("../../chemrust/chemrust-geometry/Cu111_CO.cell");
```

### E4: Camera auto-framing

To compute the default camera radius: find the bounding box of all atom positions, compute its diagonal length, set `radius = diagonal * 1.5` to ensure the entire structure is visible. Target = bounding box center.

### E5: Braille dot encoding

Each Braille character (U+2800–U+28FF) encodes 8 dots in a 2×4 grid:
- Dots 1-3: left column (top to bottom) → bits 0-2
- Dots 4-6: right column (top to bottom) → bits 3-5
- Dots 7-8: bottom row left, bottom row right → bits 6-7

Canvas coordinates map: char_col = floor(canvas_x / 2), char_row = floor(canvas_y / 4), dot_index within char = (canvas_x % 2) * 3 + (canvas_y % 4). The code point = 0x2800 | (1 << dot_index).

This comes from the Unicode Braille Patterns block specification, not from a reference implementation.
