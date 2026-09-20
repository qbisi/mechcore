//! What a fight decided, read out of a recording of it.
//!
//! `docs/spec/document/battle.md` names five fields as the fight's: the damage
//! each reactor core takes, the experience each unit gains, and which
//! contraptions, terrains and airdrop shields remain. This reads a recording
//! for as much of that as the recording holds, and names the rest rather than
//! approximating it.
//!
//! The reader is the seam the backends meet at. A fight the simulator ran and
//! a fight the game played are the same MCFR, so what turns one into the next
//! position is written once, here.

use std::{collections::BTreeMap, path::Path};

use mechcore_document::UnitPlacement;
use mechcore_mcfr::{McfrReader, WorldSnapshot};
use serde::Serialize;

use crate::{
    cli::Failure,
    scene::{self, FIRST_TICK},
    turn::Side,
};

pub(crate) const SCHEMA: &str = "mechcore.fight-outcome.v1";

/// A fight's five fields, as far as a recording decides them.
#[derive(Serialize)]
pub(crate) struct Outcome {
    schema: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    recording: Option<String>,
    /// The last tick the recording holds, which is where the fight ended.
    ticks: u32,
    sides: Sides,
    /// What this fight decided that nothing here answers. A fight with an
    /// entry here is a fight a match will not settle.
    pub(crate) unresolved: Vec<String>,
}

#[derive(Serialize)]
struct Sides {
    blue: SideOutcome,
    red: SideOutcome,
}

/// One side's share of what the fight decided.
#[derive(Serialize)]
struct SideOutcome {
    /// The formations still standing when the fight ended, by the index the
    /// document knows them under.
    survivors: Vec<Survivor>,
    /// Which of the things a fight thins out remain, by their place in the
    /// round's own list. Absent when this reader cannot say.
    #[serde(skip_serializing_if = "Option::is_none")]
    contraptions: Option<Vec<usize>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    terrains: Option<Vec<usize>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    airdrop_shields: Option<Vec<usize>>,
}

/// One formation that survived, and how much of it did.
#[derive(Serialize)]
struct Survivor {
    index: i32,
    name: String,
    /// How many members the formation went into the fight with, and how many
    /// came out of it.
    members: usize,
    alive: usize,
    /// The life those members have left, and what they would hold whole.
    life: i64,
    maximum: i64,
}

/// Reads a recording on disk.
///
/// # Errors
///
/// Returns a failure when the file is not a recording this build reads, or
/// when its scene cannot be matched to the layout it embeds.
pub(crate) fn read(path: &Path) -> Result<Outcome, Failure> {
    let reader = McfrReader::open(path)
        .map_err(|error| Failure::refused(format!("cannot read {}: {error}", path.display())))?;
    let mut outcome = of(&reader)?;
    outcome.recording = Some(path.display().to_string());
    Ok(outcome)
}

/// Reads an open recording, which is what a match does with the fight it just
/// ran.
///
/// # Errors
///
/// Returns a failure when the recording's scene cannot be matched to the
/// layout it embeds.
pub(crate) fn of(reader: &McfrReader) -> Result<Outcome, Failure> {
    let layout = scene::layout(reader)?;
    let last = reader.terminal_tick();
    let opened = scene::snapshot(reader, FIRST_TICK)?;
    let ended = scene::snapshot(reader, last)?;
    let formations = scene::formations(&layout, &opened)?;
    let started = members(&formations, &opened);
    let survived = members(&formations, &ended);

    // Two of the five fields are decided by rules nobody has. The recording
    // holds what they would be computed from and not what they are.
    let mut unresolved = vec![
        "reactor_core: no rule turns a fight's survivors into the damage the \
         losing side's reactor core takes"
            .to_owned(),
        "units.exp: an MCFR recording carries no experience, so what a fight \
         hands out is not observed either"
            .to_owned(),
    ];
    let mut answered = Vec::new();
    for side in Side::BOTH {
        let placed = scene::units_of(&layout, side);
        let carried = scene::side_of(&layout, side);
        answered.push(SideOutcome {
            survivors: survivors(side, &started, &survived, placed),
            contraptions: thinned(
                carried.contraptions.len(),
                side,
                "contraptions",
                &mut unresolved,
            ),
            terrains: thinned(carried.terrains.len(), side, "terrains", &mut unresolved),
            airdrop_shields: thinned(
                carried.airdrop_shields.len(),
                side,
                "airdrop_shields",
                &mut unresolved,
            ),
        });
    }
    let red = answered.pop().expect("both sides were read");
    let blue = answered.pop().expect("both sides were read");
    Ok(Outcome {
        schema: SCHEMA,
        recording: None,
        ticks: last,
        sides: Sides { blue, red },
        unresolved,
    })
}

/// What remains of a collection the fight thins out.
///
/// A round that carried none of them into the fight has none left, which is
/// the one answer this reader closes. Recovering the document's form of one
/// that did survive is its next piece of work, so that case is named rather
/// than guessed.
fn thinned(
    carried: usize,
    side: Side,
    field: &str,
    unresolved: &mut Vec<String>,
) -> Option<Vec<usize>> {
    if carried == 0 {
        return Some(Vec::new());
    }
    unresolved.push(format!(
        "{field}: {} carried {carried} into the fight, and this reader does not \
         yet recover the document's form of one that comes out",
        side.name()
    ));
    None
}

/// How many members of each formation one snapshot holds, and their life.
fn members(
    formations: &BTreeMap<u64, (Side, i32)>,
    snapshot: &WorldSnapshot,
) -> BTreeMap<(Side, i32), (usize, i64, i64)> {
    let mut counted: BTreeMap<(Side, i32), (usize, i64, i64)> = BTreeMap::new();
    for unit in &snapshot.live_units {
        let Some(formation) = formations.get(&unit.formation_id) else {
            continue;
        };
        let entry = counted.entry(*formation).or_default();
        entry.0 += 1;
        entry.1 += i64::from(unit.life.current);
        entry.2 += i64::from(unit.life.maximum);
    }
    counted
}

/// One side's formations that still stand, in document index order.
fn survivors(
    side: Side,
    started: &BTreeMap<(Side, i32), (usize, i64, i64)>,
    survived: &BTreeMap<(Side, i32), (usize, i64, i64)>,
    placed: &[UnitPlacement],
) -> Vec<Survivor> {
    placed
        .iter()
        .filter_map(|placement| {
            let (alive, life, maximum) = *survived.get(&(side, placement.index))?;
            Some(Survivor {
                index: placement.index,
                name: placement.type_name.clone(),
                members: started
                    .get(&(side, placement.index))
                    .map_or(alive, |(members, _, _)| *members),
                alive,
                life,
                maximum,
            })
        })
        .collect()
}
