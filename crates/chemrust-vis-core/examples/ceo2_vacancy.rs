//! Build a CeO2 2×2 supercell with one O vacancy, export as .cell file.

use castep_periodic_table::data::ELEMENT_TABLE;
use castep_periodic_table::element::LookupElement;
use chemrust_geometry::{
    ElementSymbol,
    FracCoord,
    LatticeVectors,
    Structure,
};
use nalgebra::Matrix3;
use std::collections::BTreeSet;
use std::fs;

fn main() {
    // CeO2 fluorite: a = 5.41 Å (experimental)
    let a = 5.41;
    let cell = LatticeVectors::new(Matrix3::new(
        a, 0.0, 0.0,
        0.0, a, 0.0,
        0.0, 0.0, a,
    ));

    // Ce at FCC sites: (0,0,0), (0,1/2,1/2), (1/2,0,1/2), (1/2,1/2,0)
    let ce_fracs = [
        [0.0, 0.0, 0.0],
        [0.0, 0.5, 0.5],
        [0.5, 0.0, 0.5],
        [0.5, 0.5, 0.0],
    ];

    // O at 8 tetrahedral sites in the fluorite structure
    let o_sites = [
        [0.25, 0.25, 0.25],
        [0.25, 0.25, 0.75],
        [0.25, 0.75, 0.25],
        [0.25, 0.75, 0.75],
        [0.75, 0.25, 0.25],
        [0.75, 0.25, 0.75],
        [0.75, 0.75, 0.25],
        [0.75, 0.75, 0.75],
    ];

    let mut species: Vec<ElementSymbol> = Vec::new();
    let mut coords: Vec<FracCoord> = Vec::new();

    // Ce atoms (4)
    for f in &ce_fracs {
        species.push(ElementSymbol::Ce);
        coords.push(FracCoord::new(f[0], f[1], f[2]));
    }
    // O atoms (8)
    for f in &o_sites {
        species.push(ElementSymbol::O);
        coords.push(FracCoord::new(f[0], f[1], f[2]));
    }

    let n = species.len();
    let mut structure = Structure::new(
        species,
        coords,
        Some(cell),
        [true, true, true],
        vec![0; n],
        vec![None; n],
        None,
    );

    // Build 2×2×2 supercell: replicate atoms at offsets (i,j,k) for i,j,k ∈ {0,1},
    // then scale fractional coords down by 1/2 and scale the cell up by 2.
    let sc = 2;
    let mut sc_species = Vec::new();
    let mut sc_coords = Vec::new();
    let orig_n = structure.num_atoms();
    for i in 0..sc {
        for j in 0..sc {
            for k in 0..sc {
                for a in 0..orig_n {
                    sc_species.push(structure.species[a]);
                    sc_coords.push(FracCoord::new(
                        (structure.frac_coords[a].0.x + i as f64) / sc as f64,
                        (structure.frac_coords[a].0.y + j as f64) / sc as f64,
                        (structure.frac_coords[a].0.z + k as f64) / sc as f64,
                    ));
                }
            }
        }
    }
    let sc_cell = LatticeVectors::new(cell.tensor() * sc as f64);
    let n_sc = sc_species.len();
    structure = Structure::new(
        sc_species, sc_coords, Some(sc_cell),
        [true, true, true], vec![0; n_sc], vec![None; n_sc], None,
    );

    // Create an O vacancy: remove one O atom
    let o_idx = structure.species.iter().position(|s| *s == ElementSymbol::O).unwrap();
    println!("Before vacancy: {} atoms ({} Ce, {} O)",
        structure.num_atoms(),
        structure.species.iter().filter(|s| **s == ElementSymbol::Ce).count(),
        structure.species.iter().filter(|s| **s == ElementSymbol::O).count(),
    );

    structure = remove_atom(structure, o_idx);

    println!("After vacancy:  {} atoms ({} Ce, {} O)",
        structure.num_atoms(),
        structure.species.iter().filter(|s| **s == ElementSymbol::Ce).count(),
        structure.species.iter().filter(|s| **s == ElementSymbol::O).count(),
    );

    // Export as .cell file
    let cell_text = structure_to_cell(&structure);
    let path = "CeO2_vacancy.cell";
    fs::write(path, &cell_text).unwrap();
    println!("Written to {}", path);
}

fn remove_atom(mut s: Structure, idx: usize) -> Structure {
    s.species.remove(idx);
    s.frac_coords.remove(idx);
    s.tags.remove(idx);
    s.labels.remove(idx);
    s
}

fn structure_to_cell(s: &Structure) -> String {
    let cell = s.cell.as_ref().unwrap();
    let t = cell.tensor();
    let mut out = String::new();

    // Lattice
    out.push_str("%BLOCK LATTICE_CART\n");
    for i in 0..3 {
        out.push_str(&format!("  {:20.14} {:20.14} {:20.14}\n", t[(i,0)], t[(i,1)], t[(i,2)]));
    }
    out.push_str("%ENDBLOCK LATTICE_CART\n\n");

    // Positions
    out.push_str("%BLOCK POSITIONS_FRAC\n");
    for (i, fc) in s.frac_coords.iter().enumerate() {
        out.push_str(&format!("  {:2}  {:18.14} {:18.14} {:18.14}\n",
            format!("{:?}", s.species[i]),
            fc.0.x, fc.0.y, fc.0.z));
    }
    out.push_str("%ENDBLOCK POSITIONS_FRAC\n\n");

    // Collect unique species
    let unique: BTreeSet<ElementSymbol> = s.species.iter().copied().collect();

    // SPECIES_MASS block (atomic masses from periodic table)
    out.push_str("%BLOCK SPECIES_MASS\n");
    for sp in &unique {
        let el = ELEMENT_TABLE.get_by_symbol(*sp);
        out.push_str(&format!("  {:2}  {:.10}\n",
            format!("{:?}", sp), el.mass()));
    }
    out.push_str("%ENDBLOCK SPECIES_MASS\n\n");

    // SPECIES_POT block (pseudopotential recommendations)
    out.push_str("%BLOCK SPECIES_POT\n");
    for sp in &unique {
        let el = ELEMENT_TABLE.get_by_symbol(*sp);
        out.push_str(&format!("  {:2}  {}\n",
            format!("{:?}", sp), el.potential()));
    }
    out.push_str("%ENDBLOCK SPECIES_POT\n\n");

    // SPECIES_LCAO_STATES block (valence states count)
    out.push_str("%BLOCK SPECIES_LCAO_STATES\n");
    for sp in &unique {
        let el = ELEMENT_TABLE.get_by_symbol(*sp);
        out.push_str(&format!("  {:2}  {}\n",
            format!("{:?}", sp), el.lcao()));
    }
    out.push_str("%ENDBLOCK SPECIES_LCAO_STATES\n");

    out
}
