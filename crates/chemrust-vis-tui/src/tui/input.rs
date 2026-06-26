//! Keyboard input handling: maps key events to camera and viewport actions.

use chemrust_vis_core::camera::Camera;
use chemrust_vis_core::viewport::Viewport;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

/// Actions the app can take in response to input.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Action {
    Quit,
    Redraw,
    Reset,
    None,
}

/// Map a key event to camera/viewport mutations. Returns the action to take.
pub fn handle_key_event(key: KeyEvent, camera: &mut Camera, viewport: &mut Viewport) -> Action {
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
