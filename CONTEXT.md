# CONTEXT.md — chemrust-vis Domain Glossary

**Project**: chemrust-vis — TUI chemical structure 3D visualizer
**Established**: 2026-06-26
**Canonical source of truth** for domain terms, architecture, dependencies, and
coding conventions. All downstream pipeline stages (`/drive-outcomes`,
`/make-judgement`) reference this document. When a term appears in a prompt or
code, its meaning is defined here.

---

## 1. Project Purpose

chemrust-vis is a **terminal-first 3D chemical structure visualizer** built on
[chemrust-geometry](../chemrust/chemrust-geometry/). It serves two audiences:

- **Computational chemists** — view and inspect crystal structures, slabs, and
  molecules directly in a terminal (SSH/tmux), without needing a GUI like VESTA
  or Materials Studio.
- **AI agents** (Claude Code via MCP) — the visualizer exposes atoms, bonds, and
  selections as MCP resources/tools so an AI can inspect and modify structures
  programmatically.

Key features (planned):
1. Keyboard + mouse navigation and atom selection
2. MCP server mode for AI agent interaction (inspect/modify structures)
3. Export to supported structure formats
4. TUI + tmux compatibility for remote SSH use

---

## 2. Domain Language

### 2.1 Terms inherited from chemrust-geometry

These terms are defined in `chemrust-geometry` and used verbatim in chemrust-vis.
See `chemrust-geometry/src/lib.rs:7-34` for the authoritative module structure.

| Term | Definition | Aliases to avoid |
|------|-----------|-----------------|
| **Structure** | Struct-of-arrays for any chemical system (molecule, crystal, slab). Contains `species: Vec<ElementSymbol>`, `frac_coords: Vec<FracCoord>`, `cell: Option<LatticeVectors>`, `pbc: [bool; 3]`, `tags: Vec<i32>`, `labels: Vec<Option<String>>`, `space_group: Option<SpaceGroupHallSymbol>`. One struct, no per-system traits. | "system", "configuration", "model", "molecule" |
| **LatticeVectors** | 3×3 matrix; columns = a, b, c vectors in Cartesian coordinates (Å). Wraps `nalgebra::Matrix3<f64>`. | "cell", "box", "unit cell" (ambiguous — CellConstants is the parameterization) |
| **CellConstants** | (a, b, c, α, β, γ) parameterization of a periodic cell. Lengths in Å, angles in radians. | "cell parameters", "lattice parameters" |
| **FracCoord** | Newtype over `nalgebra::Point3<f64>`. Type-level distinction from Cartesian coordinates. All atomic positions in `Structure` are fractional. | "fractional position", "crystal coordinate" |
| **Transform** | Trait for lazy geometric operations operating on fractional coordinates. Returns `TransformMatrix { linear: Matrix3<f64>, translation: Vector3<f64> }`. Implementors: `SurfaceRotation`, `Supercell`. | "operation", "modification" |
| **ElementSymbol** | Re-exported from `castep-periodic-table`. Enum of chemical elements. | "element", "species" (ambiguous — species is the Vec, ElementSymbol is the enum) |
| **SpaceGroupHallSymbol** | Re-exported from `crystallographic-group`. Hall symbol representation of space group symmetry. | "space group", "symmetry group" |

### 2.2 Terms defined by chemrust-vis

| Term | Definition | Aliases to avoid |
|------|-----------|-----------------|
| **Viewport** | The visible area of the terminal showing the 3D scene. Has a camera position, projection matrix, and zoom level. Rendered via ratatui canvas widget using Braille dots for atoms and line segments for bonds/cell boundaries. | "window", "screen", "display" |
| **Scene** | The collection of renderable objects constructed from a `Structure`: atoms (as 3D points), bonds (as line segments between nearby atom pairs), cell axes (as box outline when cell exists). The scene is what the Viewport renders. | "model", "world" |
| **Selection** | A set of atom indices (`Vec<usize>`) currently highlighted/active in the viewport. Supports single-click, multi-select (shift-click), and group operations (transform, duplicate, delete, change element, change display properties). | "picked atoms", "active set", "highlight" |
| **Camera** | The viewpoint from which the 3D scene is projected to 2D terminal space. Has position (spherical coordinates around the structure center), target (look-at point), and field-of-view. Manipulated via keyboard (wasd/qe/arrows) and mouse (drag to rotate, scroll to zoom). | "view", "perspective" |
| **MCP Tool** | A mutating operation exposed to AI agents via the Model Context Protocol. Examples: `remove_atoms(indices)`, `set_element(index, element)`, `transform_selection(matrix)`, `duplicate_selection()`, `group_selection(label)`. Tools modify the scene/Structure. | "action", "command", "operation" |
| **MCP Resource** | A read-only view of state exposed to AI agents. Examples: `scene/atoms` (list all atoms with positions), `scene/selection` (current selection), `scene/cell` (lattice parameters). Resources are polled by the AI to understand current state. | "endpoint", "query" |
| **Projection** | The mathematical mapping from 3D world coordinates to 2D terminal coordinates. Orthographic by default (toggleable to perspective). Computed using nalgebra projection matrices. | "render", "draw" |

### 2.3 Ambiguities resolved

- **"Cell"** — Always means the periodic boundary box (`LatticeVectors`). Never use "cell" as short for "unit cell parameters" (that's `CellConstants`). Never use to mean "grid cell" or "table cell."
- **"Atom"** — An entry in the `Structure` struct-of-arrays at a given index. Has a species, coordinate, tag, and optional label. Not a standalone struct — there is no `Atom` type.
- **"Transform" vs "apply"** — `transform(T)` queues a matrix lazily. `apply()` forces composition and modifies coordinates. The distinction matters: operations like `replicate_along_c` and `with_atoms` need real coordinates, so they force-apply internally.

---

## 3. Architecture

### 3.1 Crate structure

chemrust-vis is a **Cargo workspace** with two crates:

```
chemrust-vis/
  Cargo.toml              # workspace root
  CONTEXT.md              # this file
  docs/adr/               # architecture decision records
  crates/
    chemrust-vis-core/    # library crate
      Cargo.toml
      src/
        lib.rs            # public API, re-exports
        scene.rs          # Scene, AtomRenderData, BondRenderData
        viewport.rs       # Viewport, Camera, projection math
        selection.rs      # Selection, group operations
        mcp/              # MCP server module
          mod.rs
          tools.rs        # MCP tool implementations
          resources.rs    # MCP resource implementations
        export.rs         # Format export (CASTEP cell, XYZ, etc.)
    chemrust-vis-tui/     # binary crate
      Cargo.toml
      src/
        main.rs           # entry point, CLI args (--mcp vs --tui)
        tui/              # TUI application
          mod.rs
          app.rs          # ratatui App trait impl
          input.rs        # keyboard + mouse event handling
          widgets/        # custom ratatui widgets
            scene_view.rs
            status_bar.rs
            atom_inspector.rs
```

**Boundary rules:**
- `chemrust-vis-core` depends on `chemrust-geometry` (path dep), `nalgebra`, `ratatui` (for canvas rendering types), `mcp-server`, `serde`/`serde_json`. It has **zero** dependency on `crossterm` (that's the TUI binary's concern).
- `chemrust-vis-tui` depends on `chemrust-vis-core`, `ratatui`, `crossterm`, `anyhow`, `clap`.
- The lib crate exposes all domain types and rendering logic. The binary crate handles terminal I/O, event loop, and CLI argument parsing.

### 3.2 Dependency on chemrust-geometry

Path dependency (co-development):

```toml
[dependencies]
chemrust-geometry = { path = "../../chemrust/chemrust-geometry" }
```

Both repos evolve together. When chemrust-geometry is published to crates.io,
switch to a versioned dependency.

### 3.3 Data flow

```
┌──────────────┐     ┌──────────────────┐     ┌─────────────────┐
│ chemrust-     │────▶│ chemrust-vis-core │────▶│ chemrust-vis-tui│
│ geometry     │     │                  │     │                 │
│ Structure    │     │ Scene            │     │ ratatui App     │
│ LatticeVec.. │     │ Viewport/Camera  │     │ crossterm input │
│ Transform    │     │ Selection        │     │ CLI (clap)      │
│ FracCoord    │     │ MCP server       │     │                 │
│ ElementSym.. │     │ Export           │     │                 │
└──────────────┘     └──────────────────┘     └─────────────────┘
                             │
                             ▼
                      ┌──────────────┐
                      │ Claude Code  │
                      │ (MCP client) │
                      └──────────────┘
```

### 3.4 ADR index

- [ADR-0001](./docs/adr/0001-braille-canvas-rendering.md) — Braille-dot canvas widget for 3D rendering in terminal

---

## 4. Dependencies and Tooling

### 4.1 Core dependencies (lib crate)

| Crate | Version | Purpose | Why this choice |
|-------|---------|---------|----------------|
| `chemrust-geometry` | path | Domain types (Structure, LatticeVectors, etc.) | Single source of truth for structure data |
| `nalgebra` | 0.33 | 3D linear algebra: camera transforms, projections, ray-casting for selection | Already used by chemrust-geometry; consistent math library |
| `ratatui` | latest | Terminal UI framework; canvas widget for Braille-dot rendering | Most popular Rust TUI lib; built-in canvas with Braille support |
| `mcp-server` | latest | Model Context Protocol server implementation | Avoids hand-rolling JSON-RPC transport |
| `serde` + `serde_json` | latest | Serialization for MCP messages and export formats | Standard Rust serialization |

### 4.2 Binary dependencies (bin crate)

| Crate | Version | Purpose | Why this choice |
|-------|---------|---------|----------------|
| `chemrust-vis-core` | path | All domain logic | Own lib crate |
| `ratatui` | latest | TUI rendering (shared with lib for types) | Same as lib |
| `crossterm` | latest | Terminal input (keyboard + mouse events), raw mode, cursor control | ratatui's default and recommended backend |
| `anyhow` | 1 | Error handling in binary | Standard for application-level error propagation |
| `clap` | latest | CLI argument parsing (`--mcp`, `--tui`, file paths) | Standard Rust CLI library |

### 4.3 Tooling

| Tool | Purpose |
|------|---------|
| `cargo` / `rustc` | Build system, stable channel via fenix (Nix) |
| `clippy` | Linting, strict profile |
| `rustfmt` | Formatting |
| `rust-analyzer` | IDE support |
| Nix + fenix + devshell | Reproducible development environment (see `flake.nix`) |

### 4.4 Explicit non-choices

- **NOT `nalgebra` + `glam`** — Rejected dual-math-library approach. Converting between nalgebra and glam types at the boundary adds friction without benefit. nalgebra handles all 3D math needed (projections, ray casting, camera transforms).
- **NOT `sixel`/`kitty` graphics protocol** — Rejected for portability. Would not work in plain SSH/tmux without protocol forwarding. Chemrust-vis must work in a basic terminal.
- **NOT `termion`** — crossterm chosen for better maintenance, Windows support (future), and tighter ratatui integration.

---

## 5. Coding Patterns

### 5.1 Error handling

- **Lib crate (`chemrust-vis-core`)**: Use `thiserror` for typed error enums.
  ```rust
  #[derive(Debug, thiserror::Error)]
  pub enum Error {
      #[error("no structure loaded")]
      NoStructure,
      #[error("invalid atom index: {0}")]
      InvalidAtomIndex(usize),
      #[error("geometry error: {0}")]
      Geometry(#[from] chemrust_geometry::Error),
  }
  ```
- **Bin crate (`chemrust-vis-tui`)**: Use `anyhow::Result` with `.context()` for
  user-facing error messages.

### 5.2 Module visibility

- Flat module structure with `pub use` re-exports from `lib.rs`.
- Internal modules (`scene.rs`, `selection.rs`, etc.) have their types re-exported
  at the crate root for ergonomic use.
- `pub(crate)` for internal implementation details.

### 5.3 API style

- Follow chemrust-geometry patterns: chainable builders, concrete types over traits.
- Consume-and-return (`fn method(mut self, ...) -> Self`) for operations that
  modify state, enabling method chaining.
- No async in the lib crate (sync rendering, sync selection). Async only in
  the MCP server submodule (tokio runtime for JSON-RPC transport).

### 5.4 Testing

- Unit tests inline in source files (`#[cfg(test)] mod tests { ... }`), matching
  chemrust-geometry convention.
- Integration tests in `tests/` directory for cross-module scenarios.
- Property-based testing with `proptest` for math operations (projections, transforms).

### 5.5 Documentation

- Rustdoc on all `pub` items in the lib crate. At minimum: one-line summary,
  example usage for key types.
- Module-level documentation (`//!`) in each `mod.rs` / `lib.rs`.

### 5.6 Async strategy

- **Lib crate**: Synchronous by default. Rendering, selection, and export are
  compute-bound operations that don't benefit from async.
- **MCP server**: Uses `tokio` (single-threaded runtime) for the JSON-RPC
  stdio transport. The server holds a `Mutex<Scene>` for thread-safe access
  from async handlers. This is the only async code in the project.

---

## 6. Pipeline Expectations

### 6.1 Phase ordering

| Phase | Scope | Success criteria |
|-------|-------|-----------------|
| **Phase 1**: Load + Render + Navigate | Structure loading → scene construction → Braille canvas rendering → keyboard camera controls (rotate, zoom, pan) | Load a `.cell` file, see atoms in terminal, rotate with wasd/qe, zoom with +/- |
| **Phase 2**: Mouse + Selection | Mouse event handling (click, drag, scroll) → atom picking via ray-casting → single/multi selection → atom inspector panel | Click an atom to select it, see species/position/tag in sidebar, shift-click to multi-select |
| **Phase 3**: MCP Integration | MCP server mode (`--mcp` flag) → expose tools (remove_atoms, set_element, transform_selection, etc.) → expose resources (scene/atoms, scene/selection) | Launch with `--mcp`, Claude Code connects, AI can inspect and modify structure |
| **Phase 4**: Export | Format writers for CASTEP cell, XYZ, CIF → integrate with chemrust-geometry's data model | Export current scene to file in selected format |

### 6.2 Review cadence

- After each phase: run `/make-judgement` against the phase's TASKS.md
- Each phase produces a forensic record (TASKS.md + review.md)

### 6.3 Ground truth fixtures

- `Cu111_CO.cell` + `Cu111_CO.param` from chemrust-geometry examples (verified CASTEP input)
- Simple molecule: H₂O (no cell, pbc=[F,F,F])
- Bulk crystal: Cu FCC conventional cell (4 atoms)
- Slab: Cu(111) 4-layer (16 Cu atoms)

---

## 7. Non-goals (explicit scope boundaries)

- **NOT a GUI application** — No egui, iced, or native windowing. Terminal-only.
- **NOT a structure builder** — chemrust-vis visualizes and modifies existing structures. It does not build new crystal structures from symmetry operations (that's chemrust-geometry's job).
- **NOT a CASTEP job runner** — chemrust-vis does not submit or monitor DFT calculations. It exports input files that the user runs separately.
- **NOT a replacement for VESTA/Materials Studio** — Narrower scope: terminal-first, AI-integrated, focused on the chemrust ecosystem.

---

*This constitution was established via `/init-project` on 2026-06-26. All
downstream pipeline stages reference this document. Updates require a
`/grill-with-docs` session.*
