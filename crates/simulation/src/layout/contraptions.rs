//! What a layout's contraptions put on the board.
//!
//! [`config/contraptions.yaml`](../../../../config/contraptions.yaml), which
//! `scripts/extract/extract-contraptions.py` writes, holds the interceptor, the
//! missile and the shield a layout places; `docs/rules/contraptions.md` states
//! what they do.

use mechcore_document::{NativeFormation, Placement};
use serde::Deserialize;

use crate::{Error, Result, modifier::ContraptionRates};

const DEFAULT_CONTRAPTIONS: &str = include_str!("../../../../config/contraptions.yaml");
const SPACE: i64 = 1_000;
const FIXED_ONE: i64 = 1 << 32;
/// `FightUtility.LogicDeltaTime`, the seconds of one tick as the build's
/// `FPoint` holds them, which a row's times are divided by.
const LOGIC_DELTA_RAW: i64 = 0x0CCC_CCCC;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Table {
    schema: String,
    interceptors: Vec<InterceptorRow>,
    missiles: Vec<MissileRow>,
    shields: Vec<ShieldRow>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ShieldRow {
    id: i32,
    name: String,
    layout_name: String,
    energy: i64,
    effect_type: i32,
    effect_range_type: i32,
    radius: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct MissileRow {
    id: i32,
    name: String,
    layout_name: String,
    count: i32,
    damage: i32,
    max_life: i32,
    effect_type: i32,
    effect_range_type: i32,
    trigger_range: i64,
    splash_radius: i64,
    speed: i64,
    interceptible: bool,
    can_attack_construction: bool,
    buff: MissileBuffRow,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct MissileBuffRow {
    id: u32,
    name: String,
    divide: i32,
    additive: bool,
    debuff: bool,
    duration: i64,
    can_affect_construction: bool,
    move_speed_rate: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct InterceptorRow {
    id: i32,
    name: String,
    layout_name: String,
    max_life: i32,
    exp: i32,
    slot_size: i32,
    path_radius: i32,
    collider_priority: i32,
    attack: i32,
    effect_type: i32,
    effect_range_type: i32,
    range_max: i64,
    range_min: i64,
    prepare_time: i64,
    interval: i64,
    cooling_time: i64,
    rise_interval: i64,
    decline: i64,
    lower_limit: i64,
    rise: i64,
    judgment_probability: i64,
}

/// How an interceptor intercepts, in the units `InterceptEffectBase` counts
/// in: ticks for its times, whole points of attack for its numbers, and the
/// `FPoint` raw integer for its reach.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Interception {
    /// `attackNum`: what a hit takes off a projectile at full strength.
    pub(crate) attack: i64,
    /// What each hit takes off the attack, `attackNum × decline`.
    pub(crate) decline: i64,
    /// The attack never falls below this, `attackNum × lowerLimit`.
    pub(crate) lower: i64,
    /// What each rise gives back, `attackNum × rise`.
    pub(crate) rise: i64,
    /// A projectile is in reach from `range_min` up to, not including,
    /// `range_max`, both `FPoint` raw metres.
    pub(crate) range_min_q32: i64,
    pub(crate) range_max_q32: i64,
    pub(crate) prepare_ticks: u32,
    pub(crate) interval_ticks: u32,
    pub(crate) cooling_ticks: u32,
    pub(crate) rise_ticks: u32,
    /// `Utility.ConvertProbability` of `judgmentProbability`, in thousandths,
    /// which a draw of `Next(1000)` has to fall under for a hit.
    pub(crate) probability: i32,
}

/// One interceptor a layout places: a building of its side.
#[derive(Debug, Clone)]
pub(crate) struct InterceptorBuilding {
    pub(crate) team: u32,
    /// Its centre in space units, a thousand to the metre.
    pub(crate) x: i64,
    pub(crate) z: i64,
    /// Half the box a recording reports: the row's `pathRadius`.
    pub(crate) radius: i64,
    pub(crate) life: i32,
    pub(crate) exp: i32,
    pub(crate) collider_priority: i32,
    pub(crate) interception: Interception,
}

/// One missile a layout places: `FightLandMine`, standing where the layout
/// puts it until it fires.
#[derive(Debug, Clone)]
pub(crate) struct MissileMine {
    pub(crate) team: u32,
    /// Where it stands, in space units, a thousand to the metre.
    pub(crate) x: i64,
    pub(crate) z: i64,
    /// An enemy's edge nearer than this, `FPoint` raw metres, sets it off.
    pub(crate) trigger_range_q32: i64,
    /// What it fires.
    pub(crate) shot: MissileShot,
}

/// The projectile a missile fires and what its hit does, read off its row.
#[derive(Debug, Clone)]
pub(crate) struct MissileShot {
    pub(crate) damage: i64,
    /// In space units.
    pub(crate) splash_radius: i64,
    /// Space units a second.
    pub(crate) speed: i64,
    pub(crate) life: i64,
    pub(crate) interceptible: bool,
    /// The `buffDatas` row its hit writes on every unit it strikes.
    pub(crate) buff: MissileBuff,
}

/// The one buff a missile's hit writes: a slow.
#[derive(Debug, Clone, Copy)]
pub(crate) struct MissileBuff {
    pub(crate) id: u32,
    pub(crate) divide: i32,
    pub(crate) additive: bool,
    pub(crate) ticks: u32,
    /// `speedChangeRate`, an `FPoint` raw rate.
    pub(crate) move_speed_rate: i64,
    /// `debuff`: a unit a buff makes invincible does not take it.
    pub(crate) debuff: bool,
}

/// One shield a layout places: `FightEnergyShield`, a sphere of its side
/// standing on the ground where the layout puts it.
#[derive(Debug, Clone)]
pub(crate) struct ShieldPlacement {
    pub(crate) team: u32,
    /// Its centre, in space units, a thousand to the metre.
    pub(crate) x: i64,
    pub(crate) z: i64,
    /// `FPoint` raw metres.
    pub(crate) radius_q32: i64,
    /// `GetAdvancedEnergyShieldValue`: its energy, full.
    pub(crate) energy: i64,
    pub(crate) kind: ShieldKind,
}

/// What made a shield: `get_EnergyShieldData()`'s runtime type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShieldKind {
    /// `EnergyShieldContraption`.
    Contraption,
    /// `CS_EnergyShield`, a Shield Airdrop.
    CommanderSkill,
}

/// Every interceptor, missile and shield the build holds, by the id a layout
/// compiles to.
#[derive(Debug, Clone)]
pub(crate) struct Contraptions {
    interceptors: Vec<InterceptorRow>,
    missiles: Vec<MissileRow>,
    shields: Vec<ShieldRow>,
}

impl Contraptions {
    /// Reads the tracked table.
    ///
    /// # Errors
    ///
    /// Returns an error when the table is not the one this build reads.
    pub(crate) fn load() -> Result<Self> {
        let table: Table = serde_yaml::from_str(DEFAULT_CONTRAPTIONS)
            .map_err(|error| Error::new(format!("cannot read the contraption table: {error}")))?;
        if table.schema != "mechcore.contraptions" {
            return Err(Error::new(format!(
                "contraption table declares schema {:?}",
                table.schema
            )));
        }
        Ok(Self {
            interceptors: table.interceptors,
            missiles: table.missiles,
            shields: table.shields,
        })
    }

    /// The shield a placement puts on the board, as `CRC_EnergyShield`
    /// releases it: a sphere of its side at the placement's centre, on the
    /// ground, `range` across and holding its row's `energy` raised by its
    /// side's `energyRate`.
    ///
    /// # Errors
    ///
    /// Returns an error naming the shield when its row asks for what this
    /// build does not read.
    pub(crate) fn shield(
        &self,
        team: u32,
        placement: &Placement,
        rates: ContraptionRates,
    ) -> Result<ShieldPlacement> {
        let NativeFormation::Contraption(id) = placement.native else {
            return Err(Error::new(format!(
                "placement {:?} is not a contraption",
                placement.type_name
            )));
        };
        let row = self
            .shields
            .iter()
            .find(|row| row.id == id && row.layout_name == placement.type_name)
            .ok_or_else(|| {
                Error::new(format!(
                    "contraption {id} ({:?}) is not a shield of the table",
                    placement.type_name
                ))
            })?;
        if (row.effect_type, row.effect_range_type) != (7, 0) || row.radius <= 0 || row.energy <= 0
        {
            return Err(Error::new(format!(
                "shield {id} ({}) has effect type {} over range type {}, which this build does \
                 not read",
                row.name, row.effect_type, row.effect_range_type
            )));
        }
        let (local_x, local_z) = (
            i64::from(placement.position.x),
            i64::from(placement.position.y),
        );
        let (x, z) = if team == 0 {
            (local_x, local_z)
        } else {
            (-local_x, -local_z)
        };
        Ok(ShieldPlacement {
            team,
            x: x * SPACE,
            z: z * SPACE,
            radius_q32: row.radius,
            energy: raised(row.energy, rates.shield_energy),
            kind: ShieldKind::Contraption,
        })
    }

    /// The missile a placement puts on the board, as `CRC_Mine` releases it:
    /// at the placement's centre, on its side's half.
    ///
    /// # Errors
    ///
    /// Returns an error naming the missile when its row asks for what no
    /// recording has measured.
    pub(crate) fn missile(
        &self,
        team: u32,
        placement: &Placement,
        rates: ContraptionRates,
    ) -> Result<MissileMine> {
        let NativeFormation::Contraption(id) = placement.native else {
            return Err(Error::new(format!(
                "placement {:?} is not a contraption",
                placement.type_name
            )));
        };
        let row = self
            .missiles
            .iter()
            .find(|row| row.id == id && row.layout_name == placement.type_name)
            .ok_or_else(|| {
                Error::new(format!(
                    "contraption {id} ({:?}) is not a missile of the table",
                    placement.type_name
                ))
            })?;
        let named = format!("missile {id} ({})", row.name);
        if row.count != 1 || (row.effect_type, row.effect_range_type) != (7, 0) {
            return Err(Error::new(format!(
                "{named} fires {} with effect type {} over range type {}, which this build does \
                 not read",
                row.count, row.effect_type, row.effect_range_type
            )));
        }
        if !row.can_attack_construction {
            return Err(Error::new(format!(
                "{named} cannot strike a construction, which this build does not read"
            )));
        }
        if row.buff.can_affect_construction {
            return Err(Error::new(format!(
                "{named}'s buff {} ({}) reaches a construction, which is not measured",
                row.buff.id, row.buff.name
            )));
        }
        let (local_x, local_z) = (
            i64::from(placement.position.x),
            i64::from(placement.position.y),
        );
        let (x, z) = if team == 0 {
            (local_x, local_z)
        } else {
            (-local_x, -local_z)
        };
        Ok(MissileMine {
            team,
            x: x * SPACE,
            z: z * SPACE,
            trigger_range_q32: row.trigger_range,
            shot: MissileShot {
                damage: raised(i64::from(row.damage), rates.missile_damage),
                splash_radius: fixed_to_space(row.splash_radius),
                speed: fixed_to_space(row.speed),
                life: i64::from(row.max_life),
                interceptible: row.interceptible,
                buff: MissileBuff {
                    id: row.buff.id,
                    divide: row.buff.divide,
                    additive: row.buff.additive,
                    ticks: ticks(row.buff.duration)?,
                    move_speed_rate: row.buff.move_speed_rate,
                    debuff: row.buff.debuff,
                },
            },
        })
    }

    /// The interceptor a placement puts on the board, as `CRC_Interceptor`
    /// releases it: at the placement's centre, on its side's half.
    ///
    /// # Errors
    ///
    /// Returns an error naming the interceptor when its row asks for what no
    /// recording has measured: a hit that can miss, or an effect this reads
    /// differently.
    pub(crate) fn interceptor(
        &self,
        team: u32,
        placement: &Placement,
    ) -> Result<InterceptorBuilding> {
        let NativeFormation::Contraption(id) = placement.native else {
            return Err(Error::new(format!(
                "placement {:?} is not a contraption",
                placement.type_name
            )));
        };
        let row = self
            .interceptors
            .iter()
            .find(|row| row.id == id && row.layout_name == placement.type_name)
            .ok_or_else(|| {
                Error::new(format!(
                    "contraption {id} ({:?}) is not an interceptor of the table",
                    placement.type_name
                ))
            })?;
        let named = format!("interceptor {id} ({})", row.name);
        if row.judgment_probability != FIXED_ONE {
            return Err(Error::new(format!(
                "{named} hits with probability {}, and a hit that can miss is not measured",
                row.judgment_probability
            )));
        }
        if (row.effect_type, row.effect_range_type) != (7, 0) {
            return Err(Error::new(format!(
                "{named} has effect type {} over range type {}, which this build does not read",
                row.effect_type, row.effect_range_type
            )));
        }
        if row.slot_size <= 0 || row.max_life <= 0 {
            return Err(Error::new(format!(
                "{named} has slot size {} and life {}",
                row.slot_size, row.max_life
            )));
        }
        let (local_x, local_z) = (
            i64::from(placement.position.x),
            i64::from(placement.position.y),
        );
        let (x, z) = if team == 0 {
            (local_x, local_z)
        } else {
            (-local_x, -local_z)
        };
        let attack = i64::from(row.attack);
        Ok(InterceptorBuilding {
            team,
            x: x * SPACE,
            z: z * SPACE,
            radius: i64::from(row.path_radius) * SPACE,
            life: row.max_life,
            exp: row.exp,
            collider_priority: row.collider_priority,
            interception: Interception {
                attack,
                decline: scaled(attack, row.decline),
                lower: scaled(attack, row.lower_limit),
                rise: scaled(attack, row.rise),
                range_min_q32: row.range_min,
                range_max_q32: row.range_max,
                prepare_ticks: ticks(row.prepare_time)?,
                interval_ticks: ticks(row.interval)?,
                cooling_ticks: ticks(row.cooling_time)?,
                rise_ticks: ticks(row.rise_interval)?,
                probability: 1_000,
            },
        })
    }
}

/// An `FPoint` raw length in space units.
fn fixed_to_space(raw: i64) -> i64 {
    i64::try_from((i128::from(raw) * i128::from(SPACE)) >> 32).unwrap_or(i64::MAX)
}

/// A whole number of points times an `FPoint` rate, truncated as the build casts
/// the product back to an integer: `attackNum × 0.3` is 18149, not 18150,
/// because the rate the build stores is a hair under 0.3.
fn scaled(points: i64, rate_raw: i64) -> i64 {
    let product = i128::from(points) * i128::from(rate_raw);
    i64::try_from(product >> 32).unwrap_or(i64::MAX)
}

/// A contraption's number raised by its side's rate, as
/// `EnergyShieldContraption.GetAdvancedEnergyShieldValue` and
/// `LandMineContraption.GetDamage` read it: the number times `1 + rate` in
/// `FPoint`, cut back to an integer. Advanced Shield Device's `+0.4` is a hair
/// under it, so a shield of 40000 holds 55999.
fn raised(points: i64, rate_raw: i64) -> i64 {
    scaled(points, (1_i64 << 32).saturating_add(rate_raw))
}

/// A time in seconds as whole ticks, `(int)(time / LogicDeltaTime)` in `FPoint`
/// division: 0.1 s is two ticks.
fn ticks(seconds_raw: i64) -> Result<u32> {
    let quotient = (i128::from(seconds_raw) << 32) / i128::from(LOGIC_DELTA_RAW);
    u32::try_from(quotient >> 32).map_err(|_| {
        Error::new(format!(
            "an interceptor time of {seconds_raw} is not a tick count"
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The numbers the build derives from the one interceptor row, each
    /// truncated as it truncates them.
    #[test]
    fn the_interceptor_row_reads_as_the_build_reads_it() {
        let layout = mechcore_document::parse_yaml(
            b"kind: layout\nround: 1\nblue:\n  units: [{name: marksman, index: 0, position: {x: 0, y: -50}}]\n  contraptions: [{name: interceptor, index: 0, position: {x: 5, y: -85}}]\nred:\n  units: [{name: marksman, index: 0, position: {x: 0, y: -50}}]\n",
        )
        .unwrap();
        let plan = mechcore_document::compile_layout(layout).unwrap();
        let blue = Contraptions::load()
            .unwrap()
            .interceptor(0, &plan.blue.contraptions[0])
            .unwrap();
        assert_eq!((blue.x, blue.z, blue.radius), (5_000, -85_000, 3_000));
        let red = Contraptions::load()
            .unwrap()
            .interceptor(1, &plan.blue.contraptions[0])
            .unwrap();
        assert_eq!((red.x, red.z), (-5_000, 85_000));
        let interception = blue.interception;
        assert_eq!(
            (
                interception.attack,
                interception.decline,
                interception.lower,
                interception.rise
            ),
            (60_500, 1_512, 18_149, 60)
        );
        assert_eq!(
            (
                interception.prepare_ticks,
                interception.interval_ticks,
                interception.cooling_ticks,
                interception.rise_ticks
            ),
            (2, 2, 0, 2)
        );
    }
}
