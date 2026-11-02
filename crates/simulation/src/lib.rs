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

pub use fight::{DeploymentPreview, PreviewAttackRelation, PreviewObject, PreviewRelation};
pub use fight::{
    DivergentTick, Phases, SimulationComparison, SimulationProfile, SimulationResult, SlowestStep,
    TimelineSummary,
};

/// Initializes the fight and its deployment presearch without advancing a tick.
/// Missing seeds use the reproducible preview seed 1, reported in the result.
///
/// # Errors
/// Returns the simulator's existing initialization refusals or an invalid seed.
pub fn preview_document(document: &[u8], seed: Option<i32>) -> Result<DeploymentPreview> {
    let config = rules::SimulationConfig::load()?;
    let original = mechcore_document::parse_embedded_yaml(document).map_err(Error::new)?;
    let (layout_seed, layout) = layout::compile_preview(document, &config.units)?;
    let (seed, source) = seed.map_or_else(
        || layout_seed.map_or((1, "default"), |seed| (seed, "layout")),
        |seed| (seed, "external"),
    );
    if seed == 0 {
        return Err(Error::new("preview seed 0 is not reproducible"));
    }
    fight::preview(&layout, &original, &config, seed, source)
}

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

/// One random commander skill's planned strikes after native fight initialization.
/// Positions and radius are Q32.32 world metres; ticks are the planned ground
/// arrivals, before any battlefield shield intercepts a falling sub-effect.
#[derive(Debug, Clone)]
pub struct BattleSkillForecast {
    pub team: u32,
    pub kind: String,
    pub radius: i64,
    pub points: Vec<(u64, mechcore_mcfr::QVec3)>,
}

/// Reconstructs random strike plans without advancing the fight. This uses
/// the same initialization and side streams as simulation, including release
/// order and the round-derived seeds, rather than a separate visual RNG.
///
/// # Errors
/// Returns an error if the embedded layout has no seed or cannot be initialized
/// by the current simulator. The plan does not predict shield interceptions.
pub fn battle_skill_forecasts(document: &[u8]) -> Result<Vec<BattleSkillForecast>> {
    let config = rules::SimulationConfig::load()?;
    let (seed, layout) = layout::compile_with_seed(document, &config.units)?;
    let seed = seed.ok_or_else(|| Error::new("a strike forecast needs an explicit layout seed"))?;
    fight::commander_forecasts(&layout, &config, seed)
}

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
    fn strike_forecasts_keep_side_streams_and_repeated_release_order() {
        let document = br"
kind: layout
seed: 4242
round: 1
blue:
  units:
  - {name: marksman, index: 0, position: {x: 0, y: -180}}
  battle_skills:
  - {name: orbital_bombardment, positions: [{x: 0, y: 30}]}
  - {name: orbital_bombardment, positions: [{x: 0, y: 30}]}
red:
  units:
  - {name: marksman, index: 0, position: {x: 0, y: -180}}
  battle_skills:
  - {name: lightning_storm, positions: [{x: 20, y: 40}]}
";
        let plans = super::battle_skill_forecasts(document).unwrap();
        assert_eq!(plans.len(), 3);
        assert_eq!(plans[0].points.len(), 15);
        assert_eq!(plans[1].points.len(), 15);
        assert_eq!(plans[2].points.len(), 30);
        assert_ne!(plans[0].points, plans[1].points);
        assert_eq!(plans[0].points[0].0, 63);
        assert_eq!(plans[2].points[0].0, 62);
        for (plan, (x, z)) in plans.iter().zip([(0, 30), (0, 30), (-20, -40)]) {
            assert_eq!(plan.radius, 30_i64 << 32);
            for (_, point) in &plan.points {
                let dx = i128::from(point.x) - (i128::from(x) << 32);
                let dz = i128::from(point.z) - (i128::from(z) << 32);
                assert!(dx * dx + dz * dz < (100_i128 << 32).pow(2));
            }
        }
        let mut changed: serde_yaml::Value = serde_yaml::from_slice(document).unwrap();
        changed["seed"] = 12345.into();
        let other_seed =
            super::battle_skill_forecasts(serde_yaml::to_string(&changed).unwrap().as_bytes())
                .unwrap();
        assert_eq!(
            plans.iter().map(|p| &p.points).collect::<Vec<_>>(),
            other_seed.iter().map(|p| &p.points).collect::<Vec<_>>()
        );
        changed["round"] = 2.into();
        let other_round =
            super::battle_skill_forecasts(serde_yaml::to_string(&changed).unwrap().as_bytes())
                .unwrap();
        assert_ne!(plans[0].points, other_round[0].points);
    }

    #[test]
    fn a_reason_keeps_where_it_was_raised_under_a_prefix() {
        let line = line!() + 1;
        let error = Error::new("not measured").context("side blue");
        assert_eq!(error.to_string(), "side blue: not measured");
        assert_eq!(error.sites(), [format!("{}:{line}", file!())]);
    }
}
