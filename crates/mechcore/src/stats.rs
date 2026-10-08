//! What a recording holds about a unit's numbers at one tick: the buffs it
//! holds, and the numbers the build computed after every correction on it.
//!
//! This is not what a fight decided — [`crate::outcome`] answers that, and a
//! correction is an input to a fight rather than an outcome of one. The
//! corrections a technology, an officer or an equipment writes are not
//! recorded; the numbers they come to are.
//!
//! The default tick is the first, where a correction applied as the fight is
//! built has landed and nothing the fight does has moved it yet. A mechanism
//! that writes during the fight — a buff, a commander skill — is read at the
//! tick it is expected at instead.

use std::{collections::BTreeMap, path::Path};

use mechcore_mcfr::{BuffDataKind, LiveUnitState, McfrReader, WorldSnapshot};
use serde::Serialize;

use crate::{
    cli::Failure,
    scene::{self, FIRST_TICK},
    turn::Side,
};

pub(crate) const SCHEMA: &str = "mechcore.fight-stats.v4";

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
    /// the order of the first member in each. A buff the build hands a single
    /// member, one it takes on being hit, splits that member off rather than
    /// being lost behind another.
    readings: Vec<Reading>,
}

/// The state some members of a formation share at this tick.
#[derive(Serialize, Clone, PartialEq)]
struct Reading {
    /// The members in this state, by the unit id the recording names them by.
    units: Vec<u64>,
    /// The buffs these members hold, in the build's order. Absent when they
    /// hold none.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    buffs: Vec<HeldBuff>,
    /// The numbers the fight reads, after every correction on them: the
    /// unit's speed, Q32.32, and each of its skills'.
    move_speed: i64,
    skills: Vec<SkillNumbers>,
}

/// One buff a unit holds: its `buffDatas` row, or the technology that serves
/// as its own buff data, and its stack when it stacks.
#[derive(Serialize, Clone, PartialEq)]
struct HeldBuff {
    #[serde(skip_serializing_if = "Option::is_none")]
    buff_id: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    technology_id: Option<u32>,
    #[serde(skip_serializing_if = "is_zero")]
    stacks: i32,
}

impl HeldBuff {
    fn of(unit: &LiveUnitState) -> Vec<Self> {
        unit.buffs
            .iter()
            .map(|buff| {
                let id = Some(buff.data.id);
                let (buff_id, technology_id) = match buff.data.kind {
                    BuffDataKind::Buff => (id, None),
                    BuffDataKind::Technology => (None, id),
                };
                Self {
                    buff_id,
                    technology_id,
                    stacks: buff.stacks,
                }
            })
            .collect()
    }
}

#[allow(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde passes a reference"
)]
const fn is_zero(value: &i32) -> bool {
    *value == 0
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

/// Reads one tick of a recording for its units' buffs and numbers.
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
            buffs: HeldBuff::of(unit),
            move_speed: unit.move_speed,
            skills: SkillNumbers::of(unit),
        };
        let readings = held.entry(*formation).or_default();
        match readings.iter_mut().find(|known| {
            (&known.buffs, known.move_speed, &known.skills)
                == (&reading.buffs, reading.move_speed, &reading.skills)
        }) {
            Some(known) => known.units.push(unit.unit_id),
            None => readings.push(reading),
        }
    }
    held
}
