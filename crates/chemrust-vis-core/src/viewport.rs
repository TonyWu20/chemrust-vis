//! Viewport: 3D world → view → NDC → canvas coordinates.
//! Produces DrawCommands for consumption by renderers.

use crate::camera::Camera;
use crate::scene::Scene;

/// A draw command for a single atom (point).
#[derive(Debug, Clone)]
pub struct DrawPoint {
    pub x: f64,
    pub y: f64,
    pub z_view: f64,
    pub atom_index: usize,
}

/// A draw command for a line segment.
#[derive(Debug, Clone)]
pub struct DrawLine {
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
}

/// The complete set of draw commands for a frame.
#[derive(Debug, Clone)]
pub struct DrawCommands {
    pub points: Vec<DrawPoint>,
    pub lines: Vec<DrawLine>,
}

/// The viewport: maps scene geometry to canvas coordinates.
#[derive(Debug, Clone)]
pub struct Viewport {
    width: f64,
    height: f64,
}

impl Viewport {
    /// Create a new viewport with given canvas dimensions.
    pub fn new(width: f64, height: f64) -> Self {
        Viewport { width, height }
    }

    /// Render a scene through a camera into draw commands.
    pub fn render(&self, _scene: &Scene, _camera: &Camera) -> DrawCommands {
        DrawCommands {
            points: Vec::new(),
            lines: Vec::new(),
        }
    }
}
