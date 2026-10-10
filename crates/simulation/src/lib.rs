//! Deterministic single-round layout simulation into the shared MCFR format.
//!
//! The first implemented closure slice is a level-one, no-technology Marksman
//! versus Arclight fight. Unsupported layout mechanisms fail closed.

mod data;
mod fight;
mod layout;
mod modifier;
mod module;
mod rules;

use std::{
    fmt, fs,
    path::Path,
    process,
    time::{SystemTime, UNIX_EPOCH},
};

pub use fight::{
    DivergentTick, Phases, SimulationComparison, SimulationProfile, SimulationResult, SlowestStep,
    TimelineSummary,
};

/// Where a fight's timeline goes.
#[derive(Clone, Copy, Debug)]
pub enum Record<'a> {
    /// Nowhere: only its hash is kept.
    Hash,
    /// An MCFR written at this path, which must not exist yet.
    File(&'a Path),
    /// Kept in memory and handed back as [`SimulationResult::recording`], for
    /// a reader that has no use for the file.
    Memory,
}

/// Why the simulator does not fight something, and where in it each reason
/// was raised.
#[derive(Debug)]
pub struct Error {
    message: String,
    sites: Vec<&'static std::panic::Location<'static>>,
}

impl Error {
    #[track_caller]
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            sites: vec![std::panic::Location::caller()],
        }
    }

    /// The same reasons under a prefix that says where they arose, raised
    /// where they were.
    fn context(self, prefix: impl fmt::Display) -> Self {
        Self {
            message: format!("{prefix}: {}", self.message),
            sites: self.sites,
        }
    }

    /// Where in the simulator's source each reason was raised, as
    /// `file:line`, in the order the reasons are given. A batch of fights
    /// counts its refusals by them.
    #[must_use]
    pub fn sites(&self) -> Vec<String> {
        self.sites
            .iter()
            .map(|site| format!("{}:{}", site.file(), site.line()))
            .collect()
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

impl From<mechcore_mcfr::Error> for Error {
    #[track_caller]
    fn from(error: mechcore_mcfr::Error) -> Self {
        Self::new(error.to_string())
    }
}

type Result<T> = std::result::Result<T, Error>;

/// Simulates a supported layout, recording its timeline where `record` says.
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
    record: Record<'_>,
    seed: Option<i32>,
) -> Result<SimulationResult> {
    let layout_path = layout_path.as_ref();
    let config = rules::SimulationConfig::load()?;
    let loaded = layout::load(layout_path, &config.units)?;
    run_loaded(loaded, &config, record, seed, || generate_seed(layout_path))
}

/// Simulates a layout held in memory, which is what a match does with the
/// position a round ends in.
///
/// # Errors
///
/// Returns an error for unsupported layout features, an invalid document, an
/// existing requested output, simulation failure, or MCFR generation failure.
pub fn simulate_document(
    layout: &[u8],
    record: Record<'_>,
    seed: Option<i32>,
) -> Result<SimulationResult> {
    let config = rules::SimulationConfig::load()?;
    let loaded = layout::read(layout, &config.units)?;
    run_loaded(loaded, &config, record, seed, || {
        Err(Error::new(
            "a layout with no seed cannot be simulated from memory: name the seed",
        ))
    })
}

/// Runs a compiled layout, whichever way it was read.
fn run_loaded(
    (layout_seed, layout, replay_layout): (Option<i32>, layout::CompiledLayout, String),
    config: &rules::SimulationConfig,
    record: Record<'_>,
    seed: Option<i32>,
    generate: impl FnOnce() -> Result<i32>,
) -> Result<SimulationResult> {
    let (seed, source) = match (seed, layout_seed) {
        (Some(0), _) => {
            return Err(Error::new(
                "seed 0 is the system-random request, not a match seed: omit the seed to generate one",
            ));
        }
        (Some(seed), _) => (seed, "external"),
        (None, Some(seed)) => (seed, "layout"),
        (None, None) => (generate()?, "generated"),
    };
    fight::run(&layout, config, seed, source, record, &replay_layout)
}

/// Simulates the layout embedded in an MCFR and compares canonical ticks
/// directly without creating another recording.
///
/// Comparison stops after the first unequal or missing tick. The recording and
/// simulation must have the same game build.
///
/// # Errors
///
/// Returns an error for an invalid layout or config, a build mismatch, or a
/// simulation/MCFR failure.
pub fn compare_recording(recording: &mechcore_mcfr::McfrReader) -> Result<SimulationComparison> {
    let config = rules::SimulationConfig::load()?;
    let (seed, layout) =
        layout::compile_with_seed(recording.layout_yaml().as_bytes(), &config.units)
            .map_err(|error| error.context("cannot simulate embedded layout"))?;
    let seed = seed.ok_or_else(|| {
        Error::new("embedded layout has no seed, so the recording cannot be reproduced")
    })?;
    fight::compare(&layout, &config, seed, recording)
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
    let seed = i32::from_le_bytes(hash.as_bytes()[..4].try_into().unwrap());
    // A recording embeds its resolved seed, and a layout cannot carry 0, so a
    // generated seed must never land on the sentinel.
    Ok(if seed == 0 { 1 } else { seed })
}

#[cfg(test)]
mod tests {
    use super::Error;

    #[test]
    fn a_reason_keeps_where_it_was_raised_under_a_prefix() {
        let line = line!() + 1;
        let error = Error::new("not measured").context("side blue");
        assert_eq!(error.to_string(), "side blue: not measured");
        assert_eq!(error.sites(), [format!("{}:{line}", file!())]);
    }
}
