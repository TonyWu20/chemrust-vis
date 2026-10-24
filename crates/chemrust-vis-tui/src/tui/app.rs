//! TUI application: ratatui event loop, layout, and rendering.
//!
//! Two scene renderers exist:
//! - Half-block characters (universal fallback, all terminals).
//! - Kitty graphics protocol (pixel-level; kitty, WezTerm, Ghostty).
//!   The scene is transmitted as a pixel image placed under the text
//!   layer, and re-transmitted only when the camera, viewport, or
//!   terminal size changes.

use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use chemrust_vis_core::camera::Camera;
use chemrust_vis_core::scene::Scene;
use chemrust_vis_core::viewport::Viewport;
use crossterm::event::{self, Event};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    Terminal,
};

use super::input::{self, Action, NavState};
use super::kitty::{self, KittyRenderer};
use super::mcp_server::SharedState;
use super::widgets::axis_indicator::AxisIndicator;
use super::widgets::scene_view::SceneWidget;
use super::widgets::status_bar::StatusBar;

/// The main application state.
pub struct App {
    scene: Scene,
    camera: Camera,
    /// Initial camera target (for reset).
    initial_target: [f64; 3],
    /// Initial camera radius (for reset).
    initial_radius: f64,
    viewport: Viewport,
    file_path: PathBuf,
    /// Count of non-periodic atoms for display.
    num_atoms: usize,
    running: bool,
    /// Shared state for the background MCP server.
    mcp_state: Option<Arc<Mutex<SharedState>>>,
    /// Mouse gesture state for drag classification.
    mouse_state: input::MouseState,
    /// Navigation toggle state (snipe mode).
    nav_state: NavState,
    /// Pixel renderer for the scene area. `None` = half-block chars.
    kitty: Option<KittyRenderer>,
    /// True when the kitty image data must be re-sent: the camera,
    /// viewport, or terminal size changed since the last transmit.
    kitty_dirty: bool,
    /// Placement size (cols, scene_rows) of the last transmitted image.
    /// (0, 0) means no image was sent yet.
    kitty_placement: (u16, u16),
    /// Vertical pixels per terminal row in the last transmitted kitty
    /// frame; the mouse `y_scale` of the pixel renderer.
    kitty_px_per_row: f64,
    /// Last transmitted kitty frame size in pixels; shown in the
    /// status bar so the active grid is visible.
    kitty_frame_px: Option<(u32, u32)>,
    /// Force the full-canvas kitty path (no content-box delta, no
    /// black-tile clear): one image, in-place data replace.
    kitty_full_only: bool,
    /// Opt into the content-box delta (crop) path. Off by default:
    /// the full-canvas path is the default because terminals that
    /// do not evict old images (Ghostty) accumulate delta images.
    kitty_delta: bool,
}

impl App {
    /// Create a new App from a loaded scene and file path.
    pub fn new(scene: Scene, file_path: PathBuf) -> Self {
        // Use the cell's geometric center for centering, not the atom cloud
        // (which includes periodic replicas that shift the centroid).
        let center = scene.center_for_view();

        let (min, max) = bounding_box_corners(&scene);
        let diag =
            ((max[0] - min[0]).powi(2) + (max[1] - min[1]).powi(2) + (max[2] - min[2]).powi(2))
                .sqrt();
        let radius = (diag * 0.6).max(5.0);

        let num_atoms = scene.atoms.iter().filter(|a| !a.is_periodic_image).count();
        let camera = Camera::from_target(center, radius);

        App {
            scene,
            camera,
            initial_target: center,
            initial_radius: radius,
            viewport: Viewport::new(160.0, 96.0),
            file_path,
            num_atoms,
            running: true,
            mcp_state: None,
            mouse_state: input::MouseState::default(),
            nav_state: NavState::default(),
            kitty: None,
            kitty_dirty: false,
            kitty_placement: (0, 0),
            kitty_px_per_row: 4.0,
            kitty_frame_px: None,
            kitty_full_only: false,
            kitty_delta: false,
        }
    }

    /// Attach a shared state for the MCP server to read camera/viewport.
    pub fn set_mcp_state(&mut self, state: Arc<Mutex<SharedState>>) {
        self.mcp_state = Some(state);
    }

    /// Switch the scene area to the kitty pixel renderer.
    /// Enable the pixel renderer over `medium`.
    pub fn enable_kitty(&mut self, medium: kitty::Medium) {
        self.kitty = Some(KittyRenderer::with_medium(medium));
        self.kitty_dirty = true;
    }

    /// Force the full-canvas transmission path: every frame sends the
    /// whole canvas as one image and replaces the data in place. No
    /// content-box crop, no black-tile clear, no moving placements.
    /// The most conservative (and most robust) kitty mode.
    pub fn enable_kitty_full_only(&mut self) {
        self.kitty_full_only = true;
    }

    /// Enable the content-box delta (crop) path. Off by default; the
    /// full-canvas path is the default.
    pub fn enable_kitty_delta(&mut self) {
        self.kitty_delta = true;
    }

    /// Run the main event loop.
    pub fn run(
        &mut self,
        terminal: &mut Terminal<impl ratatui::backend::Backend + io::Write>,
    ) -> io::Result<()> {
        while self.running {
            terminal.draw(|f| {
                self.draw(f);
            })?;

            self.render_kitty_image(terminal)?;

            match event::read()? {
                Event::Key(key) => {
                    let action = input::handle_key_event(
                        key,
                        &mut self.camera,
                        &mut self.viewport,
                        &mut self.nav_state,
                    );
                    match action {
                        Action::Quit => self.running = false,
                        Action::Redraw | Action::Reset => self.mark_kitty_dirty(),
                        Action::None => {}
                    }
                    if action == Action::Reset {
                        self.camera = Camera::from_target(self.initial_target, self.initial_radius);
                        self.viewport.reset();
                    }
                }
                Event::Resize(cols, rows) => {
                    // Block rendering: 2 dots per column, 2 dots per row.
                    // The kitty renderer resizes itself from terminal
                    // size on the next frame, so it only needs a
                    // dirty flag.
                    self.viewport = Viewport::new(cols as f64 * 2.0, rows as f64 * 2.0);
                    self.mark_kitty_dirty();
                }
                Event::Mouse(mouse) => {
                    // Vertical viewport units per terminal row: the
                    // half-block dot grid is 2 dots per row; the kitty
                    // pixel grid uses the row height of the last
                    // transmitted frame.
                    let y_scale = if self.kitty.is_some() {
                        self.kitty_px_per_row
                    } else {
                        2.0
                    };
                    let action = input::handle_mouse_event(
                        &mouse,
                        &mut self.camera,
                        &self.viewport,
                        &self.nav_state,
                        &mut self.mouse_state,
                        y_scale,
                    );
                    if action == Action::Redraw {
                        self.mark_kitty_dirty();
                    }
                }
                // The alternate screen (and its images) may be cleared
                // when the terminal loses focus. Re-send on the next
                // frame either way.
                Event::FocusGained | Event::FocusLost => self.mark_kitty_dirty(),
                _ => {}
            }
        }

        self.clear_kitty_image(terminal)?;
        Ok(())
    }

    /// Mark the kitty image data as stale (camera/viewport changed).
    fn mark_kitty_dirty(&mut self) {
        if self.kitty.is_some() {
            self.kitty_dirty = true;
        }
    }

    /// Send a fresh kitty image after the text pass, when needed.
    ///
    /// The image covers the scene area (all rows except the status
    /// bar). The status bar and axis indicator are text written above
    /// the image (z-index -1), so they stay readable.
    fn render_kitty_image(
        &mut self,
        terminal: &mut Terminal<impl ratatui::backend::Backend + io::Write>,
    ) -> io::Result<()> {
        if self.kitty.is_none() {
            return Ok(());
        }
        let size = terminal.size()?;
        let cols = size.width;
        let rows = size.height;
        let scene_rows = rows.saturating_sub(1);
        if cols < 8 || scene_rows < 4 {
            return Ok(());
        }
        // Pixel size: the terminal's native text-area pixels when the
        // pty reports them, otherwise the 2x4 px-per-cell grid.
        let native = kitty::terminal_pixel_size();
        let (px_w, px_h) = kitty::frame_pixel_size(cols, rows, scene_rows, native);
        self.viewport.width = px_w as f64;
        self.viewport.height = px_h as f64;
        self.kitty_px_per_row = px_h as f64 / scene_rows as f64;
        self.kitty_frame_px = Some((px_w as u32, px_h as u32));
        // A placement or pixel-size change remaps the cell grid: the
        // stored placements refer to the old mapping, so delete them
        // and restart the region state.
        let mapping_changed = self.kitty_placement != (0, 0)
            && (self.kitty_placement != (cols, scene_rows)
                || self.kitty_frame_px != Some((px_w as u32, px_h as u32)));
        if !self.kitty_dirty && !mapping_changed {
            return Ok(());
        }
        let cmds = self.viewport.render(&self.scene, &self.camera);
        let seq = {
            let renderer = self.kitty.as_mut().expect("kitty renderer");
            if mapping_changed {
                renderer.reset_placement_state();
            }
            let crop_ok = native.is_some() && self.kitty_delta && !self.kitty_full_only;
            renderer.frame(&cmds, px_w, px_h, cols, scene_rows, crop_ok)
        };
        let backend = terminal.backend_mut();
        if mapping_changed {
            backend.write_all(&kitty::encode_delete_all())?;
        }
        if !seq.is_empty() {
            crossterm::execute!(backend, crossterm::cursor::MoveTo(0, 0))?;
            backend.write_all(&seq)?;
            // Restore the cursor position the next text pass expects.
            crossterm::execute!(backend, crossterm::cursor::MoveTo(0, 0))?;
        }
        std::io::Write::flush(backend)?;
        self.kitty_dirty = false;
        self.kitty_placement = (cols, scene_rows);
        Ok(())
    }

    /// Delete the placed image before leaving the alternate screen.
    fn clear_kitty_image(
        &mut self,
        terminal: &mut Terminal<impl ratatui::backend::Backend + io::Write>,
    ) -> io::Result<()> {
        if self.kitty.is_some() && self.kitty_placement != (0, 0) {
            let backend = terminal.backend_mut();
            backend.write_all(&kitty::encode_delete_all())?;
            std::io::Write::flush(backend)?;
        }
        Ok(())
    }

    /// Draw a single frame (text layer).
    fn draw(&mut self, f: &mut ratatui::Frame) {
        let area = f.area();
        let scene_rows = if area.height > 1 {
            area.height - 1
        } else {
            area.height
        };
        self.viewport
            .set_zoom_from_radius(self.camera.radius(), self.initial_radius);

        // Sync camera state to MCP server
        if let Some(ref mcp) = self.mcp_state {
            if let Ok(mut s) = mcp.lock() {
                s.theta = self.camera.theta();
                s.phi = self.camera.phi();
                s.radius = self.camera.radius();
            }
        }

        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(f.area());

        if self.kitty.is_some() {
            // Leave the scene area empty. The pixel image is placed
            // below it after this text pass (render_kitty_image).
            // First pass: compute the frame size now so the status
            // bar shows it without waiting for an event.
            if self.kitty_frame_px.is_none() && area.width >= 8 && scene_rows >= 4 {
                let rows = scene_rows + 1;
                let native = kitty::terminal_pixel_size();
                let (px_w, px_h) = kitty::frame_pixel_size(area.width, rows, scene_rows, native);
                self.kitty_frame_px = Some((px_w as u32, px_h as u32));
            }
        } else {
            // Half-block: 2x2 dots per character
            self.viewport.width = area.width as f64 * 2.0;
            self.viewport.height = scene_rows as f64 * 2.0;
            let draw_cmds = self.viewport.render(&self.scene, &self.camera);
            f.render_widget(SceneWidget::new(&draw_cmds), layout[0]);
        }

        // Axis indicator: bottom-left corner of scene area, 10x6 chars.
        // Text with a transparent background: renders above the kitty
        // image (z-index -1).
        let axis_area = Rect {
            x: 2,
            y: layout[0].height.saturating_sub(8),
            width: 10.min(layout[0].width.saturating_sub(2)),
            height: 6.min(layout[0].height),
        };
        f.render_widget(AxisIndicator::new(&self.camera), axis_area);

        // Status bar
        f.render_widget(
            StatusBar {
                file_name: self
                    .file_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown")
                    .to_string(),
                atom_count: self.num_atoms,
                theta_deg: self.camera.theta().to_degrees().rem_euclid(360.0),
                phi_deg: self.camera.phi().to_degrees().rem_euclid(360.0),
                radius: self.camera.radius(),
                renderer: if self.kitty.is_some() {
                    "kitty"
                } else {
                    "half-block"
                },
                kitty_px: self.kitty_frame_px,
                kitty_medium: self.kitty.as_ref().map(|r| r.medium_tag()),
                sniped: self.nav_state.sniped,
            },
            layout[1],
        );
    }
}

/// Compute the axis-aligned bounding box of original (non-periodic) atoms.
fn bounding_box_corners(scene: &Scene) -> ([f64; 3], [f64; 3]) {
    let originals: Vec<_> = scene
        .atoms
        .iter()
        .filter(|a| !a.is_periodic_image)
        .collect();
    if originals.is_empty() {
        return ([0.0; 3], [0.0; 3]);
    }
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    for atom in originals {
        for i in 0..3 {
            min[i] = min[i].min(atom.position[i]);
            max[i] = max[i].max(atom.position[i]);
        }
    }
    (min, max)
}
