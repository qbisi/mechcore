//! Deterministic single-round layout simulation into the shared MCFR format.
//!
//! The first implemented closure slice is a level-one, no-technology Marksman
//! versus Arclight battle. Unsupported layout mechanisms fail closed.

mod kernel;
mod layout;
mod random;
mod rules;
mod rvo;

use std::{
    fmt, fs,
    path::Path,
    process,
    time::{SystemTime, UNIX_EPOCH},
};

pub use kernel::{DivergentTick, SimulationComparison, SimulationResult, TimelineSummary};

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

/// Simulates a supported layout and optionally writes an MCFR recording.
///
/// A supplied `seed` overrides `layout.seed`. An effective seed of zero asks
/// the simulator to generate and report a system-random seed.
///
/// # Errors
///
/// Returns an error for unsupported layout features, invalid files, an
/// existing requested output, simulation failure, or MCFR generation failure.
pub fn simulate_layout(
    layout_path: impl AsRef<Path>,
    output_path: Option<&Path>,
    seed: Option<i32>,
) -> Result<SimulationResult> {
    simulate_layout_with_config(layout_path, output_path, seed, None)
}

/// Simulates a layout with either the embedded defaults or an external
/// config root containing `config.yaml`, `training_ground.yaml`, and one YAML
/// file per unit type under `units/`.
///
/// # Errors
///
/// Returns the same errors as [`simulate_layout`], plus unit-config directory
/// and validation failures.
pub fn simulate_layout_with_config(
    layout_path: impl AsRef<Path>,
    output_path: Option<&Path>,
    seed: Option<i32>,
    config_directory: Option<&Path>,
) -> Result<SimulationResult> {
    let layout_path = layout_path.as_ref();
    let config = rules::SimulationConfig::load(config_directory)?;
    let (layout_seed, layout) = layout::load(layout_path, &config.units)?;
    let requested_seed = seed.unwrap_or(layout_seed);
    let (seed, source) = if requested_seed == 0 {
        (generate_seed(layout_path)?, "generated")
    } else if seed.is_some() {
        (requested_seed, "external")
    } else {
        (requested_seed, "layout")
    };
    kernel::run(&layout, &config, seed, source, output_path)
}

/// Simulates a manifest-selected layout and compares canonical ticks directly
/// with an open MCFR recording without creating another recording.
///
/// Comparison stops after the first unequal or missing tick. The recording and
/// simulation must have the same game build and scenario hash.
///
/// # Errors
///
/// Returns an error for an invalid layout or config, a zero seed, a build or
/// scenario mismatch, or a simulation/MCFR failure.
pub fn compare_layout_to_recording_with_config(
    layout_path: impl AsRef<Path>,
    seed: i32,
    config_directory: Option<&Path>,
    recording: &mechcore_mcfr::McfrReader,
) -> Result<SimulationComparison> {
    if seed == 0 {
        return Err(Error::new("sim compare requires a non-zero manifest seed"));
    }
    let config = rules::SimulationConfig::load(config_directory)?;
    let (_, layout) = layout::load(layout_path.as_ref(), &config.units)?;
    kernel::compare(&layout, &config, seed, recording)
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
