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

use super::*;
use crate::{
    data::{Entry, Index},
    layout::{Scatter, SkillBuff, SkillEffect, SkillRelease, SubEffect, TerrainSpec},
};

/// `CommanderSkillSystem`: the releases still to land.
pub(in crate::fight) struct CommanderSkillSystem {
    /// Each side's released battle skills, in side order.
    pub(in crate::fight) releases: Vec<SkillRelease>,
}

impl CommanderSkillSystem {
    /// A layout's releases, each side's in the order it releases them.
    pub(in crate::fight) fn new(layout: &CompiledLayout) -> Self {
        let mut releases = layout.battle_skills.clone();
        releases.sort_by_key(|release| release.team);
        Self { releases }
    }
}

/// What tags a battle skill's buff, so that its end takes it away.
const SKILL_SOURCE: &str = "BuffSystem.CommanderSkill";

/// What a `CommanderSkillSubEffectController` does where one of its
/// sub-effects lands, read off its release's `SkillEffect::Strike`.
struct Circle<'a> {
    range_q32: i64,
    damage: i64,
    crosses_shields: bool,
    shield_damage: Option<i64>,
    harmful: bool,
    buff: Option<&'a SkillBuff>,
}

/// `CalculateAttackPositions`' line: the `step`th of `count` points
/// `length / (count - 1)` apart from `from` towards `to`, the way there
/// clamped to its distance, so the last falls a fraction of a millimetre off
/// `to`.
pub(in crate::fight) fn line_point(
    (x_q32, z_q32): (i64, i64),
    (to_x, to_z): (i64, i64),
    count: i64,
    step: i64,
) -> (i64, i64) {
    let (dx, dz) = (to_x.saturating_sub(x_q32), to_z.saturating_sub(z_q32));
    let spacing = q32_div(native_q32_magnitude(dx, dz), (count - 1).max(1) << 32);
    let (ox, oz) = clamp_magnitude_q32_raw(dx, dz, q32_mul(spacing, step << 32));
    (x_q32.saturating_add(ox), z_q32.saturating_add(oz))
}

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
            let (SkillEffect::Strike {
                scatter,
                sub_effects,
                ..
            }
            | SkillEffect::Terrain {
                scatter,
                sub_effects,
                ..
            }) = &self.commander.releases[index].effect
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
                    Scatter::Line { to_q32 } => line_point((x_q32, z_q32), to_q32, count, step),
                };
            }
            if let SkillEffect::Strike {
                sub_effects: placed,
                ..
            }
            | SkillEffect::Terrain {
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
                shield_damage,
                harmful,
                buff,
                sub_effects,
                ..
            } = &release.effect
            {
                // A skill that is not harmful stops at a shield as any other
                // does, which is not measured for one that falls nowhere.
                if !harmful && !self.shield.standing.is_empty() {
                    return Err(Error::new(format!(
                        "{} lands in a fight with a battlefield shield, which is not measured",
                        release.name
                    )));
                }
                let circle = Circle {
                    range_q32: *range_q32,
                    damage: *damage,
                    crosses_shields: *crosses_shields,
                    shield_damage: *shield_damage,
                    harmful: *harmful,
                    buff: buff.as_ref(),
                };
                let falling = self.step_sub_effects(
                    &release,
                    &circle,
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
            // `RangeItemEffectController.PerformEffect`: each sub-effect
            // that lands leaves its terrain there.
            if let SkillEffect::Terrain {
                spec,
                crosses_shields,
                sub_effects,
                ..
            } = &release.effect
            {
                let falling =
                    self.land_terrains(&release, (*spec, *crosses_shields), sub_effects, tick)?;
                if falling.is_empty() {
                    self.commander.releases.remove(index);
                    continue;
                }
                if let SkillEffect::Terrain { sub_effects, .. } =
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
            // A support skill's falling sub-effect stops at the first
            // shield it enters too, which is not measured. A Shield
            // Airdrop's crosses shields whatever its row says
            // (`CS_EnergyShield.CanCrossAdvancedEnergyShield`).
            if !self.shield.standing.is_empty() && matches!(release.effect, SkillEffect::Summon(_))
            {
                return Err(Error::new(format!(
                    "{} lands in a fight with a battlefield shield, which is not measured",
                    release.name
                )));
            }
            match &release.effect {
                // A strike's sub-effects land above, and a path is given out
                // as the fight starts and never lands.
                SkillEffect::Strike { .. }
                | SkillEffect::Terrain { .. }
                | SkillEffect::Path { .. } => {}
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

    /// `CSRS_Perform`'s update of a skill's agents, in the order they were
    /// activated: one that lands on this tick performs where it lands, and
    /// one that cannot cross shields stops at the first it comes inside as it
    /// falls, before it would land, and performs there (`InterruptEffect`).
    /// Answers the ones still to land.
    fn step_sub_effects(
        &mut self,
        release: &SkillRelease,
        circle: &Circle<'_>,
        sub_effects: &[SubEffect],
        (tick, target_search_order): (u64, &BTreeMap<u32, Vec<FightActorRef>>),
        events: &mut Vec<Event>,
    ) -> Result<Vec<SubEffect>> {
        let mut falling = Vec::new();
        for sub_effect in sub_effects {
            let (x_q32, z_q32, fall) = (sub_effect.x_q32, sub_effect.z_q32, sub_effect.fall);
            if !circle.crosses_shields
                && tick <= sub_effect.lands_on
                && let Some(height) = fall.height_on(tick)
            {
                let last = fall.height_on(tick - 1).unwrap_or(fall.start_q32);
                if let Some((_, point)) =
                    self.falling_into_shield((x_q32, height, z_q32), (x_q32, last, z_q32))
                {
                    self.perform_hit_effect(release, circle, point, target_search_order, events)?;
                    continue;
                }
            }
            if sub_effect.lands_on == tick {
                let point = (x_q32, 0, z_q32);
                self.perform_hit_effect(release, circle, point, target_search_order, events)?;
                continue;
            }
            falling.push(*sub_effect);
        }
        Ok(falling)
    }

    /// `CommanderSkillSubEffectController.PerformHitEffect` where a
    /// sub-effect landed or stopped. A skill that is not harmful writes its
    /// buff on its own side's units its circle reaches
    /// (`PerformPositiveEffect`, which asks the calculator of the releasing
    /// side's group alone).
    ///
    /// A harmful one (`PerformNegativeEffect`) first takes the units its
    /// circle reaches, of either side, less each that one of its own side's
    /// shields holds (`FightCalculator.IsActorInEnergyShield`). It then
    /// strikes the circle, when its damage is above nothing or it is a damage
    /// modifier (`IsDamageEffect`), and writes its buff on the units it took
    /// that are still alive, as `BuffSystem.AddBuff` skips the dead.
    fn perform_hit_effect(
        &mut self,
        release: &SkillRelease,
        circle: &Circle<'_>,
        point: (i64, i64, i64),
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let center = (point.0, point.2);
        if !circle.harmful {
            if let Some(buff) = circle.buff {
                let mut reached = self.skill_reach(center, circle.range_q32, target_search_order);
                reached.retain(|id| self.actors[id].placement.team == release.team);
                self.write_release_buff(release, buff, &reached, events)?;
            }
            return Ok(());
        }
        let reached = match circle.buff {
            Some(_) => self
                .skill_reach(center, circle.range_q32, target_search_order)
                .into_iter()
                .filter(|&id| self.shield_around(FightActorRef::Unit(id)).is_none())
                .collect(),
            None => Vec::new(),
        };
        if circle.damage > 0 || circle.shield_damage.is_some() {
            self.strike_circle(release, circle, point, events)?;
        }
        if let Some(buff) = circle.buff {
            self.write_release_buff(release, buff, &reached, events)?;
        }
        Ok(())
    }

    /// `RangeItemEffectController.PerformEffect` for each of a terrain
    /// skill's sub-effects that lands on this tick. One that cannot cross
    /// shields is tested as it falls, as a strike's is, and ends on the first
    /// shield it comes inside: `InterruptEffect`'s `PerformHitEffect` leaves
    /// no terrain and deals the shield nothing. Answers the ones still to
    /// land.
    fn land_terrains(
        &mut self,
        release: &SkillRelease,
        (spec, crosses_shields): (TerrainSpec, bool),
        sub_effects: &[SubEffect],
        tick: u64,
    ) -> Result<Vec<SubEffect>> {
        let mut falling = Vec::new();
        for sub_effect in sub_effects {
            let (x_q32, z_q32, fall) = (sub_effect.x_q32, sub_effect.z_q32, sub_effect.fall);
            if !crosses_shields
                && tick <= sub_effect.lands_on
                && let Some(height) = fall.height_on(tick)
            {
                let last = fall.height_on(tick - 1).unwrap_or(fall.start_q32);
                if self
                    .falling_into_shield((x_q32, height, z_q32), (x_q32, last, z_q32))
                    .is_some()
                {
                    continue;
                }
            }
            if sub_effect.lands_on == tick {
                self.add_terrain(release.team, &release.name, spec, (x_q32, 0, z_q32))?;
                continue;
            }
            falling.push(*sub_effect);
        }
        Ok(falling)
    }

    /// `PerformNegativeEffect`'s hit: `CommanderSkillDamageProvider`'s damage
    /// over the skill's circle where it landed or stopped, on everything of
    /// either side it reaches, with no owner. A shield takes what the skill's
    /// damage modifier adds to it.
    fn strike_circle(
        &mut self,
        release: &SkillRelease,
        circle: &Circle<'_>,
        (x_q32, y_q32, z_q32): (i64, i64, i64),
        events: &mut Vec<Event>,
    ) -> Result<()> {
        // A battle skill's circle reaches units alone.
        let hit = DamageHit {
            crosses_shields: circle.crosses_shields,
            strikes_buildings: false,
            shield_damage: circle
                .shield_damage
                .filter(|&added| added > 0)
                .map(|added| circle.damage + added),
            ..DamageHit::unowned(
                release.team,
                circle.damage,
                (x_q32, z_q32),
                y_q32,
                q32_to_space_rounded(circle.range_q32),
            )
        };
        // `PrepareRangeTargets` asks `CalculateRangeActors` with
        // `includeBuilding` off: a tower is never struck. Whether a
        // construction is, as one of its side's actors, is not measured;
        // a hit that deals a unit nothing takes nothing from it either way.
        if circle.damage > 0
            && let Some(block) = self.construction_in_reach(x_q32, z_q32, circle.range_q32)
        {
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
    /// and what `PerformNegativeEffect` keeps of it: the units of either
    /// side, ground or air, whose edge is within the range of where the skill
    /// landed. The calculator is given no team, and `IsBuffTarget` keeps
    /// units alone.
    fn skill_reach(
        &self,
        center_q32: (i64, i64),
        range_q32: i64,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
    ) -> Vec<u64> {
        self.units_in_range(
            center_q32,
            range_q32,
            (
                crate::rules::AttackTargets {
                    ground: true,
                    air: true,
                },
                true,
            ),
            target_search_order,
        )
    }

    /// A released battle skill's buff on every unit it reached, which no
    /// actor adds.
    fn write_release_buff(
        &mut self,
        release: &SkillRelease,
        buff: &SkillBuff,
        reached: &[u64],
        events: &mut Vec<Event>,
    ) -> Result<()> {
        self.write_skill_buff((&release.name, release.team), None, buff, reached, events)
    }

    /// `BuffSystem.AddBuff` of the skill's row on every unit it reached,
    /// from `source`, the actor that adds it, if one does.
    pub(in crate::fight) fn write_skill_buff(
        &mut self,
        (name, team): (&str, u32),
        source: Option<ObjectRef>,
        buff: &SkillBuff,
        reached: &[u64],
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let row = super::tower::BuffRow {
            clears_when_technologies_disabled: false,
            buff_id: buff.id,
            max_life_rate: 0,
            stacking: None,
            summons: None,
            divide: buff.divide,
            additive: buff.additive,
            ticks: buff.ticks,
            source: SKILL_SOURCE,
            entries: [
                (Index::MoveSpeed, buff.move_speed_rate),
                (Index::AmplifyDamage, buff.amplify_damage_rate),
            ]
            .into_iter()
            .filter(|&(_, rate)| rate != 0)
            .map(|(index, rate)| Entry {
                index,
                source: SKILL_SOURCE,
                correction: super::tower::rate(rate),
            })
            // `attackRangeChangeValue`: whole metres on the main skill's
            // range, which `AttackRangeProperty.GetAttackRange` adds to the
            // skill's own value before its rates.
            .chain((buff.attack_range_value != 0).then(|| Entry {
                index: Index::AttackRange,
                source: SKILL_SOURCE,
                correction: crate::data::Correction::Value(
                    buff.attack_range_value * crate::rules::SPACE_UNITS_PER_METER_SCALE,
                ),
            }))
            .collect(),
            disables_technology: buff.disable_technology,
            debuff: buff.debuff,
            invincible: buff.invincible,
            life_change: (buff.life_change_rate != 0).then_some(super::tower::LifeChange {
                rate: buff.life_change_rate,
                step_ticks: buff.step_ticks,
            }),
            current_life_rate: buff.current_life_rate,
        };
        for &id in reached {
            // `BuffSystem.AddBuff` passes over the dead: a strike's damage
            // may have killed what its circle reached.
            if !self.actors[&id].alive() || !self.buff_reaches(id, &row) {
                continue;
            }
            let actor = &self.actors[&id];
            let unmeasured = &actor.placement.technology_disable.unmeasured;
            if buff.disable_technology && !unmeasured.is_empty() {
                return Err(Error::new(format!(
                    "{name} reaches unit {id}, which carries {}, and switching that off \
                     mid-fight is not measured",
                    unmeasured.join(" and ")
                )));
            }
            if let Some(running) = actor.buff_not_beside(&row) {
                return Err(Error::new(format!(
                    "{name} reaches unit {id}, which runs buff {running}, and a skill's buff \
                     beside it is not measured"
                )));
            }
            self.write_buff(id, source, team, &row, events)?;
        }
        Ok(())
    }
}
