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
    /// Uses orthographic projection: all atoms have the same rendered size
    /// regardless of depth. The camera's look-at point maps to viewport center.
    pub fn render(&self, scene: &Scene, camera: &Camera) -> DrawCommands {
        // Scale: world units → canvas dot units.
        // A world offset of `camera.radius()` maps to half the viewport.
        let scale = self.height / (2.0 * camera.radius());
        // All atoms rendered at the same visual radius (in Angstroms → dots).
        let atom_radius = scale * 0.7;

        // Project atoms with orthographic projection.
        let mut point_data: Vec<(DrawPoint, f64)> = scene
            .atoms
            .iter()
            .map(|atom| {
                let world = Point3::new(atom.position[0], atom.position[1], atom.position[2]);
                let ndc = camera.project(&world, scale);
                let dp = DrawPoint {
                    x: (ndc.x + 1.0) / 2.0 * self.width,
                    y: (1.0 - ndc.y) / 2.0 * self.height,
                    z_view: ndc.z,
                    radius: atom_radius.max(1.5),
                    color: atom.color,
                    atom_index: atom.atom_index,
                };
                (dp, ndc.z)
            })
            .collect();

        // Sort by view-Z ascending (most negative = farthest = first).
        point_data.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        let points: Vec<DrawPoint> = point_data.into_iter().map(|(dp, _)| dp).collect();

        // Project cell edges.
        let lines: Vec<DrawLine> = scene
            .cell_edges
            .iter()
            .map(|&(start, end)| {
                let s_ndc = camera.project(
                    &Point3::new(start[0], start[1], start[2]),
                    scale,
                );
                let e_ndc = camera.project(
                    &Point3::new(end[0], end[1], end[2]),
                    scale,
                );
                DrawLine {
                    x1: (s_ndc.x + 1.0) / 2.0 * self.width,
                    y1: (1.0 - s_ndc.y) / 2.0 * self.height,
                    x2: (e_ndc.x + 1.0) / 2.0 * self.width,
                    y2: (1.0 - e_ndc.y) / 2.0 * self.height,
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
