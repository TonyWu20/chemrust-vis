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
    terminal::{enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
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

/// Kitty transmission strategy for the interactive TUI.
#[derive(ValueEnum, Clone, Copy, PartialEq, Eq, Debug)]
#[clap(name = "kitty-mode")]
enum KittyMode {
    /// Full-canvas: one image, in-place data replace. The default.
    /// No image accumulation; the most robust mode.
    Full,
    /// Content-box crop: send only the molecule's box each frame.
    /// Saves bandwidth on terminals that evict old images (kitty,
    /// WezTerm). Ghostty does not evict, so images accumulate.
    Delta,
}

/// Kitty transmission medium for the interactive TUI.
#[derive(ValueEnum, Clone, Copy, PartialEq, Eq, Debug)]
#[clap(name = "kitty-medium")]
enum KittyMedium {
    /// Auto: shared memory on local known terminals, plain base64
    /// over SSH or on unknown terminals.
    Auto,
    /// Force POSIX shared memory (local sessions only).
    Shm,
    /// Force plain base64.
    Base64,
    /// Base64 with a zlib-compressed payload (`o=z`).
    Zlib,
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

    /// Render a single frame as a kitty image and exit (no TUI).
    #[arg(long)]
    once: bool,

    /// Kitty transmission strategy for the interactive TUI.
    #[arg(long, value_enum, default_value_t = KittyMode::Full)]
    kitty_mode: KittyMode,

    /// Kitty transmission medium for the interactive TUI.
    #[arg(long, value_enum, default_value_t = KittyMedium::Auto)]
    kitty_medium: KittyMedium,

    // Deprecated aliases, kept for existing commands and hidden
    // from the help output. Each maps to a current flag.
    /// One-shot render. Use --once.
    #[arg(long, hide = true)]
    kitty: bool,
    /// Full-canvas strategy. Use --kitty-mode full.
    #[arg(long, hide = true)]
    kitty_full: bool,
    /// Content-box crop strategy. Use --kitty-mode delta.
    #[arg(long, hide = true)]
    kitty_delta: bool,
    /// zlib-compress the base64 payload. Use --kitty-medium zlib.
    #[arg(long, hide = true)]
    kitty_compress: bool,
    /// Force plain base64. Use --kitty-medium base64.
    #[arg(long, hide = true)]
    no_kitty_shm: bool,
}

/// The kitty medium for the session: the deprecated flags win over
/// the --kitty-medium value when set.
fn resolve_medium(cli: &Cli) -> tui::kitty::Medium {
    if cli.kitty_compress {
        return tui::kitty::Medium::Base64Zlib;
    }
    if cli.no_kitty_shm {
        return tui::kitty::Medium::Base64;
    }
    match cli.kitty_medium {
        KittyMedium::Auto => tui::kitty::select_medium_env(false, true),
        KittyMedium::Shm => tui::kitty::Medium::Shm,
        KittyMedium::Base64 => tui::kitty::Medium::Base64,
        KittyMedium::Zlib => tui::kitty::Medium::Base64Zlib,
    }
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

/// Best-effort terminal teardown.
///
/// Runs once, on the normal exit path or on drop (error or panic).
/// Restores the cursor, disables raw mode, and - for the interactive
/// TUI - releases the mouse capture and leaves the alternate screen.
/// Before leaving raw mode it drains any bytes still queued on stdin,
/// so the shell does not inherit them. Those bytes can include stray
/// terminal graphics-protocol responses (see ADR-0002).
struct TerminalGuard {
    done: bool,
    /// Send the kitty delete-all escape (an image may be placed).
    kitty: bool,
    /// Leave the alternate screen and release the mouse capture.
    /// Interactive TUI only; one-shot paths stay on the main screen.
    alt_screen: bool,
}

impl TerminalGuard {
    /// Interactive TUI: alternate screen, mouse capture, raw mode.
    fn interactive(kitty: bool) -> Self {
        Self {
            done: false,
            kitty,
            alt_screen: true,
        }
    }

    /// One-shot render: raw mode only, no alternate screen.
    fn one_shot(kitty: bool) -> Self {
        Self {
            done: false,
            kitty,
            alt_screen: false,
        }
    }

    /// Run the teardown. Idempotent.
    fn finish(&mut self) {
        if self.done {
            return;
        }
        self.done = true;
        let mut stdout = io::stdout();
        if self.kitty {
            let _ = std::io::Write::write_all(&mut stdout, &tui::kitty::encode_delete_all());
            let _ = std::io::Write::flush(&mut stdout);
        }
        // Swallow bytes still queued on stdin so the shell does not
        // inherit them (e.g. stray graphics-protocol responses).
        while crossterm::event::poll(std::time::Duration::ZERO).unwrap_or(false) {
            let _ = crossterm::event::read();
        }
        let _ = crossterm::execute!(stdout, crossterm::cursor::Show);
        let _ = crossterm::terminal::disable_raw_mode();
        if self.alt_screen {
            let _ = crossterm::execute!(
                stdout,
                crossterm::event::DisableMouseCapture,
                LeaveAlternateScreen,
            );
        }
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        self.finish();
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

    let once = cli.once || cli.kitty;
    if once {
        use crate::tui::{kitty, sixel};
        use chemrust_vis_core::camera::Camera;
        use chemrust_vis_core::viewport::Viewport;
        use std::io::Write;

        let center = scene.center_for_view();
        let camera = Camera::from_target(center, 16.0);
        let (cols, rows) = terminal_size();
        let vp = Viewport::new(cols as f64 * 12.0, rows as f64 * 24.0);
        let cmds = vp.render(&scene, &camera);
        // One-shot grid: 12x24 px per cell. The transmit happens
        // once, so the multi-megabyte payload is acceptable here.
        let (w, h, pixels) = sixel::render_to_pixels(&cmds, cols as usize * 12, rows as usize * 24);
        eprintln!(
            "Rendered {}×{} pixels, {} atoms, {} edges",
            w,
            h,
            cmds.points.len(),
            cmds.lines.len()
        );
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
        // Use raw mode for key read; the guard restores it on every
        // exit path and deletes the placed image.
        enable_raw_mode()?;
        let mut guard = TerminalGuard::one_shot(true);
        eprint!("\r\nPress any key to exit...");
        let _ = crossterm::event::read();
        guard.finish();
        execute!(
            std::io::stdout(),
            crossterm::terminal::Clear(crossterm::terminal::ClearType::All)
        )?;
        return Ok(());
    }

    // Resolve the scene renderer for the interactive TUI.
    let use_kitty = match cli.renderer {
        Renderer::Auto => tui::kitty::terminal_supports_kitty(),
        Renderer::Halfblock => false,
        Renderer::Kitty => true,
    };
    let medium = resolve_medium(&cli);
    let use_delta = cli.kitty_delta || cli.kitty_mode == KittyMode::Delta;
    if use_kitty && !tui::kitty::terminal_supports_kitty() {
        eprintln!(
            "Warning: this terminal does not appear to support the kitty \
             graphics protocol. The scene image may not display; the \
             text UI still works."
        );
    }

    // Print structure summary for verification.
    eprintln!("Loaded: {} atoms", structure.num_atoms());
    eprintln!(
        "Renderer: {}",
        if use_kitty {
            format!("kitty ({} transmission)", medium.tag())
        } else {
            "half-block".to_string()
        }
    );
    if let Some(cell) = &structure.cell {
        let (a, b, c) = cell.lengths();
        eprintln!("  Cell: a={:.4}  b={:.4}  c={:.4} Å", a, b, c);
        let t = cell.tensor();
        eprintln!(
            "  a = ({:.4}, {:.4}, {:.4})",
            t[(0, 0)],
            t[(1, 0)],
            t[(2, 0)]
        );
        eprintln!(
            "  b = ({:.4}, {:.4}, {:.4})",
            t[(0, 1)],
            t[(1, 1)],
            t[(2, 1)]
        );
        eprintln!(
            "  c = ({:.4}, {:.4}, {:.4})",
            t[(0, 2)],
            t[(1, 2)],
            t[(2, 2)]
        );
    }
    for (i, atom) in scene
        .atoms
        .iter()
        .filter(|a| !a.is_periodic_image)
        .take(3)
        .enumerate()
    {
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
    execute!(stdout, EnterAlternateScreen).context("Failed to enter alternate screen")?;
    let _ = execute!(stdout, crossterm::event::EnableMouseCapture);

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = ratatui::Terminal::new(backend).context("Failed to create terminal")?;

    let mut app = App::new(scene, cli.file);
    if use_kitty {
        app.enable_kitty(medium);
        if use_delta {
            app.enable_kitty_delta();
        }
    }
    if let Some(ref state) = mcp_state {
        app.set_mcp_state(state.clone());
    }

    // Best-effort teardown: the guard also runs on error or panic,
    // restoring raw mode, the mouse capture, and the alternate screen,
    // and draining bytes queued on stdin (see TerminalGuard).
    let mut guard = TerminalGuard::interactive(use_kitty);
    let result = app.run(&mut terminal);
    guard.finish();
    result?;
    Ok(())
}
