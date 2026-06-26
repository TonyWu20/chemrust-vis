//! Minimal MCP (Model Context Protocol) server for debugging and inspection.
//! Runs over stdin/stdout JSON-RPC, exposing tools to render frames and
//! resources to inspect scene state.

use crate::camera::Camera;
use crate::scene::Scene;
use crate::viewport::{DrawCommands, Viewport};
use nalgebra::Point3;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};

/// Top-level MCP message from client.
#[derive(Debug, Deserialize)]
struct Request {
    jsonrpc: String,
    #[serde(default)]
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Option<Value>,
}

/// Run the MCP server with the given scene. Blocks until stdin closes.
pub fn run_mcp_server(scene: Scene) -> io::Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let server = McpServer { scene };

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let req: Request = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                let _ = writeln!(stdout, "{}", json!({
                    "jsonrpc": "2.0", "id": null,
                    "error": {"code": -32700, "message": format!("Parse error: {}", e)}
                }));
                continue;
            }
        };

        let response = server.handle(&req);
        let _ = writeln!(stdout, "{}", serde_json::to_string(&response).unwrap_or_default());
        let _ = stdout.flush();
    }
    Ok(())
}

struct McpServer {
    scene: Scene,
}

impl McpServer {
    fn handle(&self, req: &Request) -> Value {
        match req.method.as_str() {
            "initialize" => self.initialize(req),
            "notifications/initialized" => json!({"jsonrpc": "2.0", "id": req.id}),
            "tools/list" => self.tools_list(req),
            "tools/call" => self.tools_call(req),
            "resources/list" => self.resources_list(req),
            "resources/read" => self.resources_read(req),
            _ => json!({
                "jsonrpc": "2.0", "id": req.id,
                "error": {"code": -32601, "message": format!("Unknown method: {}", req.method)}
            }),
        }
    }

    fn initialize(&self, req: &Request) -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": req.id,
            "result": {
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "tools": {},
                    "resources": {}
                },
                "serverInfo": {
                    "name": "chemrust-vis-mcp",
                    "version": "0.1.0"
                }
            }
        })
    }

    fn tools_list(&self, req: &Request) -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": req.id,
            "result": {
                "tools": [
                    {
                        "name": "render_frame",
                        "description": "Render a frame with given camera parameters. Returns the character grid as a multi-line string.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "theta_deg": {"type": "number", "description": "Azimuthal angle in degrees (0=+X, 90=+Y)"},
                                "phi_deg": {"type": "number", "description": "Polar angle from Z axis in degrees (0=+Z top-down, 90=XY plane)"},
                                "radius": {"type": "number", "description": "Camera distance from target"},
                                "target_x": {"type": "number", "description": "Camera target X (defaults to scene center)"},
                                "target_y": {"type": "number", "description": "Camera target Y"},
                                "target_z": {"type": "number", "description": "Camera target Z"},
                                "cols": {"type": "integer", "description": "Terminal columns (default 80)"},
                                "rows": {"type": "integer", "description": "Terminal rows (default 24)"},
                                "show_atoms": {"type": "boolean", "description": "Include atom spheres (default true)"},
                                "show_cell": {"type": "boolean", "description": "Include cell edges (default true)"},
                        "pan_x": {"type": "number", "description": "Pan offset X in dot units (default 0)"},
                        "pan_y": {"type": "number", "description": "Pan offset Y in dot units (default 0)"},
                        "zoom": {"type": "number", "description": "Zoom factor: 1.0=default, >1=zoom in (default 1.0)"}
                            }
                        }
                    },
                    {
                        "name": "list_atoms",
                        "description": "List all atoms with their world-space positions and elements.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "include_periodic": {"type": "boolean", "description": "Include periodic replicas (default false)"}
                            }
                        }
                    },
                    {
                        "name": "camera_info",
                        "description": "Get information about what camera parameters would frame the scene well.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {}
                        }
                    }
                ]
            }
        })
    }

    fn tools_call(&self, req: &Request) -> Value {
        let params = req.params.as_ref().and_then(|p| p.as_object());
        let name = params.and_then(|p| p.get("name")).and_then(|v| v.as_str()).unwrap_or("");

        let args = params
            .and_then(|p| p.get("arguments"))
            .cloned()
            .unwrap_or(Value::Null);

        match name {
            "render_frame" => self.render_frame(req, &args),
            "list_atoms" => self.list_atoms(req, &args),
            "camera_info" => self.camera_info(req),
            _ => json!({
                "jsonrpc": "2.0", "id": req.id,
                "error": {"code": -32602, "message": format!("Unknown tool: {}", name)}
            }),
        }
    }

    fn render_frame(&self, req: &Request, args: &Value) -> Value {
        let center = self.scene.center_for_view();
        let theta = args["theta_deg"].as_f64().unwrap_or(30.0).to_radians();
        let phi = args["phi_deg"].as_f64().unwrap_or(30.0).to_radians();
        let radius = args["radius"].as_f64().unwrap_or(20.0);
        let tx = args["target_x"].as_f64().unwrap_or(center[0]);
        let ty = args["target_y"].as_f64().unwrap_or(center[1]);
        let tz = args["target_z"].as_f64().unwrap_or(center[2]);
        let cols = args["cols"].as_u64().unwrap_or(80) as usize;
        let rows = args["rows"].as_u64().unwrap_or(24) as usize;
        let show_atoms = args["show_atoms"].as_bool().unwrap_or(true);
        let show_cell = args["show_cell"].as_bool().unwrap_or(true);
        let pan_x = args["pan_x"].as_f64().unwrap_or(0.0);
        let pan_y = args["pan_y"].as_f64().unwrap_or(0.0);
        let zoom = args["zoom"].as_f64().unwrap_or(1.0);

        let camera = Camera::with_angles(Point3::new(tx, ty, tz), radius, theta, phi);
        let mut viewport = Viewport::new(cols as f64 * 2.0, rows as f64 * 2.0);
        viewport.pan_x = pan_x;
        viewport.pan_y = pan_y;
        viewport.zoom = zoom;
        let mut cmds = viewport.render(&self.scene, &camera);

        if !show_atoms {
            cmds.points.clear();
        }
        if !show_cell {
            cmds.lines.clear();
        }

        // Render to a character grid (2×2 quadrant blocks per character)
        let grid = render_commands_to_grid(&cmds, cols, rows);

        json!({
            "jsonrpc": "2.0",
            "id": req.id,
            "result": {
                "content": [{
                    "type": "text",
                    "text": format!("```\n{}```\n\n{}×{} chars, {} atoms, {} cell edges",
                        grid, cols, rows, cmds.points.len(), cmds.lines.len())
                }]
            }
        })
    }

    fn list_atoms(&self, req: &Request, args: &Value) -> Value {
        let include_periodic = args["include_periodic"].as_bool().unwrap_or(false);
        let atoms: Vec<Value> = self
            .scene
            .atoms
            .iter()
            .filter(|a| include_periodic || !a.is_periodic_image)
            .map(|a| {
                json!({
                    "index": a.atom_index,
                    "element": format!("{:?}", a.element),
                    "position": [a.position[0], a.position[1], a.position[2]],
                    "periodic": a.is_periodic_image,
                    "color": [a.color.0, a.color.1, a.color.2]
                })
            })
            .collect();

        json!({
            "jsonrpc": "2.0",
            "id": req.id,
            "result": {
                "content": [{
                    "type": "text",
                    "text": serde_json::to_string_pretty(&json!({
                        "count": atoms.len(),
                        "atoms": atoms
                    })).unwrap_or_default()
                }]
            }
        })
    }

    fn camera_info(&self, req: &Request) -> Value {
        let center = self.scene.center_for_view();
        // Compute bounding box of non-periodic atoms
        let originals: Vec<_> = self.scene.atoms.iter().filter(|a| !a.is_periodic_image).collect();
        let mut min = [f64::INFINITY; 3];
        let mut max = [f64::NEG_INFINITY; 3];
        for a in &originals {
            for i in 0..3 {
                min[i] = min[i].min(a.position[i]);
                max[i] = max[i].max(a.position[i]);
            }
        }
        let diag = ((max[0]-min[0]).powi(2) + (max[1]-min[1]).powi(2) + (max[2]-min[2]).powi(2)).sqrt();

        json!({
            "jsonrpc": "2.0",
            "id": req.id,
            "result": {
                "content": [{
                    "type": "text",
                    "text": serde_json::to_string_pretty(&json!({
                        "center": center,
                        "bbox_min": min,
                        "bbox_max": max,
                        "diagonal": diag,
                        "suggested_radius": diag * 0.6,
                        "num_atoms": originals.len(),
                        "has_cell": !self.scene.cell_edges.is_empty()
                    })).unwrap_or_default()
                }]
            }
        })
    }

    fn resources_list(&self, req: &Request) -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": req.id,
            "result": {
                "resources": [
                    {
                        "uri": "scene://info",
                        "name": "Scene Info",
                        "description": "Atom count, cell parameters, bounding box",
                        "mimeType": "application/json"
                    },
                    {
                        "uri": "scene://atoms",
                        "name": "Atom List",
                        "description": "All atoms with positions and elements",
                        "mimeType": "application/json"
                    }
                ]
            }
        })
    }

    fn resources_read(&self, req: &Request) -> Value {
        let params = req.params.as_ref().and_then(|p| p.as_object());
        let uri = params.and_then(|p| p.get("uri")).and_then(|v| v.as_str()).unwrap_or("");

        let text = match uri {
            "scene://info" => {
                let center = self.scene.center_for_view();
                let n = self.scene.atoms.iter().filter(|a| !a.is_periodic_image).count();
                let n_periodic = self.scene.atoms.len() - n;
                serde_json::to_string_pretty(&json!({
                    "total_atoms": self.scene.atoms.len(),
                    "original_atoms": n,
                    "periodic_replicas": n_periodic,
                    "center": center,
                    "cell_edges": self.scene.cell_edges.len()
                })).unwrap_or_default()
            }
            "scene://atoms" => {
                let atoms: Vec<Value> = self.scene.atoms.iter()
                    .filter(|a| !a.is_periodic_image)
                    .map(|a| json!({
                        "element": format!("{:?}", a.element),
                        "position": [a.position[0], a.position[1], a.position[2]],
                        "index": a.atom_index
                    }))
                    .collect();
                serde_json::to_string_pretty(&atoms).unwrap_or_default()
            }
            _ => "Unknown resource".to_string(),
        };

        json!({
            "jsonrpc": "2.0",
            "id": req.id,
            "result": {
                "contents": [{
                    "uri": uri,
                    "mimeType": "application/json",
                    "text": text
                }]
            }
        })
    }
}

/// Render DrawCommands to a plain text grid using quadrant block characters.
fn render_commands_to_grid(cmds: &DrawCommands, cols: usize, rows: usize) -> String {
    // Quadrant blocks: 2×2 sub-pixels per character
    // bit 3=UL, bit 2=UR, bit 1=LL, bit 0=LR
    const Q: [char; 16] = [
        ' ', '▗', '▝', '▐', '▖', '▄', '▞', '▟',
        '▘', '▚', '▀', '▜', '▌', '▙', '▛', '█',
    ];

    let mut bits = vec![vec![0u8; cols]; rows];
    let dot_w = cols as f64 * 2.0;
    let dot_h = rows as f64 * 2.0;

    // Draw lines
    for line in &cmds.lines {
        draw_line_on_grid(&mut bits, cols, rows, line.x1, line.y1, line.x2, line.y2);
    }

    // Draw points as filled circles
    for pt in &cmds.points {
        if pt.x < -pt.radius || pt.x > dot_w + pt.radius
            || pt.y < -pt.radius || pt.y > dot_h + pt.radius
        {
            continue;
        }
        let r = pt.radius.ceil() as i64;
        for dy in -r..=r {
            for dx in -r..=r {
                if (dx as f64).powi(2) + (dy as f64).powi(2) <= pt.radius.powi(2) {
                    let sx = pt.x + dx as f64;
                    let sy = pt.y + dy as f64;
                    set_quadrant(&mut bits, cols, rows, sx, sy);
                }
            }
        }
    }

    // Build string
    let mut out = String::with_capacity((cols + 1) * rows);
    for row in 0..rows {
        for col in 0..cols {
            out.push(Q[bits[row][col] as usize]);
        }
        out.push('\n');
    }
    out
}

fn set_quadrant(bits: &mut [Vec<u8>], cols: usize, rows: usize, x: f64, y: f64) {
    let col = (x / 2.0).floor() as usize;
    let row = (y / 2.0).floor() as usize;
    if col < cols && row < rows {
        let qx = (x as usize) % 2;
        let qy = (y as usize) % 2;
        let bit: u8 = match (qx, qy) {
            (0, 0) => 8,  // UL
            (1, 0) => 4,  // UR
            (0, 1) => 2,  // LL
            (1, 1) => 1,  // LR
            _ => 0,
        };
        bits[row][col] |= bit;
    }
}

fn draw_line_on_grid(bits: &mut [Vec<u8>], cols: usize, rows: usize, x1: f64, y1: f64, x2: f64, y2: f64) {
    let (mut x, mut y) = (x1 as i64, y1 as i64);
    let (x2i, y2i) = (x2 as i64, y2 as i64);
    let dx = (x2i - x).abs();
    let dy = -(y2i - y).abs();
    let sx = if x < x2i { 1 } else { -1 };
    let sy = if y < y2i { 1 } else { -1 };
    let mut err = dx + dy;
    loop {
        set_quadrant(bits, cols, rows, x as f64, y as f64);
        if x == x2i && y == y2i { break; }
        let e2 = 2 * err;
        if e2 >= dy { if x == x2i { break; } err += dy; x += sx; }
        if e2 <= dx { if y == y2i { break; } err += dx; y += sy; }
    }
}
