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
}

impl Viewport {
    /// Create a new viewport with given canvas dimensions in dot units.
    /// Width = cols × 2, height = rows × 4 for Braille rendering.
    pub fn new(width: f64, height: f64) -> Self {
        Viewport { width, height }
    }

    /// Render a scene through a camera into draw commands.
    ///
    /// Uses perspective projection: world point → view space →
    /// perspective divide → canvas dot coordinates. The camera's
    /// look-at point maps to the viewport center.
    pub fn render(&self, scene: &Scene, camera: &Camera) -> DrawCommands {
        // Focal length: controls field of view. Larger = more zoomed in.
        // Set so the structure fills ~2/3 of the viewport for a typical slab.
        let focal = self.height * 0.6;
        // Visual atom radius in Angstroms (much larger than covalent for clarity).
        let base_radius = 4.0;

        // Project atoms.
        let mut point_data: Vec<(DrawPoint, f64)> = scene
            .atoms
            .iter()
            .map(|atom| {
                let world = Point3::new(atom.position[0], atom.position[1], atom.position[2]);
                let (sx, sy, z_view, persp) = camera.project_to_screen(
                    &world,
                    focal,
                    self.width,
                    self.height,
                );
                let dp = DrawPoint {
                    x: sx,
                    y: sy,
                    z_view,
                    radius: (base_radius * persp).max(1.0), // at least 1 dot
                    color: atom.color,
                    atom_index: atom.atom_index,
                };
                (dp, z_view)
            })
            .collect();

        // Sort by view-Z ascending (most negative = farthest = first).
        point_data.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        let points: Vec<DrawPoint> = point_data.into_iter().map(|(dp, _)| dp).collect();

        // Project cell edges with same perspective.
        let lines: Vec<DrawLine> = scene
            .cell_edges
            .iter()
            .map(|&(start, end)| {
                let (sx1, sy1, _, _) = camera.project_to_screen(
                    &Point3::new(start[0], start[1], start[2]),
                    focal,
                    self.width,
                    self.height,
                );
                let (sx2, sy2, _, _) = camera.project_to_screen(
                    &Point3::new(end[0], end[1], end[2]),
                    focal,
                    self.width,
                    self.height,
                );
                DrawLine { x1: sx1, y1: sy1, x2: sx2, y2: sy2 }
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
        assert_eq!(cmds.points.len(), 18);
        assert_eq!(cmds.lines.len(), 12);
    }

    #[test]
    fn depth_sort_farthest_first() {
        let structure = cu111_co_system(3.615);
        let scene = Scene::from_structure(&structure);
        let camera = Camera::new(Point3::new(5.0, 9.0, 30.0), 10.0);
        let viewport = Viewport::new(160.0, 96.0);
        let cmds = viewport.render(&scene, &camera);

        let cu_idx = scene.atoms.iter().position(|a| {
            a.position[0].abs() < 1e-6 && a.position[1].abs() < 1e-6 && a.position[2].abs() < 1e-6
        }).expect("origin Cu not found");
        let o_idx = scene.atoms.iter().position(|a| {
            a.element == chemrust_geometry::ElementSymbol::O
        }).expect("O atom not found");

        let cu_sort_pos = cmds.points.iter().position(|p| p.atom_index == cu_idx).unwrap();
        let o_sort_pos = cmds.points.iter().position(|p| p.atom_index == o_idx).unwrap();
        assert!(cu_sort_pos < o_sort_pos,
            "Cu at z=0 (farther) should render before O: Cu pos={}, O pos={}",
            cu_sort_pos, o_sort_pos);
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
