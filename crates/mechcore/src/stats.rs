//! What a recording holds about a unit's numbers at one tick: the corrections
//! written onto it, and the numbers the build then computed from them.
//!
//! This is not what a fight decided — [`crate::outcome`] answers that, and a
//! correction is an input to a fight rather than an outcome of one. These are
//! the two halves of a measurement, and they are one reader because a capture
//! wants them together: a rate that reads `+0.6` in one channel beside a
//! damage that reads 1.6 times the description is one fact seen twice, and a
//! rate that reads `+0.3` twice over is a different one.
//!
//! The three channels stay apart because the build keeps them apart and MCFR
//! records them apart: the unit's own `DataSet`, its skills', and the
//! `BuffManager`'s aggregate. Which one a correction lands in is half of what
//! a capture is taken to find out.
//!
//! The default tick is the first, where a correction applied as the fight is
//! built has landed and nothing the fight does has moved it yet. A mechanism
//! that writes during the fight — a buff, a commander skill — is read at the
//! tick it is expected at instead.

use std::{collections::BTreeMap, path::Path};

use mechcore_mcfr::{LiveUnitState, McfrReader, ModifierChannel, ModifierPart, WorldSnapshot};
use serde::Serialize;
use serde_json::{Map, Value};

use crate::{
    cli::Failure,
    scene::{self, FIRST_TICK},
    turn::Side,
};

pub(crate) const SCHEMA: &str = "mechcore.fight-stats.v3";

/// Every correction one tick of a recording holds.
#[derive(Serialize)]
pub(crate) struct Written {
    schema: &'static str,
    recording: String,
    /// The tick this was read at, and the last one the recording holds.
    tick: u32,
    ticks: u32,
    sides: Sides,
}

#[derive(Serialize)]
struct Sides {
    blue: Vec<Formation>,
    red: Vec<Formation>,
}

/// One formation, what was written onto its members, and what the build
/// computed.
///
/// A formation answers whether or not it survives the fight: the side that
/// spends a correction attacking is commonly the side that loses the unit
/// carrying it, and reading the correction off the winner only would miss
/// exactly the case a capture is designed around.
#[derive(Serialize)]
struct Formation {
    index: i32,
    name: String,
    /// One reading for each distinct state its standing members are in, in
    /// the order of the first member in each. A correction handed to the
    /// formation is written onto every member alike and reads as one; one
    /// the build hands a single member — a buff it takes on being hit —
    /// splits that member off rather than being lost behind another.
    readings: Vec<Reading>,
}

/// The state some members of a formation share at this tick.
#[derive(Serialize, Clone, PartialEq)]
struct Reading {
    /// The members in this state, by the unit id the recording names them by.
    units: Vec<u64>,
    /// Whether these members' technologies are switched off, which is the
    /// state a correction's absence is explained by rather than a correction
    /// of its own. Electromagnetic interference sets it for as long as it
    /// lasts. Absent when they are not.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    technologies_disabled: bool,
    /// The numbers the fight reads, after every correction on them: the
    /// unit's speed, Q32.32, and each of its skills'.
    move_speed: i64,
    skills: Vec<SkillNumbers>,
    #[serde(flatten)]
    held: Modifiers,
}

/// What one skill's own properties answer, by its slot; a skill a buff has
/// switched off answers nothing but that.
#[derive(Serialize, Clone, PartialEq)]
struct SkillNumbers {
    skill_slot: u16,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    switched_off: bool,
    /// Q32.32 metres.
    #[serde(skip_serializing_if = "Option::is_none")]
    attack_range: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    attack_damage: Option<i32>,
    /// Logic ticks, stagger included.
    #[serde(skip_serializing_if = "Option::is_none")]
    current_attack_interval: Option<i32>,
}

impl SkillNumbers {
    fn of(unit: &LiveUnitState) -> Vec<Self> {
        unit.skills
            .iter()
            .map(|skill| {
                let enabled = skill.enabled.as_ref();
                Self {
                    skill_slot: skill.skill_slot,
                    switched_off: enabled.is_none(),
                    attack_range: enabled.map(|skill| skill.attack_range),
                    attack_damage: enabled.map(|skill| skill.attack_damage),
                    current_attack_interval: enabled.map(|skill| skill.current_attack_interval),
                }
            })
            .collect()
    }
}

/// What a mechanism wrote onto a unit, as the recording holds it: its
/// modifiers grouped by where they are written, each field a number for a
/// value and its non-zero `add` and `reduce` for a rate.
///
/// A neutral channel is left out rather than printed as zeroes, so what a
/// reading says is what was written.
#[derive(Serialize, Clone, Default, PartialEq)]
struct Modifiers {
    #[serde(skip_serializing_if = "Map::is_empty")]
    buff: Map<String, Value>,
    #[serde(skip_serializing_if = "Map::is_empty")]
    unit: Map<String, Value>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    skill: Vec<SkillModifiers>,
}

#[derive(Serialize, Clone, PartialEq)]
struct SkillModifiers {
    skill_slot: u16,
    modifiers: Map<String, Value>,
}

impl Modifiers {
    fn of(unit: &LiveUnitState) -> Self {
        let mut held = Self::default();
        for modifier in &unit.modifiers {
            let fields = match (modifier.channel, modifier.skill_slot) {
                (ModifierChannel::Buff, _) => &mut held.buff,
                (channel, Some(slot)) if channel.is_skill() => {
                    if held
                        .skill
                        .last()
                        .is_none_or(|skill| skill.skill_slot != slot)
                    {
                        held.skill.push(SkillModifiers {
                            skill_slot: slot,
                            modifiers: Map::new(),
                        });
                    }
                    &mut held.skill.last_mut().expect("just pushed").modifiers
                }
                _ => &mut held.unit,
            };
            match modifier.part {
                ModifierPart::Value => {
                    fields.insert(modifier.field.clone(), Value::from(modifier.value));
                }
                ModifierPart::Add | ModifierPart::Reduce => {
                    let part = if modifier.part == ModifierPart::Add {
                        "add"
                    } else {
                        "reduce"
                    };
                    if let Value::Object(parts) = fields
                        .entry(modifier.field.clone())
                        .or_insert_with(|| Value::Object(Map::new()))
                    {
                        parts.insert(part.to_owned(), Value::from(modifier.value));
                    }
                }
            }
        }
        held
    }
}

/// Reads one tick of a recording for the corrections it holds.
///
/// # Errors
///
/// Returns a failure when the file is not a recording this build reads, when
/// it holds no such tick, or when its scene cannot be matched to the layout it
/// embeds.
pub(crate) fn read(path: &Path, tick: Option<u32>) -> Result<Written, Failure> {
    let reader = McfrReader::open(path)
        .map_err(|error| Failure::refused(format!("cannot read {}: {error}", path.display())))?;
    let layout = scene::layout(&reader)?;
    let opened = scene::snapshot(&reader, FIRST_TICK)?;
    let formations = scene::formations(&layout, &opened)?;
    let tick = tick.unwrap_or(FIRST_TICK);
    let state = if tick == FIRST_TICK {
        opened
    } else {
        scene::snapshot(&reader, tick)?
    };
    let held = carried(&formations, &state);

    let mut answered = Vec::new();
    for side in Side::BOTH {
        answered.push(
            scene::units_of(&layout, side)
                .iter()
                .filter_map(|placement| {
                    Some(Formation {
                        index: placement.index,
                        name: placement.type_name.clone(),
                        readings: held.get(&(side, placement.index))?.clone(),
                    })
                })
                .collect(),
        );
    }
    let red = answered.pop().expect("both sides were read");
    let blue = answered.pop().expect("both sides were read");
    Ok(Written {
        schema: SCHEMA,
        recording: path.display().to_string(),
        tick,
        ticks: reader.terminal_tick(),
        sides: Sides { blue, red },
    })
}

/// What each formation's members hold at this tick, for every formation that
/// stands, members in the same state read together.
///
/// A formation carrying no correction is still answered, because its numbers
/// are the description itself and that is what a control is read for.
fn carried(
    formations: &BTreeMap<u64, (Side, i32)>,
    state: &WorldSnapshot,
) -> BTreeMap<(Side, i32), Vec<Reading>> {
    let mut held: BTreeMap<(Side, i32), Vec<Reading>> = BTreeMap::new();
    for unit in &state.live_units {
        let Some(formation) = formations.get(&unit.formation_id) else {
            continue;
        };
        let reading = Reading {
            units: vec![unit.unit_id],
            technologies_disabled: unit.status_mask & TECHNOLOGY_DISABLED != 0,
            move_speed: unit.move_speed,
            skills: SkillNumbers::of(unit),
            held: Modifiers::of(unit),
        };
        let readings = held.entry(*formation).or_default();
        match readings.iter_mut().find(|known| {
            (
                known.technologies_disabled,
                known.move_speed,
                &known.skills,
                &known.held,
            ) == (
                reading.technologies_disabled,
                reading.move_speed,
                &reading.skills,
                &reading.held,
            )
        }) {
            Some(known) => known.units.push(unit.unit_id),
            None => readings.push(reading),
        }
    }
    held
}

/// The bit `status_mask` keeps `FightMech.IsTechnologyDisabled` in.
const TECHNOLOGY_DISABLED: u64 = 1 << 2;
