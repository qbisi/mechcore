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
    layout::{Scatter, SkillBuff, SkillEffect, SkillRelease, SubEffect},
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
    /// `CSRC_Common.OnFightStart`: each release's
    /// `CommanderSkillManager.CalculateAttackPositions`, side by side in
    /// release order, after every unit has drawn its first intervals.
    ///
    /// A random circle draws each sub-effect's x and then its z from its
    /// side's stream in tenths: the x within the circle's radius `r`, the z
    /// within `sqrt(r² - x²)`, so the point stays inside. A line puts its
    /// `n` sub-effects `length / (n - 1)` apart from the release towards its
    /// second position, each the way there clamped to its distance.
    pub(in crate::fight) fn place_sub_effects(&mut self) -> Result<()> {
        for index in 0..self.commander.releases.len() {
            let team = self.commander.releases[index].team;
            let SkillEffect::Strike {
                scatter,
                sub_effects,
                ..
            } = &self.commander.releases[index].effect
            else {
                continue;
            };
            let (scatter, mut sub_effects) = (*scatter, sub_effects.clone());
            let count = i64::try_from(sub_effects.len()).unwrap_or(i64::MAX);
            for (step, sub_effect) in (0_i64..).zip(sub_effects.iter_mut()) {
                let (x_q32, z_q32) = (sub_effect.x_q32, sub_effect.z_q32);
                (sub_effect.x_q32, sub_effect.z_q32) = match scatter {
                    Scatter::Point => (x_q32, z_q32),
                    Scatter::RandomCircle { radius_q32 } => {
                        let random = self.side_random(team)?;
                        let across = random
                            .next_fix_in_range(radius_q32, 10, C0_1_RAW)
                            .ok_or_else(|| Error::new("a random circle has no radius"))?;
                        let room = fpcs_sqrt_fastest(
                            q32_mul(radius_q32, radius_q32).saturating_sub(q32_mul(across, across)),
                        );
                        let along = random.next_fix_in_range(room, 10, C0_1_RAW).unwrap_or(0);
                        (x_q32.saturating_add(across), z_q32.saturating_add(along))
                    }
                    Scatter::Line {
                        to_q32: (to_x, to_z),
                    } => {
                        let (dx, dz) = (to_x.saturating_sub(x_q32), to_z.saturating_sub(z_q32));
                        let spacing =
                            q32_div(native_q32_magnitude(dx, dz), (count - 1).max(1) << 32);
                        let (ox, oz) =
                            clamp_magnitude_q32_raw(dx, dz, q32_mul(spacing, step << 32));
                        (x_q32.saturating_add(ox), z_q32.saturating_add(oz))
                    }
                };
            }
            if let SkillEffect::Strike {
                sub_effects: placed,
                ..
            } = &mut self.commander.releases[index].effect
            {
                *placed = sub_effects;
            }
        }
        Ok(())
    }

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
            if let SkillEffect::Strike {
                range_q32,
                damage,
                crosses_shields,
                buff,
                sub_effects,
                ..
            } = &release.effect
            {
                let falling = self.step_sub_effects(
                    &release,
                    (*range_q32, *damage, *crosses_shields, buff.as_ref()),
                    sub_effects,
                    (tick, target_search_order),
                    events,
                )?;
                if falling.is_empty() {
                    self.commander.releases.remove(index);
                    continue;
                }
                if let SkillEffect::Strike { sub_effects, .. } =
                    &mut self.commander.releases[index].effect
                {
                    *sub_effects = falling;
                }
                index += 1;
                continue;
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
                    let point = (space_to_q32(release.x), space_to_q32(release.z));
                    let reached = self.skill_reach(point, *range_q32, target_search_order);
                    self.write_skill_buff(&release, buff, &reached, events)?;
                }
                // A strike's sub-effects land above, and a path is given out
                // as the fight starts and never lands.
                SkillEffect::Strike { .. } | SkillEffect::Path { .. } => {}
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

    /// `CSRS_Perform`'s update of a strike's agents, in the order they were
    /// activated: one that lands on this tick strikes where it lands, and one
    /// that cannot cross shields stops at the first it comes inside as it
    /// falls, before it would land, and strikes there. Answers the ones still
    /// to land.
    ///
    /// A strike that writes a buff (`PerformNegativeEffect`) takes the units
    /// its circle reaches before it deals its damage, and writes the buff on
    /// those still alive after, as `BuffSystem.AddBuff` skips the dead. The
    /// list also leaves out the units a shield covers, which is not measured,
    /// so such a strike is refused beside a battlefield shield.
    fn step_sub_effects(
        &mut self,
        release: &SkillRelease,
        (range_q32, damage, crosses_shields, buff): (i64, i64, bool, Option<&SkillBuff>),
        sub_effects: &[SubEffect],
        (tick, target_search_order): (u64, &BTreeMap<u32, Vec<FightActorRef>>),
        events: &mut Vec<Event>,
    ) -> Result<Vec<SubEffect>> {
        if buff.is_some() && !self.shield.standing.is_empty() {
            return Err(Error::new(format!(
                "{} writes a buff in a fight with a battlefield shield, which is not measured",
                release.name
            )));
        }
        let mut falling = Vec::new();
        for sub_effect in sub_effects {
            let (x_q32, z_q32, fall) = (sub_effect.x_q32, sub_effect.z_q32, sub_effect.fall);
            if !crosses_shields
                && tick <= sub_effect.lands_on
                && let Some(height) = fall.height_on(tick)
            {
                let last = fall.height_on(tick - 1).unwrap_or(fall.start_q32);
                if let Some((_, point)) =
                    self.falling_into_shield((x_q32, height, z_q32), (x_q32, last, z_q32))
                {
                    self.strike_circle(release, range_q32, damage, false, point, events)?;
                    continue;
                }
            }
            if sub_effect.lands_on == tick {
                let reached = match buff {
                    Some(_) => self.skill_reach((x_q32, z_q32), range_q32, target_search_order),
                    None => Vec::new(),
                };
                let point = (x_q32, 0, z_q32);
                self.strike_circle(release, range_q32, damage, crosses_shields, point, events)?;
                if let Some(buff) = buff {
                    self.write_skill_buff(release, buff, &reached, events)?;
                }
                continue;
            }
            falling.push(*sub_effect);
        }
        Ok(falling)
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
                (x_q32, z_q32),
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
        (x_q32, z_q32): (i64, i64),
        range_q32: i64,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
    ) -> Vec<u64> {
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
            debuff: buff.debuff,
            invincible: false,
        };
        for &id in reached {
            // `BuffSystem.AddBuff` passes over the dead: a strike's damage
            // may have killed what its circle reached.
            if !self.actors[&id].alive() || !self.buff_reaches(id, &row) {
                continue;
            }
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
            events.push(self.write_buff(id, None, release.team, &row)?);
        }
        Ok(())
    }
}
