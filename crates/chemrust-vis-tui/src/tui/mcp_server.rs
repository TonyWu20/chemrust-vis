//! Background MCP TCP server for live TUI inspection.
//! Runs in a separate thread, sharing scene and camera state with the TUI.

use chemrust_vis_core::camera::Camera;
use chemrust_vis_core::scene::Scene;
use chemrust_vis_core::viewport::Viewport;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

/// Shared state between the TUI and the MCP server.
pub struct SharedState {
    pub scene: Scene,
    /// Current camera parameters (updated by TUI each frame).
    pub theta: f64,
    pub phi: f64,
    pub radius: f64,
    pub target: [f64; 3],
    /// Current viewport dimensions in dot units.
    pub vp_width: f64,
    pub vp_height: f64,
}

/// Start an MCP server on the given TCP port in a background thread.
/// Returns immediately; the server handles one client at a time.
pub fn start_mcp_server(port: u16, state: Arc<Mutex<SharedState>>) {
    std::thread::spawn(move || {
        let addr = format!("127.0.0.1:{}", port);
        let listener = match TcpListener::bind(&addr) {
            Ok(l) => {
                eprintln!("MCP server listening on {}", addr);
                l
            }
            Err(e) => {
                eprintln!("MCP server bind failed: {}", e);
                return;
            }
        };

        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    let state = state.clone();
                    std::thread::spawn(move || handle_client(stream, state));
                }
                Err(e) => eprintln!("MCP accept error: {}", e),
            }
        }
    });
}

fn handle_client(stream: TcpStream, state: Arc<Mutex<SharedState>>) {
    let reader = BufReader::new(stream.try_clone().unwrap_or_else(|_| panic!("clone failed")));
    let mut writer = stream;

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        if line.trim().is_empty() {
            continue;
        }
        let req: Value = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(_) => continue,
        };

        let response = handle_request(&req, &state);
        let _ = writeln!(writer, "{}", serde_json::to_string(&response).unwrap_or_default());
        let _ = writer.flush();
    }
}

fn handle_request(req: &Value, state: &Arc<Mutex<SharedState>>) -> Value {
    let method = req["method"].as_str().unwrap_or("");
    let id = &req["id"];

    match method {
        "initialize" => json!({
            "jsonrpc": "2.0", "id": id,
            "result": {
                "protocolVersion": "2024-11-05",
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "chemrust-vis-live", "version": "0.1.0"}
            }
        }),
        "notifications/initialized" => json!({"jsonrpc": "2.0", "id": id}),
        "tools/list" => json!({
            "jsonrpc": "2.0", "id": id,
            "result": {"tools": [
                {
                    "name": "get_view",
                    "description": "Get current rendering as text grid, plus camera state.",
                    "inputSchema": {"type": "object", "properties": {}}
                },
                {
                    "name": "get_state",
                    "description": "Get current camera parameters and scene info.",
                    "inputSchema": {"type": "object", "properties": {}}
                }
            ]}
        }),
        "tools/call" => {
            let name = req["params"]["name"].as_str().unwrap_or("");
            match name {
                "get_view" => tool_get_view(id, state),
                "get_state" => tool_get_state(id, state),
                _ => json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32602, "message": "unknown tool"}}),
            }
        }
        _ => json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": "unknown method"}}),
    }
}

fn tool_get_view(id: &Value, state: &Arc<Mutex<SharedState>>) -> Value {
    let s = state.lock().unwrap();
    let camera = Camera::with_angles_target(
        s.target, s.radius, s.theta, s.phi);
    let viewport = Viewport::new(s.vp_width, s.vp_height);
    let cmds = viewport.render(&s.scene, &camera);

    let grid = render_to_text(&cmds, (s.vp_width / 2.0) as usize, (s.vp_height / 2.0) as usize);

    json!({
        "jsonrpc": "2.0", "id": id,
        "result": {"content": [{"type": "text", "text": format!(
            "θ={:.0}° φ={:.0}° r={:.1} | {} atoms | {}×{} chars\n```\n{}```",
            s.theta.to_degrees().rem_euclid(360.0), s.phi.to_degrees().rem_euclid(360.0), s.radius,
            s.scene.atoms.len(),
            (s.vp_width / 2.0) as usize, (s.vp_height / 2.0) as usize,
            grid
        )}]}
    })
}

fn tool_get_state(id: &Value, state: &Arc<Mutex<SharedState>>) -> Value {
    let s = state.lock().unwrap();
    let center = s.scene.center_for_view();
    let n_orig = s.scene.atoms.iter().filter(|a| !a.is_periodic_image).count();
    json!({
        "jsonrpc": "2.0", "id": id,
        "result": {"content": [{"type": "text", "text": serde_json::to_string_pretty(&json!({
            "camera": {"theta_deg": s.theta.to_degrees().rem_euclid(360.0), "phi_deg": s.phi.to_degrees().rem_euclid(360.0), "radius": s.radius},
            "target": s.target,
            "viewport": {"width": s.vp_width, "height": s.vp_height},
            "scene_center": center,
            "atoms": {"total": s.scene.atoms.len(), "original": n_orig},
            "cell_edges": s.scene.cell_edges.len()
        })).unwrap_or_default()}]}
    })
}

/// Render DrawCommands to a text grid using half-block characters (▀▄█).
fn render_to_text(cmds: &chemrust_vis_core::viewport::DrawCommands, cols: usize, rows: usize) -> String {
    let mut top = vec![vec![false; cols]; rows];
    let mut bot = vec![vec![false; cols]; rows];
    let dot_w = cols as f64 * 2.0;
    let dot_h = rows as f64 * 2.0;

    for line in &cmds.lines {
        draw_line_half(&mut top, &mut bot, cols, rows, line.x1, line.y1, line.x2, line.y2);
    }
    for pt in &cmds.points {
        if pt.x < -pt.radius || pt.x > dot_w + pt.radius || pt.y < -pt.radius || pt.y > dot_h + pt.radius { continue; }
        let r = pt.radius.ceil() as i64;
        for dy in -r..=r {
            for dx in -r..=r {
                if (dx as f64).powi(2) + (dy as f64).powi(2) <= pt.radius.powi(2) {
                    set_half(&mut top, &mut bot, cols, rows, pt.x + dx as f64, pt.y + dy as f64);
                }
            }
        }
    }
    let mut out = String::with_capacity((cols+1)*rows);
    for row in 0..rows {
        for col in 0..cols {
            out.push(match (top[row][col], bot[row][col]) {
                (true,true)=>'█',(true,false)=>'▀',(false,true)=>'▄',_=>' '
            });
        }
        out.push('\n');
    }
    out
}

fn set_half(top: &mut[Vec<bool>], bot: &mut[Vec<bool>], cols: usize, rows: usize, x: f64, y: f64) {
    let col=(x/2.0).floor()as usize; let row=(y/2.0).floor()as usize;
    if col<cols && row<rows { if (y as usize)%2==0 {top[row][col]=true}else{bot[row][col]=true} }
}

fn draw_line_half(top: &mut[Vec<bool>], bot: &mut[Vec<bool>], cols: usize, rows: usize, x1: f64, y1: f64, x2: f64, y2: f64) {
    let(mut x,mut y)=(x1 as i64,y1 as i64); let(x2i,y2i)=(x2 as i64,y2 as i64);
    let dx=(x2i-x).abs();let dy=-(y2i-y).abs();
    let sx=if x<x2i{1}else{-1};let sy=if y<y2i{1}else{-1};
    let mut err=dx+dy;
    loop{set_half(top,bot,cols,rows,x as f64,y as f64);if x==x2i&&y==y2i{break}
        let e2=2*err;if e2>=dy{if x==x2i{break}err+=dy;x+=sx}if e2<=dx{if y==y2i{break}err+=dx;y+=sy}}
}

