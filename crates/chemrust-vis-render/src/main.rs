//! chemrust-vis-frame — render one lattice frame to PNG or SVG.
//!
//! Usage:
//!   chemrust-vis-frame <model.cell> <out.png|out.svg> [options]
//!
//! Options:
//!   --theta DEG          camera azimuth (default 30)
//!   --phi DEG           camera elevation (default 20)
//!   --zoom F            zoom, 1.0 = fit to frame (default 1.0)
//!   --size WxH          canvas, e.g. 1200x900 (default 1200x900)
//!   --no-cell           skip the cell box
//!   --no-bonds          skip covalent bonds
//!   --bond-factor F    multiply on the covalent-radius pair sum (default 1.25)
//!   --sites FILE.json  overlay site markers from a site dump
//!   --atom-r F         base atom radius in pixels (default 10)

use std::path::PathBuf;

use chemrust_vis_render::{load_sites, load_structure, render_png, render_svg, FrameSpec};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: chemrust-vis-frame <model.cell> <out.png|out.svg> [options]");
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
            "--zoom" => {
                spec.zoom = arg(&args, &mut i).parse().expect("--zoom takes a number");
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
                spec.atom_px = arg(&args, &mut i).parse().expect("--atom-r takes a number");
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
    let ext = out.extension().and_then(|e| e.to_str()).unwrap_or("");
    let bytes: Vec<u8> = match ext {
        "svg" => {
            let text = render_svg(&structure, &spec).expect("render SVG");
            text.into_bytes()
        }
        _ => render_png(&structure, &spec).expect("render PNG"),
    };
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
