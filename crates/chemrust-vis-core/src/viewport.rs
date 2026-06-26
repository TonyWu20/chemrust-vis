//! Viewport: 3D world → view space → perspective projection → canvas.
//! Produces `DrawCommands` for consumption by renderers.
//!
//! Pipeline: world point → camera view matrix → view space →
//! perspective divide → canvas dot coordinates. Depth sorting via
//! painter's algorithm (farthest from camera first).

use crate::camera::Camera;
use crate::scene::Scene;
use nalgebra::Point3;

/// A draw command for a single atom on canvas.
#[derive(Debug, Clone)]
pub struct DrawPoint {
    /// Canvas X coordinate in dot units (0 = left edge of viewport).
    pub x: f64,
    /// Canvas Y coordinate in dot units (0 = top edge of viewport).
    pub y: f64,
    /// View-space Z for depth sorting (negative = in front of camera).
    pub z_view: f64,
    /// On-screen radius in dot units (perspective-dependent).
    pub radius: f64,
    /// Element color from the scene's color map.
    pub color: crate::scene::RgbColor,
    /// Index into the scene's atoms array.
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
    /// Pan offset in canvas dot units (x, y).
    pub pan_x: f64,
    pub pan_y: f64,
}

impl Viewport {
    /// Create a new viewport with given canvas dimensions in dot units.
    /// Width = cols × 2, height = rows × 2 for block rendering.
    pub fn new(width: f64, height: f64) -> Self {
        Viewport { width, height, pan_x: 0.0, pan_y: 0.0 }
    }

    /// Pan the view by an offset in canvas dot units.
    pub fn pan(&mut self, dx: f64, dy: f64) {
        self.pan_x += dx;
        self.pan_y += dy;
    }

    /// Reset pan to zero.
    pub fn reset_pan(&mut self) {
        self.pan_x = 0.0;
        self.pan_y = 0.0;
    }

    /// Render a scene through a camera into draw commands.
    ///
    /// Orthographic projection: projects atoms to view space, finds the 2D
    /// bounding box, scales to fit the viewport, and centers the camera's
    /// look-at point (which maps to view-space origin).
    pub fn render(&self, scene: &Scene, camera: &Camera) -> DrawCommands {
        // Step 1: project all world points to view space
        let view_matrix = camera.view_matrix();
        let view_pts: Vec<(Point3<f64>, &crate::scene::AtomDrawData)> = scene
            .atoms
            .iter()
            .map(|atom| {
                let world = Point3::new(atom.position[0], atom.position[1], atom.position[2]);
                (view_matrix * world, atom)
            })
            .collect();

        // Step 2: find 2D extent in view space (x,y), ignoring z
        let mut min_x = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        for (vp, _) in &view_pts {
            min_x = min_x.min(vp.x);
            max_x = max_x.max(vp.x);
            min_y = min_y.min(vp.y);
            max_y = max_y.max(vp.y);
        }
        // Also include cell edge endpoints
        for &(start, end) in &scene.cell_edges {
            let s = view_matrix * Point3::new(start[0], start[1], start[2]);
            let e = view_matrix * Point3::new(end[0], end[1], end[2]);
            min_x = min_x.min(s.x).min(e.x);
            max_x = max_x.max(s.x).max(e.x);
            min_y = min_y.min(s.y).min(e.y);
            max_y = max_y.max(s.y).max(e.y);
        }
        if !min_x.is_finite() {
            return DrawCommands { points: vec![], lines: vec![] };
        }

        let data_w = (max_x - min_x).max(1.0);
        let data_h = (max_y - min_y).max(1.0);
        // Scale to fill 85% of viewport, preserving aspect ratio
        let margin = 0.85;
        let scale = (self.width * margin / data_w).min(self.height * margin / data_h);
        // Center on data's view-space midpoint + pan offset.
        // Camera target move (via Camera::pan) shifts view-space positions.
        // Viewport::pan() adds an additional canvas-level offset.
        let data_cx = (min_x + max_x) / 2.0;
        let data_cy = (min_y + max_y) / 2.0;
        let cx = self.width / 2.0 + self.pan_x;
        let cy = self.height / 2.0 + self.pan_y;
        // Atom radius: 5% of viewport shorter dimension, scaled to world units
        let atom_radius = (self.width.min(self.height) * 0.05 / scale).max(0.5);

        // Step 3: map to canvas with centering on data centroid + pan offset
        let mut point_data: Vec<(DrawPoint, f64)> = view_pts
            .iter()
            .map(|(vp, atom)| {
                let sx = (vp.x - data_cx) * scale + cx;
                let sy = -(vp.y - data_cy) * scale + cy;
                let dp = DrawPoint {
                    x: sx,
                    y: sy,
                    z_view: vp.z,
                    radius: atom_radius * scale,
                    color: atom.color,
                    atom_index: atom.atom_index,
                };
                (dp, vp.z)
            })
            .collect();

        // Sort by view-Z ascending (most negative = farthest = first).
        point_data.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        let points: Vec<DrawPoint> = point_data.into_iter().map(|(dp, _)| dp).collect();

        // Step 4: project cell edges (centered on data centroid + pan)
        let lines: Vec<DrawLine> = scene
            .cell_edges
            .iter()
            .map(|&(start, end)| {
                let s = view_matrix * Point3::new(start[0], start[1], start[2]);
                let e = view_matrix * Point3::new(end[0], end[1], end[2]);
                DrawLine {
                    x1: (s.x - data_cx) * scale + cx,
                    y1: -(s.y - data_cy) * scale + cy,
                    x2: (e.x - data_cx) * scale + cx,
                    y2: -(e.y - data_cy) * scale + cy,
                }
            })
            .collect();

        DrawCommands { points, lines }
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
        // With periodic expansion, we get original 18 + boundary replicas
        assert!(cmds.points.len() >= 18, "got {}", cmds.points.len());
        assert_eq!(cmds.lines.len(), 12);
    }

    #[test]
    fn depth_sort_farthest_first() {
        let structure = cu111_co_system(3.615);
        let scene = Scene::from_structure(&structure);
        let camera = Camera::new(Point3::new(5.0, 9.0, 30.0), 10.0);
        let viewport = Viewport::new(160.0, 96.0);
        let cmds = viewport.render(&scene, &camera);

        // Find the original structure index of the origin Cu and O atom
        let cu_orig_idx = scene.atoms.iter().find(|a| {
            !a.is_periodic_image
                && a.position[0].abs() < 1e-6
                && a.position[1].abs() < 1e-6
                && a.position[2].abs() < 1e-6
        }).map(|a| a.atom_index).expect("origin Cu not found");

        let o_orig_idx = scene.atoms.iter().find(|a| {
            !a.is_periodic_image && a.element == chemrust_geometry::ElementSymbol::O
        }).map(|a| a.atom_index).expect("O atom not found");

        // DrawPoint.atom_index carries the original structure index
        let cu_draw_pos = cmds.points.iter().position(|p| p.atom_index == cu_orig_idx);
        let o_draw_pos = cmds.points.iter().position(|p| p.atom_index == o_orig_idx);

        if let (Some(cu_pos), Some(o_pos)) = (cu_draw_pos, o_draw_pos) {
            assert!(cu_pos < o_pos,
                "Cu at z=0 (farther) should render before O: Cu pos={}, O pos={}",
                cu_pos, o_pos);
        }
    }

    #[test]
    fn all_points_have_finite_coordinates() {
        let structure = cu111_co_system(3.615);
        let scene = Scene::from_structure(&structure);
        let center = scene.bounding_box_center();
        let camera = Camera::with_angles(
            Point3::new(center[0], center[1], center[2]),
            20.0, 0.0, 0.001,
        );
        let viewport = Viewport::new(160.0, 96.0);
        let cmds = viewport.render(&scene, &camera);
        assert!(!cmds.points.is_empty(), "should have points");
        for pt in &cmds.points {
            assert!(pt.x.is_finite(), "x={}", pt.x);
            assert!(pt.y.is_finite(), "y={}", pt.y);
        }
    }

    #[test]
    fn molecule_has_no_cell_edges_in_output() {
        use chemrust_geometry::{ElementSymbol, FracCoord, Structure};
        let mol = Structure::new(
            vec![ElementSymbol::H; 2],
            vec![FracCoord::new(0., 0., 0.), FracCoord::new(0., 0., 0.74)],
            None, [false; 3], vec![0; 2], vec![None; 2], None,
        );
        let scene = Scene::from_structure(&mol);
        let camera = Camera::new(Point3::new(0., 0., 5.), 10.);
        let viewport = Viewport::new(160., 96.);
        let cmds = viewport.render(&scene, &camera);
        assert_eq!(cmds.points.len(), 2);
        assert!(cmds.lines.is_empty());
    }
}
