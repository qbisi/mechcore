//! What a layout's battle skills do once released.
//!
//! [`config/commander_skill_effects.yaml`](../../../../config/commander_skill_effects.yaml),
//! which `scripts/extract/extract-commander-skill-effects.py` writes, holds the
//! skills the fight releases, the buff skills and the support skills;
//! `docs/rules/battle_skill.md` states what they do. Every other battle skill
//! is refused by name.

use mechcore_document::BattleSkill;
use serde::Deserialize;

use super::contraptions::{ShieldKind, ShieldPlacement};
use crate::{
    Error, Result,
    data::{Channel, Entry},
    rules::{UnitConfig, UnitConfigs},
};

const DEFAULT_EFFECTS: &str = include_str!("../../../../config/commander_skill_effects.yaml");
const SPACE: i64 = 1_000;
/// `FightUtility.LogicDeltaTime`, the seconds of one tick as the build's
/// `FPoint` holds them, which a row's times are divided by.
const LOGIC_DELTA_RAW: i64 = 0x0CCC_CCCC;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Table {
    schema: String,
    buff_skills: Vec<BuffSkillRow>,
    support_skills: Vec<SupportSkillRow>,
    shield_skills: Vec<ShieldSkillRow>,
    damage_skills: Vec<DamageSkillRow>,
    waypoint_skills: Vec<WaypointSkillRow>,
    terrain_skills: Vec<TerrainSkillRow>,
    #[allow(dead_code, reason = "a fire's terrain reads it")]
    ground_fire: GroundFire,
    other_skills: Vec<OtherSkillRow>,
}

/// A terrain skill's row: a line of sub-effects, each leaving a `RangeItem`
/// of its kind where it lands.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct TerrainSkillRow {
    id: i32,
    name: String,
    kind: TerrainKind,
    effect_range_type: i32,
    effect_type: i32,
    sub_effect_count: u32,
    /// The rounds it stands, which a fight does not read.
    #[allow(dead_code, reason = "the rounds a terrain stands are the match's")]
    effect_duration: i32,
    start_time: i64,
    sub_effect_range: i64,
    sub_effect_move_speed: i64,
    sub_effect_move_time: i64,
    sub_effect_default_height: i64,
    sub_effect_interval_time: i64,
    life_time: i64,
    fire_life_time: i64,
    attack_range_change_rate: i64,
    #[serde(default)]
    buff: Option<BuffRow>,
    #[serde(default)]
    uncarried_buff: Option<u32>,
}

/// `Config.groundFireDamage` and `fireAttackInterval`.
#[derive(Debug, Clone, Copy, Deserialize)]
#[allow(dead_code, reason = "a fire's terrain reads it")]
#[serde(deny_unknown_fields)]
struct GroundFire {
    damage: i64,
    interval: i64,
}

/// `RangeItemType`: which `RangeItemController` holds a terrain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TerrainKind {
    Fire,
    Oil,
    Fog,
    Acid,
}

/// What a terrain does to the units standing in it, read off its row.
#[derive(Debug, Clone, Copy)]
pub(crate) enum TerrainEffect {
    /// `FogController`: an attack range rate on every ranged skill.
    Fog { attack_range_rate: i64 },
}

/// A terrain one sub-effect leaves where it lands.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TerrainSpec {
    pub(crate) kind: TerrainKind,
    /// `GetRangeItemRange`, `FPoint` raw metres.
    pub(crate) radius_q32: i64,
    /// `lifeTime` in ticks, when it burns out within a fight.
    pub(crate) life_ticks: Option<i32>,
    pub(crate) effect: TerrainEffect,
}

/// A skill of another kind, or a row this build does not release: which
/// `CommanderSkillGroupData` list it comes from, which its refusal names.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct OtherSkillRow {
    id: i32,
    name: String,
    kind: String,
}

/// A `CSD_WayPoint` row: the units it selects and the width of its path.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct WaypointSkillRow {
    id: i32,
    name: String,
    effect_target_type: i32,
    sub_effect_buff_id: i32,
    sub_effect_range: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct DamageSkillRow {
    id: i32,
    name: String,
    effect_range_type: i32,
    effect_type: i32,
    sub_effect_damage: i64,
    sub_effect_buff_id: u32,
    start_time: i64,
    effect_range: i64,
    sub_effect_move_speed: i64,
    sub_effect_move_time: i64,
    sub_effect_default_height: i64,
    sub_effect_count: u32,
    sub_effect_range: i64,
    sub_effect_interval_time: i64,
    cross_advanced_shield: bool,
    /// The `buffDatas` row its `subEffectBuffID` names, when it names one.
    #[serde(default)]
    buff: Option<BuffRow>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ShieldSkillRow {
    id: i32,
    name: String,
    effect_range_type: i32,
    effect_type: i32,
    energy: i64,
    start_time: i64,
    effect_range: i64,
    sub_effect_move_speed: i64,
    sub_effect_move_time: i64,
}

/// The Shield Airdrop an earlier round left standing: `CS_EnergyShield`
/// 800001, the only one a standard match holds.
const STANDING_SHIELD_SKILL: i32 = 800_001;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct SupportSkillRow {
    id: i32,
    name: String,
    effect_range_type: i32,
    effect_type: i32,
    unit_type_id: u32,
    max_count: u32,
    create_count_per_time: u32,
    max_batch: u32,
    appear_type: i32,
    start_time: i64,
    effect_range: i64,
    sub_effect_move_speed: i64,
    sub_effect_move_time: i64,
    create_interval: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct BuffSkillRow {
    id: i32,
    name: String,
    effect_range_type: i32,
    effect_type: i32,
    sub_effect_damage: i32,
    start_time: i64,
    effect_range: i64,
    sub_effect_move_speed: i64,
    sub_effect_move_time: i64,
    buff: BuffRow,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "the buff row's flags are independent fields"
)]
struct BuffRow {
    id: u32,
    name: String,
    divide: i32,
    duration: i64,
    additive: bool,
    debuff: bool,
    disable_technology: bool,
    invincible: bool,
    can_affect_construction: bool,
    can_affect_tower: bool,
    move_speed_rate: i64,
    amplify_damage_rate: i64,
}

/// One battle skill a side releases, as `CommanderSkillReleaseState` carries
/// it to the tick its sub-effect lands.
#[derive(Debug, Clone)]
pub(crate) struct SkillRelease {
    pub(crate) team: u32,
    /// The skill's name, for a refusal the fight makes.
    pub(crate) name: String,
    /// Where it lands, in space units, a thousand to the metre.
    pub(crate) x: i64,
    pub(crate) z: i64,
    /// The tick its sub-effect lands on, the fight's first update being 1.
    pub(crate) lands_on: u64,
    /// What its sub-effect does where it lands.
    pub(crate) effect: SkillEffect,
}

/// What a sub-effect does where it lands: the controller its skill's kind
/// creates.
#[derive(Debug, Clone)]
pub(crate) enum SkillEffect {
    /// `CommanderSkillSubEffectController`: a buff on every unit in range.
    Buff {
        /// A unit's edge this near, `FPoint` raw metres, is reached.
        range_q32: i64,
        /// What it writes on every unit it reaches.
        buff: SkillBuff,
    },
    /// `SupportUnitEffectController`: a creator of summons.
    Summon(Box<Summon>),
    /// `CS_Damage`'s: the skill's damage over each sub-effect's circle.
    Strike {
        /// `FPoint` raw metres, each circle's radius: `subEffectRange`, which
        /// `CommanderSkillData.PreProcess` makes a circle's `effectRange`.
        range_q32: i64,
        damage: i64,
        /// `isCrossAdvancedShield`: it passes shields, falling and landing.
        crosses_shields: bool,
        /// What each sub-effect writes on the units it reaches, after its
        /// damage.
        buff: Option<SkillBuff>,
        /// Where `CommanderSkillManager.CalculateAttackPositions` puts the
        /// sub-effects about the release.
        scatter: Scatter,
        /// The sub-effects still to land, in the order they are activated.
        sub_effects: Vec<SubEffect>,
    },
    /// `RangeItemEffectController`: each sub-effect leaves a terrain where
    /// it lands, `RangeItemSystem.AddItem`.
    Terrain {
        spec: TerrainSpec,
        scatter: Scatter,
        /// The sub-effects still to land, in the order they are activated.
        sub_effects: Vec<SubEffect>,
    },
    /// `CS_EnergyShield`'s: a shield of the side, standing where it lands.
    Shield {
        /// `FPoint` raw metres.
        radius_q32: i64,
        energy: i64,
    },
    /// `CSRC_WayPoint`: a path the side's units near its first point walk.
    Path {
        /// The release's positions in the world, `FPoint` raw metres.
        points: Vec<(i64, i64)>,
        /// `subEffectRange`, `FPoint` raw metres: how near the first point a
        /// unit is selected, and how wide each segment is.
        width_q32: i64,
    },
}

/// How `CommanderSkillManager.CalculateAttackPositions` places a skill's
/// sub-effects about its release.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Scatter {
    /// A circle: its one sub-effect on the release.
    Point,
    /// A random circle: each sub-effect drawn within `radius_q32`, the row's
    /// `effectRange` less its `subEffectRange`, of the release, from the
    /// side's stream at the fight's start.
    RandomCircle { radius_q32: i64 },
    /// A line: the sub-effects evenly from the release to its second
    /// position, `FPoint` raw metres in the world.
    Line { to_q32: (i64, i64) },
}

/// One sub-effect, `CommanderSkillSubEffectAgent`: where it lands, `FPoint`
/// raw metres in the world, when, and how it falls there.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SubEffect {
    pub(crate) x_q32: i64,
    pub(crate) z_q32: i64,
    pub(crate) lands_on: u64,
    pub(crate) fall: Fall,
}

/// A sub-effect's fall, `CommanderSkillSubEffectAgent`: from the tick after
/// its activation it drops a step a tick from its height above where it
/// lands, and lands once it has fallen the whole of it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Fall {
    /// The tick it first moves on.
    pub(crate) first_move_on: u64,
    /// `FPoint` raw metres: where it starts above the ground, and where it
    /// lands.
    pub(crate) start_q32: i64,
    pub(crate) floor_q32: i64,
    /// `FPoint` raw metres a tick.
    pub(crate) step_q32: i64,
}

impl Fall {
    /// Where it stands on a tick of its fall, `FPoint` raw metres up: none
    /// before it moves.
    pub(crate) fn height_on(&self, tick: u64) -> Option<i64> {
        let moves = i64::try_from(tick.checked_sub(self.first_move_on)? + 1).ok()?;
        Some(
            self.start_q32
                .saturating_sub(self.step_q32.saturating_mul(moves))
                .max(self.floor_q32),
        )
    }
}

/// What a support skill's `SupportUnitCreator` makes, read off its row.
#[derive(Debug, Clone)]
pub(crate) struct Summon {
    /// The unit it summons, `unitID`, as `IFightSetting.GetMechData` reads it.
    pub(crate) rules: UnitConfig,
    /// `maxCount`: how many it summons in all.
    pub(crate) count: u32,
    /// `createCountPerTime`: how many one creation makes.
    pub(crate) per_time: u32,
    /// `createInterval` in whole ticks: how long between two creations.
    pub(crate) interval_ticks: u32,
    /// How far from the landing point a summon may stand, `FPoint` raw
    /// metres: the skill's range when it summons two or more, and none
    /// otherwise.
    pub(crate) random_range_q32: i64,
    /// Whether a summon deals its life around it as it lands:
    /// `appearType` 2, an air drop.
    pub(crate) drop_damage: bool,
    /// How many updates the creator lives, the last of them included.
    pub(crate) updates: u64,
    /// What its side's officers, technologies and Energy Tower skills write
    /// onto it: `FightEffectSystem.AddEffect` looks up what the side
    /// registered for its mech, as for a unit of its type deployed without
    /// equipment. The layout fills it in, where the side is known.
    pub(crate) corrections: Vec<(Channel, Entry)>,
}

/// The buff a released skill writes: the Electromagnetic Impact's slow,
/// Lightning Storm's, or Photon Emission's protection.
#[derive(Debug, Clone, Copy)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "the buff row's flags are independent fields"
)]
pub(crate) struct SkillBuff {
    pub(crate) id: u32,
    pub(crate) divide: i32,
    pub(crate) additive: bool,
    pub(crate) ticks: u32,
    /// `speedChangeRate`, an `FPoint` raw rate.
    pub(crate) move_speed_rate: i64,
    /// `disableTechnology`: it switches off the technologies of what it is
    /// written on.
    pub(crate) disable_technology: bool,
    /// `debuff`: a unit a buff makes invincible does not take it.
    pub(crate) debuff: bool,
    /// `invincible`: while it runs, no debuff reaches the unit.
    pub(crate) invincible: bool,
    /// `amplifyDamageRate`, an `FPoint` raw rate on the damage the unit takes.
    pub(crate) amplify_damage_rate: i64,
}

impl SkillBuff {
    /// `BuffData.IsHarmful` over the fields the table carries: a slower
    /// speed or more damage taken. A harmful buff is written on every unit
    /// in reach, of either side (`PerformNegativeEffect`), and any other on
    /// the releasing side's alone (`PerformPositiveEffect`).
    pub(crate) const fn harmful(&self) -> bool {
        self.move_speed_rate < 0 || self.amplify_damage_rate > 0
    }
}

/// Every battle skill the fight releases, by its commander skill id.
#[derive(Debug, Clone)]
pub(crate) struct CommanderSkillEffects {
    buffs: Vec<BuffSkillRow>,
    summons: Vec<SupportSkillRow>,
    shields: Vec<ShieldSkillRow>,
    strikes: Vec<DamageSkillRow>,
    waypoints: Vec<WaypointSkillRow>,
    terrains: Vec<TerrainSkillRow>,
    others: Vec<OtherSkillRow>,
}

impl CommanderSkillEffects {
    /// Reads the tracked table.
    ///
    /// # Errors
    ///
    /// Returns an error when the table is not the one this build reads.
    pub(crate) fn load() -> Result<Self> {
        let table: Table = serde_yaml::from_str(DEFAULT_EFFECTS).map_err(|error| {
            Error::new(format!("cannot read the commander skill table: {error}"))
        })?;
        if table.schema != "mechcore.commander_skill_effects" {
            return Err(Error::new(format!(
                "commander skill table declares schema {:?}",
                table.schema
            )));
        }
        Ok(Self {
            buffs: table.buff_skills,
            summons: table.support_skills,
            shields: table.shield_skills,
            strikes: table.damage_skills,
            waypoints: table.waypoint_skills,
            terrains: table.terrain_skills,
            others: table.other_skills,
        })
    }

    /// A Shield Airdrop an earlier round left standing, full: the shield
    /// resets to its maximum as each round's fight ends.
    ///
    /// # Errors
    ///
    /// Returns an error when the table does not hold its row.
    pub(crate) fn standing_shield(
        &self,
        team: u32,
        position: mechcore_document::Position,
    ) -> Result<ShieldPlacement> {
        let row = self
            .shields
            .iter()
            .find(|row| row.id == STANDING_SHIELD_SKILL)
            .ok_or_else(|| Error::new("the Shield Airdrop is not in the table"))?;
        let (local_x, local_z) = (i64::from(position.x), i64::from(position.y));
        let (x, z) = if team == 0 {
            (local_x, local_z)
        } else {
            (-local_x, -local_z)
        };
        Ok(ShieldPlacement {
            team,
            x: x * SPACE,
            z: z * SPACE,
            radius_q32: row.effect_range,
            energy: row.energy,
            kind: ShieldKind::CommanderSkill,
        })
    }

    /// A released battle skill, as `CSRC_Common` releases it: its
    /// sub-effects falling about the release's position, on its side's half.
    ///
    /// Every kind but a damage skill is a circle, `effectRangeType` 0, which
    /// `CommanderSkillData.PreProcess` makes one sub-effect reaching
    /// `effectRange`. A damage skill's random circle or line scatters its
    /// row's `subEffectCount`, `subEffectIntervalTime` apart.
    ///
    /// # Errors
    ///
    /// Returns an error naming the skill when the table does not hold it, or
    /// when its row asks for what no recording has measured.
    pub(crate) fn release(
        &self,
        team: u32,
        skill: &BattleSkill,
        units: &UnitConfigs,
    ) -> Result<SkillRelease> {
        let named = format!(
            "battle skill {} ({})",
            skill.type_name, skill.commander_skill_id
        );
        let id = skill.commander_skill_id;
        if let Some(row) = self.waypoints.iter().find(|row| row.id == id) {
            return path_release(&format!("{named}, {}", row.name), team, skill, row);
        }
        let (row_name, common, effect) =
            if let Some(row) = self.buffs.iter().find(|row| row.id == id) {
                let named = format!("{named}, {}", row.name);
                (
                    row.name.as_str(),
                    Common::of_buff(row),
                    buff_effect(&named, row)?,
                )
            } else if let Some(row) = self.summons.iter().find(|row| row.id == id) {
                let named = format!("{named}, {}", row.name);
                (
                    row.name.as_str(),
                    Common::of_support(row),
                    SkillEffect::Summon(Box::new(summon(&named, row, units)?)),
                )
            } else if let Some(row) = self.shields.iter().find(|row| row.id == id) {
                (
                    row.name.as_str(),
                    Common::of_shield(row),
                    SkillEffect::Shield {
                        radius_q32: row.effect_range,
                        energy: row.energy,
                    },
                )
            } else if let Some(row) = self.strikes.iter().find(|row| row.id == id) {
                let named = format!("{named}, {}", row.name);
                (
                    row.name.as_str(),
                    Common::of_strike(row),
                    strike_effect(&named, row)?,
                )
            } else if let Some(row) = self.terrains.iter().find(|row| row.id == id) {
                let named = format!("{named}, {}", row.name);
                (
                    row.name.as_str(),
                    Common::of_terrain(row),
                    terrain_effect(&named, row)?,
                )
            } else {
                return Err(Error::new(
                    match self.others.iter().find(|row| row.id == id) {
                        Some(row) => format!(
                            "{named}, {}, comes from CommanderSkillGroupData's {} list, \
                             which this build does not release",
                            row.name, row.kind
                        ),
                        None => format!("{named} is not in the commander skill table"),
                    },
                ));
            };
        released(
            team,
            skill,
            &format!("{named}, {row_name}"),
            &common,
            effect,
        )
    }
}

/// A release at its positions, on its side's half: a strike's sub-effects
/// stand on its first until the fight places them, and a line's second is
/// where it runs to.
fn released(
    team: u32,
    skill: &BattleSkill,
    named: &str,
    common: &Common,
    mut effect: SkillEffect,
) -> Result<SkillRelease> {
    // `scope` is when the card may be used, which nothing in the fight
    // reads. A strike's range type is its scatter; every other kind is a
    // circle.
    let strike = matches!(
        effect,
        SkillEffect::Strike { .. } | SkillEffect::Terrain { .. }
    );
    let range_type = if strike { 0 } else { common.effect_range_type };
    if (common.effect_type, range_type) != (0, 0) {
        return Err(Error::new(format!(
            "{named} has effect type {} over range type {}, which this build does not read",
            common.effect_type, common.effect_range_type
        )));
    }
    let line = matches!(
        effect,
        SkillEffect::Strike {
            scatter: Scatter::Line { .. },
            ..
        } | SkillEffect::Terrain {
            scatter: Scatter::Line { .. },
            ..
        }
    );
    let wanted = if line { 2 } else { 1 };
    if skill.positions.len() != wanted {
        return Err(Error::new(format!(
            "{named} is released at {} positions, not {wanted}",
            skill.positions.len()
        )));
    }
    let world = |position: &mechcore_document::Position| {
        let (local_x, local_z) = (i64::from(position.x), i64::from(position.y));
        if team == 0 {
            (local_x, local_z)
        } else {
            (-local_x, -local_z)
        }
    };
    let (x, z) = world(&skill.positions[0]);
    let mut lands = lands_on(common.start_time, common.move_time, common.move_speed)?;
    if let SkillEffect::Strike {
        scatter,
        sub_effects,
        ..
    }
    | SkillEffect::Terrain {
        scatter,
        sub_effects,
        ..
    } = &mut effect
    {
        if let Scatter::Line { to_q32 } = scatter {
            let (to_x, to_z) = world(&skill.positions[1]);
            *to_q32 = (to_x << 32, to_z << 32);
        }
        for sub_effect in sub_effects.iter_mut() {
            (sub_effect.x_q32, sub_effect.z_q32) = (x << 32, z << 32);
        }
        lands = sub_effects.last().map_or(lands, |last| last.lands_on);
    }
    Ok(SkillRelease {
        team,
        name: skill.type_name.clone(),
        x: x * SPACE,
        z: z * SPACE,
        lands_on: lands,
        effect,
    })
}

/// A terrain skill's sub-effects: a line of them, each leaving its terrain.
fn terrain_effect(named: &str, row: &TerrainSkillRow) -> Result<SkillEffect> {
    if row.effect_range_type != 1 {
        return Err(Error::new(format!(
            "{named} has range type {}, which this build does not read",
            row.effect_range_type
        )));
    }
    let effect = match row.kind {
        TerrainKind::Fog => TerrainEffect::Fog {
            attack_range_rate: row.attack_range_change_rate,
        },
        kind => {
            return Err(Error::new(format!(
                "{named} leaves a terrain of kind {kind:?}, which this build does not read"
            )));
        }
    };
    let _ = (&row.buff, row.uncarried_buff, row.fire_life_time);
    let life_ticks = match row.life_time {
        0 => None,
        life => Some(
            i32::try_from(ticks(life)?)
                .map_err(|_| Error::new(format!("{named}'s terrain outlasts a fight")))?,
        ),
    };
    Ok(SkillEffect::Terrain {
        spec: TerrainSpec {
            kind: row.kind,
            radius_q32: row.sub_effect_range,
            life_ticks,
            effect,
        },
        scatter: Scatter::Line { to_q32: (0, 0) },
        sub_effects: schedule(&Timing::of_terrain(row))?
            .into_iter()
            .map(|(lands_on, fall)| SubEffect {
                x_q32: 0,
                z_q32: 0,
                lands_on,
                fall,
            })
            .collect(),
    })
}

/// A damage skill's strike: its sub-effects, scattered as its range type
/// says, each timed off its row and striking `subEffectRange` about where it
/// lands.
fn strike_effect(named: &str, row: &DamageSkillRow) -> Result<SkillEffect> {
    let buff = match (&row.buff, row.sub_effect_buff_id) {
        (None, 0) => None,
        (Some(buff), id) if buff.id == id => Some(skill_buff(named, buff)?),
        (_, id) => {
            return Err(Error::new(format!(
                "{named} writes buff {id}, which the table does not carry"
            )));
        }
    };
    let scatter = match row.effect_range_type {
        0 => Scatter::Point,
        1 => Scatter::Line { to_q32: (0, 0) },
        2 => Scatter::RandomCircle {
            radius_q32: row.effect_range.saturating_sub(row.sub_effect_range),
        },
        other => {
            return Err(Error::new(format!(
                "{named} has range type {other}, which this build does not read"
            )));
        }
    };
    Ok(SkillEffect::Strike {
        range_q32: row.sub_effect_range,
        damage: row.sub_effect_damage,
        crosses_shields: row.cross_advanced_shield,
        buff,
        scatter,
        sub_effects: schedule(&Timing::of_strike(row))?
            .into_iter()
            .map(|(lands_on, fall)| SubEffect {
                x_q32: 0,
                z_q32: 0,
                lands_on,
                fall,
            })
            .collect(),
    })
}

/// What every kind of skill row holds that places and times its release.
struct Common {
    effect_type: i32,
    effect_range_type: i32,
    start_time: i64,
    move_time: i64,
    move_speed: i64,
}

impl Common {
    const fn of_buff(row: &BuffSkillRow) -> Self {
        Self {
            effect_type: row.effect_type,
            effect_range_type: row.effect_range_type,
            start_time: row.start_time,
            move_time: row.sub_effect_move_time,
            move_speed: row.sub_effect_move_speed,
        }
    }

    const fn of_strike(row: &DamageSkillRow) -> Self {
        Self {
            effect_type: row.effect_type,
            effect_range_type: row.effect_range_type,
            start_time: row.start_time,
            move_time: row.sub_effect_move_time,
            move_speed: row.sub_effect_move_speed,
        }
    }

    const fn of_terrain(row: &TerrainSkillRow) -> Self {
        Self {
            effect_type: row.effect_type,
            effect_range_type: row.effect_range_type,
            start_time: row.start_time,
            move_time: row.sub_effect_move_time,
            move_speed: row.sub_effect_move_speed,
        }
    }

    const fn of_shield(row: &ShieldSkillRow) -> Self {
        Self {
            effect_type: row.effect_type,
            effect_range_type: row.effect_range_type,
            start_time: row.start_time,
            move_time: row.sub_effect_move_time,
            move_speed: row.sub_effect_move_speed,
        }
    }

    const fn of_support(row: &SupportSkillRow) -> Self {
        Self {
            effect_type: row.effect_type,
            effect_range_type: row.effect_range_type,
            start_time: row.start_time,
            move_time: row.sub_effect_move_time,
            move_speed: row.sub_effect_move_speed,
        }
    }
}

/// A buff skill's sub-effect: its buff on every unit within its range, which
/// `CommanderSkillData.PreProcess` made the circle's one sub-effect's.
fn buff_effect(named: &str, row: &BuffSkillRow) -> Result<SkillEffect> {
    if row.sub_effect_damage != 0 {
        return Err(Error::new(format!(
            "{named} deals damage this build does not read"
        )));
    }
    Ok(SkillEffect::Buff {
        range_q32: row.effect_range,
        buff: skill_buff(named, &row.buff)?,
    })
}

/// The buff a sub-effect writes, as `BuffSystem.AddBuff` reads its row.
fn skill_buff(named: &str, buff: &BuffRow) -> Result<SkillBuff> {
    if buff.can_affect_construction || buff.can_affect_tower {
        return Err(Error::new(format!(
            "{named}'s buff {} ({}) reaches a building, which is not measured",
            buff.id, buff.name
        )));
    }
    Ok(SkillBuff {
        id: buff.id,
        divide: buff.divide,
        additive: buff.additive,
        ticks: u32::try_from(ticks(buff.duration)?)
            .map_err(|_| Error::new(format!("{named}'s buff outlasts a fight")))?,
        move_speed_rate: buff.move_speed_rate,
        disable_technology: buff.disable_technology,
        debuff: buff.debuff,
        invincible: buff.invincible,
        amplify_damage_rate: buff.amplify_damage_rate,
    })
}

/// A support skill's creator, as `SupportUnitCreator` is built over its row.
///
/// `CSD_SupportUnit.PreProcess` gives the creator a life of the creations it
/// needs times their interval, after the skill's `startTime`, and
/// `CS_SupportUnit.CreateSubEffectController` hands it the skill's range to
/// scatter in only when it summons two or more.
fn summon(named: &str, row: &SupportSkillRow, units: &UnitConfigs) -> Result<Summon> {
    if row.max_batch != 0 {
        return Err(Error::new(format!(
            "{named} summons in {} batches, which this build does not read",
            row.max_batch
        )));
    }
    if row.max_count == 0 || row.create_count_per_time == 0 {
        return Err(Error::new(format!("{named} summons nothing")));
    }
    let config = units.by_type_id(row.unit_type_id).ok_or_else(|| {
        Error::new(format!(
            "{named} summons unit {}, which has no unit configuration",
            row.unit_type_id
        ))
    })?;
    let creations = i64::from(row.max_count.div_ceil(row.create_count_per_time));
    let life = row
        .start_time
        .saturating_add(creations.saturating_mul(row.create_interval));
    Ok(Summon {
        rules: config.clone(),
        count: row.max_count,
        per_time: row.create_count_per_time,
        interval_ticks: u32::try_from(ticks(row.create_interval)?)
            .map_err(|_| Error::new(format!("{named} creates too seldom")))?,
        random_range_q32: if row.max_count >= 2 {
            row.effect_range
        } else {
            0
        },
        drop_damage: row.appear_type == 2,
        // `SupportUnitCreator.IsFinished`: its updates times a tick have
        // reached that life.
        updates: u64::try_from(
            (i128::from(life) + i128::from(LOGIC_DELTA_RAW) - 1) / i128::from(LOGIC_DELTA_RAW),
        )
        .map_err(|_| Error::new(format!("{named} creates for no time")))?,
        corrections: Vec::new(),
    })
}

/// The tick a released skill's first sub-effect lands on.
///
/// `CSRS_Prepare` counts ticks from the release's first update and hands over
/// once it has counted `startTime - subEffectMoveTime`, never before its first
/// tick. `CSRS_Perform` starts its count there and activates the sub-effect on
/// its own first update, after that tick's agents have moved, so the
/// sub-effect falls from the next. It falls from `speed x min(startTime,
/// moveTime)` above where it lands by `speed x LogicDeltaTime` a tick, and a
/// tick's step is a hair under a twentieth of a second's, so a fall of a whole
/// number of twentieths takes one step more.
fn lands_on(start_raw: i64, move_time_raw: i64, speed_raw: i64) -> Result<u64> {
    let prepare = (ticks(start_raw)? - ticks(move_time_raw)?).max(1);
    let falls = if speed_raw == 0 {
        1
    } else {
        let height = multiply(speed_raw, start_raw.min(move_time_raw));
        let step = multiply(speed_raw, LOGIC_DELTA_RAW);
        (height + step - 1) / step
    };
    u64::try_from(prepare + 1 + falls)
        .map_err(|_| Error::new("a battle skill lands before the fight begins"))
}

/// When each of a strike's sub-effects lands, and how it falls.
///
/// `CSRS_Perform` enters with its `time` at `(startTime - subEffectTime) /
/// LogicDeltaTime` and adds one on every update; after its agents have moved
/// it activates the next sub-effect, at most one an update, once `startTime +
/// subEffectIntervalTime x activated - subEffectTime`, each in whole ticks, is
/// no later than that `time`, and on every update when `subEffectTime` is
/// zero. The first is activated on its first update, which is the one
/// `lands_on` and `fall` count, and a later one that many updates after.
fn schedule(row: &Timing) -> Result<Vec<(u64, Fall)>> {
    let first_lands = lands_on(
        row.start_time,
        row.sub_effect_move_time,
        row.sub_effect_move_speed,
    )?;
    let first_fall = fall(
        row.start_time,
        row.sub_effect_move_time,
        row.sub_effect_move_speed,
        row.sub_effect_default_height,
    )?;
    let (start, interval, move_time) = (
        ticks(row.start_time)?,
        ticks(row.sub_effect_interval_time)?,
        ticks(row.sub_effect_move_time)?,
    );
    let entered = ticks(row.start_time.saturating_sub(row.sub_effect_move_time))?;
    let mut update = 0_i64;
    let mut landings = Vec::new();
    for activated in 0..i64::from(row.sub_effect_count.max(1)) {
        update = if row.sub_effect_move_time == 0 {
            update + 1
        } else {
            (update + 1).max(start + interval * activated - move_time - entered)
        };
        let later = u64::try_from(update - 1)
            .map_err(|_| Error::new("a sub-effect is activated before the first"))?;
        landings.push((
            first_lands + later,
            Fall {
                first_move_on: first_fall.first_move_on + later,
                ..first_fall
            },
        ));
    }
    Ok(landings)
}

/// What times a row's sub-effects: when its release lands, how they fall,
/// and how many there are how far apart.
struct Timing {
    start_time: i64,
    sub_effect_move_time: i64,
    sub_effect_move_speed: i64,
    sub_effect_default_height: i64,
    sub_effect_interval_time: i64,
    sub_effect_count: u32,
}

impl Timing {
    const fn of_strike(row: &DamageSkillRow) -> Self {
        Self {
            start_time: row.start_time,
            sub_effect_move_time: row.sub_effect_move_time,
            sub_effect_move_speed: row.sub_effect_move_speed,
            sub_effect_default_height: row.sub_effect_default_height,
            sub_effect_interval_time: row.sub_effect_interval_time,
            sub_effect_count: row.sub_effect_count,
        }
    }

    const fn of_terrain(row: &TerrainSkillRow) -> Self {
        Self {
            start_time: row.start_time,
            sub_effect_move_time: row.sub_effect_move_time,
            sub_effect_move_speed: row.sub_effect_move_speed,
            sub_effect_default_height: row.sub_effect_default_height,
            sub_effect_interval_time: row.sub_effect_interval_time,
            sub_effect_count: row.sub_effect_count,
        }
    }
}

/// The fall `lands_on` counts: `CSRS_Perform` activates the sub-effect on
/// the tick after preparing hands over, and it moves from the next, from
/// `speed x min(startTime, moveTime)` above its default height.
fn fall(start_raw: i64, move_time_raw: i64, speed_raw: i64, floor_raw: i64) -> Result<Fall> {
    let prepare = (ticks(start_raw)? - ticks(move_time_raw)?).max(1);
    Ok(Fall {
        first_move_on: u64::try_from(prepare + 2)
            .map_err(|_| Error::new("a battle skill falls before the fight begins"))?,
        start_q32: floor_raw.saturating_add(multiply(speed_raw, start_raw.min(move_time_raw))),
        floor_q32: floor_raw,
        step_q32: multiply(speed_raw, LOGIC_DELTA_RAW),
    })
}

/// `FPoint` multiplication.
fn multiply(left: i64, right: i64) -> i64 {
    i64::try_from((i128::from(left) * i128::from(right)) >> 32).unwrap_or(i64::MAX)
}

/// A time in seconds as whole ticks, `(int)(time / LogicDeltaTime)` in `FPoint`
/// division: `FPoint.RawDiv` rounds the quotient's magnitude to the nearest
/// raw unit, and the cast keeps the whole part below it, so -1.5 seconds is
/// -31 ticks.
fn ticks(seconds_raw: i64) -> Result<i64> {
    let scaled = u128::from(seconds_raw.unsigned_abs()) << 32;
    let divisor = u128::from(LOGIC_DELTA_RAW.unsigned_abs());
    let magnitude = scaled / divisor + u128::from((scaled % divisor) * 2 >= divisor);
    let magnitude = i128::try_from(magnitude).unwrap_or(i128::MAX);
    let quotient = if seconds_raw < 0 {
        -magnitude
    } else {
        magnitude
    };
    i64::try_from(quotient >> 32).map_err(|_| {
        Error::new(format!(
            "a battle skill time of {seconds_raw} is not a tick count"
        ))
    })
}

/// A waypoint skill's release, `CSRC_WayPoint`: no sub-effect lands. The
/// fight gives its path to the units it selects as it starts.
fn path_release(
    named: &str,
    team: u32,
    skill: &BattleSkill,
    row: &WaypointSkillRow,
) -> Result<SkillRelease> {
    if row.effect_target_type != 0 || row.sub_effect_buff_id != 0 {
        return Err(Error::new(format!(
            "{named} selects effect target type {} and writes buff {}, which \
             this build does not read",
            row.effect_target_type, row.sub_effect_buff_id
        )));
    }
    if skill.positions.len() < 2 {
        return Err(Error::new(format!(
            "{named} is released at {} positions, which make no path",
            skill.positions.len()
        )));
    }
    let points = skill
        .positions
        .iter()
        .map(|position| {
            let (local_x, local_z) = (i64::from(position.x), i64::from(position.y));
            if team == 0 {
                (local_x * SPACE, local_z * SPACE)
            } else {
                (-local_x * SPACE, -local_z * SPACE)
            }
        })
        .collect::<Vec<_>>();
    Ok(SkillRelease {
        team,
        name: skill.type_name.clone(),
        x: points[0].0,
        z: points[0].1,
        lands_on: 0,
        effect: SkillEffect::Path {
            points,
            width_q32: row.sub_effect_range,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn seconds(tenths: i64) -> i64 {
        (tenths << 32) / 10
    }

    /// The three landings the game recorded: the Electromagnetic Impact on 57,
    /// a Missile Strike on 63 and an Orbital Javelin on 62. The speed cancels;
    /// what separates them is whether the fall is shorter than the wait.
    #[test]
    fn a_sub_effect_lands_as_the_recordings_have_it() {
        let emp = CommanderSkillEffects::load().unwrap().buffs[0].clone();
        let rhino = CommanderSkillEffects::load().unwrap().summons[1].clone();
        assert_eq!(rhino.id, 1_200_002);
        // A Rhino Assault lands, and its creator makes the Rhino, on 28.
        assert_eq!(
            lands_on(
                rhino.start_time,
                rhino.sub_effect_move_time,
                rhino.sub_effect_move_speed
            )
            .unwrap(),
            28
        );
        assert_eq!(emp.id, 200_001);
        assert_eq!(
            lands_on(
                emp.start_time,
                emp.sub_effect_move_time,
                emp.sub_effect_move_speed
            )
            .unwrap(),
            57
        );
        // Missile Strike: 3 s against a 4.5 s fall at 650 m/s.
        assert_eq!(lands_on(seconds(30), seconds(45), 650 << 32).unwrap(), 63);
        // Orbital Javelin: 3 s against a 2 s fall at 1000 m/s.
        assert_eq!(lands_on(seconds(30), seconds(20), 1000 << 32).unwrap(), 62);
    }

    /// A scattered strike's sub-effects land as the recordings have them:
    /// Orbital Bombardment's every 20 ticks from 63, Ion Blast's on 62, 67
    /// and every six after (`tests/battle_skill/fights/`).
    #[test]
    fn a_scattered_strikes_sub_effects_land_their_interval_apart() {
        let table = CommanderSkillEffects::load().unwrap();
        let landings = |id: i32| {
            let row = table.strikes.iter().find(|row| row.id == id).unwrap();
            schedule(&Timing::of_strike(row))
                .unwrap()
                .into_iter()
                .map(|(lands_on, _)| lands_on)
                .collect::<Vec<_>>()
        };
        let orbital = landings(300_003);
        assert_eq!(orbital.len(), 15);
        assert_eq!(&orbital[..3], [63, 83, 103]);
        assert_eq!(orbital[14], 343);
        let ion = landings(300_006);
        assert_eq!(ion.len(), 60);
        assert_eq!(&ion[..4], [62, 67, 73, 79]);
    }

    /// A Smoke Bomb's seven fogs land every four ticks from 63, as
    /// `tests/terrain/fights/smoke.yaml` has them.
    #[test]
    fn a_smoke_bombs_fogs_land_every_four_ticks() {
        let table = CommanderSkillEffects::load().unwrap();
        let row = table.terrains.iter().find(|row| row.id == 600_002).unwrap();
        let landings = schedule(&Timing::of_terrain(row))
            .unwrap()
            .into_iter()
            .map(|(lands_on, _)| lands_on)
            .collect::<Vec<_>>();
        assert_eq!(landings, [63, 67, 71, 75, 79, 83, 87]);
    }
}
