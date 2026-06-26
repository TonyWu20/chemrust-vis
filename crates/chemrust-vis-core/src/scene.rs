//! Scene construction from chemrust-geometry Structure.
//! Produces renderable atom data and cell edge geometry.

use chemrust_geometry::ElementSymbol;
use chemrust_geometry::Structure;

/// Renderer-agnostic RGB color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RgbColor(pub u8, pub u8, pub u8);

/// Per-atom data ready for rendering.
#[derive(Debug, Clone)]
pub struct AtomDrawData {
    /// Cartesian position in Angstrom.
    pub position: [f64; 3],
    /// Element color.
    pub color: RgbColor,
    /// Chemical element.
    pub element: ElementSymbol,
    /// Index into the original Structure (wraps for periodic images).
    pub atom_index: usize,
    /// Whether this is a periodic replica (rendered dimmer).
    pub is_periodic_image: bool,
}

/// The renderable scene: atoms and cell edges.
#[derive(Debug, Clone)]
pub struct Scene {
    /// All atoms with draw data.
    pub atoms: Vec<AtomDrawData>,
    /// Cell box edges: each edge is (start_pos, end_pos) in Cartesian.
    pub cell_edges: Vec<([f64; 3], [f64; 3])>,
}

impl Scene {
    /// Construct a Scene from a chemrust-geometry Structure.
    ///
    /// For periodic structures (with cell), generates periodic replicas of atoms
    /// near cell boundaries so the structure appears continuous across the cell.
    /// For molecules (no cell), fractional coords are treated as Cartesian.
    pub fn from_structure(structure: &Structure) -> Self {
        let cell_edges = if let Some(cell) = &structure.cell {
            build_cell_edges(cell)
        } else {
            Vec::new()
        };

        let atoms = if let Some(cell) = &structure.cell {
            let tensor = cell.tensor();
            let boundary_margin: f64 = 0.15;

            let mut atoms = Vec::new();

            for (i, (&element, &fc)) in structure
                .species
                .iter()
                .zip(structure.frac_coords.iter())
                .enumerate()
            {
                let color = element_color(element);
                let fx = fc.0.x;
                let fy = fc.0.y;
                let fz = fc.0.z;

                // Always include the original atom
                let p = tensor * fc.0;
                atoms.push(AtomDrawData {
                    position: [p.x, p.y, p.z],
                    color,
                    element,
                    atom_index: i,
                    is_periodic_image: false,
                });

                // Generate boundary replicas: only for atoms near a cell face.
                // Include 0 in each shift list so we get all face/edge/corner
                // combinations, then filter out the (0,0,0) original.
                let mut x_shifts = vec![0.0];
                if fx < boundary_margin { x_shifts.push(1.0); }
                if fx > 1.0 - boundary_margin { x_shifts.push(-1.0); }

                let mut y_shifts = vec![0.0];
                if fy < boundary_margin { y_shifts.push(1.0); }
                if fy > 1.0 - boundary_margin { y_shifts.push(-1.0); }

                let mut z_shifts = vec![0.0];
                if fz < boundary_margin { z_shifts.push(1.0); }
                if fz > 1.0 - boundary_margin { z_shifts.push(-1.0); }

                for &dx in &x_shifts {
                    for &dy in &y_shifts {
                        for &dz in &z_shifts {
                            if dx == 0.0 && dy == 0.0 && dz == 0.0 {
                                continue; // original atom already added above
                            }
                            let sfx = fx + dx;
                            let sfy = fy + dy;
                            let sfz = fz + dz;
                            let fc_shifted = nalgebra::Point3::new(sfx, sfy, sfz);
                            let p = tensor * fc_shifted;
                            atoms.push(AtomDrawData {
                                position: [p.x, p.y, p.z],
                                color,
                                element,
                                atom_index: i,
                                is_periodic_image: true,
                            });
                        }
                    }
                }
            }
            atoms
        } else {
            // Molecule: treat fractional coords as Cartesian
            structure
                .species
                .iter()
                .enumerate()
                .zip(structure.frac_coords.iter())
                .map(|((i, &element), fc)| {
                    let position = [fc.0.x, fc.0.y, fc.0.z];
                    AtomDrawData {
                        position,
                        color: element_color(element),
                        element,
                        atom_index: i,
                        is_periodic_image: false,
                    }
                })
                .collect()
        };

        Scene { atoms, cell_edges }
    }

    /// Compute the bounding box center (centroid of all atom positions).
    pub fn bounding_box_center(&self) -> [f64; 3] {
        if self.atoms.is_empty() {
            return [0.0, 0.0, 0.0];
        }
        let n = self.atoms.len() as f64;
        let mut sum = [0.0; 3];
        for atom in &self.atoms {
            sum[0] += atom.position[0];
            sum[1] += atom.position[1];
            sum[2] += atom.position[2];
        }
        [sum[0] / n, sum[1] / n, sum[2] / n]
    }

    /// Return the best center for camera framing.
    ///
    /// Uses the axis-aligned bounding box midpoint of original atoms.
    /// This gives a geometrically centered view regardless of atom clustering.
    pub fn center_for_view(&self) -> [f64; 3] {
        let originals: Vec<&AtomDrawData> = self
            .atoms
            .iter()
            .filter(|a| !a.is_periodic_image)
            .collect();
        if originals.is_empty() {
            return self.bounding_box_center();
        }
        let mut min = [f64::INFINITY; 3];
        let mut max = [f64::NEG_INFINITY; 3];
        for atom in &originals {
            for i in 0..3 {
                min[i] = min[i].min(atom.position[i]);
                max[i] = max[i].max(atom.position[i]);
            }
        }
        [
            (min[0] + max[0]) / 2.0,
            (min[1] + max[1]) / 2.0,
            (min[2] + max[2]) / 2.0,
        ]
    }
}

/// Build the 12 edges of the unit cell parallelepiped from lattice vectors.
///
/// The 8 corners are: origin, a, b, c, a+b, a+c, b+c, a+b+c.
/// The 12 edges connect adjacent corners (those differing by one basis vector).
fn build_cell_edges(cell: &chemrust_geometry::LatticeVectors) -> Vec<([f64; 3], [f64; 3])> {
    let t = cell.tensor();
    // Columns are a, b, c vectors
    let a = [t[(0, 0)], t[(1, 0)], t[(2, 0)]];
    let b = [t[(0, 1)], t[(1, 1)], t[(2, 1)]];
    let c = [t[(0, 2)], t[(1, 2)], t[(2, 2)]];

    let origin = [0.0, 0.0, 0.0];
    let corners: [[f64; 3]; 8] = [
        origin,
        a,
        b,
        c,
        [a[0] + b[0], a[1] + b[1], a[2] + b[2]],
        [a[0] + c[0], a[1] + c[1], a[2] + c[2]],
        [b[0] + c[0], b[1] + c[1], b[2] + c[2]],
        [a[0] + b[0] + c[0], a[1] + b[1] + c[1], a[2] + b[2] + c[2]],
    ];

    // Edge pairs: indices of corners differing by one basis vector
    let edge_pairs: [(usize, usize); 12] = [
        (0, 1), // origin → a
        (0, 2), // origin → b
        (0, 3), // origin → c
        (1, 4), // a → a+b
        (1, 5), // a → a+c
        (2, 4), // b → a+b
        (2, 6), // b → b+c
        (3, 5), // c → a+c
        (3, 6), // c → b+c
        (4, 7), // a+b → a+b+c
        (5, 7), // a+c → a+b+c
        (6, 7), // b+c → a+b+c
    ];

    edge_pairs
        .iter()
        .map(|&(i, j)| (corners[i], corners[j]))
        .collect()
}

/// Map an element symbol to a renderer-agnostic RGB color.
pub fn element_color(element: ElementSymbol) -> RgbColor {
    use chemrust_geometry::ElementSymbol::*;
    match element {
        H => RgbColor(255, 255, 255), // white
        He => RgbColor(217, 255, 255), // pale cyan
        Li => RgbColor(204, 128, 255), // violet
        Be => RgbColor(194, 255, 0),   // lime
        B => RgbColor(255, 181, 181),  // salmon
        C => RgbColor(64, 64, 64),     // dark gray
        N => RgbColor(48, 80, 248),    // blue
        O => RgbColor(255, 13, 13),    // red
        F => RgbColor(144, 224, 80),   // green
        Ne => RgbColor(179, 227, 245), // light blue
        Na => RgbColor(171, 92, 242),  // purple
        Mg => RgbColor(138, 255, 0),   // bright green
        Al => RgbColor(191, 166, 166), // silver-gray
        Si => RgbColor(240, 200, 160), // tan
        P => RgbColor(255, 128, 0),    // orange
        S => RgbColor(255, 255, 48),   // yellow
        Cl => RgbColor(31, 240, 31),   // green
        Ar => RgbColor(128, 209, 227), // pale blue
        K => RgbColor(143, 64, 212),   // purple
        Ca => RgbColor(61, 255, 0),    // bright green
        Sc => RgbColor(230, 230, 230), // light gray
        Ti => RgbColor(191, 194, 199), // gray
        V => RgbColor(166, 166, 171),  // gray
        Cr => RgbColor(138, 153, 199), // steel blue
        Mn => RgbColor(156, 122, 199), // purple-gray
        Fe => RgbColor(224, 102, 51),  // rust
        Co => RgbColor(240, 144, 160), // pink
        Ni => RgbColor(80, 208, 80),   // green
        Cu => RgbColor(184, 115, 51),  // copper/orange
        Zn => RgbColor(125, 128, 176), // blue-gray
        Ga => RgbColor(194, 143, 143), // brown-gray
        Ge => RgbColor(102, 143, 143), // gray
        As => RgbColor(189, 128, 227), // purple
        Se => RgbColor(255, 161, 0),   // orange
        Br => RgbColor(166, 41, 41),   // dark red
        Kr => RgbColor(92, 184, 209),  // blue-gray
        Rb => RgbColor(112, 46, 176),  // violet
        Sr => RgbColor(0, 255, 0),     // green
        Y => RgbColor(148, 255, 255),  // cyan
        Zr => RgbColor(148, 224, 224), // pale cyan
        Nb => RgbColor(115, 194, 201), // steel blue
        Mo => RgbColor(84, 181, 181),  // teal
        Tc => RgbColor(59, 158, 158),  // teal
        Ru => RgbColor(36, 143, 143),  // dark teal
        Rh => RgbColor(10, 125, 140),  // dark cyan
        Pd => RgbColor(0, 105, 133),   // dark blue
        Ag => RgbColor(192, 192, 192), // silver
        Cd => RgbColor(255, 217, 143), // gold-yellow
        In => RgbColor(166, 117, 115), // brown
        Sn => RgbColor(102, 128, 128), // gray
        Sb => RgbColor(158, 99, 181),  // purple
        Te => RgbColor(212, 122, 0),   // orange-brown
        I => RgbColor(148, 0, 148),    // purple
        Xe => RgbColor(66, 158, 176),  // blue-gray
        Cs => RgbColor(87, 23, 143),   // dark violet
        Ba => RgbColor(0, 201, 0),     // green
        La => RgbColor(112, 212, 255), // light blue
        Ce => RgbColor(255, 255, 199), // cream
        Pr => RgbColor(217, 255, 199), // pale green
        Nd => RgbColor(199, 255, 199), // pale green
        Pm => RgbColor(163, 255, 199), // pale green
        Sm => RgbColor(143, 255, 199), // pale green
        Eu => RgbColor(97, 255, 199),  // pale green
        Gd => RgbColor(69, 255, 199),  // pale green
        Tb => RgbColor(48, 255, 199),  // pale green
        Dy => RgbColor(31, 255, 199),  // pale green
        Ho => RgbColor(0, 255, 156),   // green
        Er => RgbColor(0, 230, 117),   // green
        Tm => RgbColor(0, 212, 82),    // green
        Yb => RgbColor(0, 191, 56),    // green
        Lu => RgbColor(0, 171, 36),    // green
        Hf => RgbColor(77, 194, 255),  // light blue
        Ta => RgbColor(77, 166, 255),  // blue
        W => RgbColor(33, 148, 214),   // blue
        Re => RgbColor(38, 125, 171),  // blue
        Os => RgbColor(38, 102, 150),  // dark blue
        Ir => RgbColor(23, 84, 135),   // dark blue
        Pt => RgbColor(208, 208, 224), // light gray
        Au => RgbColor(255, 209, 35),  // gold
        Hg => RgbColor(184, 184, 208), // gray
        Tl => RgbColor(166, 84, 77),   // brown
        Pb => RgbColor(87, 89, 97),    // dark gray
        Bi => RgbColor(158, 79, 181),  // purple
        Po => RgbColor(171, 92, 0),    // brown
        At => RgbColor(117, 79, 69),   // dark brown
        Rn => RgbColor(66, 130, 150),  // dark blue-gray
        Fr => RgbColor(66, 0, 102),    // dark violet
        Ra => RgbColor(0, 125, 0),     // dark green
        Ac => RgbColor(112, 171, 250), // light blue
        Th => RgbColor(0, 186, 255),   // bright blue
        Pa => RgbColor(0, 161, 255),   // blue
        U => RgbColor(0, 143, 255),    // blue
        Np => RgbColor(0, 128, 255),   // blue
        Pu => RgbColor(0, 107, 255),   // blue
        Am => RgbColor(84, 92, 242),   // blue
        _ => RgbColor(128, 128, 128),  // default gray
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chemrust_geometry::slab::cu111_co_system;
    use chemrust_geometry::{ElementSymbol, FracCoord, Structure};

    #[test]
    fn scene_from_cu111_co_has_atoms() {
        let structure = cu111_co_system(3.615);
        let scene = Scene::from_structure(&structure);
        // With periodic expansion, we get original 18 atoms plus boundary replicas
        assert!(scene.atoms.len() >= 18, "got {}", scene.atoms.len());
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
        // Find the non-periodic Cu atom at ~(1.2781, 0.7379, 2.0871)
        let pos = scene.atoms.iter()
            .find(|a| !a.is_periodic_image
                && (a.position[0] - 1.2781).abs() < 0.001
                && (a.position[1] - 0.7379).abs() < 0.001)
            .map(|a| a.position)
            .expect("second Cu atom not found");
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
        assert!(
            (start[0]).abs() < 1e-6 && (start[1]).abs() < 1e-6 && (start[2]).abs() < 1e-6
        );
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
        // All known elements should return a valid RGB triple
        let color = element_color(ElementSymbol::He);
        assert!(color.0 <= 255 && color.1 <= 255 && color.2 <= 255);
    }
}

