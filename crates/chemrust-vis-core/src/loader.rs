//! CASTEP .cell file loader (feature-gated behind "castep-loader").
//!
//! Parses `.cell` files using `castep-cell-io` and constructs
//! `chemrust_geometry::Structure` from the parsed document.

use castep_cell_io::{CellDocument, Lattice, Positions};
use chemrust_geometry::{ElementSymbol, FracCoord, LatticeVectors, Structure};
use nalgebra::Matrix3;
use std::path::Path;
use std::str::FromStr;

/// Errors that can occur during cell file loading.
#[derive(Debug)]
pub enum LoaderError {
    /// I/O error reading the file.
    Io(std::io::Error),
    /// Parsing error from castep_cell_fmt.
    Parse(String),
    /// Unknown chemical species string.
    UnknownSpecies(String),
}

impl std::fmt::Display for LoaderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoaderError::Io(e) => write!(f, "I/O error: {}", e),
            LoaderError::Parse(msg) => write!(f, "Parse error: {}", msg),
            LoaderError::UnknownSpecies(s) => write!(f, "Unknown species: {}", s),
        }
    }
}

impl std::error::Error for LoaderError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            LoaderError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for LoaderError {
    fn from(e: std::io::Error) -> Self {
        LoaderError::Io(e)
    }
}

impl From<castep_cell_fmt::Error> for LoaderError {
    fn from(e: castep_cell_fmt::Error) -> Self {
        LoaderError::Parse(e.to_string())
    }
}

/// Loader for CASTEP `.cell` files.
pub struct CellLoader;

impl CellLoader {
    /// Load a `Structure` from a CASTEP `.cell` file at the given path.
    ///
    /// Reads the file, parses it via `castep-cell-fmt`, and converts the
    /// resulting `CellDocument` into a `chemrust_geometry::Structure`.
    pub fn load(path: &Path) -> Result<Structure, LoaderError> {
        let text = std::fs::read_to_string(path)?;
        let doc: CellDocument = castep_cell_fmt::parse(&text)?;
        Self::doc_to_structure(&doc)
    }

    /// Convert a parsed `CellDocument` into a `Structure`.
    fn doc_to_structure(doc: &CellDocument) -> Result<Structure, LoaderError> {
        let (species, frac_coords): (Vec<_>, Vec<_>) = match &doc.positions {
            Positions::Frac(positions_frac) => positions_frac
                .positions
                .iter()
                .map(|entry| {
                    let element = species_to_element(&entry.species)?;
                    Ok((
                        element,
                        FracCoord::new(entry.coord[0], entry.coord[1], entry.coord[2]),
                    ))
                })
                .collect::<Result<Vec<_>, LoaderError>>()?
                .into_iter()
                .unzip(),
            Positions::Abs(positions_abs) => positions_abs
                .positions
                .iter()
                .map(|entry| {
                    let element = species_to_element(&entry.species)?;
                    Ok((
                        element,
                        FracCoord::new(entry.coord[0], entry.coord[1], entry.coord[2]),
                    ))
                })
                .collect::<Result<Vec<_>, LoaderError>>()?
                .into_iter()
                .unzip(),
        };

        let cell = match &doc.lattice {
            Lattice::Cart(lattice_cart) => {
                let m = Matrix3::new(
                    lattice_cart.a[0], lattice_cart.b[0], lattice_cart.c[0],
                    lattice_cart.a[1], lattice_cart.b[1], lattice_cart.c[1],
                    lattice_cart.a[2], lattice_cart.b[2], lattice_cart.c[2],
                );
                Some(LatticeVectors::new(m))
            }
            Lattice::Abc(_) => {
                // LATTICE_ABC format: we don't directly handle in Phase 1
                return Err(LoaderError::Parse(
                    "LATTICE_ABC format not yet supported".into(),
                ));
            }
        };

        let num = species.len();
        Ok(Structure::new(
            species,
            frac_coords,
            cell,
            [true, true, true], // pbc defaults
            vec![0; num],
            vec![None; num],
            None,
        ))
    }
}

/// Convert a `Species` from castep-cell-io to `ElementSymbol`.
fn species_to_element(species: &castep_cell_io::cell::species::Species) -> Result<ElementSymbol, LoaderError> {
    match species {
        castep_cell_io::cell::species::Species::Symbol(s) => {
            ElementSymbol::from_str(s).map_err(|_| LoaderError::UnknownSpecies(s.clone()))
        }
        castep_cell_io::cell::species::Species::AtomicNumber(n) => {
            ElementSymbol::try_from(*n)
                .map_err(|_| LoaderError::UnknownSpecies(format!("atomic number {}", n)))
        }
    }
}
