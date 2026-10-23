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

/// Navigation state toggled by keys.
#[derive(Debug, Clone, Default)]
pub struct NavState {
    /// Snipe mode: orbit and move-zoom run at 1/10 sensitivity.
    pub sniped: bool,
}

/// Map a key event to camera/viewport mutations. Returns the action to take.
pub fn handle_key_event(
    key: KeyEvent,
    camera: &mut Camera,
    viewport: &mut Viewport,
    nav: &mut NavState,
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

        // Snipe toggle: f switches orbit and move-zoom to 1/10
        // sensitivity for fine control.
        (KeyModifiers::NONE, KeyCode::Char('f')) => {
            nav.sniped = !nav.sniped;
            Action::Redraw
        }

        // Zoom: +/- keys (and the mouse wheel). '+' shrinks the
        // radius (zoom in), '-' grows it (zoom out), matching the
        // scroll wheel: up = in, down = out.
        //
        // '=' is deliberately not bound. Terminals that implement the
        // kitty graphics protocol answer requests with `i=<id>;OK` on
        // stdin, and the crossterm event reader surfaces the `=`
        // character of such a response as a key event. Binding it
        // would let a terminal response mutate the camera; the
        // renderer suppresses responses with q=1, and this is the
        // second line of defense (see ADR-0002).
        (KeyModifiers::NONE, KeyCode::Char('+')) => {
            camera.zoom(-2.0);
            Action::Redraw
        }
        (KeyModifiers::NONE, KeyCode::Char('-')) => {
            camera.zoom(2.0);
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
    /// Last known pointer position in viewport units (col*2, row*y_scale).
    pub last_x: f64,
    pub last_y: f64,
}

/// Map a crossterm mouse event to camera mutations.
///
/// Keyboard modifiers set the camera action; the left button is optional.
/// This makes trackball use natural: hold a key, roll the ball.
///
/// - Shift (move or left-drag): orbit the camera (1/10 in snipe mode).
/// - Alt (move or left-drag): pan the target ("push the paper").
/// - Ctrl (move or left-drag): zoom on the vertical axis (1/10 in snipe mode).
/// - Plain left-drag (no modifier): orbit (legacy mouse gesture).
/// - Scroll up: zoom in. Scroll down: zoom out.
/// - Move without modifier: no camera action (reserved for hover).
/// - Click (press→release below the threshold): atom pick (no-op for now).
///
/// Two modifiers compose: their actions apply together.
///
/// `y_scale` is viewport units per terminal row: 2.0 for half-block
/// rendering (dots) and 4.0 for the kitty renderer (pixels). The
/// horizontal axis is 2.0 units per column in both renderers.
pub fn handle_mouse_event(
    mouse: &MouseEvent,
    camera: &mut Camera,
    viewport: &Viewport,
    nav: &NavState,
    state: &mut MouseState,
    y_scale: f64,
) -> Action {
    let x = mouse.column as f64 * 2.0;
    let y = mouse.row as f64 * y_scale;
    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            state.left_down = true;
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
            let dx = x - state.last_x;
            let dy = y - state.last_y;
            let action = apply_pointer_action(
                dx,
                dy,
                mouse.modifiers,
                true,
                camera,
                viewport,
                nav,
            );
            state.last_x = x;
            state.last_y = y;
            action
        }
        // Pointer moved with no button held: the trackball channel.
        MouseEventKind::Moved => {
            let dx = x - state.last_x;
            let dy = y - state.last_y;
            let action = apply_pointer_action(
                dx,
                dy,
                mouse.modifiers,
                false,
                camera,
                viewport,
                nav,
            );
            state.last_x = x;
            state.last_y = y;
            action
        }
        MouseEventKind::Up(MouseButton::Left) => {
            state.left_down = false;
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

/// Apply the modifier-key camera actions for one pointer delta.
///
/// Shift orbits, Alt pans, Ctrl zooms on the vertical axis. With the
/// left button down and no modifier, a plain drag orbits (legacy).
/// With no button and no modifier, the delta is a no-op.
fn apply_pointer_action(
    dx: f64,
    dy: f64,
    modifiers: KeyModifiers,
    button_down: bool,
    camera: &mut Camera,
    viewport: &Viewport,
    nav: &NavState,
) -> Action {
    if dx == 0.0 && dy == 0.0 {
        return Action::None;
    }
    let shift = modifiers.contains(KeyModifiers::SHIFT);
    let alt = modifiers.contains(KeyModifiers::ALT);
    let ctrl = modifiers.contains(KeyModifiers::CONTROL);
    let plain_drag = !shift && !alt && !ctrl && button_down;
    let orbit = shift || plain_drag;
    if !orbit && !alt && !ctrl {
        return Action::None;
    }
    let radius = camera.radius();
    let vp_h = viewport.height;
    if orbit {
        let (d_theta, d_phi) =
            chemrust_vis_core::mouse_nav::orbit_deltas_snipe(dx, dy, radius, nav.sniped);
        camera.orbit(d_theta, d_phi);
    }
    if alt {
        let (wx, wy) =
            chemrust_vis_core::mouse_nav::alt_drag_pan_deltas(dx, dy, radius, vp_h);
        camera.pan(wx, wy);
    }
    if ctrl {
        let dr = chemrust_vis_core::mouse_nav::move_zoom_delta(dy, radius, nav.sniped);
        camera.zoom(dr);
    }
    Action::Redraw
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

    /// Build a mouse event with the given kind, position, and modifiers.
    fn ev(kind: MouseEventKind, col: u16, row: u16, modifiers: KeyModifiers) -> MouseEvent {
        MouseEvent { kind, column: col, row, modifiers }
    }

    /// Regression for the "stuck at a specific angle" report. The old phi
    /// clamp pinned the camera at 0.057 degrees and 179.94 degrees, and
    /// further presses in the same direction did nothing. A full sweep
    /// now rolls through both poles and every keypress moves the camera.
    #[test]
    fn w_s_sweeps_through_both_poles_without_pin() {
        let mut vp = Viewport::new(160.0, 96.0);
        let mut cam = camera();
        let mut nav = NavState::default();
        let mut prev = cam.position();
        for _ in 0..80 {
            let action = handle_key_event(key(KeyCode::Char('w')), &mut cam, &mut vp, &mut nav);
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
        let mut nav = NavState::default();
        let mut prev = cam.position();
        for _ in 0..80 {
            handle_key_event(key(KeyCode::Char('s')), &mut cam, &mut vp, &mut nav);
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
        let mut nav = NavState::default();
        let mut prev = cam.position();
        for _ in 0..40 {
            handle_key_event(key(KeyCode::Char('a')), &mut cam, &mut vp, &mut nav);
            let pos = cam.position();
            assert!(
                (pos - prev).norm() > 1e-6,
                "a sweep pinned the camera at theta={}",
                cam.theta()
            );
            prev = pos;
        }
        let mut cam = camera();
        let mut nav = NavState::default();
        let mut prev = cam.position();
        for _ in 0..40 {
            handle_key_event(key(KeyCode::Char('d')), &mut cam, &mut vp, &mut nav);
            let pos = cam.position();
            assert!(
                (pos - prev).norm() > 1e-6,
                "d sweep pinned the camera at theta={}",
                cam.theta()
            );
            prev = pos;
        }
    }

    /// f toggles snipe mode on and off.
    #[test]
    fn f_key_toggles_snipe_mode() {
        let mut nav = NavState::default();
        let mut cam = camera();
        let mut vp = Viewport::new(160.0, 96.0);
        assert!(!nav.sniped);
        let action = handle_key_event(key(KeyCode::Char('f')), &mut cam, &mut vp, &mut nav);
        assert!(matches!(action, Action::Redraw));
        assert!(nav.sniped);
        handle_key_event(key(KeyCode::Char('f')), &mut cam, &mut vp, &mut nav);
        assert!(!nav.sniped);
    }

    /// A pointer move with no modifier and no button is a no-op.
    #[test]
    fn moved_without_modifier_is_noop() {
        let mut cam = camera();
        let mut nav = NavState::default();
        let mut state = MouseState::default();
        let vp = Viewport::new(160.0, 96.0);
        let prime = ev(MouseEventKind::Moved, 0, 0, KeyModifiers::NONE);
        handle_mouse_event(&prime, &mut cam, &vp, &nav, &mut state, 2.0);
        let (theta, phi, radius) = (cam.theta(), cam.phi(), cam.radius());
        let target = cam.target();
        let m = ev(MouseEventKind::Moved, 5, 5, KeyModifiers::NONE);
        let action = handle_mouse_event(&m, &mut cam, &vp, &nav, &mut state, 2.0);
        assert!(matches!(action, Action::None));
        assert!((cam.theta() - theta).abs() < 1e-15);
        assert!((cam.phi() - phi).abs() < 1e-15);
        assert!((cam.radius() - radius).abs() < 1e-15);
        assert!((cam.target() - target).norm() < 1e-15);
    }

    /// Shift + move (no button) orbits the camera.
    #[test]
    fn moved_with_shift_orbits() {
        let mut cam = camera();
        let mut nav = NavState::default();
        let mut state = MouseState::default();
        let vp = Viewport::new(160.0, 96.0);
        let prime = ev(MouseEventKind::Moved, 0, 0, KeyModifiers::SHIFT);
        handle_mouse_event(&prime, &mut cam, &vp, &nav, &mut state, 2.0);
        let theta0 = cam.theta();
        let phi0 = cam.phi();
        let m = ev(MouseEventKind::Moved, 5, 2, KeyModifiers::SHIFT);
        // dx = 10, dy = 4 viewport units at radius 10.
        let (d_theta, d_phi) = chemrust_vis_core::mouse_nav::orbit_deltas(10.0, 4.0, 10.0);
        let action = handle_mouse_event(&m, &mut cam, &vp, &nav, &mut state, 2.0);
        assert!(matches!(action, Action::Redraw));
        assert!((cam.theta() - (theta0 + d_theta)).abs() < 1e-12);
        assert!((cam.phi() - (phi0 + d_phi)).abs() < 1e-12);
    }

    /// Alt + move pans the target: pushing left reveals the right.
    #[test]
    fn moved_with_alt_pans() {
        let mut cam = camera();
        let mut nav = NavState::default();
        let mut state = MouseState::default();
        let vp = Viewport::new(160.0, 96.0);
        let prime = ev(MouseEventKind::Moved, 0, 0, KeyModifiers::ALT);
        handle_mouse_event(&prime, &mut cam, &vp, &nav, &mut state, 2.0);
        let t0 = cam.target();
        let m = ev(MouseEventKind::Moved, 5, 0, KeyModifiers::ALT);
        // dx = 10 viewport units at radius 10, viewport height 96.
        let (wx, wy) =
            chemrust_vis_core::mouse_nav::alt_drag_pan_deltas(10.0, 0.0, 10.0, 96.0);
        let action = handle_mouse_event(&m, &mut cam, &vp, &nav, &mut state, 2.0);
        assert!(matches!(action, Action::Redraw));
        let d = (cam.target() - t0).norm();
        let expected = (wx * wx + wy * wy).sqrt();
        assert!((d - expected).abs() < 1e-12);
    }

    /// Ctrl + move zooms: down-travel grows the radius.
    #[test]
    fn moved_with_ctrl_zooms() {
        let mut cam = camera();
        let mut nav = NavState::default();
        let mut state = MouseState::default();
        let vp = Viewport::new(160.0, 96.0);
        let prime = ev(MouseEventKind::Moved, 0, 0, KeyModifiers::CONTROL);
        handle_mouse_event(&prime, &mut cam, &vp, &nav, &mut state, 2.0);
        let r0 = cam.radius();
        let m = ev(MouseEventKind::Moved, 0, 1, KeyModifiers::CONTROL);
        // dy = 2 viewport units at radius 10.
        let dr = chemrust_vis_core::mouse_nav::move_zoom_delta(2.0, 10.0, false);
        let action = handle_mouse_event(&m, &mut cam, &vp, &nav, &mut state, 2.0);
        assert!(matches!(action, Action::Redraw));
        assert!((cam.radius() - (r0 + dr)).abs() < 1e-12);
    }

    /// Snipe mode divides the shift-orbit delta by ten.
    #[test]
    fn snipe_mode_divides_orbit_by_ten() {
        fn orbit_step(sniped: bool) -> f64 {
            let mut cam = camera();
            let mut nav = NavState { sniped };
            let mut state = MouseState::default();
            let vp = Viewport::new(160.0, 96.0);
            let theta0 = cam.theta();
            let prime = ev(MouseEventKind::Moved, 0, 0, KeyModifiers::SHIFT);
            handle_mouse_event(&prime, &mut cam, &vp, &nav, &mut state, 2.0);
            let m = ev(MouseEventKind::Moved, 5, 0, KeyModifiers::SHIFT);
            handle_mouse_event(&m, &mut cam, &vp, &nav, &mut state, 2.0);
            cam.theta() - theta0
        }
        let free = orbit_step(false);
        let sniped = orbit_step(true);
        assert!((sniped - free / 10.0).abs() < 1e-15);
    }

    /// The kitty y-scale (4 px/row) doubles the vertical effect of the
    /// same row movement compared with half-block (2 dots/row).
    #[test]
    fn y_scale_doubles_vertical_effect() {
        fn phi_delta(y_scale: f64) -> f64 {
            let mut cam = camera();
            let mut nav = NavState::default();
            let mut state = MouseState::default();
            let vp = Viewport::new(160.0, 96.0);
            let prime = ev(MouseEventKind::Moved, 0, 0, KeyModifiers::SHIFT);
            handle_mouse_event(&prime, &mut cam, &vp, &nav, &mut state, y_scale);
            let phi0 = cam.phi();
            let m = ev(MouseEventKind::Moved, 0, 1, KeyModifiers::SHIFT);
            handle_mouse_event(&m, &mut cam, &vp, &nav, &mut state, y_scale);
            cam.phi() - phi0
        }
        let half_block = phi_delta(2.0);
        let kitty = phi_delta(4.0);
        assert!((kitty - 2.0 * half_block).abs() < 1e-12);
    }

    /// A plain left-drag still orbits (legacy mouse gesture).
    #[test]
    fn plain_drag_orbits() {
        let mut cam = camera();
        let mut nav = NavState::default();
        let mut state = MouseState::default();
        let vp = Viewport::new(160.0, 96.0);
        let down = ev(MouseEventKind::Down(MouseButton::Left), 0, 0, KeyModifiers::NONE);
        handle_mouse_event(&down, &mut cam, &vp, &nav, &mut state, 2.0);
        let theta0 = cam.theta();
        let drag = ev(MouseEventKind::Drag(MouseButton::Left), 5, 0, KeyModifiers::NONE);
        let (d_theta, _) = chemrust_vis_core::mouse_nav::orbit_deltas(10.0, 0.0, 10.0);
        let action = handle_mouse_event(&drag, &mut cam, &vp, &nav, &mut state, 2.0);
        assert!(matches!(action, Action::Redraw));
        assert!((cam.theta() - (theta0 + d_theta)).abs() < 1e-12);
    }

    /// Ctrl-drag zooms on the vertical axis, matching Ctrl-move.
    #[test]
    fn ctrl_drag_zooms_like_ctrl_move() {
        let mut cam = camera();
        let mut nav = NavState::default();
        let mut state = MouseState::default();
        let vp = Viewport::new(160.0, 96.0);
        let down = ev(MouseEventKind::Down(MouseButton::Left), 0, 0, KeyModifiers::CONTROL);
        handle_mouse_event(&down, &mut cam, &vp, &nav, &mut state, 2.0);
        let r0 = cam.radius();
        let drag = ev(MouseEventKind::Drag(MouseButton::Left), 0, 1, KeyModifiers::CONTROL);
        let dr = chemrust_vis_core::mouse_nav::move_zoom_delta(2.0, 10.0, false);
        let action = handle_mouse_event(&drag, &mut cam, &vp, &nav, &mut state, 2.0);
        assert!(matches!(action, Action::Redraw));
        assert!((cam.radius() - (r0 + dr)).abs() < 1e-12);
    }
}
