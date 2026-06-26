//! TUI application: ratatui event loop, layout, and rendering.

use std::io;
use std::path::PathBuf;

use chemrust_vis_core::camera::Camera;
use chemrust_vis_core::scene::Scene;
use chemrust_vis_core::viewport::Viewport;
use crossterm::event::{self, Event};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    Terminal,
};

use super::input::{self, Action};
use super::widgets::scene_view::SceneWidget;
use super::widgets::status_bar::StatusBar;

/// The main application state.
pub struct App {
    scene: Scene,
    camera: Camera,
    viewport: Viewport,
    file_path: PathBuf,
    running: bool,
}

impl App {
    /// Create a new App from a loaded scene and file path.
    pub fn new(scene: Scene, file_path: PathBuf) -> Self {
        let center = scene.bounding_box_center();

        // Compute initial camera: auto-frame the structure.
        // Radius = distance from target → camera. Use diagonal * 0.6 so the
        // structure fills ~70% of the viewport with the default focal length.
        let (min, max) = bounding_box_corners(&scene);
        let diag = ((max[0] - min[0]).powi(2) + (max[1] - min[1]).powi(2) + (max[2] - min[2]).powi(2)).sqrt();
        let radius = (diag * 0.6).max(5.0);

        let camera = Camera::from_target(center, radius);

        App {
            scene,
            camera,
            viewport: Viewport::new(160.0, 96.0), // default; updated on first resize
            file_path,
            running: true,
        }
    }

    /// Run the main event loop.
    pub fn run(&mut self, terminal: &mut Terminal<impl ratatui::backend::Backend>) -> io::Result<()> {
        while self.running {
            terminal.draw(|f| {
                self.draw(f);
            })?;

            match event::read()? {
                Event::Key(key) => {
                    let action = input::handle_key_event(key, &mut self.camera);
                    if action == Action::Quit {
                        self.running = false;
                    }
                }
                Event::Resize(cols, rows) => {
                    self.viewport = Viewport::new(cols as f64 * 2.0, rows as f64 * 4.0);
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Draw a single frame.
    fn draw(&mut self, f: &mut ratatui::Frame) {
        // Sync viewport size to terminal area before rendering
        let area = f.area();
        self.viewport = Viewport::new(
            area.width as f64 * 2.0,
            area.height as f64 * 4.0,
        );
        let draw_cmds = self.viewport.render(&self.scene, &self.camera);

        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1),       // scene view fills remaining space
                Constraint::Length(1),    // status bar
            ])
            .split(f.area());

        // Scene view
        f.render_widget(SceneWidget::new(&draw_cmds), layout[0]);

        // Status bar
        let theta_deg = self.camera_theta_deg();
        let phi_deg = self.camera_phi_deg();
        f.render_widget(
            StatusBar {
                file_name: self.file_path.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown")
                    .to_string(),
                atom_count: self.scene.atoms.len(),
                theta_deg,
                phi_deg,
                radius: self.camera.radius(),
            },
            layout[1],
        );
    }

    fn camera_theta_deg(&self) -> f64 {
        self.camera.theta().to_degrees()
    }

    fn camera_phi_deg(&self) -> f64 {
        self.camera.phi().to_degrees()
    }
}

/// Compute the axis-aligned bounding box corners of the scene.
fn bounding_box_corners(scene: &Scene) -> ([f64; 3], [f64; 3]) {
    if scene.atoms.is_empty() {
        return ([0.0; 3], [0.0; 3]);
    }
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    for atom in &scene.atoms {
        for i in 0..3 {
            min[i] = min[i].min(atom.position[i]);
            max[i] = max[i].max(atom.position[i]);
        }
    }
    (min, max)
}
