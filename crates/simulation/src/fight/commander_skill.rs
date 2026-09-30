//! `CommanderSkillSystem`: a released battle skill lands and writes its buff.
//!
//! A released skill is `CommanderSkillReleaseState` under `CSRC_Common`: it
//! waits, its one sub-effect falls, and on the tick the sub-effect lands
//! `CommanderSkillSubEffectController.PerformNegativeEffect` reaches every
//! unit whose edge is within the skill's range of where it landed, of either
//! domain and of either side, the releasing side's own included, and writes
//! the skill's buff on each. `CommanderSkillSystem`
//! updates after `BuffSystem` and before `MineSystem`, so what it writes runs
//! from the next tick. The layout compiled the tick it lands on;
//! `docs/rules/battle_skill.md` states the rule.

use super::rvo::fpoint_less_or_equal;
use super::*;
use crate::{
    data::{Entry, Index},
    layout::{SkillBuff, SkillEffect, SkillRelease},
};

/// `CommanderSkillSystem`: the releases still to land, and which sides a
/// technology-disabling skill would find researched.
pub(in crate::fight) struct CommanderSkillSystem {
    /// Each side's released battle skills, in side order.
    pub(in crate::fight) releases: Vec<SkillRelease>,
    /// The sides that researched a unit technology.
    pub(in crate::fight) researched: BTreeSet<u32>,
}

impl CommanderSkillSystem {
    /// A layout's releases, each side's in the order it releases them.
    pub(in crate::fight) fn new(layout: &CompiledLayout) -> Self {
        let mut releases = layout.battle_skills.clone();
        releases.sort_by_key(|release| release.team);
        Self {
            releases,
            researched: layout.researched.clone(),
        }
    }
}

/// What tags a battle skill's buff, so that its end takes it away.
const SKILL_SOURCE: &str = "BuffSystem.CommanderSkill";

impl Simulation {
    /// `CommanderSkillSystem`'s update: every release whose sub-effect lands
    /// on this tick, in the order the sides released them.
    pub(in crate::fight) fn step_battle_skills(
        &mut self,
        step: u64,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let tick = step + 1;
        let mut index = 0;
        while index < self.commander.releases.len() {
            let release = self.commander.releases[index].clone();
            // A falling strike that cannot cross shields stops at the first it
            // comes inside, before it would land, and strikes there.
            if let SkillEffect::Strike {
                range_q32,
                damage,
                crosses_shields: false,
                fall,
                ..
            } = &release.effect
                && tick <= release.lands_on
                && let Some(height) = fall.height_on(tick)
            {
                let last = fall.height_on(tick - 1).unwrap_or(fall.start_q32);
                let (x_q32, z_q32) = (space_to_q32(release.x), space_to_q32(release.z));
                if let Some((_, point)) =
                    self.falling_into_shield((x_q32, height, z_q32), (x_q32, last, z_q32))
                {
                    self.commander.releases.remove(index);
                    self.strike_circle(&release, *range_q32, *damage, false, point, events)?;
                    continue;
                }
            }
            index += 1;
            if release.lands_on != tick {
                continue;
            }
            // A falling sub-effect of any other kind stops at the first
            // shield it enters too, and a buff skill's damage modifier
            // strikes shields: neither is measured. A Shield Airdrop's
            // crosses shields whatever its row says
            // (`CS_EnergyShield.CanCrossAdvancedEnergyShield`).
            if !self.shield.standing.is_empty()
                && matches!(
                    release.effect,
                    SkillEffect::Buff { .. } | SkillEffect::Summon(_)
                )
            {
                return Err(Error::new(format!(
                    "{} lands in a fight with a battlefield shield, which is not measured",
                    release.name
                )));
            }
            match &release.effect {
                SkillEffect::Buff { range_q32, buff } => {
                    let reached = self.skill_reach(&release, *range_q32, target_search_order);
                    self.write_skill_buff(&release, buff, &reached, events)?;
                }
                SkillEffect::Strike {
                    range_q32,
                    damage,
                    crosses_shields,
                    ..
                } => {
                    let point = (space_to_q32(release.x), 0, space_to_q32(release.z));
                    self.strike_circle(
                        &release,
                        *range_q32,
                        *damage,
                        *crosses_shields,
                        point,
                        events,
                    )?;
                }
                // Given out as the fight starts, and never landed.
                SkillEffect::Path { .. } => {}
                SkillEffect::Shield { radius_q32, energy } => {
                    self.create_shield(release.team, release.x, release.z, *radius_q32, *energy);
                }
                // `SupportUnitEffectController.PerformEffect`: a creator for
                // the side's `TeamSupportUnitManager`, which updates later in
                // this very tick.
                SkillEffect::Summon(summon) => {
                    self.support
                        .creators
                        .push(super::support_unit::Creator::new(
                            release.team,
                            release.x,
                            release.z,
                            (**summon).clone(),
                        ));
                }
            }
        }
        Ok(())
    }

    /// `CommanderSkillSubEffectController.PerformNegativeEffect` for a damage
    /// skill: `CommanderSkillDamageProvider`'s damage over the skill's circle
    /// where it landed, on everything of either side it reaches, with no
    /// owner.
    fn strike_circle(
        &mut self,
        release: &SkillRelease,
        range_q32: i64,
        damage: i64,
        crosses_shields: bool,
        (x_q32, y_q32, z_q32): (i64, i64, i64),
        events: &mut Vec<Event>,
    ) -> Result<()> {
        // A battle skill's circle reaches units alone.
        let hit = DamageHit {
            crosses_shields,
            strikes_buildings: false,
            ..DamageHit::unowned(
                release.team,
                damage,
                (q32_to_space_rounded(x_q32), q32_to_space_rounded(z_q32)),
                y_q32,
                q32_to_space_rounded(range_q32),
            )
        };
        // `PrepareRangeTargets` asks `CalculateRangeActors` with
        // `includeBuilding` off: a tower is never struck. Whether a
        // construction is, as one of its side's actors, is not measured.
        if let Some(block) = self.construction_in_reach(x_q32, z_q32, range_q32) {
            return Err(Error::new(format!(
                "{} reaches construction building {block}, and whether a battle skill strikes \
                 a construction is not measured",
                release.name
            )));
        }
        let struck = self.perform_damage(hit, events)?;
        self.record_ends(struck.ends, events);
        Ok(())
    }

    /// The first standing construction block whose edge a circle reaches.
    fn construction_in_reach(&self, x_q32: i64, z_q32: i64, range_q32: i64) -> Option<u64> {
        self.buildings
            .iter()
            .filter(|building| {
                building_alive(building) && self.constructions.contains_key(&building.building_id)
            })
            .find(|building| {
                native_q32_magnitude(
                    building.position.x.saturating_sub(x_q32),
                    building.position.z.saturating_sub(z_q32),
                )
                .saturating_sub(space_to_q32(building_radius(building)))
                    <= range_q32
            })
            .map(|building| building.building_id)
    }

    /// `RangeTargetCalculator.CalculateRangeActors` as the sub-effect asks it,
    /// and what `PerformNegativeEffect` keeps of it: every live unit of either
    /// side, ground or air, whose edge is within the range of where the skill
    /// landed by `FPoint.op_LessThanOrEqual`, side by side in the order each
    /// side's objects are searched. The calculator is given no team, and
    /// `IsBuffTarget` keeps units alone.
    fn skill_reach(
        &self,
        release: &SkillRelease,
        range_q32: i64,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
    ) -> Vec<u64> {
        let (x_q32, z_q32) = (space_to_q32(release.x), space_to_q32(release.z));
        target_search_order
            .values()
            .flatten()
            .filter_map(|&candidate| {
                let FightActorRef::Unit(id) = candidate else {
                    return None;
                };
                if !self.actors[&id].alive() {
                    return None;
                }
                let view = self.fight_actor(candidate)?;
                let distance = native_q32_magnitude(
                    view.x_q32.saturating_sub(x_q32),
                    view.z_q32.saturating_sub(z_q32),
                )
                .saturating_sub(space_to_q32(view.radius));
                fpoint_less_or_equal(distance, range_q32).then_some(id)
            })
            .collect()
    }

    /// `BuffSystem.AddBuff` of the skill's row on every unit it reached.
    fn write_skill_buff(
        &mut self,
        release: &SkillRelease,
        buff: &SkillBuff,
        reached: &[u64],
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let row = super::tower::BuffRow {
            buff_id: buff.id,
            divide: buff.divide,
            additive: buff.additive,
            ticks: buff.ticks,
            source: SKILL_SOURCE,
            entries: vec![Entry {
                index: Index::MoveSpeed,
                source: SKILL_SOURCE,
                correction: super::tower::rate(buff.move_speed_rate),
            }],
            disables_technology: buff.disable_technology,
        };
        for &id in reached {
            let actor = &self.actors[&id];
            if buff.disable_technology && self.commander.researched.contains(&actor.placement.team)
            {
                return Err(Error::new(format!(
                    "{} reaches unit {id}, whose side researched a technology, and disabling \
                     a technology mid-fight is not measured",
                    release.name
                )));
            }
            if let Some(running) = actor.other_buff(buff.id) {
                return Err(Error::new(format!(
                    "{} reaches unit {id}, which runs buff {running}, and a skill's buff over \
                     another is not measured",
                    release.name
                )));
            }
            events.push(self.write_buff(id, release.team, &row)?);
        }
        Ok(())
    }
}
