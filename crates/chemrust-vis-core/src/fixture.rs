//! Test fixtures built on top of `chemrust-geometry` slab builders.

use chemrust_geometry::slab::cu111_4layer;
use chemrust_geometry::{ElementSymbol, FracCoord, Structure};

/// Build a Cu(111)+CO ground-truth fixture: a 4-layer Cu(111) slab with a
/// CO molecule adsorbed at the atop site above the cell center.
///
/// Pipeline: `cu111_4layer(a)` (64 Cu atoms, 2x2 surface cell, 12 A
/// vacuum) plus one C and one O atom above the top-layer Cu closest to
/// the cell center. Total: 64 Cu + 1 C + 1 O = 66 atoms.
pub fn cu111_co_system(a: f64) -> Structure {
    let mut sys = cu111_4layer(a).apply();

    // Find the top-layer Cu nearest the cell center (x,y = 0.5, 0.5)
    let top_cu = sys
        .species
        .iter()
        .zip(sys.frac_coords.iter())
        .zip(sys.tags.iter())
        .enumerate()
        .filter(|(_, ((sp, _), &tag))| **sp == ElementSymbol::Cu && tag == 3)
        .min_by(|(_, ((_, x), _)), (_, ((_, y), _))| {
            let dx = (x[0] - 0.5).powi(2) + (x[1] - 0.5).powi(2);
            let dy = (y[0] - 0.5).powi(2) + (y[1] - 0.5).powi(2);
            dx.total_cmp(&dy)
        })
        .map(|(i, _)| i)
        .expect("No top-layer Cu found");

    // CO vertical atop: convert bond lengths to fractional using c-length
    let c_len = sys.require_cell().expect("slab has no cell").lengths().2;
    let z_cu = sys.frac_coords[top_cu][2];
    let z_c = z_cu + 1.9 / c_len;
    let z_o = z_cu + (1.9 + 1.15) / c_len;

    sys = sys.with_atoms(
        vec![ElementSymbol::C, ElementSymbol::O],
        vec![FracCoord::new(0.5, 0.5, z_c), FracCoord::new(0.5, 0.5, z_o)],
        vec![-1, -1],
        vec![Some("C_atop".into()), Some("O_atop".into())],
    );

    sys.pbc = [true, true, false];
    sys.wrap_frac_coords()
}
