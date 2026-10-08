//! The buff a buff source adds, whatever owns it.
//!
//! A `BuffEquipment` and a `BuffTech` are both an `IEffectBuffDataSource`, and
//! `BuffEffectProvider` reads one the same way whichever it is: what triggers
//! the buff (`GetBuffTechListener`), whom it reaches (`GetEffectTargetTypes`),
//! how likely (`GetProbablity`), how its `BuffCycleController` finds and
//! times its targets, and the `buffDatas` row it adds (`GetBuffData`). The
//! triggers read are the fight's start, a hit, the unit's being hit and its
//! losing life, each
//! of which hands its unit a [`BuffSource`].

use serde::Deserialize;

use super::sources::{
    AllCycle, BuffReach, BuffSource, BuffTargets, BuffTrigger, DeadSummon, StackCondition, Stacking,
};
use crate::{layout::TerrainSpec, rules::AttackTargets};

/// `BuffEffectAdditiveResetCondition.None` and `Hitted`.
const NO_RESET: i32 = 0;
const RESET_ON_HIT: i32 = 1;

/// `DamageDistanceType.None`, `Melee` and `remote`.
const NO_DISTANCE_TYPE: i32 = 0;
const MELEE: i32 = 1;
const REMOTE: i32 = 2;

/// `BuffTechListener.Hit`, `FightStart` and `GetDamage`.
const HIT: i32 = 0;
const FIGHT_START: i32 = 1;
const GET_DAMAGE: i32 = 3;

/// `BuffTechListener.BeHit`.
const BE_HIT: i32 = 2;

/// `RangeItemType.Acid`.
const ACID: i32 = 3;

/// `TargetType.MechUnit`: the unit the buff's source is on.
const MECH_UNIT: i32 = 1;

/// `TargetType.OtherSelfUnits`, `FriendUnits` and `OpponentUnits`.
const OTHER_SELF_UNITS: i32 = 2;
const FRIEND_UNITS: i32 = 3;
const OPPONENT_UNITS: i32 = 4;

/// `BuffTargetUpdateModel.All`: the controller counts its delay and interval
/// itself.
const ALL: i32 = 0;

/// `BuffTargetUpdateModel.Each`: a `RangeUnitCycle` keeps the buff on every
/// unit in reach.
const EACH: i32 = 1;

/// `AttackTargetType.Ground`, `Air` and `Both`.
const GROUND: i32 = 0;
const AIR: i32 = 1;
const BOTH: i32 = 2;

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
    attack_range_rate: i64,
    max_life_rate: i64,
    step_time: i64,
    additive_effect: bool,
    additive_condition: i32,
    /// `buffEffectAdditiveConditionParam`, Q32.32.
    additive_condition_param: i64,
    max_additive_stack: u32,
    /// `isClearSelfBuffWhenDisableTech`.
    clear_when_technologies_disabled: bool,
    /// `summonUnitID` and `isSummonUnitLevelInherit`.
    summon_unit: i32,
    summon_level_inherit: bool,
    /// `lifeChangeRate`, Q32.32.
    life_change_rate: i64,
    /// `disableRecover`.
    disable_recover: bool,
    /// `buffEffectAdditiveResetCondition`.
    additive_reset_condition: i32,
    /// The other fields it sets.
    #[serde(default)]
    special: Vec<String>,
}

/// A source's `BuffRangeItem`, which its constructor makes when the row names
/// a `triggerRangeItemBuffId`: what a hit leaves where it lands.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RangeItemBlock {
    /// `GetShowRangeItemType`, a `RangeItemType`.
    kind: i32,
    /// `GetRangeItemRange`, whole metres.
    range: i64,
    /// `GetLifeTime`, Q32.32 seconds.
    life: i64,
    /// `GetRoundDuration`.
    rounds: i32,
    /// The `buffDatas` row of `triggerRangeItemBuffId`.
    buff: BuffBlock,
}

/// The buff a source adds, when and to whom, or why this build will not apply
/// it. `BuffCycleController.OnEnterFight` runs a controller whose listener is
/// `FightStart`. Under `BuffTargetUpdateModel.All` its `UpdateModel1`
/// triggers once its delay is over, and again every interval when it has
/// one; it gives the buff to the unit itself when its targets are `MechUnit`
/// alone, and otherwise to the units in reach. Under `Each` it hands its `Update` to a
/// `RangeUnitCycle`, which keeps the buff on every unit in reach; the
/// controller's constructor never gives the cycle the source's delay or
/// interval, so neither is read. `RegisterMechEvent` hands a controller whose
/// listener is `Hit` to its unit's skills as a hit effect, and
/// `TriggerBuffOrBuffRangeItemFromHit` adds its buff through
/// `BuffSystem.AddBuff` to whatever a hit struck, reading none of the
/// source's targets, domains or distance type. `AddListener` hands a
/// controller whose listener is `GetDamage` to its unit's `OnLifeChange`,
/// and `OnGetDamage` adds the buff to the unit itself
/// (`BuffSystem.AddBuffByCheck`) when its targets are `MechUnit` alone,
/// reading none of the cycle; one that reaches the unit's attacker is not
/// read.
///
/// A buff that stacks (`IsAdditiveEffect`) is read when it stacks a step at
/// a time (`BuffAdditiveStackConditionTimeController`) and every rate it
/// stacks raises: `IBEC_AdditiveEffectBuff` multiplies each by the stack, and
/// enhancements sum, where impairments would compound.
pub(crate) fn buff_source(
    who: &str,
    (trigger, targets, probability): (Option<i32>, &[i32], Option<i64>),
    cycle: &CycleBlock,
    can_disable: bool,
    source_special: &[String],
    (buff, range_item): (Option<&BuffBlock>, Option<&RangeItemBlock>),
) -> std::result::Result<BuffSource, String> {
    let Some(buff) = buff else {
        return Err(format!("{who} names no buff"));
    };
    let trigger = match trigger {
        Some(FIGHT_START) if cycle.update_model == EACH => {
            BuffTrigger::Around(reach(who, targets, cycle)?)
        }
        // `UpdateModel1`: the unit itself when its targets are `MechUnit`
        // alone, and otherwise what `CalculateRangeActors` finds.
        Some(FIGHT_START) if cycle.update_model == ALL => BuffTrigger::All(AllCycle {
            delay_q32: cycle.delay,
            interval_q32: cycle.interval,
            reach: if targets == [MECH_UNIT] {
                None
            } else {
                Some(reach(who, targets, cycle)?)
            },
        }),
        Some(HIT) if cycle.update_model != EACH && cycle.interval == 0 && cycle.delay == 0 => {
            BuffTrigger::Hit
        }
        Some(GET_DAMAGE) if targets == [MECH_UNIT] => BuffTrigger::Damaged,
        Some(BE_HIT) if targets == [OPPONENT_UNITS] => BuffTrigger::BeHit,
        Some(FIGHT_START) => {
            return Err(format!(
                "{who} adds its buff under BuffTargetUpdateModel {}, and only All and Each are \
                 read",
                cycle.update_model
            ));
        }
        _ => {
            return Err(format!(
                "{who} adds its buff on BuffTechListener {trigger:?} to {targets:?}, and only \
                 the fight's start, a hit, the unit's being hit and its own losing life are \
                 read"
            ));
        }
    };
    let Some(probability) = probability else {
        return Err(format!("{who} names no probability"));
    };
    if !source_special.is_empty() {
        return Err(format!(
            "{who} sets {}, which no mechanism here reads",
            source_special.join(", ")
        ));
    }
    if !buff.special.is_empty() {
        return Err(format!(
            "{who} adds buff {} ({}), which sets {}, and no mechanism here reads it on a \
             source's buff",
            buff.id,
            buff.name,
            buff.special.join(", ")
        ));
    }
    if buff.life_change_rate > 0 {
        return Err(format!(
            "{who} adds buff {} ({}), which heals, and a buff's healing is not measured",
            buff.id, buff.name
        ));
    }
    let stacking = stacking(who, buff)?;
    Ok(BuffSource {
        buff_id: buff.id,
        divide: buff.divide,
        additive: buff.additive,
        duration_q32: buff.duration,
        debuff: buff.debuff,
        invincible: buff.invincible,
        disables_technology: buff.disable_technology,
        amplify_damage_rate: buff.amplify_damage_rate,
        damage_rate: buff.damage_rate,
        speed_rate: buff.speed_rate,
        attack_range_value: buff.attack_range_value,
        attack_range_rate: buff.attack_range_rate,
        max_life_rate: buff.max_life_rate,
        step_q32: buff.step_time,
        life_change_rate: buff.life_change_rate,
        disables_recover: buff.disable_recover,
        stacking,
        summons: summons(who, buff)?,
        probability: convert_probability(probability),
        range_item: range_item
            .map(|item| range_item_terrain(who, item, trigger))
            .transpose()?,
        trigger,
        can_disable,
        clears_when_technologies_disabled: buff.clear_when_technologies_disabled,
    })
}

/// How a buff stacks, when it is additive in effect
/// (`BuffAdditiveStackConditionTimeController` or
/// `BuffAdditiveStackConditionDistanceController`), or why it is not read.
fn stacking(who: &str, buff: &BuffBlock) -> std::result::Result<Option<Stacking>, String> {
    if !buff.additive_effect {
        return Ok(None);
    }
    let condition = match buff.additive_condition {
        STACK_BY_TIME => Some(StackCondition::Time),
        STACK_BY_DISTANCE if buff.additive_condition_param > 0 => Some(StackCondition::Distance {
            metres_q32: buff.additive_condition_param,
        }),
        _ => None,
    };
    let lowers = [
        buff.amplify_damage_rate,
        buff.damage_rate,
        buff.speed_rate,
        buff.attack_range_value,
        buff.attack_range_rate,
        buff.max_life_rate,
    ]
    .iter()
    .any(|rate| *rate < 0);
    // `BuffAdditiveStackResetControllerFactory.Create`: a stack that resets
    // on a hit registers with its unit's main skill's hits.
    let resets_on_main_hit = match buff.additive_reset_condition {
        NO_RESET => false,
        RESET_ON_HIT if buff.max_life_rate == 0 => true,
        other => {
            return Err(format!(
                "{who} adds buff {} ({}), which resets its stack on \
                 BuffEffectAdditiveResetCondition {other} or holds a life rate, and only a \
                 reset on its unit's main skill's hit of a buff with no life rate is read",
                buff.id, buff.name
            ));
        }
    };
    match condition {
        Some(condition) if buff.step_time > 0 && !lowers => Ok(Some(Stacking {
            max: buff.max_additive_stack,
            condition,
            resets_on_main_hit,
        })),
        _ => Err(format!(
            "{who} adds buff {} ({}), which stacks on condition {} or lowers what it stacks, \
             and only a stack by time or distance raising what it writes is read",
            buff.id, buff.name, buff.additive_condition
        )),
    }
}

/// The terrain a source's `BuffRangeItem` is: an acid whose
/// `BuffItemController` keeps its buff on the units standing in it. Only
/// `TriggerBuffOrBuffRangeItemFromHit` leaves one, so a source of another
/// trigger, or a range item of another kind or whose buff writes what a
/// terrain's buff here does not, is refused.
fn range_item_terrain(
    who: &str,
    item: &RangeItemBlock,
    trigger: BuffTrigger,
) -> std::result::Result<TerrainSpec, String> {
    let buff = &item.buff;
    if trigger != BuffTrigger::Hit {
        return Err(format!(
            "{who} leaves a range item on a trigger other than a hit, which leaves none"
        ));
    }
    if item.kind != ACID {
        return Err(format!(
            "{who} leaves a range item of RangeItemType {}, and only an acid is read",
            item.kind
        ));
    }
    let unread = [
        ("damageChangeRate", buff.damage_rate != 0),
        ("attackRangeChangeRate", buff.attack_range_rate != 0),
        ("maxLifeChangeRate", buff.max_life_rate != 0),
        ("isAdditiveEffect", buff.additive_effect),
        ("summonUnitID", buff.summon_unit != 0),
        ("disableRecover", buff.disable_recover),
        ("a heal", buff.life_change_rate > 0),
    ]
    .into_iter()
    .filter_map(|(field, set)| set.then_some(field))
    .chain(buff.special.iter().map(String::as_str))
    .collect::<Vec<_>>();
    if !unread.is_empty() {
        return Err(format!(
            "{who} leaves an acid whose buff {} ({}) sets {}, which a terrain's buff here does \
             not write",
            buff.id,
            buff.name,
            unread.join(", ")
        ));
    }
    crate::layout::buff_item_terrain(
        who,
        (item.range, item.life, item.rounds),
        &crate::layout::ItemBuff {
            id: buff.id,
            divide: buff.divide,
            additive: buff.additive,
            duration_raw: buff.duration,
            debuff: buff.debuff,
            invincible: buff.invincible,
            disable_technology: buff.disable_technology,
            amplify_damage_rate: buff.amplify_damage_rate,
            move_speed_rate: buff.speed_rate,
            life_change_rate: buff.life_change_rate,
            step_time_raw: buff.step_time,
            attack_range_value: buff.attack_range_value,
        },
    )
    .map_err(|error| error.to_string())
}

/// `Utility.ConvertProbability`: an `FPoint` chance in whole thousandths,
/// truncated, so the 0.35 a table writes, a raw value just short of it, is
/// 349.
fn convert_probability(raw: i64) -> i32 {
    let thousandths = (i128::from(raw) * 1000) >> 32;
    i32::try_from(thousandths).unwrap_or(if thousandths < 0 { i32::MIN } else { i32::MAX })
}

/// What a buff's unit summons as it dies, `IBEC_DeadSummon`, which `Buff.Init`
/// gives a buff of a nonzero `summonUnitID`. A summon that takes the adding
/// unit's level is not read.
fn summons(who: &str, buff: &BuffBlock) -> std::result::Result<Option<DeadSummon>, String> {
    if buff.summon_unit == 0 {
        return Ok(None);
    }
    let summon = if buff.summon_unit < 1 {
        DeadSummon::SourceType
    } else {
        DeadSummon::Unit(buff.summon_unit)
    };
    if buff.summon_level_inherit && buff.summon_unit > 0 {
        return Err(format!(
            "{who} adds buff {} ({}), whose summon takes its adder's level, which is not read",
            buff.id, buff.name
        ));
    }
    Ok(Some(summon))
}

/// The units in reach a source's controller gives its buff, of the target
/// types it names.
fn reach(who: &str, targets: &[i32], cycle: &CycleBlock) -> std::result::Result<BuffReach, String> {
    if cycle.self_radius {
        return Err(format!(
            "{who} measures its reach from its unit's edge (isDistanceCalculateSelfRadius), \
             whose radius is not measured"
        ));
    }
    let melee = match cycle.distance_type {
        NO_DISTANCE_TYPE => None,
        MELEE => Some(true),
        REMOTE => Some(false),
        other => {
            return Err(format!(
                "{who} reaches only units of DamageDistanceType {other}"
            ));
        }
    };
    let mut named = BuffTargets::default();
    for &target in targets {
        match target {
            MECH_UNIT => named.itself = true,
            OTHER_SELF_UNITS => named.own_others = true,
            // `AvailableCheck` passes a unit of its group of another team.
            FRIEND_UNITS => {}
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
    Ok(BuffReach {
        range_q32: cycle.range,
        domains,
        target_radius: cycle.target_radius,
        targets: named,
        melee,
    })
}

#[cfg(test)]
mod tests {
    use super::convert_probability;

    #[test]
    fn a_chance_is_truncated_to_thousandths() {
        // The raw values the Ignites' tables write for 0.35, 0.14 and 0.7,
        // each just short of the decimal, and a certainty.
        assert_eq!(convert_probability(1_503_238_553), 349);
        assert_eq!(convert_probability(601_295_421), 139);
        assert_eq!(convert_probability(3_006_477_107), 699);
        assert_eq!(convert_probability(1 << 32), 1000);
    }
}
