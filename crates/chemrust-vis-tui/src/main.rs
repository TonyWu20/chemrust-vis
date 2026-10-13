//! chemrust-vis-tui — Terminal 3D chemical structure visualizer.
//!
//! Usage: chemrust-vis-tui <file.cell>

mod tui;

use anyhow::{Context, Result};
use chemrust_vis_core::loader::CellLoader;
use chemrust_vis_core::scene::Scene;
use clap::{Parser, ValueEnum};
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::backend::CrosstermBackend;
use std::io;
use std::path::PathBuf;
use tui::app::App;

/// Scene renderer for the interactive TUI.
#[derive(ValueEnum, Clone, Copy, PartialEq, Eq, Debug)]
#[clap(name = "renderer")]
enum Renderer {
    /// Auto-detect: the kitty graphics protocol when the terminal
    /// supports it, half-block characters otherwise.
    Auto,
    /// Half-block characters. Works in any Unicode terminal.
    Halfblock,
    /// Kitty graphics protocol: pixel-level scene rendering.
    /// Requires kitty, WezTerm, or Ghostty.
    Kitty,
}

/// Terminal-first 3D chemical structure visualizer.
#[derive(Parser)]
#[command(name = "chemrust-vis-tui", version)]
struct Cli {
    /// Path to a CASTEP .cell file to visualize.
    #[arg(value_name = "FILE")]
    file: PathBuf,

    /// Run in MCP (Model Context Protocol) server mode instead of TUI.
    #[arg(long)]
    mcp: bool,

    /// Start a background MCP TCP server on this port alongside the TUI.
    #[arg(long, default_value_t = 0)]
    mcp_port: u16,

    /// Scene renderer for the interactive TUI.
    #[arg(long, value_enum, default_value_t = Renderer::Auto)]
    renderer: Renderer,

    /// Render a single frame as sixel and exit (no TUI).
    #[arg(long, conflicts_with = "kitty")]
    sixel: bool,

    /// Render a single frame as a kitty image and exit (no TUI).
    #[arg(long)]
    kitty: bool,
}


/// Terminal size, falling back to 80x24 when the query fails or
/// reports an empty pty.
fn terminal_size() -> (u16, u16) {
    let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
    if cols == 0 || rows == 0 {
        (80, 24)
    } else {
        (cols, rows)
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Load the structure from the cell file.
    let structure = CellLoader::load(&cli.file)
        .with_context(|| format!("Failed to load cell file: {}", cli.file.display()))?;

    let scene = Scene::from_structure(&structure);

    if cli.mcp {
        eprintln!("chemrust-vis MCP server started. Waiting for client...");
        chemrust_vis_core::mcp::run_mcp_server(scene)?;
        return Ok(());
    }

    if cli.sixel {
        use chemrust_vis_core::camera::Camera;
        use chemrust_vis_core::viewport::Viewport;
        use crate::tui::sixel;
        use std::io::Write;

        let center = scene.center_for_view();
        let camera = Camera::from_target(center, 16.0);
        let (cols, rows) = terminal_size();
        let vp = Viewport::new(cols as f64 * 12.0, rows as f64 * 24.0);
        let cmds = vp.render(&scene, &camera);
        let (w, h, pixels) = sixel::render_to_pixels(&cmds, cols as usize * 12, rows as usize * 24);
        eprintln!("Rendered {}×{} pixels, {} atoms, {} edges",
            w, h, cmds.points.len(), cmds.lines.len());
        let data = sixel::encode_sixel(&pixels, w, h);
        eprintln!("Sixel data: {} bytes", data.len());

        let mut stdout = std::io::stdout();
        write!(stdout, "\x1b[2J\x1b[H")?; // clear screen, home cursor
        stdout.write_all(data.as_bytes())?;
        stdout.flush()?;
        // Use raw mode for key read
        enable_raw_mode()?;
        eprint!("\r\nPress any key to exit...");
        let _ = crossterm::event::read();
        disable_raw_mode()?;
        execute!(std::io::stdout(), crossterm::terminal::Clear(crossterm::terminal::ClearType::All))?;
        return Ok(());
    }

    if cli.kitty {
        use chemrust_vis_core::camera::Camera;
        use chemrust_vis_core::viewport::Viewport;
        use crate::tui::{kitty, sixel};
        use std::io::Write;

        let center = scene.center_for_view();
        let camera = Camera::from_target(center, 16.0);
        let (cols, rows) = terminal_size();
        let vp = Viewport::new(cols as f64 * 12.0, rows as f64 * 24.0);
        let cmds = vp.render(&scene, &camera);
        // Same content fit as the sixel one-shot, sent over the
        // kitty protocol instead.
        let (w, h, pixels) = sixel::render_to_pixels(&cmds, cols as usize * 12, rows as usize * 24);
        eprintln!("Rendered {}×{} pixels, {} atoms, {} edges",
            w, h, cmds.points.len(), cmds.lines.len());
        let seq = kitty::encode_transmission_rgb(&pixels, w as u32, h as u32, kitty::IMAGE_ID);
        let placement = kitty::encode_placement(kitty::IMAGE_ID, cols, rows.saturating_sub(1));
        eprintln!("Kitty data: {} bytes", seq.len() + placement.len());
        if !kitty::terminal_supports_kitty() {
            eprintln!("Note: terminal does not appear to support the kitty protocol; the image may not display.");
        }

        let mut stdout = std::io::stdout();
        write!(stdout, "\x1b[2J\x1b[H")?; // clear screen, home cursor
        stdout.write_all(&seq)?;
        stdout.write_all(&placement)?;
        stdout.flush()?;
        // Use raw mode for key read
        enable_raw_mode()?;
        eprint!("\r\nPress any key to exit...");
        let _ = crossterm::event::read();
        disable_raw_mode()?;
        stdout.write_all(&kitty::encode_delete_all())?;
        stdout.flush()?;
        execute!(std::io::stdout(), crossterm::terminal::Clear(crossterm::terminal::ClearType::All))?;
        return Ok(());
    }

    // Resolve the scene renderer for the interactive TUI.
    let use_kitty = match cli.renderer {
        Renderer::Auto => tui::kitty::terminal_supports_kitty(),
        Renderer::Halfblock => false,
        Renderer::Kitty => true,
    };
    if use_kitty && !tui::kitty::terminal_supports_kitty() {
        eprintln!(
            "Warning: this terminal does not appear to support the kitty \
             graphics protocol. The scene image may not display; the \
             text UI still works."
        );
    }

    // Print structure summary for verification.
    eprintln!("Loaded: {} atoms", structure.num_atoms());
    eprintln!("Renderer: {}", if use_kitty { "kitty" } else { "half-block" });
    if let Some(cell) = &structure.cell {
        let (a, b, c) = cell.lengths();
        eprintln!("  Cell: a={:.4}  b={:.4}  c={:.4} Å", a, b, c);
        let t = cell.tensor();
        eprintln!("  a = ({:.4}, {:.4}, {:.4})", t[(0,0)], t[(1,0)], t[(2,0)]);
        eprintln!("  b = ({:.4}, {:.4}, {:.4})", t[(0,1)], t[(1,1)], t[(2,1)]);
        eprintln!("  c = ({:.4}, {:.4}, {:.4})", t[(0,2)], t[(1,2)], t[(2,2)]);
    }
    for (i, atom) in scene.atoms.iter().filter(|a| !a.is_periodic_image).take(3).enumerate() {
        let el = format!("{:?}", atom.element);
        eprintln!(
            "  atom[{}] {} at ({:.4}, {:.4}, {:.4})",
            i, el, atom.position[0], atom.position[1], atom.position[2]
        );
    }

    // Set up background MCP server if port specified (before scene moves into App)
    let mcp_state: Option<std::sync::Arc<std::sync::Mutex<tui::mcp_server::SharedState>>> =
        if cli.mcp_port > 0 {
            let center = scene.center_for_view();
            let s = std::sync::Arc::new(std::sync::Mutex::new(tui::mcp_server::SharedState {
                scene: Scene::from_structure(&structure),
                theta: std::f64::consts::FRAC_PI_6,
                phi: std::f64::consts::FRAC_PI_6,
                radius: 20.0,
                target: center,
                vp_width: 160.0,
                vp_height: 96.0,
            }));
            tui::mcp_server::start_mcp_server(cli.mcp_port, s.clone());
            Some(s)
        } else {
            None
        };

    // Terminal setup.
    enable_raw_mode().context("Failed to enable raw mode")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)
        .context("Failed to enter alternate screen")?;
    let _ = execute!(stdout, crossterm::event::EnableMouseCapture);

    let backend = CrosstermBackend::new(stdout);
    let mut terminal =
        ratatui::Terminal::new(backend).context("Failed to create terminal")?;

    let mut app = App::new(scene, cli.file);
    if use_kitty {
        app.enable_kitty();
    }
    if let Some(ref state) = mcp_state {
        app.set_mcp_state(state.clone());
    }

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
