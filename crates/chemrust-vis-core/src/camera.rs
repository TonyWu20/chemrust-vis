//! Camera model with spherical coordinates and orthographic projection.
//! Z-up physics convention.
//!
//! Spherical coordinates:
//! - theta: azimuthal angle in XY plane (0 = +X, π/2 = +Y)
//! - phi: polar angle from Z axis (0 = +Z, π/2 = XY plane)

use nalgebra::{Isometry3, Point3, Vector3};

/// Camera with spherical coordinate positioning around a target.
#[derive(Debug, Clone)]
pub struct Camera {
    target: Point3<f64>,
    radius: f64,
    theta: f64,
    phi: f64,
}

impl Camera {
    /// Create a new camera looking at target from default position
    /// (theta=0, phi=π/4 = 45° elevation from XY plane toward +Z).
    pub fn new(target: Point3<f64>, radius: f64) -> Self {
        Camera {
            target,
            radius,
            theta: 0.0,
            phi: std::f64::consts::FRAC_PI_4,
        }
    }

    /// Create a camera looking at a target specified as [x, y, z].
    /// Convenience constructor so callers don't need to depend on nalgebra.
    pub fn from_target(target: [f64; 3], radius: f64) -> Self {
        Camera::new(Point3::new(target[0], target[1], target[2]), radius)
    }

    /// Create a camera with explicit angles.
    pub fn with_angles(target: Point3<f64>, radius: f64, theta: f64, phi: f64) -> Self {
        Camera {
            target,
            radius,
            theta,
            phi,
        }
    }

    /// Camera position in world space (Cartesian).
    pub fn position(&self) -> Point3<f64> {
        let x = self.target.x + self.radius * self.phi.sin() * self.theta.cos();
        let y = self.target.y + self.radius * self.phi.sin() * self.theta.sin();
        let z = self.target.z + self.radius * self.phi.cos();
        Point3::new(x, y, z)
    }

    /// Choose an up vector for look_at_rh, avoiding gimbal lock.
    ///
    /// When the view direction is nearly parallel to (0,0,1), the default
    /// up vector would cause a degenerate cross product.
    fn compute_up(&self) -> Vector3<f64> {
        let pos = self.position();
        let dir = (self.target - pos).normalize();
        let z_axis = Vector3::new(0.0, 0.0, 1.0);
        // If view direction is nearly parallel to ±Z, use -Y as up.
        // |dir · z_axis| ≈ 1 means nearly parallel.
        if dir.dot(&z_axis).abs() > 0.9999 {
            Vector3::new(0.0, -1.0, 0.0)
        } else {
            Vector3::new(0.0, 0.0, 1.0)
        }
    }

    /// Compute the view matrix (world → camera space).
    ///
    /// `view_matrix * world_point` gives camera-relative coordinates.
    pub fn view_matrix(&self) -> Isometry3<f64> {
        let pos = self.position();
        let up = self.compute_up();
        let pose = Isometry3::look_at_rh(&pos, &self.target, &up);
        // look_at_rh gives camera→world; invert for world→camera
        pose.inverse()
    }

    /// Camera-to-world isometry (for extracting camera axes).
    fn camera_pose(&self) -> Isometry3<f64> {
        let pos = self.position();
        let up = self.compute_up();
        Isometry3::look_at_rh(&pos, &self.target, &up)
    }

    /// Project a world-space point to normalized device coordinates (NDC)
    /// using orthographic projection with the given scale factor.
    pub fn project(&self, world_pt: &Point3<f64>, scale: f64) -> Point3<f64> {
        let view_pt = self.view_matrix() * world_pt;
        Point3::new(view_pt.x / scale, view_pt.y / scale, view_pt.z / scale)
    }

    /// Get the current target.
    pub fn target(&self) -> Point3<f64> {
        self.target
    }

    /// Get the current radius.
    pub fn radius(&self) -> f64 {
        self.radius
    }

    /// Get the azimuthal angle theta (radians).
    pub fn theta(&self) -> f64 {
        self.theta
    }

    /// Get the polar angle phi (radians).
    pub fn phi(&self) -> f64 {
        self.phi
    }

    /// Orbit the camera by delta angles.
    pub fn orbit(&mut self, d_theta: f64, d_phi: f64) {
        self.theta += d_theta;
        self.phi += d_phi;
        self.phi = self.phi.clamp(0.001, std::f64::consts::PI - 0.001);
    }

    /// Zoom by changing radius. Clamped to minimum 0.1.
    pub fn zoom(&mut self, dr: f64) {
        self.radius = (self.radius + dr).max(0.1);
    }

    /// Pan the target point in camera-local right/up directions.
    pub fn pan(&mut self, dx: f64, dy: f64) {
        let pose = self.camera_pose();
        let right = pose * Vector3::new(1.0, 0.0, 0.0);
        let up = pose * Vector3::new(0.0, 1.0, 0.0);
        self.target.x += dx * right.x + dy * up.x;
        self.target.y += dx * right.y + dy * up.y;
        self.target.z += dx * right.z + dy * up.z;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nalgebra::Point3;

    #[test]
    fn position_at_theta_0_phi_pi_half() {
        let cam = Camera::with_angles(
            Point3::new(5.0, 5.0, 5.0),
            10.0,
            0.0,
            std::f64::consts::FRAC_PI_2,
        );
        let pos = cam.position();
        assert!((pos.x - 15.0).abs() < 1e-10);
        assert!((pos.y - 5.0).abs() < 1e-10);
        assert!((pos.z - 5.0).abs() < 1e-10);
    }

    #[test]
    fn position_at_theta_pi_half_phi_pi_half() {
        let cam = Camera::with_angles(
            Point3::new(5.0, 5.0, 5.0),
            10.0,
            std::f64::consts::FRAC_PI_2,
            std::f64::consts::FRAC_PI_2,
        );
        let pos = cam.position();
        assert!((pos.x - 5.0).abs() < 1e-10);
        assert!((pos.y - 15.0).abs() < 1e-10);
        assert!((pos.z - 5.0).abs() < 1e-10);
    }

    #[test]
    fn position_at_top_down() {
        // phi=0, camera on +Z axis above target
        let cam = Camera::with_angles(Point3::new(0.0, 0.0, 0.0), 10.0, 0.0, 0.0);
        let pos = cam.position();
        assert!((pos.x).abs() < 1e-10);
        assert!((pos.y).abs() < 1e-10);
        assert!((pos.z - 10.0).abs() < 1e-10);
    }

    #[test]
    fn orbit_updates_angles() {
        let mut cam = Camera::new(Point3::new(0.0, 0.0, 0.0), 10.0);
        let pos_before = cam.position();
        cam.orbit(std::f64::consts::FRAC_PI_2, 0.0);
        let pos_after = cam.position();
        assert!((pos_before - pos_after).norm() > 1e-6);
    }

    #[test]
    fn zoom_changes_radius() {
        let mut cam = Camera::new(Point3::new(0.0, 0.0, 0.0), 10.0);
        cam.zoom(5.0);
        assert!((cam.radius() - 15.0).abs() < 1e-10);
        cam.zoom(-20.0);
        assert!(cam.radius() >= 0.1);
    }

    #[test]
    fn project_returns_finite_values() {
        let cam = Camera::with_angles(Point3::new(0.0, 0.0, 0.0), 10.0, 0.0, 0.5);
        let ndc = cam.project(&Point3::new(2.0, 3.0, 0.0), 1.0);
        assert!(ndc.x.is_finite());
        assert!(ndc.y.is_finite());
        assert!(ndc.z.is_finite());
    }

    #[test]
    fn pan_moves_target() {
        let mut cam = Camera::new(Point3::new(5.0, 5.0, 5.0), 10.0);
        let target_before = cam.target();
        cam.pan(2.0, 3.0);
        let target_after = cam.target();
        assert!((target_after - target_before).norm() > 1e-6);
    }
}
