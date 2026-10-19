//! chemrust-vis-render — file output for chemrust-vis.
//!
//! Renders one frame of a lattice structure to PNG or SVG. The
//! pipeline is orthographic: camera angles around the structure
//! center, painter's-algorithm depth order, element-colored atoms,
//! covalent-radius bonds, the cell box, and optional site markers
//! (e.g. the exact engine's found sites from a site dump).

use std::path::Path;

use castep_periodic_table::data::ELEMENT_TABLE;
use castep_periodic_table::element::LookupElement;
use chemrust_geometry::{FracCoord, Structure};
use chemrust_vis_core::loader::CellLoader;
use chemrust_vis_core::scene::element_color;
use nalgebra::Point3;
use serde::Deserialize;

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

/// Frame options for one render.
#[derive(Debug, Clone)]
pub struct FrameSpec {
    /// Canvas width in pixels (PNG) or user units (SVG).
    pub width: u32,
    /// Canvas height in pixels (PNG) or user units (SVG).
    pub height: u32,
    /// Camera azimuth in degrees. 0 = looking down -X.
    pub theta: f64,
    /// Camera elevation in degrees. 0 = in the XY plane.
    pub phi: f64,
    /// Zoom factor, 1.0 = fit to frame.
    pub zoom: f64,
    /// Draw the cell box edges.
    pub show_cell: bool,
    /// Draw bonds between atoms within the covalent cutoff.
    pub show_bonds: bool,
    /// Multiply on the covalent-radius pair sum for the bond cutoff.
    pub bond_factor: f64,
    /// Site markers to draw on top.
    pub sites: Vec<SiteMarker>,
    /// Draw site markers above every atom, ignoring depth.
    pub sites_on_top: bool,
    /// Site class to RGB. Classes not in the map use a gray ring.
    pub site_colors: Vec<(String, [u8; 3])>,
    /// Atom radius in pixels at the default zoom, for a covalent
    /// radius of 1.0 A. Other elements scale with their radius.
    pub atom_px: f64,
}

impl Default for FrameSpec {
    fn default() -> Self {
        Self {
            width: 1200,
            height: 900,
            theta: 30.0,
            phi: 20.0,
            zoom: 1.0,
            show_cell: true,
            show_bonds: true,
            bond_factor: 1.25,
            sites: Vec::new(),
            sites_on_top: true,
            site_colors: vec![
                ("multi".into(), [214, 40, 40]),
                ("double".into(), [235, 130, 10]),
                ("single".into(), [30, 100, 220]),
            ],
            atom_px: 10.0,
        }
    }
}

/// Load a `.cell` model into a Structure.
pub fn load_structure(path: &Path) -> Result<Structure, String> {
    CellLoader::load(path).map_err(|e| format!("{e:?}"))
}

/// Load a site dump into markers.
pub fn load_sites(path: &Path) -> Result<Vec<SiteMarker>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let dump: SiteDump = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    Ok(dump.sites)
}

fn covalent_radius(sym: &chemrust_geometry::ElementSymbol) -> f64 {
    ELEMENT_TABLE
        .get_by_symbol(*sym)
        .covalent_radius()
        .unwrap_or(1.0)
}

fn atoms_cart(s: &Structure) -> Vec<(chemrust_geometry::ElementSymbol, Point3<f64>)> {
    let tensor = s
        .cell
        .as_ref()
        .expect("the model carries a lattice")
        .tensor();
    let n = s.frac_coords.len();
    (0..n)
        .map(|i| {
            let mut f: FracCoord = s.frac_coords[i];
            f.wrap();
            let cart = tensor * Point3::new(f.0.x, f.0.y, f.0.z);
            (s.species[i], cart)
        })
        .collect()
}

fn cell_edges(s: &Structure) -> Vec<(Point3<f64>, Point3<f64>)> {
    let t = s
        .cell
        .as_ref()
        .expect("the model carries a lattice")
        .tensor();
    let col = |i: usize| nalgebra::Vector3::new(
        t.column(i)[0],
        t.column(i)[1],
        t.column(i)[2],
    );
    let a = col(0);
    let b = col(1);
    let c = col(2);
    let o = nalgebra::Vector3::zeros();
    let to_pt = |v: &nalgebra::Vector3<f64>| Point3::new(v.x, v.y, v.z);
    let corners: Vec<nalgebra::Vector3<f64>> = vec![
        o.clone(),
        a.clone(),
        b.clone(),
        c.clone(),
        &a + &b,
        &a + &c,
        &b + &c,
        &a + &b + &c,
    ];
    let pairs = [
        (0usize, 1),
        (0, 2),
        (0, 3),
        (1, 4),
        (1, 5),
        (2, 4),
        (2, 6),
        (3, 5),
        (3, 6),
        (4, 7),
        (5, 7),
        (6, 7),
    ];
    pairs
        .iter()
        .map(|(i, j)| (to_pt(&corners[*i]), to_pt(&corners[*j])))
        .collect()
}

fn bonds(
    atoms: &[(chemrust_geometry::ElementSymbol, Point3<f64>)],
    factor: f64,
) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let n = atoms.len();
    for i in 0..n {
        for j in (i + 1)..n {
            let d = (atoms[i].1 - atoms[j].1).norm();
            let cutoff =
                factor * (covalent_radius(&atoms[i].0) + covalent_radius(&atoms[j].0));
            if d <= cutoff {
                out.push((i, j));
            }
        }
    }
    out
}

/// Projected primitive for the painter's algorithm.
#[derive(Debug, Clone)]
enum Prim {
    Line {
        depth: f64,
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
        color: [u8; 3],
        width: f64,
    },
    Atom {
        depth: f64,
        x: f64,
        y: f64,
        r: f64,
        color: [u8; 3],
    },
    Site {
        depth: f64,
        x: f64,
        y: f64,
        r: f64,
        color: [u8; 3],
    },
}

/// Project the frame and return the depth-ordered primitives plus
/// the frame scale, for raster or vector output.
pub fn project(
    structure: &Structure,
    spec: &FrameSpec,
) -> Result<(Vec<Prim>, f64), String> {
    let atoms = atoms_cart(structure);
    if atoms.is_empty() {
        return Err("the model has no atoms".into());
    }
    // Center: mean of the atom cloud.
    let center_v = atoms
        .iter()
        .fold(nalgebra::Vector3::zeros(), |acc, (_, p)| acc + (p - Point3::origin()))
        / atoms.len() as f64;
    let center = Point3::new(center_v.x, center_v.y, center_v.z);

    // Camera basis from spherical angles around the center.
    let th = spec.theta.to_radians();
    let ph = spec.phi.to_radians();
    // Forward: from camera position toward the center.
    let cam_dir = nalgebra::Vector3::new(
        th.cos() * ph.cos(),
        th.sin() * ph.cos(),
        ph.sin(),
    ).normalize();
    let up_world = nalgebra::Vector3::z();
    let right = cam_dir.cross(&up_world).normalize();
    let up = right.cross(&cam_dir).normalize();

    // Fit: the largest projected radius of the atom cloud plus
    // the covalent radii.
    let scale_fit = 0.95 * spec.zoom.min(4.0).max(0.1);
    let max_r = atoms
        .iter()
        .map(|(sym, p)| {
            let q = p - center;
            let px = q.dot(&right);
            let py = q.dot(&up);
            let rad = (px * px + py * py).sqrt() + covalent_radius(sym);
            rad
        })
        .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
        .unwrap_or(1.0)
        + 1.0;
    let w = spec.width as f64;
    let h = spec.height as f64;
    let px_per_a = (w.min(h) * 0.5 * scale_fit) / max_r;

    let to_xy = |p: &Point3<f64>| -> (f64, f64, f64) {
        let q = p - center;
        let px = w * 0.5 + q.dot(&right) * px_per_a;
        let py = h * 0.5 - q.dot(&up) * px_per_a;
        let depth = -q.dot(&cam_dir);
        (px, py, depth)
    };

    let mut prims: Vec<Prim> = Vec::new();

    if spec.show_cell {
        for (p, q) in cell_edges(structure) {
            let (x1, y1, d1) = to_xy(&p);
            let (x2, y2, d2) = to_xy(&q);
            prims.push(Prim::Line {
                depth: (d1 + d2) / 2.0,
                x1,
                y1,
                x2,
                y2,
                color: [130, 130, 130],
                width: 1.5,
            });
        }
    }

    if spec.show_bonds {
        for (i, j) in bonds(&atoms, spec.bond_factor) {
            let (x1, y1, d1) = to_xy(&atoms[i].1);
            let (x2, y2, d2) = to_xy(&atoms[j].1);
            prims.push(Prim::Line {
                depth: (d1 + d2) / 2.0,
                x1,
                y1,
                x2,
                y2,
                color: [90, 90, 90],
                width: 2.0,
            });
        }
    }

    for (sym, p) in &atoms {
        let (x, y, depth) = to_xy(p);
        let r = spec.atom_px * (covalent_radius(sym) / 1.3).clamp(0.5, 2.2) * scale_fit;
        prims.push(Prim::Atom {
            depth,
            x,
            y,
            r,
            color: {
                let c = element_color(*sym);
                [c.0, c.1, c.2]
            },
        });
    }

    for s in &spec.sites {
        let p = Point3::new(s.x, s.y, s.z);
        let (x, y, depth) = to_xy(&p);
        let color = spec
            .site_colors
            .iter()
            .find(|(name, _)| name == &s.class)
            .map(|(_, c)| *c)
            .unwrap_or([120, 120, 120]);
        let depth = if spec.sites_on_top { f64::INFINITY } else { depth };
        prims.push(Prim::Site {
            depth,
            x,
            y,
            r: 3.5 * scale_fit,
            color,
        });
    }

    prims.sort_by(|a, b| {
        let da = match a {
            Prim::Line { depth, .. } | Prim::Atom { depth, .. } | Prim::Site { depth, .. } => depth,
        };
        let db = match b {
            Prim::Line { depth, .. } | Prim::Atom { depth, .. } | Prim::Site { depth, .. } => depth,
        };
        da.partial_cmp(db).unwrap_or(std::cmp::Ordering::Equal)
    });

    Ok((prims, px_per_a))
}

/// Render the frame to a PNG byte string.
pub fn render_png(structure: &Structure, spec: &FrameSpec) -> Result<Vec<u8>, String> {
    let (prims, _) = project(structure, spec)?;
    let w = spec.width;
    let h = spec.height;
    let mut img = image::RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            img.put_pixel(x, y, image::Rgba([255, 255, 255, 255]));
        }
    }
    for p in &prims {
        match p {
            Prim::Line {
                x1,
                y1,
                x2,
                y2,
                color,
                width,
                ..
            } => draw_thick_line(&mut img, *x1, *y1, *x2, *y2, *width, *color),
            Prim::Atom { x, y, r, color, .. } => {
                draw_circle(&mut img, *x, *y, *r, *color, Some([60, 60, 60]), 1.0)
            }
            Prim::Site { x, y, r, color, .. } => {
                // Hollow marker: white fill, colored ring.
                draw_circle(&mut img, *x, *y, *r, [255, 255, 255], Some(*color), 1.8 * r)
            }
        }
    }
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

/// Render the frame to an SVG string.
pub fn render_svg(structure: &Structure, spec: &FrameSpec) -> Result<String, String> {
    let (prims, _) = project(structure, spec)?;
    let mut s = String::new();
    s.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\">\n",
        spec.width, spec.height, spec.width, spec.height
    ));
    s.push_str(&format!(
        "<rect width=\"{}\" height=\"{}\" fill=\"white\"/>\n",
        spec.width, spec.height
    ));
    for p in &prims {
        match p {
            Prim::Line {
                x1,
                y1,
                x2,
                y2,
                color,
                width,
                ..
            } => {
                s.push_str(&format!(
                    "<line x1=\"{:.2}\" y1=\"{:.2}\" x2=\"{:.2}\" y2=\"{:.2}\" stroke=\"#{:02x}{:02x}{:02x}\" stroke-width=\"{:.1}\"/>\n",
                    x1, y1, x2, y2, color[0], color[1], color[2], width
                ));
            }
            Prim::Atom { x, y, r, color, .. } => {
                s.push_str(&format!(
                    "<circle cx=\"{:.2}\" cy=\"{:.2}\" r=\"{:.2}\" fill=\"#{:02x}{:02x}{:02x}\" stroke=\"#3c3c3c\" stroke-width=\"1\"/>\n",
                    x, y, r, color[0], color[1], color[2]
                ));
            }
            Prim::Site { x, y, r, color, .. } => {
                s.push_str(&format!(
                    "<circle cx=\"{:.2}\" cy=\"{:.2}\" r=\"{:.2}\" fill=\"white\" stroke=\"#{:02x}{:02x}{:02x}\" stroke-width=\"{:.1}\"/>\n",
                    x, y, r, color[0], color[1], color[2], 1.8 * r
                ));
            }
        }
    }
    s.push_str("</svg>\n");
    Ok(s)
}

fn draw_thick_line(
    img: &mut image::RgbaImage,
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    width: f64,
    color: [u8; 3],
) {
    let steps = ((x2 - x1).abs().max((y2 - y1).abs()) * 2.0).ceil() as usize;
    if steps == 0 {
        return;
    }
    for i in 0..=steps {
        let t = i as f64 / steps as f64;
        let x = x1 + (x2 - x1) * t;
        let y = y1 + (y2 - y1) * t;
        paint_square(img, x, y, width, color, 255);
    }
}

fn paint_square(
    img: &mut image::RgbaImage,
    cx: f64,
    cy: f64,
    size: f64,
    color: [u8; 3],
    alpha: u8,
) {
    let w = img.width();
    let h = img.height();
    let half = (size / 2.0) as i64;
    let (ix, iy) = (cx as i64, cy as i64);
    for dy in -half..=half {
        for dx in -half..=half {
            let x = ix + dx;
            let y = iy + dy;
            if x < 0 || y < 0 || x >= w as i64 || y >= h as i64 {
                continue;
            }
            let px = *img.get_pixel(x as u32, y as u32);
            let old_a = px[3] as u32;
            let new_a = alpha as u32;
            let a = new_a.max(old_a);
            let blend = |ch: u8, old: u8| -> u8 {
                if a == 0 {
                    return 0;
                }
                ((ch as u32 * new_a + old as u32 * old_a * ((255 - new_a) / 255)) / a) as u8
            };
            img.put_pixel(
                x as u32,
                y as u32,
                image::Rgba([
                    blend(color[0], px[0]),
                    blend(color[1], px[1]),
                    blend(color[2], px[2]),
                    a.min(255) as u8,
                ]),
            );
        }
    }
}

fn coverage(
    img_size: (u32, u32),
    cx: f64,
    cy: f64,
    max_r: f64,
    cond: impl Fn(f64) -> f64,
) -> Vec<(i64, i64, f64)> {
    // Collect supersampled coverage per pixel in the covered box.
    let x0 = (cx - max_r).floor() as i64;
    let x1 = (cx + max_r).ceil() as i64;
    let y0 = (cy - max_r).floor() as i64;
    let y1 = (cy + max_r).ceil() as i64;
    let (w, h) = img_size;
    let samples = 3;
    let mut out: Vec<(i64, i64, f64)> = Vec::new();
    for sy in y0..=y1 {
        for sx in x0..=x1 {
            if sx < 0 || sy < 0 || sx >= w as i64 || sy >= h as i64 {
                continue;
            }
            let mut cov = 0.0;
            for j in 0..samples {
                for i in 0..samples {
                    let px = sx as f64 + (i as f64 + 0.5) / samples as f64;
                    let py = sy as f64 + (j as f64 + 0.5) / samples as f64;
                    let d = ((px - cx) * (px - cx) + (py - cy) * (py - cy)).sqrt();
                    cov += cond(d) / (samples * samples) as f64;
                }
            }
            if cov > 0.0 {
                out.push((sx, sy, cov.min(1.0)));
            }
        }
    }
    out
}

fn draw_circle(
    img: &mut image::RgbaImage,
    cx: f64,
    cy: f64,
    r: f64,
    fill: [u8; 3],
    ring: Option<[u8; 3]>,
    ring_width: f64,
) {
    let size = (img.width(), img.height());
    let fill_covs = if ring.is_none() {
        coverage(size, cx, cy, r, |d| (r - d).max(0.0).min(1.0))
    } else {
        Vec::new()
    };
    for (x, y, cov) in fill_covs {
        paint_disc(img, x, y, cov, fill);
    }
    if let Some(rc) = ring {
        let inner = r - ring_width;
        let outer = r + ring_width;
        let ring_covs = coverage(size, cx, cy, outer, |d| {
            if d > inner && d < outer {
                ((d - inner).max(outer - d)).min(ring_width).min(1.0)
            } else {
                0.0
            }
        });
        for (x, y, cov) in ring_covs {
            paint_disc(img, x, y, cov, rc);
        }
    }
}

fn paint_disc(img: &mut image::RgbaImage, x: i64, y: i64, cov: f64, color: [u8; 3]) {
    if x < 0 || y < 0 || x >= img.width() as i64 || y >= img.height() as i64 {
        return;
    }
    let px = *img.get_pixel(x as u32, y as u32);
    let a = cov;
    let b = 1.0 - a;
    let ch = |idx: usize| {
        ((color[idx] as f64 * a + px[idx] as f64 * b).round() as u32).min(255) as u8
    };
    let old_a = px[3] as f64;
    let new_a = (a.max(old_a / 255.0) * 255.0).round() as u8;
    img.put_pixel(x as u32, y as u32, image::Rgba([ch(0), ch(1), ch(2), new_a]));
}
