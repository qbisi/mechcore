//! Collects requested observations and reads final facts through the match settlement reader.
use crate::cli::Failure;
use mechcore_document::{Fight, Layout};
use mechcore_simulation::ObservationPlan;
use serde_json::{Value, json};
use std::sync::atomic::AtomicBool;

pub(crate) fn run(
    layout: &Layout,
    plan: ObservationPlan,
    cancel: &AtomicBool,
) -> Result<Value, Failure> {
    let names = plan.settlement_names();
    let mut observation = plan
        .run(layout, cancel)
        .map_err(|e| Failure::unsimulated(&e).with_details(json!({"status":"unresolved"})))?;
    if let Some(evidence) = observation.evidence {
        let fight = crate::outcome::observed(layout.clone(), evidence, observation.tick)?;
        for name in names {
            observation
                .values
                .insert(name, json!({"blue":fight.blue,"red":fight.red}));
        }
    }
    Ok(
        json!({"tick":observation.tick,"stop":observation.stop,"settled":observation.settled,"collect":observation.values}),
    )
}

pub(crate) fn fight(layout: Layout) -> Result<Fight, String> {
    let plan = ObservationPlan::parse(None, None)?;
    let observation = plan
        .run(&layout, &AtomicBool::new(false))
        .map_err(|e| e.to_string())?;
    crate::outcome::observed(
        layout,
        observation.evidence.ok_or("fight did not settle")?,
        observation.tick,
    )
    .map_err(|e| e.reason().to_owned())
}
