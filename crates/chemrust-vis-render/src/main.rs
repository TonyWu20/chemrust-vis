//! chemrust-vis-frame — render one lattice frame to PNG.
//!
//! Usage:
//!   chemrust-vis-frame <model.cell> <out.png> [options]
//!
//! Options:
//!   --theta DEG        camera azimuth in degrees (default 30)
//!   --phi DEG         camera polar angle from +Z in degrees;
//!                     0 = top view, 90 = horizon (default 30)
//!   --atom-r PX        on-screen atom radius in pixels (default 10)
//!   --site-r PX        on-screen site marker radius in pixels (default 6)
//!   --size WxH         canvas, e.g. 1600x1000 (default 1600x1000)
//!   --no-cell          skip the cell box
//!   --no-bonds         skip covalent bonds
//!   --no-sites-on-top  depth-sort site markers instead of drawing them last
//!   --bond-factor F    multiply on the covalent-radius pair sum (default 1.15)
//!   --sites FILE.json  overlay site markers from a site dump

use std::path::PathBuf;

use chemrust_vis_render::{load_sites, load_structure, render_png, FrameSpec};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: chemrust-vis-frame <model.cell> <out.png> [options]");
        std::process::exit(2);
    }
    let model = PathBuf::from(&args[1]);
    let out = PathBuf::from(&args[2]);

    let mut spec = FrameSpec::default();
    let mut i = 3;
    let mut sites: Vec<chemrust_vis_render::SiteMarker> = Vec::new();
    while i < args.len() {
        match args[i].as_str() {
            "--theta" => {
                spec.theta = arg(&args, &mut i).parse().expect("--theta takes a number");
            }
            "--phi" => {
                spec.phi = arg(&args, &mut i).parse().expect("--phi takes a number");
            }
            "--size" => {
                let s = arg(&args, &mut i);
                let mut it = s.split('x');
                let w: u32 = it.next().expect("--size takes WxH").parse().expect("width");
                let h: u32 = it.next().expect("--size takes WxH").parse().expect("height");
                spec.width = w;
                spec.height = h;
            }
            "--no-cell" => spec.show_cell = false,
            "--no-bonds" => spec.show_bonds = false,
            "--no-sites-on-top" => spec.sites_on_top = false,
            "--bond-factor" => {
                spec.bond_factor =
                    arg(&args, &mut i).parse().expect("--bond-factor takes a number");
            }
            "--sites" => {
                let p = PathBuf::from(arg(&args, &mut i));
                sites = load_sites(&p).expect("read --sites file");
            }
            "--atom-r" => {
                spec.atom_radius_px =
                    arg(&args, &mut i).parse().expect("--atom-r takes a number");
            }
            "--site-r" => {
                spec.site_radius_px =
                    arg(&args, &mut i).parse().expect("--site-r takes a number");
            }
            other => {
                eprintln!("unknown option {other}");
                std::process::exit(2);
            }
        }
        i += 1;
    }
    spec.sites = sites;

    let structure = load_structure(&model).expect("load model");
    let bytes = render_png(&structure, &spec).expect("render PNG");
    std::fs::write(&out, bytes).expect("write output");
    println!("wrote {}", out.display());
}

fn arg(args: &[String], i: &mut usize) -> String {
    *i += 1;
    args.get(*i).cloned().unwrap_or_else(|| {
        eprintln!("option {} takes a value", args[*i - 1]);
        std::process::exit(2);
    })
}
