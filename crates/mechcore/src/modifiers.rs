//! What a recording holds written onto its units, per channel.
//!
//! This is not what a fight decided — [`crate::outcome`] answers that, and a
//! correction is an input to a fight rather than an outcome of one. This reads
//! the other half of a capture: what the build *stored* when a mechanism was
//! applied, beside what it then computed. A rate that reads `+0.6` in one
//! channel and a damage that reads 1.6 times the description are the same fact
//! seen twice; a rate that reads `+0.3` twice over is a different one.
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

use mechcore_mcfr::{
    BuffModifierSet, LiveUnitState, McfrReader, SkillNumericModifierState, UnitDynamicModifierSet,
    WorldSnapshot,
};
use serde::Serialize;

use crate::{
    cli::Failure,
    scene::{self, FIRST_TICK},
    turn::Side,
};

pub(crate) const SCHEMA: &str = "mechcore.fight-modifiers.v1";

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

/// One formation and what was written onto it, in document index order.
///
/// A formation answers whether or not it survives the fight: the side that
/// spends a correction attacking is commonly the side that loses the unit
/// carrying it, and reading the correction off the winner only would miss
/// exactly the case a capture is designed around.
#[derive(Serialize)]
struct Formation {
    index: i32,
    name: String,
    #[serde(flatten)]
    held: Modifiers,
}

/// What a mechanism wrote onto a unit, as the recording holds it.
///
/// A neutral channel is left out rather than printed as zeroes, so what a
/// reading says is what was written.
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
                    let held = held.get(&(side, placement.index))?;
                    Some(Formation {
                        index: placement.index,
                        name: placement.type_name.clone(),
                        held: Modifiers {
                            buff: held.buff,
                            unit: held.unit,
                            skill: held.skill.clone(),
                        },
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

/// What each formation carried, for the formations that carried anything.
///
/// One member answers for its formation: a correction is written onto the
/// formation the build hands it to, so its members hold the same entries.
fn carried(
    formations: &BTreeMap<u64, (Side, i32)>,
    state: &WorldSnapshot,
) -> BTreeMap<(Side, i32), Modifiers> {
    let mut held = BTreeMap::new();
    for unit in &state.live_units {
        let Some(formation) = formations.get(&unit.formation_id) else {
            continue;
        };
        if let Some(modifiers) = Modifiers::of(unit) {
            held.entry(*formation).or_insert(modifiers);
        }
    }
    held
}
