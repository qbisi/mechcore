//! The buff a buff source adds, whatever owns it.
//!
//! A `BuffEquipment` and a `BuffTech` are both an `IEffectBuffDataSource`, and
//! `BuffEffectProvider` reads one the same way whichever it is: what triggers
//! the buff (`GetBuffTechListener`), whom it reaches (`GetEffectTargetTypes`),
//! how likely (`GetProbablity`), and the `buffDatas` row it adds
//! (`GetBuffData`). The one trigger read is the fight's start onto the unit
//! itself, which hands its unit a [`StartBuff`].

use serde::Deserialize;

use super::sources::{Stacking, StartBuff};

/// `BuffTechListener.FightStart`.
const FIGHT_START: i32 = 1;

/// `TargetType.MechUnit`: the unit the buff's source is on.
const MECH_UNIT: i32 = 1;

/// One, Q32.32.
const ONE: i64 = 1 << 32;

/// `BuffEffectAdditiveCondition.Time`: a stack a step.
const STACK_BY_TIME: i32 = 1;

/// A `buffDatas` row a buff source adds, with the fields the simulator reads.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "the buff row's flags are independent fields"
)]
pub(crate) struct BuffBlock {
    id: u32,
    name: String,
    duration: i64,
    divide: i32,
    additive: bool,
    debuff: bool,
    invincible: bool,
    disable_technology: bool,
    amplify_damage_rate: i64,
    damage_rate: i64,
    max_life_rate: i64,
    step_time: i64,
    additive_effect: bool,
    additive_condition: i32,
    max_additive_stack: u32,
    /// `isClearSelfBuffWhenDisableTech`. A buff is cleared by it only when
    /// its unit's technologies are disabled, which no fight here does to a
    /// unit with such a buff; nothing reads it.
    #[allow(
        dead_code,
        reason = "no read buff runs on a unit whose technologies go off"
    )]
    clear_when_technologies_disabled: bool,
    /// The other fields it sets.
    #[serde(default)]
    special: Vec<String>,
}

/// The buff a source adds as the fight starts, or why this build will not
/// apply it. `BuffCycleController.OnEnterFight` runs a controller whose
/// listener is `FightStart`, and its first `Update` triggers once, since a
/// source with no delay and no interval does not cycle.
///
/// A buff that stacks (`IsAdditiveEffect`) is read when it stacks a step at
/// a time (`BuffAdditiveStackConditionTimeController`) and every rate it
/// stacks raises: `IBEC_AdditiveEffectBuff` multiplies each by the stack, and
/// enhancements sum, where impairments would compound.
pub(crate) fn start_buff(
    who: &str,
    (trigger, targets, probability): (Option<i32>, &[i32], Option<i64>),
    source_special: &[String],
    buff: Option<&BuffBlock>,
) -> std::result::Result<StartBuff, String> {
    let Some(buff) = buff else {
        return Err(format!("{who} names no buff"));
    };
    if trigger != Some(FIGHT_START) || targets != [MECH_UNIT] {
        return Err(format!(
            "{who} adds its buff on BuffTechListener {trigger:?} to TargetTypes {targets:?}, \
             and only the fight's start onto the unit itself is read"
        ));
    }
    if probability != Some(ONE) {
        return Err(format!(
            "{who} adds its buff with probability {probability:?}, and only a certain one is \
             read"
        ));
    }
    if !source_special.is_empty() {
        return Err(format!(
            "{who} sets {}, which no mechanism here reads",
            source_special.join(", ")
        ));
    }
    if !buff.special.is_empty() || buff.disable_technology {
        return Err(format!(
            "{who} adds buff {} ({}), which sets {}, and no mechanism here reads it on a \
             unit's own buff",
            buff.id,
            buff.name,
            if buff.disable_technology {
                "disableTechnology".to_owned()
            } else {
                buff.special.join(", ")
            }
        ));
    }
    let stacking = if buff.additive_effect {
        if buff.additive_condition != STACK_BY_TIME
            || buff.step_time <= 0
            || [
                buff.amplify_damage_rate,
                buff.damage_rate,
                buff.max_life_rate,
            ]
            .iter()
            .any(|rate| *rate < 0)
        {
            return Err(format!(
                "{who} adds buff {} ({}), which stacks on condition {} or lowers what it \
                 stacks, and only a stack a step raising its rates is read",
                buff.id, buff.name, buff.additive_condition
            ));
        }
        Some(Stacking {
            max: buff.max_additive_stack,
        })
    } else {
        None
    };
    Ok(StartBuff {
        buff_id: buff.id,
        divide: buff.divide,
        additive: buff.additive,
        duration_q32: buff.duration,
        debuff: buff.debuff,
        invincible: buff.invincible,
        amplify_damage_rate: buff.amplify_damage_rate,
        damage_rate: buff.damage_rate,
        max_life_rate: buff.max_life_rate,
        step_q32: buff.step_time,
        stacking,
    })
}
