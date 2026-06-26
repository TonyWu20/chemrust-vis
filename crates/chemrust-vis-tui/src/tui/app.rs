//! TUI application: ratatui event loop, layout, and rendering.

use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use chemrust_vis_core::camera::Camera;
use chemrust_vis_core::scene::Scene;
use chemrust_vis_core::viewport::Viewport;
use crossterm::event::{self, Event};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    Terminal,
};

use super::input::{self, Action};
use super::mcp_server::SharedState;
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
}

impl App {
    /// Create a new App from a loaded scene and file path.
    pub fn new(scene: Scene, file_path: PathBuf) -> Self {
        // Use the cell's geometric center for centering, not the atom cloud
        // (which includes periodic replicas that shift the centroid).
        let center = scene.center_for_view();

        let (min, max) = bounding_box_corners(&scene);
        let diag = ((max[0] - min[0]).powi(2)
            + (max[1] - min[1]).powi(2)
            + (max[2] - min[2]).powi(2))
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
        }
    }

    /// Attach a shared state for the MCP server to read camera/viewport.
    pub fn set_mcp_state(&mut self, state: Arc<Mutex<SharedState>>) {
        self.mcp_state = Some(state);
    }

    /// Run the main event loop.
    pub fn run(
        &mut self,
        terminal: &mut Terminal<impl ratatui::backend::Backend>,
    ) -> io::Result<()> {
        while self.running {
            terminal.draw(|f| {
                self.draw(f);
            })?;

            match event::read()? {
                Event::Key(key) => {
                    let action = input::handle_key_event(key, &mut self.camera, &mut self.viewport);
                    match action {
                        Action::Quit => self.running = false,
                        Action::Reset => {
                            self.camera =
                                Camera::from_target(self.initial_target, self.initial_radius);
                            self.viewport.reset();
                        }
                        _ => {}
                    }
                }
                Event::Resize(cols, rows) => {
                    // Block rendering: 2 dots per column, 2 dots per row
                    self.viewport = Viewport::new(cols as f64 * 2.0, rows as f64 * 2.0);
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Draw a single frame.
    fn draw(&mut self, f: &mut ratatui::Frame) {
        let area = f.area();
        // Block rendering: 2 dots per column, 2 dots per row
        // Update viewport dimensions without resetting pan/zoom
        self.viewport.width = area.width as f64 * 2.0;
        self.viewport.height = area.height as f64 * 2.0;
        self.viewport.set_zoom_from_radius(self.camera.radius(), self.initial_radius);

        // Sync camera state to MCP server
        if let Some(ref mcp) = self.mcp_state {
            if let Ok(mut s) = mcp.lock() {
                s.theta = self.camera.theta();
                s.phi = self.camera.phi();
                s.radius = self.camera.radius();
            }
        }

        let draw_cmds = self.viewport.render(&self.scene, &self.camera);

        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(f.area());

        f.render_widget(SceneWidget::new(&draw_cmds), layout[0]);

        f.render_widget(
            StatusBar {
                file_name: self
                    .file_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown")
                    .to_string(),
                atom_count: self.num_atoms,
                theta_deg: self.camera.theta().to_degrees(),
                phi_deg: self.camera.phi().to_degrees(),
                radius: self.camera.radius(),
            },
            layout[1],
        );
    }
}

/// Compute the axis-aligned bounding box of original (non-periodic) atoms.
fn bounding_box_corners(scene: &Scene) -> ([f64; 3], [f64; 3]) {
    let originals: Vec<_> = scene.atoms.iter().filter(|a| !a.is_periodic_image).collect();
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
