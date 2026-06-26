//! chemrust-vis-tui — Terminal 3D chemical structure visualizer.
//!
//! Usage: chemrust-vis-tui <file.cell>

mod tui;

use anyhow::{Context, Result};
use chemrust_vis_core::loader::CellLoader;
use chemrust_vis_core::scene::Scene;
use clap::Parser;
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::backend::CrosstermBackend;
use std::io;
use std::path::PathBuf;
use tui::app::App;

/// Terminal-first 3D chemical structure visualizer.
#[derive(Parser)]
#[command(name = "chemrust-vis-tui", version)]
struct Cli {
    /// Path to a CASTEP .cell file to visualize.
    #[arg(value_name = "FILE")]
    file: PathBuf,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Load the structure from the cell file.
    let structure = CellLoader::load(&cli.file)
        .with_context(|| format!("Failed to load cell file: {}", cli.file.display()))?;

    // Build the scene.
    let scene = Scene::from_structure(&structure);

    // Terminal setup.
    enable_raw_mode().context("Failed to enable raw mode")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)
        .context("Failed to enter alternate screen")?;
    // Mouse capture for future Phase 2.
    let _ = execute!(stdout, crossterm::event::EnableMouseCapture);

    // Create terminal backend and app.
    let backend = CrosstermBackend::new(stdout);
    let mut terminal =
        ratatui::Terminal::new(backend).context("Failed to create terminal")?;

    let mut app = App::new(scene, cli.file);

    let result = app.run(&mut terminal);

    // Terminal teardown — always restore, even on error.
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        crossterm::event::DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    result?;
    Ok(())
}
