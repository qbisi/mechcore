//! Runs official in-memory arena battle simulation and settlement extraction.

use std::sync::atomic::AtomicBool;

use mechcore_document::{Fight, Layout};
use mechcore_simulation::ObservationPlan;

use crate::cli::Failure;

/// Simulates `layout` to completion and returns its settled `Fight`, preserving
/// simulator source `sites` when refused for unmodeled mechanics.
pub(crate) fn fight(layout: Layout) -> Result<Fight, Failure> {
    let plan = ObservationPlan::parse(None, None).map_err(Failure::failed)?;
    let observation = plan
        .run(&layout, &AtomicBool::new(false))
        .map_err(|e| Failure::unsimulated(&e))?;
    let evidence = observation
        .evidence
        .ok_or_else(|| Failure::failed("fight did not settle"))?;
    crate::outcome::observed(layout, evidence, observation.tick)
}
