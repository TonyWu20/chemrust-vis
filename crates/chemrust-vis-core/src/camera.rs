//! Camera model with spherical coordinates and orthographic projection.
//! Z-up physics convention.

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
    /// Create a new camera looking at target from default position.
    pub fn new(target: Point3<f64>, radius: f64) -> Self {
        Camera {
            target,
            radius,
            theta: 0.0,
            phi: std::f64::consts::FRAC_PI_4,
        }
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

    /// Camera position in world space.
    pub fn position(&self) -> Point3<f64> {
        let x = self.target.x + self.radius * self.phi.sin() * self.theta.cos();
        let y = self.target.y + self.radius * self.phi.sin() * self.theta.sin();
        let z = self.target.z + self.radius * self.phi.cos();
        Point3::new(x, y, z)
    }

    /// Compute the view matrix (world → camera space).
    pub fn view_matrix(&self) -> Isometry3<f64> {
        let pos = self.position();
        let up = Vector3::new(0.0, 0.0, 1.0);
        Isometry3::look_at_rh(&pos, &self.target, &up)
    }

    /// Project a world-space point to normalized device coordinates.
    pub fn project(&self, world_pt: &Point3<f64>, _scale: f64) -> Point3<f64> {
        let view = self.view_matrix();
        let view_pt = view * world_pt;
        // Placeholder orthographic projection
        Point3::new(view_pt.x, view_pt.y, view_pt.z)
    }

    /// Get the current target.
    pub fn target(&self) -> Point3<f64> {
        self.target
    }

    /// Get the current radius.
    pub fn radius(&self) -> f64 {
        self.radius
    }

    /// Orbit the camera by delta angles.
    pub fn orbit(&mut self, d_theta: f64, d_phi: f64) {
        self.theta += d_theta;
        self.phi += d_phi;
    }

    /// Zoom by changing radius. Clamped to minimum 0.1.
    pub fn zoom(&mut self, dr: f64) {
        self.radius = (self.radius + dr).max(0.1);
    }

    /// Pan the target point.
    pub fn pan(&mut self, _dx: f64, _dy: f64) {
        // Placeholder
    }
}
