//! In-memory full-battle settlement shared by the match and the arena.
use crate::cli::Failure;
use mechcore_document::{Fight, Layout};
use mechcore_simulation::{Record, simulate_document};
use serde_json::{Value, json};

pub(crate) fn run(layout: Layout) -> Result<Value, Failure> {
    let fight = recorded(layout)?;
    let tick = fight.trajectory.as_ref().expect("recorded fight").ticks;
    Ok(json!({"tick":tick, "settled":true,"stop":"fight_end",
        "collect":{"settlement":{"blue":fight.blue,"red":fight.red}}}))
}

pub(crate) fn fight(layout: Layout) -> Result<Fight, String> {
    recorded(layout).map_err(|failure| failure.reason().to_owned())
}

fn recorded(layout: Layout) -> Result<Fight, Failure> {
    let yaml = mechcore_document::canonical_yaml(layout).map_err(Failure::refused)?;
    let result = simulate_document(yaml.as_bytes(), Record::Memory, None)
        .map_err(|e| Failure::unsimulated(&e))?;
    crate::outcome::read(&result.recording.expect("in-memory recording"))?.fight()
}
