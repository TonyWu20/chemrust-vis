//! Keyboard and mouse input handling: maps key and mouse events to camera
//! and viewport actions.

use chemrust_vis_core::camera::Camera;
use chemrust_vis_core::viewport::Viewport;
use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};

/// Actions the app can take in response to input.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Action {
    Quit,
    Redraw,
    Reset,
    None,
}

/// Map a key event to camera/viewport mutations. Returns the action to take.
pub fn handle_key_event(
    key: KeyEvent,
    camera: &mut Camera,
    viewport: &mut Viewport,
) -> Action {
    // Only process key press events (not releases or repeats).
    if key.kind != KeyEventKind::Press {
        return Action::None;
    }

    match (key.modifiers, key.code) {
        // Quit
        (KeyModifiers::NONE, KeyCode::Esc)
        | (KeyModifiers::NONE, KeyCode::Char('q')) => Action::Quit,

        // Orbit: a/d = rotate around horizontally (theta), w/s = tilt up/down (phi)
        (KeyModifiers::NONE, KeyCode::Char('a')) => {
            camera.orbit(-5.0_f64.to_radians(), 0.0);
            Action::Redraw
        }
        (KeyModifiers::NONE, KeyCode::Char('d')) => {
            camera.orbit(5.0_f64.to_radians(), 0.0);
            Action::Redraw
        }
        (KeyModifiers::NONE, KeyCode::Char('w')) => {
            camera.orbit(0.0, 5.0_f64.to_radians());
            Action::Redraw
        }
        (KeyModifiers::NONE, KeyCode::Char('s')) => {
            camera.orbit(0.0, -5.0_f64.to_radians());
            Action::Redraw
        }

        // Fine orbit: z/x = fine theta
        (KeyModifiers::NONE, KeyCode::Char('z')) => {
            camera.orbit(-1.0_f64.to_radians(), 0.0);
            Action::Redraw
        }
        (KeyModifiers::NONE, KeyCode::Char('e')) => {
            camera.orbit(1.0_f64.to_radians(), 0.0);
            Action::Redraw
        }

        // Zoom: +/- keys
        (KeyModifiers::NONE, KeyCode::Char('+')) | (KeyModifiers::NONE, KeyCode::Char('=')) => {
            camera.zoom(2.0);
            Action::Redraw
        }
        (KeyModifiers::NONE, KeyCode::Char('-')) => {
            camera.zoom(-2.0);
            Action::Redraw
        }

        // Reset camera
        (KeyModifiers::NONE, KeyCode::Char('r')) => Action::Reset,

        // Pan: Shift+WASD (viewport-space offset in dot units)
        (KeyModifiers::SHIFT, KeyCode::Char('W'))
        | (KeyModifiers::NONE, KeyCode::Char('W')) => {
            viewport.pan(0.0, 10.0);
            Action::Redraw
        }
        (KeyModifiers::SHIFT, KeyCode::Char('S'))
        | (KeyModifiers::NONE, KeyCode::Char('S')) => {
            viewport.pan(0.0, -10.0);
            Action::Redraw
        }
        (KeyModifiers::SHIFT, KeyCode::Char('A'))
        | (KeyModifiers::NONE, KeyCode::Char('A')) => {
            viewport.pan(10.0, 0.0);
            Action::Redraw
        }
        (KeyModifiers::SHIFT, KeyCode::Char('D'))
        | (KeyModifiers::NONE, KeyCode::Char('D')) => {
            viewport.pan(-10.0, 0.0);
            Action::Redraw
        }

        _ => Action::None,
    }
}

// ── Mouse state and handler ───────────────────────────────────────────────────

/// State needed to classify mouse press/move/release gestures.
#[derive(Debug, Clone, Default)]
pub struct MouseState {
    /// The left button is currently pressed.
    pub left_down: bool,
    /// Dot-unit position where the left button was pressed.
    pub press_x: f64,
    pub press_y: f64,
    /// Last known mouse position in dot units (col*2, row*2).
    pub last_x: f64,
    pub last_y: f64,
}

/// Map a crossterm mouse event to camera mutations.
///
/// - Plain left-drag: orbit the camera (Ctrl for 10× snipe).
/// - Alt+left-drag: pan the target ("push the paper").
/// - Scroll up: zoom in. Scroll down: zoom out.
/// - Click (press→release below the threshold): atom pick (no-op for now).
///
/// Coordinates arrive in terminal cells; we convert to dot units (×2)
/// to match the viewport. The sensitivity functions in
/// `chemrust_vis_core::mouse_nav` implement the mouse-nav sensitivity model.
pub fn handle_mouse_event(
    mouse: &MouseEvent,
    camera: &mut Camera,
    viewport: &Viewport,
    state: &mut MouseState,
) -> Action {
    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            state.left_down = true;
            let x = mouse.column as f64 * 2.0;
            let y = mouse.row as f64 * 2.0;
            state.press_x = x;
            state.press_y = y;
            state.last_x = x;
            state.last_y = y;
            Action::None
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            if !state.left_down {
                return Action::None;
            }
            let x = mouse.column as f64 * 2.0;
            let y = mouse.row as f64 * 2.0;
            let dx = x - state.last_x;
            let dy = y - state.last_y;
            if dx != 0.0 || dy != 0.0 {
                let radius = camera.radius();
                let vp_h = viewport.height;
                let ctrl = mouse.modifiers.contains(KeyModifiers::CONTROL);
                let alt = mouse.modifiers.contains(KeyModifiers::ALT);
                if alt {
                    let (wx, wy) =
                        chemrust_vis_core::mouse_nav::alt_drag_pan_deltas(dx, dy, radius, vp_h);
                    camera.pan(wx, wy);
                } else {
                    let (d_theta, d_phi) =
                        chemrust_vis_core::mouse_nav::orbit_deltas_snipe(
                            dx, dy, radius, ctrl,
                        );
                    camera.orbit(d_theta, d_phi);
                }
                state.last_x = x;
                state.last_y = y;
            }
            Action::Redraw
        }
        MouseEventKind::Up(MouseButton::Left) => {
            state.left_down = false;
            let x = mouse.column as f64 * 2.0;
            let y = mouse.row as f64 * 2.0;
            let total_dx = x - state.press_x;
            let total_dy = y - state.press_y;
            let is_click = chemrust_vis_core::mouse_nav::is_click(
                total_dx,
                total_dy,
                chemrust_vis_core::mouse_nav::CLICK_THRESHOLD,
            );
            if is_click {
                // TODO: ray-cast atom pick at (x, y) when selection is
                // implemented.
            }
            Action::None
        }
        MouseEventKind::ScrollUp => {
            let dr =
                chemrust_vis_core::mouse_nav::zoom_delta(-1.0, camera.radius());
            camera.zoom(dr);
            Action::Redraw
        }
        MouseEventKind::ScrollDown => {
            let dr =
                chemrust_vis_core::mouse_nav::zoom_delta(1.0, camera.radius());
            camera.zoom(dr);
            Action::Redraw
        }
        _ => Action::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn camera() -> Camera {
        Camera::from_target([0.0; 3], 10.0)
    }

    /// Regression for the "stuck at a specific angle" report. The old phi
    /// clamp pinned the camera at 0.057 degrees and 179.94 degrees, and
    /// further presses in the same direction did nothing. A full sweep
    /// now rolls through both poles and every keypress moves the camera.
    #[test]
    fn w_s_sweeps_through_both_poles_without_pin() {
        let mut vp = Viewport::new(160.0, 96.0);
        let mut cam = camera();
        let mut prev = cam.position();
        for _ in 0..80 {
            let action = handle_key_event(key(KeyCode::Char('w')), &mut cam, &mut vp);
            assert!(matches!(action, Action::Redraw));
            let pos = cam.position();
            assert!(
                (pos - prev).norm() > 1e-6,
                "w sweep pinned the camera at phi={}",
                cam.phi()
            );
            prev = pos;
        }
        let mut cam = camera();
        let mut prev = cam.position();
        for _ in 0..80 {
            handle_key_event(key(KeyCode::Char('s')), &mut cam, &mut vp);
            let pos = cam.position();
            assert!(
                (pos - prev).norm() > 1e-6,
                "s sweep pinned the camera at phi={}",
                cam.phi()
            );
            prev = pos;
        }
    }

    /// Theta has no bound: fine yaw steps move the camera every time.
    #[test]
    fn a_d_sweeps_yaw_without_pin() {
        let mut vp = Viewport::new(160.0, 96.0);
        let mut cam = camera();
        let mut prev = cam.position();
        for _ in 0..40 {
            handle_key_event(key(KeyCode::Char('a')), &mut cam, &mut vp);
            let pos = cam.position();
            assert!(
                (pos - prev).norm() > 1e-6,
                "a sweep pinned the camera at theta={}",
                cam.theta()
            );
            prev = pos;
        }
        let mut cam = camera();
        let mut prev = cam.position();
        for _ in 0..40 {
            handle_key_event(key(KeyCode::Char('d')), &mut cam, &mut vp);
            let pos = cam.position();
            assert!(
                (pos - prev).norm() > 1e-6,
                "d sweep pinned the camera at theta={}",
                cam.theta()
            );
            prev = pos;
        }
    }
}
