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

| Group | Tasks | Depends On | Kind | Notes |
|-------|-------|-----------|------|-------|
| group-scaffold | TASK-SCAFFOLD | none | direct | |
| group-core | TASK-CORE-SCENE, TASK-CORE-CAMERA, TASK-CORE-LOADER | group-scaffold | lib-tdd | These three are independent of each other |
| group-viewport | TASK-CORE-VIEWPORT | group-core (needs SCENE + CAMERA) | lib-tdd | Must be implemented AFTER SCENE and CAMERA are complete |
| group-tui | TASK-TUI-CANVAS, TASK-TUI-APP | group-viewport | direct | |

**Intra-group ordering:** TASK-CORE-VIEWPORT depends on both TASK-CORE-SCENE and TASK-CORE-CAMERA.
TASK-CORE-SCENE, TASK-CORE-CAMERA, and TASK-CORE-LOADER are independent of each other and can be parallelized.

---

## Group: scaffold

### TASK-SCAFFOLD: Workspace and crate scaffolding
**Kind:** direct
**Goal:** Set up the Cargo workspace with two crates and all dependencies declared.

**Files to create/modify:**
- `Cargo.toml` — workspace root with `members = ["crates/chemrust-vis-core", "crates/chemrust-vis-tui"]`
- `crates/chemrust-vis-core/Cargo.toml` — lib crate with deps: chemrust-geometry (path), nalgebra 0.33, serde, serde_json. **NO ratatui dependency** (canvas rendering is the bin crate's concern). Optional dep: castep-cell-io 0.5 + castep-cell-fmt 0.1 behind feature "castep-loader". Dev-deps: proptest.
- `crates/chemrust-vis-core/src/lib.rs` — stub with `//! chemrust-vis-core` module doc
- `crates/chemrust-vis-tui/Cargo.toml` — bin crate with deps: chemrust-vis-core (path, features=["castep-loader"]), ratatui (with crossterm backend, **verify canvas feature name** — may not need a feature flag in recent ratatui versions), crossterm, anyhow, clap
- `crates/chemrust-vis-tui/src/main.rs` — stub with `fn main() { println!("chemrust-vis-tui"); }`

**Pre-flight verification:** Before writing Cargo.toml, check the current ratatui version's feature list. The `unstable-canvas` feature name was from the `tui` crate predecessor; modern `ratatui` may expose the canvas widget without a feature flag. Use `cargo search ratatui` and check docs.rs for the active feature list.

**Acceptance:**
```bash
cargo check --workspace
```

---

## Group: core

### TASK-CORE-SCENE: Scene struct, color map, and construction from Structure
**Kind:** lib-tdd
**Goal:** Define `Scene`, `AtomDrawData`, and `RgbColor` types in the lib crate. Implement construction from `chemrust_geometry::Structure`, handling both periodic and molecular structures. Include an element→color map.

**Design decisions (from architecture review):**
- `AtomDrawData.color` uses a lib-crate `RgbColor(u8, u8, u8)` type, NOT `ratatui::style::Color`. The bin crate converts to ratatui colors. This keeps the scene renderer-agnostic (ADR-0001 consequence: future SVG/sixel renderers can consume the same data).
- `Scene::from_structure()` handles molecules (cell=None) by treating fractional coordinates as Cartesian (Angstrom). Rationale: for isolated molecules, there is no cell to be fractions of, so the stored values are effectively Cartesian positions.
- `Scene` provides `bounding_box_center() -> [f64; 3]` for camera auto-framing (used by VIEWPORT and TUI-APP).
- Color map is a free function `fn element_color(element: ElementSymbol) -> RgbColor` in `scene.rs`. Covers at minimum: H, C, N, O, Cu, plus a default gray for unknown elements. Expandable later.

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
- `scene.cell_edges[0]` connects origin corner to a-vector corner (edge ≈ (0,0,0)→(10.2248,0,0) in axis-aligned cell)
  (Source: edge 0-1 connects corner at indices 0 and 1 = origin + a)
- **Molecule path**: `Scene::from_structure(&mol_with_no_cell)` produces `atoms` with positions equal to the stored fractional coords treated as Cartesian, and `cell_edges.is_empty()`
  (Source: for cell=None, there are no lattice vectors to convert; treat stored values as Cartesian)
- `scene.bounding_box_center()` returns the centroid of all atom positions
- `element_color(ElementSymbol::Cu)` returns an orange/copper color; `element_color(ElementSymbol::C)` returns dark gray; `element_color(ElementSymbol::O)` returns red
- Different elements produce different colors: `element_color(ElementSymbol::Cu) != element_color(ElementSymbol::C)`
  (Source: visual requirement for distinguishing atoms)

**Files:**
- `crates/chemrust-vis-core/src/scene.rs` — `Scene`, `AtomDrawData`, `RgbColor` structs, `Scene::from_structure()`, `element_color()`
- `crates/chemrust-vis-core/src/lib.rs` — `pub mod scene; pub use scene::*;`

**Test file:** `crates/chemrust-vis-core/src/scene.rs` — `#[cfg(test)] mod tests`

**Test code sketch:**
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use chemrust_geometry::slab::cu111_co_system;
    use chemrust_geometry::{ElementSymbol, FracCoord, Structure};

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
        let origin = scene.atoms[0].position;
        assert!((origin[0]).abs() < 1e-6);
        assert!((origin[1]).abs() < 1e-6);
        assert!((origin[2]).abs() < 1e-6);
    }

    #[test]
    fn second_atom_cartesian_position() {
        let structure = cu111_co_system(3.615);
        let scene = Scene::from_structure(&structure);
        let pos = scene.atoms[1].position;
        assert!((pos[0] - 1.2781).abs() < 0.001, "x={}", pos[0]);
        assert!((pos[1] - 0.7379).abs() < 0.001, "y={}", pos[1]);
        assert!((pos[2] - 2.0871).abs() < 0.001, "z={}", pos[2]);
    }

    #[test]
    fn cell_edges_count_is_12() {
        let structure = cu111_co_system(3.615);
        let scene = Scene::from_structure(&structure);
        assert_eq!(scene.cell_edges.len(), 12);
    }

    #[test]
    fn first_cell_edge_is_a_vector() {
        let structure = cu111_co_system(3.615);
        let scene = Scene::from_structure(&structure);
        let (start, end) = scene.cell_edges[0];
        // Edge 0 connects origin to a-vector corner
        assert!((start[0]).abs() < 1e-6 && (start[1]).abs() < 1e-6 && (start[2]).abs() < 1e-6);
        assert!((end[0] - 10.2248).abs() < 0.01);
    }

    #[test]
    fn molecule_path_produces_atoms_no_edges() {
        let mol = Structure::new(
            vec![ElementSymbol::H, ElementSymbol::H],
            vec![FracCoord::new(0.0, 0.0, 0.0), FracCoord::new(0.0, 0.0, 0.74)],
            None, // no cell → molecule
            [false, false, false],
            vec![0, 0],
            vec![None, None],
            None,
        );
        let scene = Scene::from_structure(&mol);
        // For molecules: 2 atoms, positions treated as Cartesian from frac_coords
        assert_eq!(scene.atoms.len(), 2);
        assert!((scene.atoms[0].position[2]).abs() < 1e-10);
        assert!((scene.atoms[1].position[2] - 0.74).abs() < 1e-10);
        assert!(scene.cell_edges.is_empty());
    }

    #[test]
    fn bounding_box_center_is_centroid() {
        let structure = cu111_co_system(3.615);
        let scene = Scene::from_structure(&structure);
        let center = scene.bounding_box_center();
        // Center should be within the cell volume, roughly (5, 9, 9) for axis-aligned
        assert!(center[0] > 0.0 && center[0] < 11.0);
        assert!(center[1] > 0.0 && center[1] < 18.0);
        assert!(center[2] > 0.0 && center[2] < 19.0);
    }

    #[test]
    fn element_colors_are_distinct() {
        assert_ne!(element_color(ElementSymbol::Cu), element_color(ElementSymbol::C));
        assert_ne!(element_color(ElementSymbol::C), element_color(ElementSymbol::O));
        assert_ne!(element_color(ElementSymbol::Cu), element_color(ElementSymbol::O));
    }

    #[test]
    fn unknown_element_has_default_color() {
        // ElementSymbol has many variants; all should return a valid color
        let color = element_color(ElementSymbol::He);
        // Just verify it doesn't panic and produces a valid RGB triple
        assert!(color.r <= 255 && color.g <= 255 && color.b <= 255);
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

**Coordinate convention:** Z-up (physics convention). Spherical coords: theta = azimuthal (XY plane, 0 = +X), phi = polar (from Z axis, 0 = +Z). Camera position = `target + radius * (sin(phi)*cos(theta), sin(phi)*sin(theta), cos(phi))`. Up vector = (0, 0, 1) for `look_at_rh`.

**Success Criteria:**
- `Camera::new(target, radius)` sets theta=0, phi=PI/4, target, radius. Camera is at `target + (0, radius*sin(PI/4), radius*cos(PI/4))` — roughly at 45° elevation from Z axis.
- `camera.position()` for theta=0, phi=PI/2, radius=10, target=(5,5,5): position ≈ (5 + 10, 5, 5) = (15, 5, 5) ± 1e-10
  (Source: at phi=PI/2, sin(phi)=1, cos(phi)=0, theta=0: pos = target + (10, 0, 0) — camera is on +X side)
- `camera.position()` for theta=PI/2, phi=PI/2, radius=10, target=(5,5,5): position ≈ (5, 5 + 10, 5) = (5, 15, 5) ± 1e-10
  (Source: at phi=PI/2, theta=PI/2: pos = target + (0, 10, 0) — camera is on +Y side)
- `camera.view_matrix()` for camera at (0,0,10) looking at origin: the view matrix should map world point (0,0,0) to camera-local origin, and world right vector (1,0,0) approximately to camera-local right
  (Source: `look_at_rh` from (0,0,10) to (0,0,0) with up=(0,1,0) or (0,0,1))
- `camera.project(world_point)` for orthographic: a point offset from target by (dx, dy, 0) should project to NDC (dx/scale, dy/scale) where scale = terminal_height / (2 * radius)
  (Source: orthographic projection is linear; scale factor from PHASE_PLAN design notes)
- Specific projection test: camera at (0,0,10) looking at (0,0,0), orthographic scale = 1.0. Point at (2, 3, 0) projects to NDC ≈ (2, 3, ??) — the exact values depend on the view matrix orientation
  (Source: manually computed: world (2,3,0) in view space of camera at z=10 looking at origin is approximately (2, 3, -10). With no perspective, orthographic projects (x/z_scale, y/z_scale) → NDC (0.2, 0.3) for scale=1)
- `camera.orbit(d_theta, d_phi)` updates internal theta and phi by the given deltas
- `camera.zoom(dr)` changes `radius` by exactly `dr`; `radius` is clamped to >= 0.1
- `camera.pan(dx, dy)` translates `target` by `dx * right + dy * up` where right/up are camera-local axes

**Anti-placebo note:** Tests MUST assert concrete position values for specific camera parameters, not just round-trip symmetry. The orbit round-trip test (`orbit(θ,φ); orbit(-θ,-φ)`) is banned — it's a circular placebo (ODD taxonomy §2). Instead, assert that `camera.position()` produces a known coordinate for known parameters.

**Files:**
- `crates/chemrust-vis-core/src/camera.rs` — `Camera` struct with `position()`, `view_matrix()`, `project()`, `orbit()`, `zoom()`, `pan()`
- `crates/chemrust-vis-core/src/lib.rs` — `pub mod camera;`

**Test file:** `crates/chemrust-vis-core/src/camera.rs` — `#[cfg(test)] mod tests`

**Test code sketch:**
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use nalgebra::Point3;

    #[test]
    fn position_at_theta_0_phi_pi_half() {
        // Camera on +X side, looking at target
        let cam = Camera::with_angles(Point3::new(5.0, 5.0, 5.0), 10.0, 0.0, std::f64::consts::FRAC_PI_2);
        let pos = cam.position();
        // phi=PI/2 means in XY plane, theta=0 means +X direction
        assert!((pos.x - 15.0).abs() < 1e-10, "x={}", pos.x);
        assert!((pos.y - 5.0).abs() < 1e-10, "y={}", pos.y);
        assert!((pos.z - 5.0).abs() < 1e-10, "z={}", pos.z);
    }

    #[test]
    fn position_at_theta_pi_half_phi_pi_half() {
        // Camera on +Y side
        let cam = Camera::with_angles(Point3::new(5.0, 5.0, 5.0), 10.0, std::f64::consts::FRAC_PI_2, std::f64::consts::FRAC_PI_2);
        let pos = cam.position();
        assert!((pos.x - 5.0).abs() < 1e-10, "x={}", pos.x);
        assert!((pos.y - 15.0).abs() < 1e-10, "y={}", pos.y);
        assert!((pos.z - 5.0).abs() < 1e-10, "z={}", pos.z);
    }

    #[test]
    fn position_at_top_down() {
        // Camera directly above (+Z), looking down
        let cam = Camera::with_angles(Point3::new(0.0, 0.0, 0.0), 10.0, 0.0, 0.0);
        let pos = cam.position();
        assert!((pos.x).abs() < 1e-10);
        assert!((pos.y).abs() < 1e-10);
        assert!((pos.z - 10.0).abs() < 1e-10);
    }

    #[test]
    fn orbit_updates_angles() {
        let mut cam = Camera::new(Point3::new(0.0, 0.0, 0.0), 10.0);
        let pos_before = cam.position();
        cam.orbit(std::f64::consts::FRAC_PI_2, 0.0);
        let pos_after = cam.position();
        // Position should have changed (different theta)
        assert!((pos_before - pos_after).norm() > 1e-6);
    }

    #[test]
    fn zoom_changes_radius() {
        let mut cam = Camera::new(Point3::new(0.0, 0.0, 0.0), 10.0);
        cam.zoom(5.0);
        assert!((cam.radius() - 15.0).abs() < 1e-10);
        cam.zoom(-20.0); // clamp: won't go below 0.1
        assert!(cam.radius() >= 0.1);
    }

    #[test]
    fn project_offset_point() {
        // Camera at (0,0,10) looking at (0,0,0), orthographic scale=1.0
        let cam = Camera::with_angles(Point3::new(0.0, 0.0, 0.0), 10.0, 0.0, 0.0);
        // Point at (2, 3, 0): offset from target in XY plane
        let ndc = cam.project(&Point3::new(2.0, 3.0, 0.0), 1.0);
        // With camera above, (x=2, y=3) projects to positive NDC quadrant
        assert!(ndc.x > 0.0, "ndc.x should be positive, got {}", ndc.x);
        assert!(ndc.y > 0.0, "ndc.y should be positive, got {}", ndc.y);
    }

    #[test]
    fn pan_moves_target() {
        let mut cam = Camera::new(Point3::new(5.0, 5.0, 5.0), 10.0);
        let target_before = cam.target();
        cam.pan(2.0, 3.0);
        let target_after = cam.target();
        assert!((target_after - target_before).norm() > 1e-6);
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

## Group: viewport

**Dependency:** TASK-CORE-VIEWPORT MUST be implemented after TASK-CORE-SCENE and TASK-CORE-CAMERA.
The viewport consumes both `Scene` and `Camera` types. Do not attempt to implement in parallel.

### TASK-CORE-VIEWPORT: Viewport projection pipeline with depth sorting
**Kind:** lib-tdd
**Goal:** Implement the viewport rendering pipeline: 3D world → view (via Camera) → NDC → canvas coordinates. Produce `DrawCommands` (points + lines) with correct depth sorting (painter's algorithm: farthest first).

**Prerequisites:** TASK-CORE-SCENE and TASK-CORE-CAMERA must be complete and passing.

**Success Criteria:**
- `Viewport::new(width, height)` creates a viewport with given canvas dimensions (f64, in dot units: width = cols×2, height = rows×4 for Braille)
- `Viewport::render(&scene, &camera)` returns `DrawCommands` with `points: Vec<DrawPoint>` and `lines: Vec<DrawLine>`
- `DrawPoint` has `x: f64, y: f64, z_view: f64, color: RgbColor` (z_view for depth sorting verification)
- `DrawLine` has `x1: f64, y1: f64, x2: f64, y2: f64`
- **Depth sort (non-vacuous)**: Points are sorted by decreasing view-space Z (farthest from camera = index 0). Verify with two atoms at known different world-Z depths from the Cu111_CO fixture: the O atom (z≈14.42 in Cartesian) should appear BEFORE a Cu atom at the origin (z=0) when camera looks from +Z
  (Source: painter's algorithm: farthest first. With camera at (5,5,30), O at z≈14 is farther than Cu at z≈0, relative to camera at z=30: view-Z(O) = |30-14.4| = 15.6, view-Z(Cu) = |30-0| = 30. So Cu is farther → Cu should be first.)
- **Content assertion**: With camera looking directly down -Z at the structure center, the origin Cu atom (Cartesian (0,0,0)) projects to canvas coordinates near the center: `canvas_x ≈ width/2`, `canvas_y ≈ height/2`
  (Source: top-down orthographic view, target at structure center, origin is offset from center → projects to a specific quadrant)
- **Cell edges rendered**: `draw_commands.lines.len() == 12` for the Cu111_CO scene
  (Source: 12 edges of the parallelepiped)
- **Molecule path**: Scene with no cell → `draw_commands.lines.is_empty()`
- NDC → canvas transformation: `canvas_x = (ndc_x + 1.0) / 2.0 * width`, `canvas_y = (1.0 - ndc_y) / 2.0 * height`
  (Source: standard NDC→screen mapping, y-axis flipped for top-left origin)
- All projected points fall within `[0, width] × [0, height]` or are culled (no panics for off-screen atoms)

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
    fn render_has_correct_point_count() {
        let structure = cu111_co_system(3.615);
        let scene = Scene::from_structure(&structure);
        let center = scene.bounding_box_center();
        let camera = Camera::new(Point3::new(center[0], center[1], center[2] + 20.0), 20.0);
        let viewport = Viewport::new(160.0, 96.0);
        let cmds = viewport.render(&scene, &camera);
        assert_eq!(cmds.points.len(), 18);
        assert_eq!(cmds.lines.len(), 12);
    }

    #[test]
    fn depth_sort_farthest_first() {
        // Camera looking from +Z above. Two atoms at different Z depths.
        // O atom at z≈14.4, Cu atom at z=0. Camera at z≈30.
        // Cu at z=0 is FARTHER from camera (view-Z ≈ 30) than O at z≈14 (view-Z ≈ 15.6)
        // Painter's algorithm: Cu (farther) should be at index 0, O (closer) later.
        let structure = cu111_co_system(3.615);
        let scene = Scene::from_structure(&structure);
        // Camera at z=30, looking down at structure center
        let camera = Camera::new(Point3::new(5.0, 9.0, 30.0), 10.0);
        let viewport = Viewport::new(160.0, 96.0);
        let cmds = viewport.render(&scene, &camera);

        // Find indices of origin Cu (z≈0) and O atom (z≈14)
        let cu_idx = scene.atoms.iter().position(|a|
            a.position[0].abs() < 1e-6 && a.position[1].abs() < 1e-6 && a.position[2].abs() < 1e-6
        ).expect("origin Cu not found");
        let o_idx = scene.atoms.iter().position(|a|
            a.element == chemrust_geometry::ElementSymbol::O
        ).expect("O atom not found");

        // Verify that the origin Cu (farther from camera) appears before O in sorted list
        let cu_sort_pos = cmds.points.iter().position(|p| p.atom_index == cu_idx).unwrap();
        let o_sort_pos = cmds.points.iter().position(|p| p.atom_index == o_idx).unwrap();
        assert!(cu_sort_pos < o_sort_pos,
            "Cu at z=0 (farther) should render before O at z≈14 (closer): Cu pos={}, O pos={}",
            cu_sort_pos, o_sort_pos);
    }

    #[test]
    fn origin_projects_near_center_top_down() {
        let structure = cu111_co_system(3.615);
        let scene = Scene::from_structure(&structure);
        let center = scene.bounding_box_center();
        // Camera above, looking down
        let camera = Camera::with_angles(
            Point3::new(center[0], center[1], center[2]),
            20.0, 0.0, 0.0, // theta=0, phi=0: top-down
        );
        let viewport = Viewport::new(160.0, 96.0);
        let cmds = viewport.render(&scene, &camera);

        // Origin atom should be in the bottom-left quadrant relative to center
        // (center at ~(5,9,9), origin at (0,0,0) → offset (-5,-9,-9))
        let origin_pt = cmds.points.iter().find(|p| p.atom_index == 0).unwrap();
        // In top-down orthographic, smaller world coords → smaller canvas coords
        // origin at (0,0) is below-left of center at (5,9) → canvas_x < width/2, canvas_y > height/2
        assert!(origin_pt.x < 80.0, "origin should be left of center, x={}", origin_pt.x);
        assert!(origin_pt.y > 48.0, "origin should be below center (higher y), y={}", origin_pt.y);
    }

    #[test]
    fn molecule_has_no_cell_edges_in_output() {
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

### TASK-TUI-CANVAS: Ratatui Braille canvas rendering with tests
**Kind:** direct
**Goal:** Implement a ratatui `Canvas` widget that consumes `DrawCommands` and renders atoms as Braille dots with element colors, and cell edges as lines. Include unit tests for the Braille encoding function.

**Changes:**
- Braille encoding function (pure, in the bin crate or as a utility):
  - Input: canvas coordinates (f64, f64) → output: (char_col, char_row, dot_bit)
  - Map: `char_col = floor(canvas_x / 2)`, `char_row = floor(canvas_y / 4)`
  - Dot index within char: `(canvas_x % 2) * 3 + (canvas_y % 4)` (left column = dots 0-2, right column = dots 3-5, bottom = dots 6-7)
  - Code point: `0x2800 | (1 << dot_index)`
  - (Source: Unicode Braille Patterns block, U+2800–U+28FF)
- Element color conversion: `RgbColor` → `ratatui::style::Color`
- `SceneWidget`: ratatui widget that iterates `DrawCommands`, accumulates Braille characters into a cell grid, draws lines on canvas
- Status bar widget: displays file name, atom count, camera (θ, φ, radius)

**Test criteria for Braille encoding:**
- Dot at canvas (0, 0) → char_col=0, char_row=0, dot_index=0, code point = U+2801
  (Source: Unicode Braille: dot 0 (top-left) = bit 0 = U+2801)
- Dot at canvas (1, 0) → char_col=0, char_row=0, dot_index=3, code point = U+2808
  (Source: x%2=1, y%4=0 → 1*3+0=3 → bit 3 = U+2808 — dot 4, top of right column)
- Dot at canvas (0, 3) → char_col=0, char_row=0, dot_index=3, code point = U+2808? Wait: y%4=3 → 1*3+3... no. x%2=0, y%4=3 → 0*3+3=3 → bit 3 = U+2808 (dot 4)
  Actually: dot_index = (x % 2) * 3 + (y % 4). For (0,3): (0)*3+3 = 3, bit 3, U+2808.
  For (1,3): (1)*3+3 = 6, bit 6, U+2840.
  For (0,0): (0)*3+0 = 0, bit 0, U+2801. ✓
- Two dots in the same char cell: (0,0) and (1,2) → code point = U+2801 | (1 << ((1)*3+2)) = U+2801 | (1<<5) = U+2801 | 0x20 = U+2821
  (Source: combining Braille bits in same cell)
- Canvas coord (3.7, 8.2): char_col=floor(3.7/2)=1, char_row=floor(8.2/4)=2, dot_index=(1)*3+(0)=3 (since 3%2=1, 8%4=0) → bit 3 → U+2808
- Status bar renders without panic (at minimum: program doesn't crash on draw)

**Files:**
- `crates/chemrust-vis-tui/src/tui/widgets/scene_view.rs` — `SceneWidget`, Braille encoding function
- `crates/chemrust-vis-tui/src/tui/widgets/status_bar.rs` — `StatusBar`
- `crates/chemrust-vis-tui/src/tui/widgets/mod.rs`
- `crates/chemrust-vis-tui/src/tui/mod.rs`

**Test file:** `crates/chemrust-vis-tui/src/tui/widgets/scene_view.rs` — `#[cfg(test)] mod tests` for Braille encoding

**Test code sketch for Braille encoding:**
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn braille_dot_0_0() {
        let (col, row, dot) = canvas_to_braille(0.0, 0.0);
        assert_eq!(col, 0);
        assert_eq!(row, 0);
        assert_eq!(dot, 0);
        assert_eq!(0x2800 | (1 << dot), 0x2801);
    }

    #[test]
    fn braille_dot_1_0_right_column_top() {
        let (col, row, dot) = canvas_to_braille(1.0, 0.0);
        assert_eq!(col, 0);
        assert_eq!(row, 0);
        assert_eq!(dot, 3); // right column, top
        assert_eq!(0x2800 | (1 << dot), 0x2808);
    }

    #[test]
    fn braille_two_dots_same_cell() {
        let (c1, r1, d1) = canvas_to_braille(0.0, 0.0);
        let (c2, r2, d2) = canvas_to_braille(1.0, 2.0);
        assert_eq!(c1, c2);
        assert_eq!(r1, r2);
        let combined = 0x2800u32 | (1 << d1) | (1 << d2);
        assert_eq!(combined, 0x2801 | 0x20); // bits 0 and 5
    }

    #[test]
    fn braille_subpixel_flooring() {
        let (col, row, dot) = canvas_to_braille(3.7, 8.2);
        assert_eq!(col, 1); // floor(3.7/2) = 1
        assert_eq!(row, 2); // floor(8.2/4) = 2
        assert_eq!(dot, 3); // (3%2=1)*3 + (8%4=0) = 3
    }
}
```

**Acceptance:**
```bash
cargo test -p chemrust-vis-tui -- scene_view
cargo build -p chemrust-vis-tui
```

---

### TASK-TUI-APP: TUI application with keyboard camera controls, resize, and CLI
**Kind:** direct
**Goal:** Build the full TUI application: ratatui event loop, keyboard→camera mapping, scene view + status bar layout, CLI argument parsing, terminal resize handling.

**Changes:**
- `App` struct holding: `Scene`, `Camera`, `Viewport`, `file_path: String`
- Event loop: `crossterm::event::poll` + `read`, map keys to camera actions:
  - `w/s`: orbit θ (+/- 5°)
  - `a/d`: orbit φ (+/- 5°)
  - `q/e`: fine orbit θ (+/- 1°)
  - `+/-`: zoom in/out
  - `Shift+w/a/s/d`: pan
  - `r`: reset camera to default (auto-framed view)
  - `Esc/q`: quit
- **Terminal resize**: handle `crossterm::event::Event::Resize(cols, rows)` — recreate `Viewport` with new dimensions `(cols as f64 * 2.0, rows as f64 * 4.0)` and trigger redraw
- Layout: main area = SceneView (flex 1), bottom 1 line = StatusBar
- StatusBar shows: `file.cell | 18 atoms | θ=30° φ=45° r=20.0`
- CLI: `chemrust-vis-tui <file.cell>` using clap. If no file provided, print usage and exit.
- On startup: load structure via `CellLoader`, construct `Scene`, compute bounding box, initialize `Camera` with auto-framing (target = bounding box center, radius = diagonal * 1.5)
- Terminal setup: `crossterm::terminal::enable_raw_mode()`, `EnterAlternateScreen`, `EnableMouseCapture` (mouse capture for future Phase 2, but mouse events are ignored in Phase 1)
- Terminal teardown on exit: `disable_raw_mode()`, `LeaveAlternateScreen`, `DisableMouseCapture`

**Files:**
- `crates/chemrust-vis-tui/src/tui/app.rs` — `App` struct, event loop, `ratatui::Terminal` management
- `crates/chemrust-vis-tui/src/tui/input.rs` — key event → camera action mapping
- `crates/chemrust-vis-tui/src/main.rs` — CLI parsing, terminal init, app launch

**Acceptance:**
```bash
cargo build -p chemrust-vis-tui
# Manual verification: run with Cu111_CO.cell, see atoms in terminal, rotate camera
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

To compute the default camera radius: find the bounding box of all atom positions (from `Scene::bounding_box_center()` and min/max positions), compute its diagonal length, set `radius = diagonal * 1.5` to ensure the entire structure is visible. Target = bounding box center.

### E5: Braille dot encoding

Each Braille character (U+2800–U+28FF) encodes 8 dots in a 2×4 grid:
- Dots 1-3: left column (top to bottom) → bits 0-2
- Dots 4-6: right column (top to bottom) → bits 3-5
- Dots 7-8: bottom row left, bottom row right → bits 6-7

Canvas coordinates map: `char_col = floor(canvas_x / 2)`, `char_row = floor(canvas_y / 4)`, `dot_index = (canvas_x % 2) * 3 + (canvas_y % 4)`. The code point = `0x2800 | (1 << dot_index)`.

(Source: Unicode Braille Patterns block specification, U+2800–U+28FF.)

### E6: Coordinate system convention (resolved during architecture review)

**Decision:** Z-up (physics convention). Camera spherical coordinates: theta = azimuthal angle in XY plane (0 = +X, π/2 = +Y), phi = polar angle from Z axis (0 = +Z looking down, π/2 = XY plane, π = -Z looking up). Camera position = `target + radius * (sin(phi)*cos(theta), sin(phi)*sin(theta), cos(phi))`. Up vector for `look_at_rh` = (0, 0, 1) when phi < π/2, and needs to handle the gimbal-lock case at phi=0 and phi=π with an alternative up vector.

This convention was chosen because:
- Crystal structures naturally have the c-axis as the "vertical" direction
- chemrust-geometry's `align_axes()` puts c along +Z
- The Cu111_CO fixture has c = (0, 0, 18.26) — Z axis

### E7: Crate boundary color type (resolved during architecture review)

**Decision:** `chemrust-vis-core` defines `pub struct RgbColor(pub u8, pub u8, pub u8)` in `scene.rs`. `AtomDrawData.color` is `RgbColor`. The bin crate converts `RgbColor → ratatui::style::Color::Rgb(r, g, b)` in `SceneWidget`. This keeps the lib crate free of ratatui dependencies (CONTEXT.md boundary rule: lib crate has zero dependency on crossterm; ratatui in lib must be minimized).

### E8: Molecule handling (resolved during architecture review)

**Decision:** When `Structure.cell` is `None` (molecule), `Scene::from_structure()` treats `frac_coords` values as Cartesian coordinates (Å). This is because fractional coordinates have no meaning without a reference cell. The molecule path is tested via a synthesized H₂ molecule with `cell: None`.

### E9: Placebo test migration

Architecture review identified 5 placebo test patterns in the original TASKS draft:
1. **`orbit_round_trip`** (circular) → replaced with known-position assertions for specific (θ, φ) pairs
2. **`depth_sort_back_to_front`** (vacuous `len() >= 2`) → replaced with sort-order assertion using specific atom indices
3. **`render_produces_draw_commands`** (shape-only) → kept as cardinality check, augmented with content assertion
4. **`project_origin_to_ndc_center`** (tautology) → replaced with offset-point projection test
5. **`view_matrix_is_rotation`** (property) → replaced with known-position assertions that implicitly validate the view transform

All fixes applied in this revision.
