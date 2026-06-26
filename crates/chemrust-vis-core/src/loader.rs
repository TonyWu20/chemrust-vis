//! CASTEP .cell file loader (feature-gated behind "castep-loader").
//! Parses .cell files and constructs chemrust_geometry::Structure.

use chemrust_geometry::Structure;

/// Loader for CASTEP .cell files.
pub struct CellLoader;

impl CellLoader {
    /// Load a Structure from a CASTEP .cell file.
    pub fn load(_path: &std::path::Path) -> Result<Structure, LoaderError> {
        Err(LoaderError::NotImplemented)
    }
}

/// Errors from cell loading.
#[derive(Debug)]
pub enum LoaderError {
    NotImplemented,
}

impl std::fmt::Display for LoaderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoaderError::NotImplemented => write!(f, "cell loader not yet implemented"),
        }
    }
}

impl std::error::Error for LoaderError {}
