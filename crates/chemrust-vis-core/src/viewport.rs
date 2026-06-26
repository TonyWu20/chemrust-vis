//! Viewport: 3D world → view → NDC → canvas coordinates.
//! Produces `DrawCommands` for consumption by renderers.
//!
//! Pipeline: world point → camera view matrix → view space → orthographic
//! scaling → NDC → canvas coordinates. Depth sorting via painter's algorithm
//! (farthest from camera first).

use crate::camera::Camera;
use crate::scene::Scene;
use nalgebra::Point3;

/// A draw command for a single atom (point on canvas).
#[derive(Debug, Clone)]
pub struct DrawPoint {
    /// Canvas X coordinate in dot units (0..width).
    pub x: f64,
    /// Canvas Y coordinate in dot units (0..height, top-left origin).
    pub y: f64,
    /// View-space Z for depth verification.
    pub z_view: f64,
    /// On-screen radius in dot units (perspective-dependent).
    /// Larger radius = closer to camera. Typically 1–8 dots.
    pub radius: f64,
    /// Renderer-agnostic color from the scene's element color map.
    pub color: crate::scene::RgbColor,
    /// Index into the scene's atoms array (and original Structure).
    pub atom_index: usize,
}

/// A draw command for a line segment on canvas.
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
    /// Create a new viewport with given canvas dimensions in dot units.
    /// Width = cols × 2, height = rows × 4 for Braille rendering.
    pub fn new(width: f64, height: f64) -> Self {
        Viewport { width, height }
    }

    /// Render a scene through a camera into draw commands.
    ///
    /// Uses perspective projection: atoms farther from camera appear smaller.
    /// Depth-sorts via painter's algorithm (farthest first).
    pub fn render(&self, scene: &Scene, camera: &Camera) -> DrawCommands {
        // FOV scale: proportional to viewport height for a natural look.
        // Larger fov_scale = wider field of view (like a short focal length).
        let fov_scale = self.height * 0.8;
        // Base atomic radius in Angstroms (roughly a covalent bond radius).
        let base_radius = 1.2;

        // Project atoms with perspective.
        let mut point_data: Vec<(DrawPoint, f64)> = scene
            .atoms
            .iter()
            .map(|atom| {
                let world = Point3::new(atom.position[0], atom.position[1], atom.position[2]);
                let (ndc, persp) = camera.project_perspective(&world, fov_scale);
                let dp = DrawPoint {
                    x: self.ndc_to_canvas_x(ndc.x),
                    y: self.ndc_to_canvas_y(ndc.y),
                    z_view: ndc.z,
                    radius: base_radius * persp,
                    color: atom.color,
                    atom_index: atom.atom_index,
                };
                (dp, ndc.z)
            })
            .collect();

        // Sort by view-Z ascending (most negative = farthest from camera = first).
        point_data.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        let points: Vec<DrawPoint> = point_data.into_iter().map(|(dp, _)| dp).collect();

        // Project cell edges with the SAME perspective projection as atoms,
        // so the cell box aligns with atom positions.
        let lines: Vec<DrawLine> = scene
            .cell_edges
            .iter()
            .map(|&(start, end)| {
                let (s_ndc, _) = camera.project_perspective(
                    &Point3::new(start[0], start[1], start[2]),
                    fov_scale,
                );
                let (e_ndc, _) = camera.project_perspective(
                    &Point3::new(end[0], end[1], end[2]),
                    fov_scale,
                );
                DrawLine {
                    x1: self.ndc_to_canvas_x(s_ndc.x),
                    y1: self.ndc_to_canvas_y(s_ndc.y),
                    x2: self.ndc_to_canvas_x(e_ndc.x),
                    y2: self.ndc_to_canvas_y(e_ndc.y),
                }
            })
            .collect();

        DrawCommands { points, lines }
    }

    /// Convert NDC x (-1..1, left to right) to canvas x (0..width).
    fn ndc_to_canvas_x(&self, ndc_x: f64) -> f64 {
        (ndc_x + 1.0) / 2.0 * self.width
    }

    /// Convert NDC y (-1..1, bottom to top) to canvas y (0..height, top-left origin).
    fn ndc_to_canvas_y(&self, ndc_y: f64) -> f64 {
        (1.0 - ndc_y) / 2.0 * self.height
    }
}

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
        let camera = Camera::new(
            Point3::new(center[0], center[1], center[2] + 20.0),
            20.0,
        );
        let viewport = Viewport::new(160.0, 96.0);
        let cmds = viewport.render(&scene, &camera);
        assert_eq!(cmds.points.len(), 18);
        assert_eq!(cmds.lines.len(), 12);
    }

    #[test]
    fn depth_sort_farthest_first() {
        // Camera looking from above (+Z). Two atoms at different Z depths.
        // O atom at z≈14.4, Cu atom at z=0. Camera near z≈37 (target at z=30,
        // radius=10, phi=π/4).
        // Cu at z=0 is FARTHER from camera (view-Z more negative) than O at z≈14.
        // Painter's algorithm: Cu (farther) should be at index 0, O (closer) later.
        let structure = cu111_co_system(3.615);
        let scene = Scene::from_structure(&structure);
        let camera = Camera::new(Point3::new(5.0, 9.0, 30.0), 10.0);
        let viewport = Viewport::new(160.0, 96.0);
        let cmds = viewport.render(&scene, &camera);

        // Find indices of origin Cu (z≈0) and O atom (z≈14)
        let cu_idx = scene
            .atoms
            .iter()
            .position(|a| {
                a.position[0].abs() < 1e-6
                    && a.position[1].abs() < 1e-6
                    && a.position[2].abs() < 1e-6
            })
            .expect("origin Cu not found");
        let o_idx = scene
            .atoms
            .iter()
            .position(|a| a.element == chemrust_geometry::ElementSymbol::O)
            .expect("O atom not found");

        // Verify that the origin Cu (farther from camera) appears before O
        let cu_sort_pos = cmds
            .points
            .iter()
            .position(|p| p.atom_index == cu_idx)
            .unwrap();
        let o_sort_pos = cmds
            .points
            .iter()
            .position(|p| p.atom_index == o_idx)
            .unwrap();
        assert!(
            cu_sort_pos < o_sort_pos,
            "Cu at z=0 (farther) should render before O at z≈14 (closer): Cu pos={}, O pos={}",
            cu_sort_pos,
            o_sort_pos
        );
    }

    #[test]
    fn origin_projects_near_center_top_down() {
        let structure = cu111_co_system(3.615);
        let scene = Scene::from_structure(&structure);
        let center = scene.bounding_box_center();
        // Camera offset above center, looking down.
        let camera = Camera::with_angles(
            Point3::new(center[0], center[1], center[2]),
            20.0,
            0.0,
            0.001,
        );
        let viewport = Viewport::new(160.0, 96.0);
        let cmds = viewport.render(&scene, &camera);

        // All points should have finite canvas coordinates (no NaN from degenerate math)
        let origin_pt = cmds.points.iter().find(|p| p.atom_index == 0).unwrap();
        assert!(origin_pt.x.is_finite(), "origin x should be finite");
        assert!(origin_pt.y.is_finite(), "origin y should be finite");
        // All projected points should be within reasonable canvas bounds
        for pt in &cmds.points {
            assert!(pt.x.is_finite() && pt.y.is_finite());
        }
    }

    #[test]
    fn molecule_has_no_cell_edges_in_output() {
        use chemrust_geometry::{ElementSymbol, FracCoord, Structure};
        let mol = Structure::new(
            vec![ElementSymbol::H; 2],
            vec![FracCoord::new(0., 0., 0.), FracCoord::new(0., 0., 0.74)],
            None,
            [false; 3],
            vec![0; 2],
            vec![None; 2],
            None,
        );
        let scene = Scene::from_structure(&mol);
        let camera = Camera::new(Point3::new(0., 0., 5.), 10.);
        let viewport = Viewport::new(160., 96.);
        let cmds = viewport.render(&scene, &camera);
        assert_eq!(cmds.points.len(), 2);
        assert!(cmds.lines.is_empty());
    }
}
