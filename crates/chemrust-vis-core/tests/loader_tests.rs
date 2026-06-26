#![cfg(feature = "castep-loader")]

use chemrust_vis_core::loader::CellLoader;
use chemrust_geometry::ElementSymbol;
use std::path::Path;

/// Path to the Cu111_CO.cell fixture, resolved relative to CARGO_MANIFEST_DIR.
fn fixture_path() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../chemrust/chemrust-geometry/Cu111_CO.cell")
}

#[test]
fn load_cu111_co_cell_has_18_atoms() {
    let structure = CellLoader::load(&fixture_path()).unwrap();
    assert_eq!(structure.num_atoms(), 18);
}

#[test]
fn load_cu111_co_cell_species_counts() {
    let structure = CellLoader::load(&fixture_path()).unwrap();
    let n_cu = structure.species.iter().filter(|s| **s == ElementSymbol::Cu).count();
    let n_c = structure.species.iter().filter(|s| **s == ElementSymbol::C).count();
    let n_o = structure.species.iter().filter(|s| **s == ElementSymbol::O).count();
    assert_eq!(n_cu, 16);
    assert_eq!(n_c, 1);
    assert_eq!(n_o, 1);
}

#[test]
fn load_cu111_co_cell_lattice_vectors() {
    let structure = CellLoader::load(&fixture_path()).unwrap();
    let cell = structure.cell.expect("should have cell");
    let (a, b, c) = cell.lengths();
    assert!((a - 10.2248).abs() < 0.01, "a={}", a);
    assert!((b - 17.7098).abs() < 0.01, "b={}", b);
    assert!((c - 18.2614).abs() < 0.01, "c={}", c);
}

#[test]
fn load_cu111_co_cell_origin_atom() {
    let structure = CellLoader::load(&fixture_path()).unwrap();
    let first = structure.frac_coords[0];
    assert!((first.0.x).abs() < 1e-10);
    assert!((first.0.y).abs() < 1e-10);
    assert!((first.0.z).abs() < 1e-10);
}

#[test]
fn load_nonexistent_file_returns_err() {
    let result = CellLoader::load(Path::new("/nonexistent/path/file.cell"));
    assert!(result.is_err());
}
