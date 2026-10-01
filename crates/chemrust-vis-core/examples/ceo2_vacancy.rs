//! Build a CeO2 2×2 supercell with one O vacancy, export as .cell file
//! using castep-cell-io structs.

use castep_cell_fmt::{ToCellFile, format::to_string_many_spaced};
use castep_cell_io::{CellDocument, Lattice, Positions};
use castep_cell_io::cell::{
    kpoints_params::KpointsParams,
    bz_sampling_kpoints::KpointsMpSpacing,
    lattice_param::LatticeCart,
    positions::{PositionFracEntry, PositionsFrac},
    species::{Species, SpeciesLcaoState, SpeciesLcaoStates,
              SpeciesMass, SpeciesMassEntry,
              SpeciesPot, SpeciesPotEntry},
    species_params::SpeciesParams,
};
use castep_periodic_table::data::ELEMENT_TABLE;
use castep_periodic_table::element::LookupElement;
use chemrust_geometry::{ElementSymbol, FracCoord, LatticeVectors, Structure};
use nalgebra::Matrix3;
use std::collections::BTreeSet;
use std::fs;

fn main() {
    let a = 5.41;
    let cell = LatticeVectors::new(Matrix3::new(a, 0.0, 0.0, 0.0, a, 0.0, 0.0, 0.0, a));

    let ce_fracs = [[0.0, 0.0, 0.0], [0.0, 0.5, 0.5], [0.5, 0.0, 0.5], [0.5, 0.5, 0.0]];
    let o_sites = [
        [0.25, 0.25, 0.25], [0.25, 0.25, 0.75], [0.25, 0.75, 0.25], [0.25, 0.75, 0.75],
        [0.75, 0.25, 0.25], [0.75, 0.25, 0.75], [0.75, 0.75, 0.25], [0.75, 0.75, 0.75],
    ];

    let mut species_vec = Vec::new();
    let mut coords_vec = Vec::new();
    for f in &ce_fracs { species_vec.push(ElementSymbol::Ce); coords_vec.push(FracCoord::new(f[0], f[1], f[2])); }
    for f in &o_sites  { species_vec.push(ElementSymbol::O);  coords_vec.push(FracCoord::new(f[0], f[1], f[2])); }

    let n = species_vec.len();
    let structure = Structure::new(species_vec, coords_vec, Some(cell), [true;3], vec![0;n], vec![None;n], None);

    // 2×2×2 supercell
    let sc = 2;
    let (sc_sp, sc_fc) = build_supercell(&structure, sc);
    let sc_cell = LatticeVectors::new(cell.tensor() * sc as f64);
    let n_sc = sc_sp.len();
    let mut sc_struct = Structure::new(sc_sp, sc_fc, Some(sc_cell), [true;3], vec![0;n_sc], vec![None;n_sc], None);

    // Remove first O → vacancy
    let o_idx = sc_struct.species.iter().position(|s| *s == ElementSymbol::O).unwrap();
    let vac_frac = sc_struct.frac_coords[o_idx];
    sc_struct = remove_atom(sc_struct, o_idx);
    println!("Atoms: {} ({} Ce, {} O)", sc_struct.num_atoms(),
        sc_struct.species.iter().filter(|s| **s == ElementSymbol::Ce).count(),
        sc_struct.species.iter().filter(|s| **s == ElementSymbol::O).count());

    // Find 4 nearest Ce → spin
    let vac_cart = sc_cell.tensor() * vac_frac.0;
    let spin_ce = nearest_ce(&sc_struct, [vac_cart.x, vac_cart.y, vac_cart.z], 4, sc_cell.tensor());
    println!("Spin Ce: {:?}", spin_ce);

    // Build CellDocument
    let t = sc_cell.tensor();
    let lattice = Lattice::Cart(LatticeCart {
        unit: None,
        a: [t[(0,0)], t[(1,0)], t[(2,0)]],
        b: [t[(0,1)], t[(1,1)], t[(2,1)]],
        c: [t[(0,2)], t[(1,2)], t[(2,2)]],
    });

    let positions = Positions::Frac(PositionsFrac {
        positions: sc_struct.frac_coords.iter().enumerate().map(|(i, fc)| {
            let sp_str = format!("{:?}", sc_struct.species[i]);
            let spin = if spin_ce.contains(&i) { Some(1.0) } else { None };
            PositionFracEntry {
                species: Species::Symbol(sp_str),
                coord: [fc.0.x, fc.0.y, fc.0.z],
                spin,
                mixture: None,
            }
        }).collect(),
    });

    // Unique species from periodic table
    let unique: BTreeSet<ElementSymbol> = sc_struct.species.iter().copied().collect();
    let mass_entries: Vec<_> = unique.iter().map(|sp| {
        let el = ELEMENT_TABLE.get_by_symbol(*sp);
        SpeciesMassEntry { species: Species::Symbol(format!("{:?}", sp)), mass: el.mass() }
    }).collect();
    let pot_entries: Vec<_> = unique.iter().map(|sp| {
        let el = ELEMENT_TABLE.get_by_symbol(*sp);
        SpeciesPotEntry { species: Species::Symbol(format!("{:?}", sp)), filename: el.potential().to_string() }
    }).collect();
    let lcao_states: Vec<_> = unique.iter().map(|sp| {
        let el = ELEMENT_TABLE.get_by_symbol(*sp);
        SpeciesLcaoState { species: Species::Symbol(format!("{:?}", sp)), num_states: el.lcao() as u32 }
    }).collect();

    let doc = CellDocument::builder()
        .lattice(lattice)
        .positions(positions)
        .kpoints(KpointsParams {
            kpoints_mp_spacing: Some(KpointsMpSpacing { value: 0.07, unit: None }),
            ..Default::default()
        })
        .species(SpeciesParams {
            species_mass: Some(SpeciesMass { unit: None, masses: mass_entries }),
            species_pot: Some(SpeciesPot { potentials: pot_entries }),
            species_lcao_states: Some(SpeciesLcaoStates { states: lcao_states }),
            ..Default::default()
        })
        .build()
        .expect("CellDocument build failed");

    let output = to_string_many_spaced(&doc.to_cell_file());
    fs::write("CeO2_vacancy.cell", &output).unwrap();
    println!("Written to CeO2_vacancy.cell");
}

fn build_supercell(s: &Structure, sc: usize) -> (Vec<ElementSymbol>, Vec<FracCoord>) {
    let mut sp = Vec::new(); let mut fc = Vec::new();
    let n = s.num_atoms();
    for i in 0..sc { for j in 0..sc { for k in 0..sc {
        for a in 0..n {
            sp.push(s.species[a]);
            fc.push(FracCoord::new(
                (s.frac_coords[a].0.x + i as f64) / sc as f64,
                (s.frac_coords[a].0.y + j as f64) / sc as f64,
                (s.frac_coords[a].0.z + k as f64) / sc as f64,
            ));
        }
    }}}
    (sp, fc)
}

fn remove_atom(mut s: Structure, idx: usize) -> Structure {
    s.species.remove(idx); s.frac_coords.remove(idx); s.tags.remove(idx); s.labels.remove(idx); s
}

fn nearest_ce(s: &Structure, pos: [f64;3], n: usize, cell: &Matrix3<f64>) -> Vec<usize> {
    let mut dists: Vec<(usize, f64)> = s.species.iter().enumerate()
        .filter(|(_, sp)| **sp == ElementSymbol::Ce)
        .map(|(i, _)| {
            let cart = cell * s.frac_coords[i].0;
            let dx = cart.x - pos[0]; let dy = cart.y - pos[1]; let dz = cart.z - pos[2];
            (i, (dx*dx + dy*dy + dz*dz).sqrt())
        }).collect();
    dists.sort_by(|a,b| a.1.partial_cmp(&b.1).unwrap());
    dists.truncate(n);
    dists.into_iter().map(|(i,_)| i).collect()
}
