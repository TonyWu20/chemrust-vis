//! chemrust-vis-core — Core library for terminal-first 3D chemical structure visualization.
//!
//! Provides scene construction, camera model, viewport projection pipeline,
//! and (feature-gated) CASTEP `.cell` file loading.

pub mod camera;
pub mod scene;
pub mod viewport;

#[cfg(feature = "castep-loader")]
pub mod loader;
