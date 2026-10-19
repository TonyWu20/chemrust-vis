//! chemrust-vis-render — offscreen file output for chemrust-vis.
//!
//! Drives the core camera and viewport pipeline, then rasterizes the
//! draw commands with the same shaded-sphere model the TUI uses
//! (Lambert diffuse plus Blinn-Phong specular, light from the
//! upper left). Output is a PNG frame with optional covalent-radius
//! bonds, the cell box, and site markers (for example the exact
//! engine's found sites from a site dump).
//!
//! The site markers are projected through the same view transform as
//! the atoms and interleave with them in depth, so markers behind an
//! atom draw behind it.

use std::path::Path;

use castep_periodic_table::data::ELEMENT_TABLE;
use castep_periodic_table::element::LookupElement;
use chemrust_geometry::ElementSymbol;
use chemrust_vis_core::camera::Camera;
use chemrust_vis_core::loader::CellLoader;
use chemrust_vis_core::scene::Scene;
use chemrust_vis_core::viewport::Viewport;
use image::Rgba;
use nalgebra::Point3;
use serde::Deserialize;
use std::f64::consts::PI;

/// One site marker to overlay on the frame.
#[derive(Debug, Clone, Deserialize)]
pub struct SiteMarker {
    /// Site class, e.g. "single", "double", "multi".
    pub class: String,
    /// Cartesian x in Angstrom.
    pub x: f64,
    /// Cartesian y in Angstrom.
    pub y: f64,
    /// Cartesian z in Angstrom.
    pub z: f64,
}

/// Load a site dump JSON of the form
/// `{ "sites": [ {"class": ..., "x": ..., "y": ..., "z": ...} ] }`.
#[derive(Debug, Deserialize)]
struct SiteDump {
    sites: Vec<SiteMarker>,
}

/// Load a site dump into markers.
pub fn load_sites(path: &Path) -> Result<Vec<SiteMarker>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let dump: SiteDump = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    Ok(dump.sites)
}

/// Load a `.cell` model into a Structure.
pub fn load_structure(path: &Path) -> Result<chemrust_geometry::Structure, String> {
    CellLoader::load(path).map_err(|e| format!("{e:?}"))
}

/// Frame options for one render.
#[derive(Debug, Clone)]
pub struct FrameSpec {
    /// Canvas width in pixels.
    pub width: u32,
    /// Canvas height in pixels.
    pub height: u32,
    /// Camera azimuth in degrees. 0 looks down from +X toward +Y.
    pub theta: f64,
    /// Camera polar angle in degrees from +Z. 0 is the top view,
    /// 90 is the horizon, 180 is the bottom view.
    pub phi: f64,
    /// On-screen atom radius in pixels.
    pub atom_radius_px: f64,
    /// On-screen site marker radius in pixels.
    pub site_radius_px: f64,
    /// Draw the cell box edges.
    pub show_cell: bool,
    /// Draw covalent-radius bonds.
    pub show_bonds: bool,
    /// Multiply on the covalent-radius pair sum for the bond cutoff.
    pub bond_factor: f64,
    /// Site markers to draw on top.
    pub sites: Vec<SiteMarker>,
    /// Draw site markers above every atom, ignoring depth.
    pub sites_on_top: bool,
    /// Site class to RGB. Classes not in the map use gray.
    pub site_colors: Vec<(String, [u8; 3])>,
}

impl Default for FrameSpec {
    fn default() -> Self {
        Self {
            width: 1600,
            height: 1000,
            theta: 30.0,
            phi: 30.0,
            atom_radius_px: 10.0,
            site_radius_px: 6.0,
            show_cell: true,
            show_bonds: true,
            bond_factor: 1.15,
            sites: Vec::new(),
            sites_on_top: true,
            site_colors: vec![
                ("multi".into(), [227, 26, 35]),
                ("double".into(), [255, 140, 0]),
                ("single".into(), [0, 128, 255]),
            ],
        }
    }
}

fn covalent_radius(sym: &ElementSymbol) -> f64 {
    ELEMENT_TABLE
        .get_by_symbol(*sym)
        .covalent_radius()
        .unwrap_or(1.0)
}

/// Bonded pairs among all scene atoms (originals and replicas), by
/// covalent cutoff.
fn bond_pairs(scene: &Scene, factor: f64) -> Vec<(usize, usize)> {
    let n = scene.atoms.len();
    let rc: Vec<f64> = scene
        .atoms
        .iter()
        .map(|a| covalent_radius(&a.element))
        .collect();
    let mut out: Vec<(usize, usize)> = Vec::new();
    for i in 0..n {
        for j in (i + 1)..n {
            let dx = scene.atoms[i].position[0] - scene.atoms[j].position[0];
            let dy = scene.atoms[i].position[1] - scene.atoms[j].position[1];
            let dz = scene.atoms[i].position[2] - scene.atoms[j].position[2];
            let d = (dx * dx + dy * dy + dz * dz).sqrt();
            if d > 1e-3 && d < factor * (rc[i] + rc[j]) {
                out.push((i, j));
            }
        }
    }
    out
}

/// One drawable item in depth order.
struct Item {
    z: f64,
    x: f64,
    y: f64,
    r: f64,
    color: [u8; 3],
    site: bool,
}

/// Render one frame to an RGBA pixel buffer.
pub fn render_frame(
    structure: &chemrust_geometry::Structure,
    spec: &FrameSpec,
) -> Result<image::RgbaImage, String> {
    let scene = Scene::from_structure(structure);
    let center = scene.bounding_box_center();
    let w = spec.width as f64;
    let h = spec.height as f64;
    let theta = spec.theta * PI / 180.0;
    let phi = spec.phi * PI / 180.0;
    let camera =
        Camera::with_angles_target(center, 1.0, theta, phi);

    // The viewport draws atoms at radius min(w,h) * 0.05 * zoom.
    // Invert that to land on the requested atom radius.
    let mut vp = Viewport::new(w, h);
    vp.zoom = (spec.atom_radius_px / (w.min(h) * 0.05)).clamp(0.05, 20.0);
    let cmds = vp.render(&scene, &camera);

    // Duplicate the viewport fit so site markers project through the
    // same transform. Keep this in step with Viewport::render.
    let view_matrix = camera.view_matrix();
    let mut min_x = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    let view_pts: Vec<Point3<f64>> = scene
        .atoms
        .iter()
        .map(|atom| {
            let world = Point3::new(atom.position[0], atom.position[1], atom.position[2]);
            view_matrix * world
        })
        .collect();
    for vp_ in &view_pts {
        min_x = min_x.min(vp_.x);
        max_x = max_x.max(vp_.x);
        min_y = min_y.min(vp_.y);
        max_y = max_y.max(vp_.y);
    }
    for &(start, end) in &scene.cell_edges {
        let s = view_matrix * Point3::new(start[0], start[1], start[2]);
        let e = view_matrix * Point3::new(end[0], end[1], end[2]);
        min_x = min_x.min(s.x).min(e.x);
        max_x = max_x.max(s.x).max(e.x);
        min_y = min_y.min(s.y).min(e.y);
        max_y = max_y.max(s.y).max(e.y);
    }
    let data_w = (max_x - min_x).max(1.0);
    let data_h = (max_y - min_y).max(1.0);
    let base_scale = (w * 0.85 / data_w).min(h * 0.85 / data_h);
    let scale = base_scale * vp.zoom;
    let cx = w / 2.0;
    let cy = h / 2.0;

    let project = |p: Point3<f64>| -> (f64, f64, f64) {
        let q = view_matrix * p;
        (q.x * scale + cx, -q.y * scale + cy, q.z)
    };

    let mut items: Vec<Item> = cmds
        .points
        .iter()
        .map(|p| Item {
            z: p.z_view,
            x: p.x,
            y: p.y,
            r: p.radius,
            color: [p.color.0, p.color.1, p.color.2],
            site: false,
        })
        .collect();
    for s in &spec.sites {
        let p = Point3::new(s.x, s.y, s.z);
        let (sx, sy, sz) = project(p);
        let color = spec
            .site_colors
            .iter()
            .find(|(name, _)| name == &s.class)
            .map(|(_, c)| *c)
            .unwrap_or([120, 120, 120]);
        items.push(Item {
            z: sz,
            x: sx,
            y: sy,
            r: spec.site_radius_px,
            color,
            site: true,
        });
    }
    if spec.sites_on_top {
        for it in items.iter_mut() {
            if it.site {
                it.z = f64::INFINITY;
            }
        }
    }
    items.sort_by(|a, b| {
        a.z.partial_cmp(&b.z).unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut img = image::RgbaImage::new(spec.width, spec.height);
    for px in img.pixels_mut() {
        *px = Rgba([255, 255, 255, 255]);
    }

    // Lines first: they sit behind the spheres.
    if spec.show_cell {
        for &(start, end) in &scene.cell_edges {
            let (x1, y1, _) = project(Point3::new(start[0], start[1], start[2]));
            let (x2, y2, _) = project(Point3::new(end[0], end[1], end[2]));
            draw_line(&mut img, x1, y1, x2, y2, [150, 150, 150]);
        }
    }
    if spec.show_bonds {
        let pairs = bond_pairs(&scene, spec.bond_factor);
        let atom_xy: Vec<(f64, f64)> = view_pts
            .iter()
            .map(|q| (q.x * scale + cx, -q.y * scale + cy))
            .collect();
        for (i, j) in pairs {
            let (x1, y1) = atom_xy[i];
            let (x2, y2) = atom_xy[j];
            draw_line(&mut img, x1, y1, x2, y2, [110, 110, 110]);
        }
    }

    for it in &items {
        if it.x + it.r < 0.0
            || it.x - it.r > w
            || it.y + it.r < 0.0
            || it.y - it.r > h
        {
            continue;
        }
        paint_shaded_sphere(&mut img, it.x, it.y, it.r, it.color);
    }
    Ok(img)
}

/// Encode an RGBA buffer as PNG bytes.
pub fn encode_png(img: &image::RgbaImage) -> Result<Vec<u8>, String> {
    use image::codecs::png::PngEncoder;
    use image::ImageEncoder;
    let raw: Vec<u8> = img.as_raw().to_vec();
    let w = img.width();
    let h = img.height();
    let mut buf: Vec<u8> = Vec::new();
    let cursor = std::io::Cursor::new(&mut buf);
    PngEncoder::new(cursor)
        .write_image(&raw, w, h, image::ExtendedColorType::Rgba8)
        .map_err(|e| e.to_string())?;
    Ok(buf)
}

/// Render one frame and encode it as PNG bytes.
pub fn render_png(
    structure: &chemrust_geometry::Structure,
    spec: &FrameSpec,
) -> Result<Vec<u8>, String> {
    let img = render_frame(structure, spec)?;
    encode_png(&img)
}

/// Shaded sphere: Lambert diffuse plus Blinn-Phong specular, with
/// the TUI's light constants and intensity curve. Anti-aliased
/// circle coverage, alpha-composited over the buffer.
fn paint_shaded_sphere(
    img: &mut image::RgbaImage,
    cx: f64,
    cy: f64,
    r: f64,
    color: [u8; 3],
) {
    let iw = img.width() as i64;
    let ih = img.height() as i64;
    let x0 = ((cx - r - 1.5).floor().max(0.0)) as i64;
    let x1 = ((cx + r + 1.5).ceil().min(iw as f64)) as i64;
    let y0 = ((cy - r - 1.5).floor().max(0.0)) as i64;
    let y1 = ((cy + r + 1.5).ceil().min(ih as f64)) as i64;
    if x0 >= x1 || y0 >= y1 || r <= 0.0 {
        return;
    }

    // TUI light: upper-left front. Same constants as
    // chemrust-vis-tui scene_view fill_circle.
    let (lx, ly, lz): (f64, f64, f64) = (0.5, -0.5, 0.707);
    let ln = (lx * lx + ly * ly + lz * lz).sqrt();
    let hx = lx / ln;
    let hy = ly / ln;
    let hz = (lz / ln + 1.0) / 2.0;
    let hn = (hx * hx + hy * hy + hz * hz).sqrt();

    for py in y0..y1 {
        for px in x0..x1 {
            let dx = (px as f64 + 0.5) - cx;
            let dy = (py as f64 + 0.5) - cy;
            let dist = (dx * dx + dy * dy).sqrt();
            let cov = ((r + 0.5) - dist).clamp(0.0, 1.0);
            if cov <= 0.0 {
                continue;
            }
            let d = (dist / r).min(1.0);
            let z = (1.0 - d * d).max(0.0).sqrt();
            let nx = dx / r;
            let ny = dy / r;
            let diffuse = ((nx * lx + ny * ly + z * lz) / ln).max(0.0);
            let spec = ((nx * hx + ny * hy + z * hz) / hn).max(0.0).powi(16) * 0.6;
            let intensity = (0.25 + diffuse * 0.55 + spec).min(1.0);
            let c = [
                (color[0] as f64 * intensity).round().min(255.0) as u8,
                (color[1] as f64 * intensity).round().min(255.0) as u8,
                (color[2] as f64 * intensity).round().min(255.0) as u8,
            ];
            let old = img.get_pixel(px as u32, py as u32);
            let b = 1.0 - cov;
            img.put_pixel(
                px as u32,
                py as u32,
                Rgba([
                    (c[0] as f64 * cov + old[0] as f64 * b).round() as u8,
                    (c[1] as f64 * cov + old[1] as f64 * b).round() as u8,
                    (c[2] as f64 * cov + old[2] as f64 * b).round() as u8,
                    255,
                ]),
            );
        }
    }
}

/// One-pixel-wide Bresenham line.
fn draw_line(img: &mut image::RgbaImage, x1: f64, y1: f64, x2: f64, y2: f64, color: [u8; 3]) {
    let (mut x, mut y) = (x1.round() as i64, y1.round() as i64);
    let (xi, yi) = (x2.round() as i64, y2.round() as i64);
    let dx = (xi - x).abs();
    let dy = -(yi - y).abs();
    let sx = if x < xi { 1 } else { -1 };
    let sy = if y < yi { 1 } else { -1 };
    let mut err = dx + dy;
    loop {
        if x >= 0
            && y >= 0
            && x < img.width() as i64
            && y < img.height() as i64
        {
            let old = img.get_pixel(x as u32, y as u32);
            img.put_pixel(
                x as u32,
                y as u32,
                Rgba([
                    (color[0] as f64 * 0.7 + old[0] as f64 * 0.3) as u8,
                    (color[1] as f64 * 0.7 + old[1] as f64 * 0.3) as u8,
                    (color[2] as f64 * 0.7 + old[2] as f64 * 0.3) as u8,
                    255,
                ]),
            );
        }
        if x == xi && y == yi {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            if x == xi {
                break;
            }
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            if y == yi {
                break;
            }
            err += dx;
            y += sy;
        }
    }
}
