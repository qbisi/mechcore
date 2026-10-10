//! What a layout's battle skills do once released.
//!
//! [`config/commander_skill_effects.yaml`](../../../../config/commander_skill_effects.yaml),
//! which `scripts/extract/extract-commander-skill-effects.py` writes, holds the
//! skills the fight releases, the buff skills and the support skills;
//! `docs/rules/battle_skill.md` states what they do. Every other battle skill
//! is refused by name.

use mechcore_document::{BattleSkill, OilArea};
use serde::Deserialize;

use super::contraptions::{ShieldKind, ShieldPlacement};
use crate::{
    Error, Result,
    rules::{BuffConfig, UnitConfig, UnitConfigs},
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
    /// The rounds it stands.
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
    cross_advanced_shield: bool,
    #[serde(default)]
    buff: Option<BuffRow>,
}

/// `Config.groundFireDamage` and `fireAttackInterval`.
#[derive(Debug, Clone, Copy, Deserialize)]
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
    /// `FogSand`, which a move ability's technology leaves.
    FogSand,
    Acid,
}

/// What a terrain does to the units standing in it, read off its row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TerrainEffect {
    /// `FogController`: an attack range rate on every ranged skill.
    Fog { attack_range_rate: i64 },
    /// `FogSandController`: an attack range rate on every ranged skill, as
    /// a fog's, and a rate on the remote hits every unit takes.
    FogSand {
        attack_range_rate: i64,
        remote_damage_rate: i64,
    },
    /// `GroundFireController`: `Config.groundFireDamage` on entering, and
    /// again every `fireAttackInterval`, in ticks.
    Fire { damage: i64, period_ticks: i32 },
    /// `BuffItemController`: the row's buff on entering, and again every
    /// tick short of its duration, so that it runs while the unit stays.
    Buff { buff: SkillBuff, period_ticks: i32 },
}

/// A terrain one sub-effect leaves where it lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TerrainSpec {
    pub(crate) kind: TerrainKind,
    /// `GetRangeItemRange`, `FPoint` raw metres.
    pub(crate) radius_q32: i64,
    /// `lifeTime` in ticks, when it burns out within a fight.
    pub(crate) life_ticks: Option<i32>,
    /// `GetRoundDuration`: the rounds it stands, which a fire ignores.
    pub(crate) rounds: i32,
    pub(crate) effect: TerrainEffect,
    /// The fire it turns to when a fire reaches it, an oil's alone.
    pub(crate) burns: Option<Burning>,
}

/// The fire an oil turns to: `RangeItemSystem.CheckInteractableItems` makes
/// it with the oil's provider, so it takes the oil's range and rounds and
/// burns the oil row's `fireLifeTime` (`CS_Oil.GetFireLifeTime`), dealing
/// `Config`'s fire as any fire does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Burning {
    pub(crate) life_ticks: i32,
    pub(crate) damage: i64,
    pub(crate) period_ticks: i32,
}

impl TerrainSpec {
    /// The fire this terrain turns to, when it is an oil.
    pub(crate) const fn burning(&self) -> Option<Self> {
        let Some(burning) = self.burns else {
            return None;
        };
        Some(Self {
            kind: TerrainKind::Fire,
            radius_q32: self.radius_q32,
            life_ticks: Some(burning.life_ticks),
            rounds: self.rounds,
            effect: TerrainEffect::Fire {
                damage: burning.damage,
                period_ticks: burning.period_ticks,
            },
            burns: None,
        })
    }
}

/// A standard skill of a kind the fight does not release: which
/// `CommanderSkillGroupData` list it comes from, which its refusal names.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct OtherSkillRow {
    id: i32,
    name: String,
    kind: String,
}

/// A `CSD_WayPoint` row: the width of its path. Every standard beacon walks
/// its own side's units and writes no buff, which the extraction checks.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct WaypointSkillRow {
    id: i32,
    name: String,
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

/// The Sticky Oil Bomb, whose oil stands into the next round.
const STANDING_OIL_SKILL: i32 = 400_002;

/// The oil a Sticky Oil Bomb of an earlier round left on a side: the line it
/// was released along, and which of the points that line expands into still
/// stand.
#[derive(Debug, Clone)]
pub(crate) struct StandingOil {
    pub(crate) team: u32,
    pub(crate) name: String,
    pub(crate) spec: TerrainSpec,
    /// The two control points, Q32.32 world metres.
    pub(crate) from_q32: (i64, i64),
    pub(crate) to_q32: (i64, i64),
    /// The row's `subEffectCount`, the points the line expands into.
    pub(crate) count: usize,
    /// The native indices of the points that still stand, in order, each
    /// with the cells it still holds when a shield cut it, as a recording
    /// reads them in the world's frame.
    pub(crate) points: Vec<(u32, Option<Vec<u32>>)>,
}

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
    energy_shield_damage: i64,
    start_time: i64,
    effect_range: i64,
    sub_effect_move_speed: i64,
    sub_effect_move_time: i64,
    cross_advanced_shield: bool,
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
    /// The English name the game gives it; none for a buff the game never shows.
    #[serde(default)]
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
    life_change_rate: i64,
    step_time: i64,
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
    /// `SupportUnitEffectController`: a creator of summons.
    Summon(Box<Summon>),
    /// `CommanderSkillSubEffectController`, which `CS_Damage` and `CS_Buff`
    /// both create: the skill's damage over each sub-effect's circle, and
    /// its buff on the units the circle reaches.
    Strike {
        /// `FPoint` raw metres, each circle's radius: `subEffectRange`, which
        /// `CommanderSkillData.PreProcess` makes a circle's `effectRange`.
        range_q32: i64,
        damage: i64,
        /// `isCrossAdvancedShield`: it passes shields, falling and landing.
        crosses_shields: bool,
        /// `CS_Buff`'s `energyShieldDamage`, as its damage modifier holds
        /// it; none for a skill that is no damage modifier. A modifier makes
        /// the sub-effect a hit even when its damage is nothing
        /// (`IsDamageEffect`), and adds this to what the hit deals a shield
        /// when it is above zero (`ChangeHitEnergyShieldDamage`).
        shield_damage: Option<i64>,
        /// `ICommanderSkill.IsHarmful`: the circle reaches either side, and
        /// strikes; a skill that is not reaches its own side's units with its
        /// buff alone (`PerformHitEffect`). `CS_Damage` always is, and
        /// `CS_Buff` is as its buff is.
        harmful: bool,
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
        /// `isCrossAdvancedShield`: whether a falling sub-effect passes a
        /// shield it comes inside.
        crosses_shields: bool,
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
    /// Whether its row gives it a speed: `CommanderSkillSubEffectAgent.Update`
    /// finishes one with none where it stands, without moving it or testing
    /// it against a shield.
    pub(crate) moves: bool,
}

impl Fall {
    /// Where it stands on a tick of its fall, `FPoint` raw metres up: none
    /// before it moves, and none ever for one that does not move.
    pub(crate) fn height_on(&self, tick: u64) -> Option<i64> {
        if !self.moves {
            return None;
        }
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
    /// What its side's loadout hands it: `FightEffectSystem.AddEffect`
    /// looks up what the side registered for its mech, as for a unit of its
    /// type deployed without equipment. The layout fills it in, where the
    /// side is known.
    pub(crate) effects: crate::layout::UnitEffects,
    /// The lines it runs of its own, as it joins.
    pub(crate) productions: Vec<crate::layout::Production>,
}

/// The buff a released skill writes: the Electromagnetic Impact's slow,
/// Lightning Storm's, or Photon Emission's protection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    /// `lifeChangeRate`, an `FPoint` raw rate of the unit's maximum life it
    /// changes by every step.
    pub(crate) life_change_rate: i64,
    /// `stepTime` in ticks, `Buff.Init`'s `stepTimeConfig`.
    pub(crate) step_ticks: u32,
    /// `attackRangeChangeValue`, whole metres the main skill's range changes
    /// by (`BuffManager.GetAttackRangeAddValue`, `GetAttackRangeReduceValue`).
    pub(crate) attack_range_value: i64,
    /// `currentLifeDisposableChangeRate`, an `FPoint` raw rate of the unit's
    /// life it changes by once, as it is written.
    pub(crate) current_life_rate: i64,
}

impl SkillBuff {
    /// `BuffData.IsHarmful` over the fields the table carries: a loss of
    /// life, a slower speed or more damage taken. A harmful buff is written
    /// on every unit in reach, of either side (`PerformNegativeEffect`), and
    /// any other on the releasing side's alone (`PerformPositiveEffect`).
    pub(crate) const fn harmful(&self) -> bool {
        self.life_change_rate < 0
            || self.move_speed_rate < 0
            || self.amplify_damage_rate > 0
            || self.attack_range_value < 0
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
    ground_fire: GroundFire,
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
            ground_fire: table.ground_fire,
            others: table.other_skills,
        })
    }

    /// The fire a unit's extra weapon leaves where its shot lands
    /// (`GroundFireController.GetFireMech`): of the range and the lifetime its
    /// technology wrote onto the unit, the skill's splash and its row's
    /// `fireLifeTime`, dealing `Config`'s fire as any fire does.
    ///
    /// # Errors
    ///
    /// Returns an error when a time is no whole number of ticks within a
    /// fight.
    pub(crate) fn unit_fire(&self, radius_q32: i64, life_seconds_raw: i64) -> Result<TerrainSpec> {
        let period_ticks = i32::try_from(ticks(self.ground_fire.interval)?)
            .map_err(|_| Error::new("a fire's interval outlasts a fight"))?;
        let life_ticks = i32::try_from(ticks(life_seconds_raw)?)
            .map_err(|_| Error::new("a unit's fire outlasts a fight"))?;
        Ok(TerrainSpec {
            kind: TerrainKind::Fire,
            radius_q32,
            life_ticks: Some(life_ticks),
            rounds: 0,
            effect: TerrainEffect::Fire {
                damage: self.ground_fire.damage,
                period_ticks,
            },
            burns: None,
        })
    }

    /// The oil a unit's extra weapon leaves where its shot lands
    /// (`ExtraWeaponTech` as its `IRangeItemProvider`): as wide as the
    /// skill's splash, standing for no set time and one round, writing the
    /// row's buff on what stands in it, and burning its `fireLifeTime` once a
    /// fire reaches it, dealing `Config`'s fire as any fire does.
    ///
    /// # Errors
    ///
    /// Returns an error when a time is no whole number of ticks within a
    /// fight.
    pub(crate) fn unit_oil(
        &self,
        named: &str,
        radius_q32: i64,
        buff: SkillBuff,
        fire_life_seconds_raw: i64,
    ) -> Result<TerrainSpec> {
        let period_ticks = i32::try_from(ticks(self.ground_fire.interval)?)
            .map_err(|_| Error::new("a fire's interval outlasts a fight"))?;
        let life_ticks = i32::try_from(ticks(fire_life_seconds_raw)?)
            .map_err(|_| Error::new(format!("{named}'s oil burns past a fight")))?;
        if life_ticks <= 0 {
            return Err(Error::new(format!("{named}'s oil burns for no time")));
        }
        Ok(TerrainSpec {
            kind: TerrainKind::Oil,
            radius_q32,
            life_ticks: None,
            rounds: 1,
            effect: buff_terrain(named, buff)?,
            burns: Some(Burning {
                life_ticks,
                damage: self.ground_fire.damage,
                period_ticks,
            }),
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

    /// The oil a Sticky Oil Bomb of an earlier round left on a side. A
    /// point's cells are the side's, so red's turn half a turn.
    ///
    /// # Errors
    ///
    /// Returns an error when the table does not hold the skill.
    pub(crate) fn standing_oil(&self, team: u32, area: &OilArea) -> Result<StandingOil> {
        let row = self
            .terrains
            .iter()
            .find(|row| row.id == STANDING_OIL_SKILL)
            .ok_or_else(|| Error::new("the Sticky Oil Bomb is not in the table"))?;
        let name = "sticky_oil_bomb";
        let SkillEffect::Terrain {
            spec, sub_effects, ..
        } = terrain_effect(name, row, self.ground_fire)?
        else {
            unreachable!("a terrain row leaves a terrain");
        };
        let world = |position: &mechcore_document::Position| {
            let (x, z) = (i64::from(position.x), i64::from(position.y));
            if team == 0 {
                (x << 32, z << 32)
            } else {
                (-x << 32, -z << 32)
            }
        };
        let [from, to] = area.control_points.as_slice() else {
            return Err(Error::new(format!(
                "standing {name} has {} control points, not two",
                area.control_points.len()
            )));
        };
        let points = if area.grid_rows.is_empty() {
            (0..u32::try_from(sub_effects.len()).unwrap_or(u32::MAX))
                .map(|point| (point, None))
                .collect()
        } else {
            area.grid_rows
                .iter()
                .map(|(&point, rows)| {
                    let rows = (!rows.is_empty()).then(|| {
                        if team == 0 {
                            rows.clone()
                        } else {
                            mechcore_document::rotate_oil_grid_rows(rows)
                        }
                    });
                    (point, rows)
                })
                .collect()
        };
        Ok(StandingOil {
            team,
            name: name.to_owned(),
            spec,
            from_q32: world(from),
            to_q32: world(to),
            count: sub_effects.len(),
            points,
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
                    terrain_effect(&named, row, self.ground_fire)?,
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
    // A support skill's sub-effect that falls would be tested against the
    // shields it comes inside (`CommanderSkillSubEffectAgent.Update`), and
    // stopped there by `InterruptEffect`; every row gives it no speed, so it
    // lands where it is released.
    if matches!(effect, SkillEffect::Summon(_)) && common.move_speed != 0 {
        return Err(Error::new(format!(
            "{named}'s sub-effect falls, which no support skill's does"
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
fn terrain_effect(named: &str, row: &TerrainSkillRow, fire: GroundFire) -> Result<SkillEffect> {
    if row.effect_range_type != 1 {
        return Err(Error::new(format!(
            "{named} has range type {}, which this build does not read",
            row.effect_range_type
        )));
    }
    let fire_period = i32::try_from(ticks(fire.interval)?)
        .map_err(|_| Error::new("a fire's interval outlasts a fight"))?;
    let effect = match row.kind {
        TerrainKind::Fog => TerrainEffect::Fog {
            attack_range_rate: row.attack_range_change_rate,
        },
        TerrainKind::Oil | TerrainKind::Acid => {
            let Some(buff) = &row.buff else {
                return Err(Error::new(format!(
                    "{named} leaves a terrain that writes no buff"
                )));
            };
            buff_terrain(named, skill_buff(named, buff)?)?
        }
        TerrainKind::Fire => TerrainEffect::Fire {
            damage: fire.damage,
            period_ticks: fire_period,
        },
        TerrainKind::FogSand => {
            return Err(Error::new(format!(
                "{named} leaves a sand fog, which only a move ability's technology does"
            )));
        }
    };
    let lifetime = |life: i64| -> Result<Option<i32>> {
        match life {
            0 => Ok(None),
            life => i32::try_from(ticks(life)?)
                .map(Some)
                .map_err(|_| Error::new(format!("{named}'s terrain outlasts a fight"))),
        }
    };
    let life_ticks = lifetime(row.life_time)?;
    let burns = match (row.kind, lifetime(row.fire_life_time)?) {
        (TerrainKind::Oil, Some(life_ticks)) => Some(Burning {
            life_ticks,
            damage: fire.damage,
            period_ticks: fire_period,
        }),
        (TerrainKind::Oil, None) => {
            return Err(Error::new(format!("{named}'s oil burns for no time")));
        }
        _ => None,
    };
    Ok(SkillEffect::Terrain {
        spec: TerrainSpec {
            kind: row.kind,
            radius_q32: row.sub_effect_range,
            life_ticks,
            rounds: row.effect_duration,
            effect,
            burns,
        },
        crosses_shields: row.cross_advanced_shield,
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
        shield_damage: None,
        harmful: true,
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
/// `CommanderSkillData.PreProcess` made the circle's one sub-effect's, and
/// its damage modifier's hit over it.
fn buff_effect(named: &str, row: &BuffSkillRow) -> Result<SkillEffect> {
    if row.sub_effect_damage != 0 {
        return Err(Error::new(format!(
            "{named} deals damage this build does not read"
        )));
    }
    if row.effect_range_type != 0 {
        return Err(Error::new(format!(
            "{named} has range type {}, which this build does not read",
            row.effect_range_type
        )));
    }
    let buff = skill_buff(named, &row.buff)?;
    Ok(SkillEffect::Strike {
        range_q32: row.effect_range,
        damage: 0,
        crosses_shields: row.cross_advanced_shield,
        shield_damage: Some(row.energy_shield_damage),
        harmful: buff.harmful(),
        buff: Some(buff),
        scatter: Scatter::Point,
        sub_effects: schedule(&Timing::of_buff(row))?
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

/// The buff a sub-effect writes, as `BuffSystem.AddBuff` reads its row.
/// A terrain that writes its buff on what stands in it.
fn buff_terrain(named: &str, buff: SkillBuff) -> Result<TerrainEffect> {
    Ok(TerrainEffect::Buff {
        buff,
        // `BuffItemController.Add`: the buff's duration in ticks, less one,
        // and never under one.
        period_ticks: i32::try_from(buff.ticks)
            .map_err(|_| Error::new(format!("{named}'s buff outlasts a fight")))?
            .saturating_sub(1)
            .max(1),
    })
}

/// A buff source's range item's buff, its fields as `buffDatas` writes them.
#[allow(
    clippy::struct_excessive_bools,
    reason = "the buff row's flags are independent fields"
)]
pub(crate) struct ItemBuff {
    pub(crate) id: u32,
    pub(crate) divide: i32,
    pub(crate) additive: bool,
    /// `duration` and `stepTime`, `FPoint` raw seconds.
    pub(crate) duration_raw: i64,
    pub(crate) debuff: bool,
    pub(crate) invincible: bool,
    pub(crate) disable_technology: bool,
    pub(crate) amplify_damage_rate: i64,
    pub(crate) move_speed_rate: i64,
    pub(crate) life_change_rate: i64,
    pub(crate) step_time_raw: i64,
    pub(crate) attack_range_value: i64,
}

/// A buff source's `BuffRangeItem` as the terrain it leaves: an acid of
/// `range` whole metres, `life` `FPoint` raw seconds and `rounds`, whose
/// `BuffItemController` keeps its buff on the units in it as a battle
/// skill's acid does.
///
/// # Errors
///
/// Returns an error when the terrain or its buff outlasts a fight.
pub(crate) fn buff_item_terrain(
    named: &str,
    (range, life, rounds): (i64, i64, i32),
    buff: &ItemBuff,
) -> Result<TerrainSpec> {
    let fight_ticks = |raw: i64, what: &str| -> Result<u32> {
        u32::try_from(ticks(raw)?)
            .map_err(|_| Error::new(format!("{named}'s {what} outlasts a fight")))
    };
    let skill_buff = SkillBuff {
        id: buff.id,
        divide: buff.divide,
        additive: buff.additive,
        ticks: fight_ticks(buff.duration_raw, "buff")?,
        move_speed_rate: buff.move_speed_rate,
        disable_technology: buff.disable_technology,
        debuff: buff.debuff,
        invincible: buff.invincible,
        amplify_damage_rate: buff.amplify_damage_rate,
        life_change_rate: buff.life_change_rate,
        step_ticks: fight_ticks(buff.step_time_raw, "buff's step")?,
        attack_range_value: buff.attack_range_value,
        current_life_rate: 0,
    };
    let life_ticks = match life {
        0 => None,
        life => Some(
            i32::try_from(fight_ticks(life, "terrain")?)
                .map_err(|_| Error::new(format!("{named}'s terrain outlasts a fight")))?,
        ),
    };
    Ok(TerrainSpec {
        kind: TerrainKind::Acid,
        radius_q32: range << 32,
        life_ticks,
        rounds,
        effect: buff_terrain(named, skill_buff)?,
        burns: None,
    })
}

/// A technology's skill's buff, as a battle skill's is written.
///
/// # Errors
///
/// Returns an error naming the buff when it corrects a speed by a value,
/// which a skill's buff here does not write, or outlasts a fight.
pub(crate) fn technology_buff(named: &str, buff: &BuffConfig) -> Result<SkillBuff> {
    if buff.move_speed_value != 0 {
        return Err(Error::new(format!(
            "{named}'s buff {} adds a move speed value, which a skill's buff here does not write",
            buff.id
        )));
    }
    Ok(SkillBuff {
        id: buff.id,
        divide: buff.divide,
        additive: buff.additive,
        ticks: u32::try_from(ticks(crate::rules::metres_q32(buff.duration))?)
            .map_err(|_| Error::new(format!("{named}'s buff outlasts a fight")))?,
        move_speed_rate: buff.move_speed_rate,
        disable_technology: buff.disable_technology,
        debuff: buff.debuff,
        invincible: buff.invincible,
        amplify_damage_rate: buff.amplify_damage_rate,
        life_change_rate: 0,
        step_ticks: u32::try_from(ticks(crate::rules::metres_q32(buff.step_time))?)
            .map_err(|_| Error::new(format!("{named}'s buff steps beyond a fight")))?,
        attack_range_value: buff.attack_range_value,
        current_life_rate: buff.current_life_rate,
    })
}

fn skill_buff(named: &str, buff: &BuffRow) -> Result<SkillBuff> {
    if buff.life_change_rate > 0 {
        return Err(Error::new(format!(
            "{named}'s buff {} ({}) heals, and a buff's healing is not measured",
            buff.id, buff.name
        )));
    }
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
        life_change_rate: buff.life_change_rate,
        step_ticks: u32::try_from(ticks(buff.step_time)?)
            .map_err(|_| Error::new(format!("{named}'s buff steps beyond a fight")))?,
        attack_range_value: 0,
        current_life_rate: 0,
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
        effects: crate::layout::UnitEffects::default(),
        productions: Vec::new(),
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
    /// A buff skill's circle: one sub-effect, falling to the ground.
    const fn of_buff(row: &BuffSkillRow) -> Self {
        Self {
            start_time: row.start_time,
            sub_effect_move_time: row.sub_effect_move_time,
            sub_effect_move_speed: row.sub_effect_move_speed,
            sub_effect_default_height: 0,
            sub_effect_interval_time: 0,
            sub_effect_count: 1,
        }
    }

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
        moves: speed_raw != 0,
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
/// The sand fog a move ability's technology leaves: its row's range and
/// life, one round (`MoveAbilityRangeItemTech.GetRoundDuration`), and its
/// two rates.
///
/// # Errors
///
/// Returns an error when its life outlasts a fight.
pub(crate) fn sand_fog(source: crate::modifier::MoveAbilityRangeItem) -> Result<TerrainSpec> {
    let life_ticks = i32::try_from(ticks(source.life_time)?)
        .map_err(|_| Error::new("a sand fog outlasts a fight"))?;
    Ok(TerrainSpec {
        kind: TerrainKind::FogSand,
        radius_q32: source.range,
        life_ticks: Some(life_ticks),
        rounds: 1,
        effect: TerrainEffect::FogSand {
            attack_range_rate: source.attack_range_rate,
            remote_damage_rate: source.remote_damage_rate,
        },
        burns: None,
    })
}

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
    /// and every six after (`tests/battle_skill/`).
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
    /// `tests/terrain/smoke.yaml` has them.
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

    #[test]
    fn an_acid_blasts_buff_takes_life_every_ten_ticks() {
        let table = CommanderSkillEffects::load().unwrap();
        let row = table.terrains.iter().find(|row| row.id == 500_002).unwrap();
        let Ok(SkillEffect::Terrain {
            spec:
                TerrainSpec {
                    effect: TerrainEffect::Buff { buff, period_ticks },
                    ..
                },
            ..
        }) = terrain_effect("acid_blast", row, table.ground_fire)
        else {
            panic!("an acid writes a buff");
        };
        assert_eq!((buff.id, buff.ticks, period_ticks), (500_001, 20, 19));
        assert_eq!((buff.step_ticks, buff.life_change_rate), (10, -64_424_509));
        assert!(buff.harmful());
    }
}
