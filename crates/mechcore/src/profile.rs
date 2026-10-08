//! `--profile`: where a conversion's time goes, as a flame graph.
//!
//! The phases a computation reports say how its duration splits between
//! building the scene, stepping, reading and recording; this says which
//! functions inside them the time is spent in. The conversion's call stacks are
//! sampled while it runs and drawn as an SVG flame graph. A release build strips
//! its symbols, so its graph names addresses; the `profiling` profile is the
//! release build with them kept.

use std::path::Path;

use crate::cli::Failure;

/// How often the call stacks are sampled, per second.
const FREQUENCY: i32 = 1000;

/// Runs `conversion`, sampling it into a flame graph at `destination` when one
/// is named, and refusing an existing one unless `force` replaces it.
///
/// # Errors
///
/// Returns a refusal for an existing destination without `force`, a failure
/// when the sampler cannot start or the graph cannot be written, and whatever
/// `conversion` returns.
pub(crate) fn sampled<T>(
    destination: Option<&Path>,
    force: bool,
    conversion: impl FnOnce() -> Result<T, Failure>,
) -> Result<T, Failure> {
    let Some(destination) = destination else {
        return conversion();
    };
    if destination.exists() && !force {
        return Err(Failure::refused(format!(
            "{} already exists; pass --force to replace it",
            destination.display()
        )));
    }
    let guard = pprof::ProfilerGuardBuilder::default()
        .frequency(FREQUENCY)
        .blocklist(&["libc", "libgcc", "pthread", "vdso"])
        .build()
        .map_err(|error| Failure::failed(format!("cannot start the sampler: {error}")))?;
    let converted = conversion()?;
    let report = guard
        .report()
        .build()
        .map_err(|error| Failure::failed(format!("cannot read the samples: {error}")))?;
    let file = std::fs::File::create(destination).map_err(|error| {
        Failure::failed(format!("cannot write {}: {error}", destination.display()))
    })?;
    report.flamegraph(file).map_err(|error| {
        Failure::failed(format!(
            "cannot draw the flame graph into {}: {error}",
            destination.display()
        ))
    })?;
    Ok(converted)
}
