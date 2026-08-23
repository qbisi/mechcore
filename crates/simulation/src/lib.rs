//! Deterministic single-round layout simulation into the shared MCFR format.
//!
//! The first implemented closure slice is a level-one, no-technology Marksman
//! versus Arclight battle. Unsupported layout mechanisms fail closed.

mod kernel;
mod layout;
mod random;
mod rules;

use std::{
    fmt, fs,
    path::{Path, PathBuf},
    process,
    time::{SystemTime, UNIX_EPOCH},
};

pub use kernel::SimulationResult;

#[derive(Debug)]
pub struct Error(String);

impl Error {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl From<mechcore_mcfr::Error> for Error {
    fn from(error: mechcore_mcfr::Error) -> Self {
        Self(error.to_string())
    }
}

type Result<T> = std::result::Result<T, Error>;

/// Simulates a supported layout and writes a verified MCFR recording.
///
/// When `seed` is `None`, a seed is generated and returned in the report so
/// the run can be reproduced with an explicit seed.
///
/// # Errors
///
/// Returns an error for unsupported layout features, invalid files, an
/// existing output, simulation failure, or MCFR write/verification failure.
pub fn simulate_layout(
    layout_path: impl AsRef<Path>,
    output_path: impl AsRef<Path>,
    seed: Option<i32>,
) -> Result<SimulationResult> {
    simulate_layout_with_config(layout_path, output_path, seed, None)
}

/// Simulates a layout with either the embedded defaults or an external
/// directory containing one YAML file per unit type.
///
/// # Errors
///
/// Returns the same errors as [`simulate_layout`], plus unit-config directory
/// and validation failures.
pub fn simulate_layout_with_config(
    layout_path: impl AsRef<Path>,
    output_path: impl AsRef<Path>,
    seed: Option<i32>,
    config_directory: Option<&Path>,
) -> Result<SimulationResult> {
    let layout_path = layout_path.as_ref();
    let output_path = output_path.as_ref();
    let layout = layout::load(layout_path)?;
    let unit_directory = config_directory.map(|directory| directory.join("units"));
    let unit_configs = rules::UnitConfigs::load(unit_directory.as_deref())?;
    let (seed, source) = match seed {
        Some(seed) => (seed, "external"),
        None => (generate_seed(layout_path)?, "generated"),
    };
    kernel::run(&layout, &unit_configs, seed, source, output_path)
}

/// Returns the default sibling `.mcfr` output path for a layout.
#[must_use]
pub fn default_output_path(layout_path: &Path) -> PathBuf {
    layout_path.with_extension("mcfr")
}

fn generate_seed(layout_path: &Path) -> Result<i32> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| Error::new(format!("system clock cannot generate a seed: {error}")))?;
    let metadata = fs::metadata(layout_path).map_err(|error| {
        Error::new(format!("cannot inspect {}: {error}", layout_path.display()))
    })?;
    let material = format!(
        "{}:{}:{}:{}:{}",
        now.as_secs(),
        now.subsec_nanos(),
        process::id(),
        metadata.len(),
        layout_path.display()
    );
    let hash = blake3::hash(material.as_bytes());
    Ok(i32::from_le_bytes(hash.as_bytes()[..4].try_into().unwrap()))
}
