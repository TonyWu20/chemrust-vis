//! Camera model with spherical coordinates and orthographic projection.
//! Z-up physics convention.
//!
//! Spherical coordinates:
//! - theta: azimuthal angle in XY plane (0 = +X, π/2 = +Y)
//! - phi: polar angle from Z axis (0 = +Z, π/2 = XY plane, π = −Z).
//!   Wraps at 2π during orbit; the position formula is periodic.

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
    /// Create a new camera looking at target from a diagonal view.
    /// theta=30° (from +X toward +Y), phi=30° elevation from XY plane.
    /// This gives a natural 3/4 view that shows three faces of the unit cell.
    pub fn new(target: Point3<f64>, radius: f64) -> Self {
        Camera {
            target,
            radius,
            theta: std::f64::consts::FRAC_PI_6,  // 30° azimuth
            phi: std::f64::consts::FRAC_PI_6,    // 30° elevation
        }
    }

    /// Create a camera looking at a target specified as [x, y, z].
    /// Convenience constructor so callers don't need to depend on nalgebra.
    pub fn from_target(target: [f64; 3], radius: f64) -> Self {
        Camera::new(Point3::new(target[0], target[1], target[2]), radius)
    }

    /// Create a camera with explicit angles.
    pub fn with_angles(target: Point3<f64>, radius: f64, theta: f64, phi: f64) -> Self {
        Camera { target, radius, theta, phi }
    }

    /// Create a camera with explicit angles, target as [x, y, z].
    pub fn with_angles_target(target: [f64; 3], radius: f64, theta: f64, phi: f64) -> Self {
        Camera {
            target: Point3::new(target[0], target[1], target[2]),
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
    /// World +Z is the screen up wherever it is not nearly parallel to
    /// the view direction. In the narrow band near a pole (where +Z is
    /// unusable), the up blends smoothly toward the horizontal outward
    /// direction `(-sin theta, cos theta, 0)`. That direction matches
    /// `orbit`'s pole reflection, so a view that rolls through a pole
    /// reorients the way a real orbit does instead of snapping 90
    /// degrees mid-orbit.
    fn compute_up(&self) -> Vector3<f64> {
        let pos = self.position();
        let dir = (self.target - pos).normalize();
        let z_axis = Vector3::new(0.0, 0.0, 1.0);
        // |dir · z| = |cos phi|: 1 at the poles, 0 at the equator.
        let c = dir.dot(&z_axis).abs();
        const BAND_FAR: f64 = 0.995; // 1.82 degrees from a pole
        const BAND_NEAR: f64 = 0.9999; // 0.81 degrees from a pole
        if c <= BAND_FAR {
            return z_axis;
        }
        let t = ((c - BAND_FAR) / (BAND_NEAR - BAND_FAR)).min(1.0);
        let s = t * t * (3.0 - 2.0 * t); // smoothstep in the band
        let outward = Vector3::new(-self.theta.sin(), self.theta.cos(), 0.0);
        (z_axis * (1.0 - s) + outward * s).normalize()
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

    /// Perspective projection: returns (NDC position, perspective scale factor).
    pub fn project_perspective(
        &self,
        world_pt: &Point3<f64>,
        fov_scale: f64,
    ) -> (Point3<f64>, f64) {
        let view_pt = self.view_matrix() * world_pt;
        let depth = (-view_pt.z).max(0.01);
        let persp = fov_scale / depth;
        let ndc = Point3::new(view_pt.x * persp, view_pt.y * persp, view_pt.z);
        (ndc, persp)
    }

    /// Project a world point directly to canvas dot coordinates.
    ///
    /// The camera's look-at target maps to the viewport center `(width/2, height/2)`.
    /// Returns `(canvas_x, canvas_y, view_z, persp_scale)`.
    ///
    /// - `canvas_x`, `canvas_y`: dot coordinates (0 = left/top edge)
    /// - `view_z`: view-space Z (negative = in front of camera, for depth sort)
    /// - `persp_scale`: perspective factor for computing atom screen radius
    ///   (`screen_radius = base_radius * persp_scale`)
    pub fn project_to_screen(
        &self,
        world_pt: &Point3<f64>,
        focal: f64,
        viewport_w: f64,
        viewport_h: f64,
    ) -> (f64, f64, f64, f64) {
        let view_pt = self.view_matrix() * world_pt;
        let depth = (-view_pt.z).max(0.01);
        let persp = focal / depth;
        // Screen X: view+x goes right → canvas+x goes right
        let sx = view_pt.x * persp + viewport_w / 2.0;
        // Screen Y: view+y goes up → canvas+y goes up (but canvas origin is top-left)
        // In view space, +Y is up. In canvas, +Y is down. So we flip:
        let sy = -view_pt.y * persp + viewport_h / 2.0;
        (sx, sy, view_pt.z, persp)
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

    /// Project world-space direction vectors (X, Y, Z axes) to 2D screen-space
    /// unit vectors. Returns `(x_dir, y_dir, z_dir)` where each is `(dx, dy)`
    /// in screen coordinates (+X = right, +Y = down for canvas).
    pub fn axis_directions(&self) -> ([f64; 2], [f64; 2], [f64; 2]) {
        let pose = self.camera_pose();
        // pose maps camera→world. Extract rotation: pose * camera_axis = world_axis
        // World axes in camera space: camera_axis = pose⁻¹ * world_axis
        // For screen projection: screen_x = camera_x, screen_y = -camera_y (flip Y)
        let world_to_camera = pose.inverse();
        let x = world_to_camera * Vector3::new(1.0, 0.0, 0.0);
        let y = world_to_camera * Vector3::new(0.0, 1.0, 0.0);
        let z = world_to_camera * Vector3::new(0.0, 0.0, 1.0);
        ([x.x, -x.y], [y.x, -y.y], [z.x, -z.y])
    }

    /// Orbit the camera by delta angles.
    ///
    /// Theta is unbounded. Phi wraps at 2π: the position formula
    /// `sin(φ)cos(θ), sin(φ)sin(θ), cos(φ)` is periodic, so the
    /// camera orbits freely through both poles. No angle is a dead
    /// end. The up-vector logic in `compute_up` handles the pole
    /// region via the view direction.
    pub fn orbit(&mut self, d_theta: f64, d_phi: f64) {
        self.theta += d_theta;
        self.phi = (self.phi + d_phi)
            .rem_euclid(2.0 * std::f64::consts::PI);
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

    #[test]
    fn orbit_wraps_through_top_pole() {
        // From phi=5deg, theta=0: a -10deg step crosses the top pole.
        // Phi wraps to 355deg (= 2pi - 5deg). Theta is unchanged.
        let mut cam = Camera::with_angles(
            Point3::new(0.0, 0.0, 0.0),
            10.0,
            0.0,
            5.0_f64.to_radians(),
        );
        cam.orbit(0.0, -10.0_f64.to_radians());
        let expected_phi = 2.0 * std::f64::consts::PI - 5.0_f64.to_radians();
        assert!((cam.phi() - expected_phi).abs() < 1e-12);
        assert!(cam.theta().abs() < 1e-12, "theta should stay 0");
        // Position equals P(-5deg, 0): 5deg past the top pole.
        let pos = cam.position();
        let s5 = 5.0_f64.to_radians();
        assert!((pos.x + 10.0 * s5.sin()).abs() < 1e-9, "x={}", pos.x);
        assert!(pos.y.abs() < 1e-9, "y={}", pos.y);
        assert!((pos.z - 10.0 * s5.cos()).abs() < 1e-9, "z={}", pos.z);
    }

    #[test]
    fn orbit_wraps_through_bottom_pole() {
        // From phi=175deg, theta=0: a +10deg step crosses the bottom pole.
        // Phi becomes 185deg. Theta is unchanged.
        let mut cam = Camera::with_angles(
            Point3::new(0.0, 0.0, 0.0),
            10.0,
            0.0,
            175.0_f64.to_radians(),
        );
        cam.orbit(0.0, 10.0_f64.to_radians());
        assert!((cam.phi() - 185.0_f64.to_radians()).abs() < 1e-9);
        assert!(cam.theta().abs() < 1e-12, "theta should stay 0");
        // Position equals P(185deg, 0): 5deg past the bottom pole.
        let pos = cam.position();
        let r185 = 185.0_f64.to_radians();
        assert!((pos.x - 10.0 * r185.sin()).abs() < 1e-9, "x={}", pos.x);
        assert!((pos.z - 10.0 * r185.cos()).abs() < 1e-9, "z={}", pos.z);
    }

    #[test]
    fn repeated_w_presses_at_bottom_pole_keep_theta_stable() {
        // Regression: pressing w repeatedly at the bottom pole must not
        // cycle theta. Phi simply increases past 180deg and wraps at 360deg.
        let mut cam = Camera::with_angles(
            Point3::new(0.0, 0.0, 0.0),
            10.0,
            0.0,
            std::f64::consts::PI, // start at bottom pole
        );
        for i in 1..=4 {
            cam.orbit(0.0, 5.0_f64.to_radians());
            let expected_phi = (std::f64::consts::PI + (i as f64) * 5.0_f64.to_radians())
                .rem_euclid(2.0 * std::f64::consts::PI);
            assert!((cam.phi() - expected_phi).abs() < 1e-9, "press {}", i);
            assert!(cam.theta().abs() < 1e-12,
                "theta shifted on press {}: {}", i, cam.theta());
        }
    }

    #[test]
    fn orbit_has_no_dead_end_at_the_poles() {
        // Regression for the "stuck at a specific angle" report. The old
        // clamp pinned phi at 0.001 rad and pi - 0.001 rad. Further
        // pushes in the same direction did nothing. Now every step
        // moves the camera, through both poles.
        let mut cam =
            Camera::with_angles(Point3::new(0.0, 0.0, 0.0), 10.0, 0.0, 0.001);
        let mut prev = cam.position();
        for _ in 0..4 {
            cam.orbit(0.0, -5.0_f64.to_radians());
            let pos = cam.position();
            assert!((pos - prev).norm() > 1e-6, "pinned at phi={}", cam.phi());
            prev = pos;
        }
        let mut cam = Camera::with_angles(
            Point3::new(0.0, 0.0, 0.0),
            10.0,
            0.0,
            std::f64::consts::PI - 0.001,
        );
        let mut prev = cam.position();
        for _ in 0..4 {
            cam.orbit(0.0, 5.0_f64.to_radians());
            let pos = cam.position();
            assert!((pos - prev).norm() > 1e-6, "pinned at phi={}", cam.phi());
            prev = pos;
        }
    }

    #[test]
    fn up_vector_is_world_z_at_equator() {
        let cam = Camera::with_angles(
            Point3::new(0.0, 0.0, 0.0),
            10.0,
            0.7,
            std::f64::consts::FRAC_PI_4,
        );
        let up = cam.compute_up();
        assert!((up - Vector3::new(0.0, 0.0, 1.0)).norm() < 1e-12);
    }

    #[test]
    fn up_vector_at_pole_points_outward() {
        // Exactly at the top pole the up is the horizontal outward
        // direction for the current theta: theta=0 gives (0, 1, 0),
        // theta=pi/2 gives (-1, 0, 0).
        let cam = Camera::with_angles(Point3::new(0.0, 0.0, 0.0), 10.0, 0.0, 0.0);
        assert!((cam.compute_up() - Vector3::new(0.0, 1.0, 0.0)).norm() < 1e-9);
        let cam = Camera::with_angles(
            Point3::new(0.0, 0.0, 0.0),
            10.0,
            std::f64::consts::FRAC_PI_2,
            0.0,
        );
        assert!((cam.compute_up() - Vector3::new(-1.0, 0.0, 0.0)).norm() < 1e-9);
    }

    #[test]
    fn up_vector_changes_smoothly_through_pole_band() {
        // No sudden roll outside the pole band: sweep phi from the top
        // pole to 45 degrees and keep every step within about 2.5
        // degrees of the previous up vector.
        let mut prev: Option<Vector3<f64>> = None;
        let mut phi = 0.001f64;
        while phi <= std::f64::consts::FRAC_PI_4 {
            let cam =
                Camera::with_angles(Point3::new(0.0, 0.0, 0.0), 10.0, 0.0, phi);
            let up = cam.compute_up();
            if let Some(p) = prev {
                assert!(
                    p.dot(&up) > 0.999,
                    "up snapped between phi={} and {}",
                    phi - 0.0005,
                    phi
                );
            }
            prev = Some(up);
            phi += 0.0005;
        }
    }
}
