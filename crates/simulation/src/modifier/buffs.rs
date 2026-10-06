//! The buff a buff source adds, whatever owns it.
//!
//! A `BuffEquipment` and a `BuffTech` are both an `IEffectBuffDataSource`, and
//! `BuffEffectProvider` reads one the same way whichever it is: what triggers
//! the buff (`GetBuffTechListener`), whom it reaches (`GetEffectTargetTypes`),
//! how likely (`GetProbablity`), how its `BuffCycleController` finds and
//! times its targets, and the `buffDatas` row it adds (`GetBuffData`). The one
//! trigger read is the fight's start, which hands its unit a [`BuffSource`].

use serde::Deserialize;

use super::sources::{BuffReach, BuffSource, BuffTargets, StackCondition, Stacking};
use crate::rules::AttackTargets;

/// `BuffTechListener.FightStart`.
const FIGHT_START: i32 = 1;

/// `TargetType.MechUnit`: the unit the buff's source is on.
const MECH_UNIT: i32 = 1;

/// `TargetType.OtherSelfUnits`, `FriendUnits` and `OpponentUnits`.
const OTHER_SELF_UNITS: i32 = 2;
const FRIEND_UNITS: i32 = 3;
const OPPONENT_UNITS: i32 = 4;

/// `BuffTargetUpdateModel.Each`: a `RangeUnitCycle` keeps the buff on every
/// unit in reach.
const EACH: i32 = 1;

/// `AttackTargetType.Ground`, `Air` and `Both`.
const GROUND: i32 = 0;
const AIR: i32 = 1;
const BOTH: i32 = 2;

/// One, Q32.32.
const ONE: i64 = 1 << 32;

/// `BuffEffectAdditiveCondition.Time`: a stack a step.
const STACK_BY_TIME: i32 = 1;

/// `BuffEffectAdditiveCondition.Distance`: a stack for so many metres moved.
const STACK_BY_DISTANCE: i32 = 2;

/// The fields of a buff source its `BuffCycleController` reads, those set.
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct CycleBlock {
    /// `max`, Q32.32 metres.
    range: i64,
    /// `intervalTime` and `delayTime`, Q32.32 seconds.
    interval: i64,
    delay: i64,
    /// `targetFlyType`, an `AttackTargetType`.
    fly_type: i32,
    /// `targetDamageDistanceType`.
    distance_type: i32,
    /// `buffTargetUpdateModel`.
    update_model: i32,
    /// `isDistanceCalculateTargetRadius` and `isDistanceCalculateSelfRadius`.
    target_radius: bool,
    self_radius: bool,
}

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
    speed_rate: i64,
    attack_range_value: i64,
    max_life_rate: i64,
    step_time: i64,
    additive_effect: bool,
    additive_condition: i32,
    /// `buffEffectAdditiveConditionParam`, Q32.32.
    additive_condition_param: i64,
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

/// The buff a source adds as the fight starts, and to whom, or why this build
/// will not apply it. `BuffCycleController.OnEnterFight` runs a controller
/// whose listener is `FightStart`. Under `BuffTargetUpdateModel.All` its first
/// `Update` triggers once onto the unit itself, since a source with no delay
/// and no interval does not cycle. Under `Each` it hands its `Update` to a
/// `RangeUnitCycle`, which keeps the buff on every unit in reach; the
/// controller's constructor never gives the cycle the source's delay or
/// interval, so neither is read.
///
/// A buff that stacks (`IsAdditiveEffect`) is read when it stacks a step at
/// a time (`BuffAdditiveStackConditionTimeController`) and every rate it
/// stacks raises: `IBEC_AdditiveEffectBuff` multiplies each by the stack, and
/// enhancements sum, where impairments would compound.
pub(crate) fn buff_source(
    who: &str,
    (trigger, targets, probability): (Option<i32>, &[i32], Option<i64>),
    cycle: &CycleBlock,
    source_special: &[String],
    buff: Option<&BuffBlock>,
) -> std::result::Result<BuffSource, String> {
    let Some(buff) = buff else {
        return Err(format!("{who} names no buff"));
    };
    if trigger != Some(FIGHT_START) {
        return Err(format!(
            "{who} adds its buff on BuffTechListener {trigger:?}, and only the fight's start \
             is read"
        ));
    }
    let reach = reach(who, targets, cycle)?;
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
             source's buff",
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
        let condition = match buff.additive_condition {
            STACK_BY_TIME => Some(StackCondition::Time),
            STACK_BY_DISTANCE if buff.additive_condition_param > 0 => {
                Some(StackCondition::Distance {
                    metres_q32: buff.additive_condition_param,
                })
            }
            _ => None,
        };
        let lowers = [
            buff.amplify_damage_rate,
            buff.damage_rate,
            buff.speed_rate,
            buff.attack_range_value,
            buff.max_life_rate,
        ]
        .iter()
        .any(|rate| *rate < 0);
        match condition {
            Some(condition) if buff.step_time > 0 && !lowers => Some(Stacking {
                max: buff.max_additive_stack,
                condition,
            }),
            _ => {
                return Err(format!(
                    "{who} adds buff {} ({}), which stacks on condition {} or lowers what it \
                     stacks, and only a stack by time or distance raising what it writes is \
                     read",
                    buff.id, buff.name, buff.additive_condition
                ));
            }
        }
    } else {
        None
    };
    Ok(BuffSource {
        buff_id: buff.id,
        divide: buff.divide,
        additive: buff.additive,
        duration_q32: buff.duration,
        debuff: buff.debuff,
        invincible: buff.invincible,
        amplify_damage_rate: buff.amplify_damage_rate,
        damage_rate: buff.damage_rate,
        speed_rate: buff.speed_rate,
        attack_range_value: buff.attack_range_value,
        max_life_rate: buff.max_life_rate,
        step_q32: buff.step_time,
        stacking,
        reach,
    })
}

/// Whom a source's controller gives its buff: under `All`, the unit itself
/// once, and under `Each`, the units in reach that its target types name.
fn reach(
    who: &str,
    targets: &[i32],
    cycle: &CycleBlock,
) -> std::result::Result<Option<BuffReach>, String> {
    if cycle.update_model != EACH {
        if targets != [MECH_UNIT] || cycle.interval != 0 || cycle.delay != 0 {
            return Err(format!(
                "{who} adds its buff under BuffTargetUpdateModel {} to TargetTypes {targets:?} \
                 after {} and every {} (Q32.32 seconds), and only once onto the unit itself \
                 is read",
                cycle.update_model, cycle.delay, cycle.interval
            ));
        }
        return Ok(None);
    }
    if cycle.self_radius {
        return Err(format!(
            "{who} measures its reach from its unit's edge (isDistanceCalculateSelfRadius), \
             whose radius is not measured"
        ));
    }
    if cycle.distance_type != 0 {
        return Err(format!(
            "{who} reaches only units of DamageDistanceType {}, which no mechanism here reads",
            cycle.distance_type
        ));
    }
    let mut named = BuffTargets::default();
    for &target in targets {
        match target {
            MECH_UNIT => named.itself = true,
            OTHER_SELF_UNITS => named.own_others = true,
            FRIEND_UNITS => named.friends = true,
            OPPONENT_UNITS => named.opponents = true,
            other => {
                return Err(format!(
                    "{who} adds its buff to TargetType {other}, and only units are read"
                ));
            }
        }
    }
    let domains = match cycle.fly_type {
        GROUND => AttackTargets {
            ground: true,
            air: false,
        },
        AIR => AttackTargets {
            ground: false,
            air: true,
        },
        BOTH => AttackTargets {
            ground: true,
            air: true,
        },
        other => return Err(format!("{who} reaches AttackTargetType {other}")),
    };
    Ok(Some(BuffReach {
        range_q32: cycle.range,
        domains,
        target_radius: cycle.target_radius,
        targets: named,
    }))
}
