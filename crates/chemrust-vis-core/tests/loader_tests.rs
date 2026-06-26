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

#[test]
fn lattice_vectors_column_major_verification() {
    // Verify that the LatticeVectors matrix stores a,b,c as columns.
    // Column 0 = a, Column 1 = b, Column 2 = c.
    let structure = CellLoader::load(&fixture_path()).unwrap();
    let cell = structure.cell.expect("should have cell");
    let t = cell.tensor();

    // Column-major: t[(row, col)]
    let a = [t[(0, 0)], t[(1, 0)], t[(2, 0)]]; // column 0
    let b = [t[(0, 1)], t[(1, 1)], t[(2, 1)]]; // column 1
    let c = [t[(0, 2)], t[(1, 2)], t[(2, 2)]]; // column 2

    // Axis-aligned cell: a along X, b along Y, c along Z
    assert!((a[0] - 10.2248).abs() < 0.01, "a_x={}", a[0]);
    assert!((a[1]).abs() < 1e-10, "a_y={}", a[1]);
    assert!((a[2]).abs() < 1e-10, "a_z={}", a[2]);

    assert!((b[0]).abs() < 1e-10, "b_x={}", b[0]);
    assert!((b[1] - 17.7098).abs() < 0.01, "b_y={}", b[1]);
    assert!((b[2]).abs() < 1e-10, "b_z={}", b[2]);

    assert!((c[0]).abs() < 1e-10, "c_x={}", c[0]);
    assert!((c[1]).abs() < 1e-10, "c_y={}", c[1]);
    assert!((c[2] - 18.2614).abs() < 0.01, "c_z={}", c[2]);

    // Verify frac→cart gives correct position for second atom
    // frac = (0.125, 0.041667, 0.114292) → cart should be ≈ (1.2781, 0.7379, 2.0871)
    let f = structure.frac_coords[1];
    let cart_x = f.0.x * a[0] + f.0.y * b[0] + f.0.z * c[0];
    let cart_y = f.0.x * a[1] + f.0.y * b[1] + f.0.z * c[1];
    let cart_z = f.0.x * a[2] + f.0.y * b[2] + f.0.z * c[2];
    assert!((cart_x - 1.2781).abs() < 0.001, "cart_x={}", cart_x);
    assert!((cart_y - 0.7379).abs() < 0.001, "cart_y={}", cart_y);
    assert!((cart_z - 2.0871).abs() < 0.001, "cart_z={}", cart_z);
}
