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

use mechcore_document::{Layout, Position, UnitPlacement};
use mechcore_mcfr::{
    BuffModifierSet, LiveUnitState, McfrReader, SkillNumericModifierState, UnitDynamicModifierSet,
    WorldSnapshot,
};
use serde::Serialize;

use crate::{cli::Failure, turn::Side};

pub(crate) const SCHEMA: &str = "mechcore.fight-outcome.v1";

/// The first tick a recording holds, which is the scene the fight starts from.
const FIRST_TICK: u32 = 1;

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
    /// What was written onto this side's formations before the fight moved
    /// them, whether or not they came out of it. A correction is commonly
    /// carried by the side that spends it attacking, and a formation that
    /// dies carried it just as much as one that lives.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    modifiers: Vec<Written>,
    /// Which of the things a fight thins out remain, by their place in the
    /// round's own list. Absent when this reader cannot say.
    #[serde(skip_serializing_if = "Option::is_none")]
    contraptions: Option<Vec<usize>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    terrains: Option<Vec<usize>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    airdrop_shields: Option<Vec<usize>>,
}

/// What a mechanism wrote onto a unit, as the recording holds it.
///
/// The three channels are separately attributable, so this is the other half
/// of a capture's reading: what the game stored, beside what it then computed.
/// They are taken at the first tick, which is where a correction applied as the
/// fight is built has landed and nothing a fight does has moved it yet.
#[derive(Serialize)]
struct Modifiers {
    #[serde(skip_serializing_if = "neutral_buff")]
    buff: BuffModifierSet,
    #[serde(skip_serializing_if = "neutral_unit")]
    unit: UnitDynamicModifierSet,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    skill: Vec<SkillNumericModifierState>,
}

fn neutral_buff(held: &BuffModifierSet) -> bool {
    held.is_zero()
}

fn neutral_unit(held: &UnitDynamicModifierSet) -> bool {
    held.is_zero()
}

impl Modifiers {
    fn of(unit: &LiveUnitState) -> Option<Self> {
        let held = Self {
            buff: unit.buff_modifiers,
            unit: unit.unit_dynamic_modifiers,
            skill: unit.skill_dynamic_modifiers.clone(),
        };
        let neutral = held.buff.is_zero() && held.unit.is_zero() && held.skill.is_empty();
        (!neutral).then_some(held)
    }
}

/// One formation and what a mechanism had written onto it.
#[derive(Serialize)]
struct Written {
    index: i32,
    name: String,
    #[serde(flatten)]
    held: Modifiers,
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
    let layout: Layout = mechcore_document::parse_yaml(reader.layout_yaml().as_bytes())
        .map_err(|error| Failure::refused(format!("recording embeds no layout: {error}")))?;
    let last = reader.terminal_tick();
    let opened = snapshot(reader, FIRST_TICK)?;
    let ended = snapshot(reader, last)?;
    let formations = formations(&layout, &opened)?;
    let started = members(&formations, &opened);
    let survived = members(&formations, &ended);
    let written = written(&formations, &opened);

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
        let placed = units_of(&layout, side);
        let carried = layout_side(&layout, side);
        answered.push(SideOutcome {
            survivors: survivors(side, &started, &survived, placed),
            modifiers: corrections(side, &written, placed),
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

fn snapshot(reader: &McfrReader, tick: u32) -> Result<WorldSnapshot, Failure> {
    reader
        .state(tick)
        .map_err(|error| Failure::refused(format!("recording has no tick {tick}: {error}")))
}

fn layout_side(layout: &Layout, side: Side) -> &mechcore_document::Side {
    match side {
        Side::Blue => &layout.blue,
        Side::Red => &layout.red,
    }
}

fn units_of(layout: &Layout, side: Side) -> &[UnitPlacement] {
    &layout_side(layout, side).units
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

/// Which layout formation each recorded formation is.
///
/// A recording numbers its formations by where their members stand rather than
/// by the order a layout declares them, so the two are matched by the one
/// thing they share: a formation's members stand in its own slot. Each
/// formation's members are averaged at the first tick and the nearest
/// placement of the same type on the same side is its own. A tie is refused
/// rather than broken, because a formation that cannot be named cannot be
/// written back into a document.
fn formations(
    layout: &Layout,
    opened: &WorldSnapshot,
) -> Result<BTreeMap<u64, (Side, i32)>, Failure> {
    let mut grouped: BTreeMap<u64, Vec<&LiveUnitState>> = BTreeMap::new();
    for unit in &opened.live_units {
        grouped.entry(unit.formation_id).or_default().push(unit);
    }
    let mut named = BTreeMap::new();
    for (formation, members) in &grouped {
        let first = members[0];
        let side = match first.team_id {
            0 => Side::Blue,
            1 => Side::Red,
            other => {
                return Err(Failure::refused(format!(
                    "recording holds team {other}, and a match has two"
                )));
            }
        };
        let name = type_name(first.unit_type_id)?;
        let centre = centre(members);
        let mut nearest: Option<(i64, i32)> = None;
        let mut tied = false;
        for placement in units_of(layout, side) {
            if placement.type_name != name {
                continue;
            }
            let distance = distance(world(placement.position, side), centre);
            match nearest {
                Some((best, _)) if distance > best => {}
                Some((best, _)) if distance == best => tied = true,
                _ => {
                    nearest = Some((distance, placement.index));
                    tied = false;
                }
            }
        }
        let Some((_, index)) = nearest.filter(|_| !tied) else {
            return Err(Failure::refused(format!(
                "formation {formation} of {name} on {} matches no single placement \
                 of the layout the recording embeds",
                side.name()
            )));
        };
        named.insert(*formation, (side, index));
    }
    Ok(named)
}

fn type_name(unit_type: u32) -> Result<&'static str, Failure> {
    i32::try_from(unit_type)
        .ok()
        .and_then(mechcore_document::unit_type_from_id)
        .map(|(name, _)| name)
        .ok_or_else(|| {
            Failure::refused(format!(
                "recording holds unit type {unit_type}, which this build does not name"
            ))
        })
}

/// A side's placement in the world the fight runs in.
///
/// A layout states each side's board in its own frame, and red's is the same
/// board turned around, which is the transform the simulator applies when it
/// builds the scene.
const fn world(position: Position, side: Side) -> (i64, i64) {
    let (x, z) = (position.x as i64, position.y as i64);
    match side {
        Side::Blue => (x, z),
        Side::Red => (-x, -z),
    }
}

/// Where a formation's members stand on average, in world units.
fn centre(members: &[&LiveUnitState]) -> (i64, i64) {
    let count = i64::try_from(members.len()).unwrap_or(1).max(1);
    let sum = members.iter().fold((0_i64, 0_i64), |sum, unit| {
        (
            sum.0 + (unit.position.x >> 32),
            sum.1 + (unit.position.z >> 32),
        )
    });
    (sum.0 / count, sum.1 / count)
}

/// Squared distance, which orders as the distance does and needs no root.
const fn distance(left: (i64, i64), right: (i64, i64)) -> i64 {
    let (x, z) = (left.0 - right.0, left.1 - right.1);
    x * x + z * z
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

/// What each formation carried at the first tick, for the formations that
/// carried anything.
fn written(
    formations: &BTreeMap<u64, (Side, i32)>,
    opened: &WorldSnapshot,
) -> BTreeMap<(Side, i32), Modifiers> {
    let mut held = BTreeMap::new();
    for unit in &opened.live_units {
        let Some(formation) = formations.get(&unit.formation_id) else {
            continue;
        };
        if let Some(modifiers) = Modifiers::of(unit) {
            held.entry(*formation).or_insert(modifiers);
        }
    }
    held
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

/// One side's formations that carried a correction, in document index order.
fn corrections(
    side: Side,
    written: &BTreeMap<(Side, i32), Modifiers>,
    placed: &[UnitPlacement],
) -> Vec<Written> {
    placed
        .iter()
        .filter_map(|placement| {
            let held = written.get(&(side, placement.index))?;
            Some(Written {
                index: placement.index,
                name: placement.type_name.clone(),
                held: Modifiers {
                    buff: held.buff,
                    unit: held.unit,
                    skill: held.skill.clone(),
                },
            })
        })
        .collect()
}
