//! What a fight decided, read out of a recording of it.
//!
//! `docs/spec/document/match.md` names four fields as the fight's: the damage
//! each reactor core takes, the experience each unit gains, and which
//! contraptions and which objects earlier releases left standing remain. This reads a recording
//! for as much of that as the recording holds, and names the rest rather than
//! approximating it.
//!
//! The reader is the seam the backends meet at. A fight the simulator ran and
//! a fight the game played are the same MCFR, so what turns one into the next
//! position is written once, here.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use mechcore_document::{
    BattleSkillEntry, Layout, UnitPlacement,
    reactor_damage::{self, Survivor as Scored},
};
use mechcore_mcfr::{EventPayload, McfrReader, ObjectKind, WorldSnapshot};
use serde::Serialize;

use crate::{
    cli::Failure,
    scene::{self, FIRST_TICK},
    turn::Side,
};

pub(crate) const SCHEMA: &str = "mechcore.fight-outcome.v3";

/// A fight's four fields, as far as a recording decides them.
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
    /// What the fight took off the side's reactor core: the other side's
    /// score. Absent when a unit's score cannot be answered, which
    /// `unresolved` names.
    #[serde(skip_serializing_if = "Option::is_none")]
    core_damage: Option<i64>,
    /// The formations still standing when the fight ended, by the index the
    /// document knows them under.
    survivors: Vec<Survivor>,
    /// Which of the things a fight thins out remain, by their place in the
    /// round's own list. Absent when this reader cannot say.
    #[serde(skip_serializing_if = "Option::is_none")]
    contraptions: Option<Vec<usize>>,
    /// Which standing `battle_skills` entries remain, by their place in the
    /// round's `battle_skills` list.
    #[serde(skip_serializing_if = "Option::is_none")]
    battle_skills: Option<Vec<usize>>,
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

    // One of the four fields is not answered here, by a mapping this command
    // does not make yet.
    let mut unresolved = vec![
        "units.exp: the recording holds each formation's experience \
         (formations.parquet), and this command does not yet carry it onto \
         the layout's units"
            .to_owned(),
    ];
    let scores = scores(
        reader,
        &layout,
        &formations,
        &opened,
        &ended,
        &mut unresolved,
    )?;
    let damage = match scores {
        [Some(blue), Some(red)] => {
            let (blue, red) = reactor_damage::damage(blue, red);
            [Some(blue), Some(red)]
        }
        _ => [None, None],
    };
    let mut answered = Vec::new();
    for side in Side::BOTH {
        let placed = scene::units_of(&layout, side);
        let carried = scene::side_of(&layout, side);
        answered.push(SideOutcome {
            core_damage: damage[side.seat()],
            survivors: survivors(side, &started, &survived, placed),
            contraptions: thinned(
                carried.contraptions.len(),
                side,
                "contraptions",
                &mut unresolved,
            ),
            battle_skills: thinned(
                carried
                    .battle_skills
                    .iter()
                    .filter(|entry| matches!(entry, BattleSkillEntry::Standing(_)))
                    .count(),
                side,
                "battle_skills.standing",
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

/// Each side's score, `TeamScoreCalculator.CalculateTeamScore`: what the
/// units alive at the fight's end score for the side they then serve.
///
/// A unit is classified by what the recording shows of it. It came from its
/// side's formations when it opened the fight in a formation a placement
/// takes and the fight did not create it; any other unit was summoned,
/// produced or spawned. It changed sides when its team is not the one it
/// started on. It was reborn when it died in the fight and stands at the end:
/// a rebirth revives the unit that died, `RebirthTask.RebirthMech`, which is
/// the one way a dead unit stands again.
fn scores(
    reader: &McfrReader,
    layout: &Layout,
    formations: &BTreeMap<u64, (Side, i32)>,
    opened: &WorldSnapshot,
    ended: &WorldSnapshot,
    unresolved: &mut Vec<String>,
) -> Result<[Option<i64>; 2], Failure> {
    let mut created = BTreeSet::new();
    let mut died = BTreeSet::new();
    for tick in FIRST_TICK..=reader.terminal_tick() {
        let events = reader.events(tick).map_err(|error| {
            Failure::refused(format!("recording has no events at tick {tick}: {error}"))
        })?;
        for event in events.events {
            let Some(subject) = event
                .subject
                .filter(|subject| subject.kind == ObjectKind::Unit)
            else {
                continue;
            };
            match event.payload {
                EventPayload::UnitCreated { .. } => {
                    created.insert(subject.id);
                }
                EventPayload::UnitDied { .. } => {
                    died.insert(subject.id);
                }
                _ => {}
            }
        }
    }
    let opened_in: BTreeMap<u64, u64> = opened
        .live_units
        .iter()
        .map(|unit| (unit.unit_id, unit.formation_id))
        .collect();
    let mut scores = [Some(0_i64), Some(0_i64)];
    for unit in ended
        .live_units
        .iter()
        .filter(|unit| unit.life.current > 0 && unit.active)
    {
        let side = match unit.team_id {
            0 => Side::Blue,
            1 => Side::Red,
            other => {
                return Err(Failure::refused(format!(
                    "recording holds team {other}, and a match has two"
                )));
            }
        };
        let name = mechcore_document::unit_type_from_id(
            i32::try_from(unit.unit_type_id).unwrap_or(i32::MAX),
        )
        .map_or_else(
            || unit.unit_type_id.to_string(),
            |(name, _)| name.to_owned(),
        );
        // A unit that changes sides joins a formation of the side it serves, so
        // where it came from is the formation it opened the fight in.
        let placement = opened_in
            .get(&unit.unit_id)
            .and_then(|formation| formations.get(formation))
            .and_then(|(owner, index)| {
                scene::units_of(layout, *owner)
                    .iter()
                    .find(|placement| placement.index == *index)
            });
        let survivor = Scored {
            unit_type: unit.unit_type_id,
            level: placement.map(|placement| placement.level.unwrap_or(1)),
            support: created.contains(&unit.unit_id) || placement.is_none(),
            reborn: died.contains(&unit.unit_id),
            team_changed: unit.team_id != unit.original_team_id,
        };
        match reactor_damage::score(survivor) {
            Ok(score) => {
                if let Some(total) = &mut scores[side.seat()] {
                    *total += score;
                }
            }
            Err(reason) => {
                unresolved.push(format!(
                    "reactor_core: unit {} of {name} on {}: {reason}",
                    unit.unit_id,
                    side.name()
                ));
                scores[side.seat()] = None;
            }
        }
    }
    Ok(scores)
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
