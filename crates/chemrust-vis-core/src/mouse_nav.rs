//! Mouse-navigation sensitivity mappings.
//!
//! These pure functions map raw input (pixel deltas, scroll ticks) to
//! camera motion (angle deltas, world-unit displacements, radius changes).
//!
//! Coordinate convention (Z-up, matches `Camera`):
//! - `theta`: azimuth in the XY plane (0 = +X, increases toward +Y).
//! - `phi`: polar angle from +Z, wrapped into `[0, 2*PI)`.
//! - Screen axes: +x = right, +y = down (terminal row grows downward).

/// Orbit sensitivity: radians of camera rotation per pixel of drag at
/// radius 1. A 10-pixel drag at radius 100 produces 0.001 rad (~0.06 deg).
pub const S_ROT: f64 = 0.01;

/// Zoom sensitivity: radius delta per scroll tick per unit radius.
/// One scroll tick at radius 30 changes the radius by 0.6.
pub const S_ZOOM: f64 = 0.02;

/// Smallest allowed camera-to-target distance.
pub const MIN_RADIUS: f64 = 0.1;

/// Default click/drag threshold in the same units as the pixel deltas.
/// A press-release whose total displacement is below this value is a
/// click (atom pick); at or above it is a drag (navigation).
pub const CLICK_THRESHOLD: f64 = 3.0;

/// Orbit deltas: `(d_theta, d_phi)` in radians for a pixel drag of
/// `(px, py)` at the given camera radius.
///
/// Sensitivity is inversely proportional to radius so a fixed screen drag
/// produces the same angular change at every zoom level.
pub fn orbit_deltas(px: f64, py: f64, radius: f64) -> (f64, f64) {
    (px * S_ROT / radius, py * S_ROT / radius)
}

/// Base pan deltas in world units for a pixel drag of `(px, py)` at the
/// given camera radius and viewport height (same units as `px`, `py`).
///
/// Sensitivity is proportional to radius: the same screen drag moves the
/// target by the same world amount at every zoom level.
pub fn pan_deltas(px: f64, py: f64, radius: f64, viewport_h: f64) -> (f64, f64) {
    (px * radius / viewport_h, py * radius / viewport_h)
}

/// Alt+drag ("push the paper") pan deltas: the horizontal component is
/// inverted so that pushing the paper left reveals content on the right.
/// The vertical component is unchanged (screen rows grow downward, but
/// the camera up axis also points up in view space).
pub fn alt_drag_pan_deltas(px: f64, py: f64, radius: f64, viewport_h: f64) -> (f64, f64) {
    (-px * radius / viewport_h, py * radius / viewport_h)
}

/// Scroll ticks to radius delta.
/// Ticks are signed: up-scroll is negative (zoom in, radius shrinks),
/// down-scroll is positive (zoom out, radius grows).
pub fn zoom_delta(ticks: f64, radius: f64) -> f64 {
    ticks * S_ZOOM * radius
}

/// Sensitivity divisor when the snipe modifier (Ctrl) is held.
pub fn snipe_factor(snipe: bool) -> f64 {
    if snipe { 10.0 } else { 1.0 }
}

/// Orbit deltas with the snipe factor applied.
/// When `snipe` is true the sensitivity is divided by 10 for fine control.
pub fn orbit_deltas_snipe(px: f64, py: f64, radius: f64, snipe: bool) -> (f64, f64) {
    let f = snipe_factor(snipe);
    (px * S_ROT / f / radius, py * S_ROT / f / radius)
}

/// A press-release whose total displacement is below the threshold is a
/// click (atom pick), not a drag (navigation).
///
/// # Arguments
/// * `px` - horizontal displacement in screen units
/// * `py` - vertical displacement in screen units
/// * `threshold` - click/drag threshold in the same units
pub fn is_click(px: f64, py: f64, threshold: f64) -> bool {
    px * px + py * py < threshold * threshold
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orbit_delta_theta() {
        let (d_theta, _) = orbit_deltas(10.0, 5.0, 100.0);
        assert!((d_theta - 0.001).abs() < 1e-15, "d_theta = {}", d_theta);
    }

    #[test]
    fn orbit_delta_phi() {
        let (_, d_phi) = orbit_deltas(10.0, 5.0, 100.0);
        assert!((d_phi - 0.0005).abs() < 1e-15, "d_phi = {}", d_phi);
    }

    #[test]
    fn zoom_delta_negative_tick() {
        let dr = zoom_delta(-1.0, 100.0);
        assert!((dr - (-2.0)).abs() < 1e-15, "dr = {}", dr);
    }

    #[test]
    fn zoom_delta_positive_tick() {
        let dr = zoom_delta(1.0, 100.0);
        assert!((dr - 2.0).abs() < 1e-15, "dr = {}", dr);
    }

    #[test]
    fn alt_drag_pan_inverts_horizontal() {
        let (wx, _) = alt_drag_pan_deltas(-10.0, 0.0, 50.0, 100.0);
        assert!((wx - 5.0).abs() < 1e-15, "wx = {}", wx);
    }

    #[test]
    fn alt_drag_pan_y_matches_base_pan() {
        let (_, wy_base) = pan_deltas(10.0, 5.0, 50.0, 100.0);
        let (_, wy_alt) = alt_drag_pan_deltas(10.0, 5.0, 50.0, 100.0);
        assert!((wy_base - wy_alt).abs() < 1e-15);
    }

    #[test]
    fn alt_drag_x_is_negated_base_pan_x() {
        let (wx_base, _) = pan_deltas(10.0, 0.0, 50.0, 100.0);
        let (wx_alt, _) = alt_drag_pan_deltas(10.0, 0.0, 50.0, 100.0);
        assert!((wx_base + wx_alt).abs() < 1e-15);
    }

    #[test]
    fn click_below_threshold() {
        assert!(is_click(2.0, 0.0, 3.0));
    }

    #[test]
    fn click_at_threshold_is_drag() {
        assert!(!is_click(3.0, 0.0, 3.0));
    }

    #[test]
    fn click_zero_is_click() {
        assert!(is_click(0.0, 0.0, 3.0));
    }

    #[test]
    fn snipe_divides_orbit_sensitivity_by_ten() {
        let normal = orbit_deltas(10.0, 5.0, 100.0).0;
        let sniped = orbit_deltas_snipe(10.0, 5.0, 100.0, true).0;
        assert!((sniped - normal / 10.0).abs() < 1e-15);
    }

    #[test]
    fn snipe_disabled_is_identity() {
        let a = orbit_deltas(10.0, 5.0, 100.0);
        let b = orbit_deltas_snipe(10.0, 5.0, 100.0, false);
        assert!((a.0 - b.0).abs() < 1e-15);
        assert!((a.1 - b.1).abs() < 1e-15);
    }

    #[test]
    fn orbit_deltas_inverse_proportional_to_radius() {
        let (d1, _) = orbit_deltas(10.0, 0.0, 50.0);
        let (d2, _) = orbit_deltas(10.0, 0.0, 100.0);
        assert!((d1 - 2.0 * d2).abs() < 1e-12, "{} vs {}", d1, d2);
    }

    #[test]
    fn pan_deltas_proportional_to_radius() {
        let (w1, _) = pan_deltas(10.0, 0.0, 50.0, 100.0);
        let (w2, _) = pan_deltas(10.0, 0.0, 100.0, 100.0);
        assert!((w2 - 2.0 * w1).abs() < 1e-12, "{} vs {}", w1, w2);
    }

    #[test]
    fn min_radius_constant() {
        assert!((MIN_RADIUS - 0.1).abs() < 1e-15);
    }

    #[test]
    fn click_threshold_constant() {
        assert!((CLICK_THRESHOLD - 3.0).abs() < 1e-15);
    }
}
