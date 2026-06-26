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
    /// Index into the original Structure.
    pub atom_index: usize,
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
    pub fn from_structure(_structure: &Structure) -> Self {
        Scene {
            atoms: Vec::new(),
            cell_edges: Vec::new(),
        }
    }

    /// Compute the bounding box center (centroid of all atom positions).
    pub fn bounding_box_center(&self) -> [f64; 3] {
        [0.0, 0.0, 0.0]
    }
}

/// Map an element symbol to a renderer-agnostic RGB color.
pub fn element_color(element: ElementSymbol) -> RgbColor {
    let _ = element;
    RgbColor(128, 128, 128)
}
