//! A preemptive skill about its unit (`FightAroundSkill`).
//!
//! An around skill starts only through `AroundSkillStartAttackChecker`: the
//! preemptive checker first (`PreemptiveSkillStartAttackChecker`), which
//! asks for no preemptive skill running, the main skill at rest and the
//! skill's own target in its attack area, and then for enough enemies within
//! its select radius. Leaving its idle state it takes the main skill's place
//! (`PreemptiveSkillExitIdleBehaviour`): the main skill locks and the skill
//! searches, taking the motion. It strikes once, about its unit
//! (`SkillDamageProvider.CalculateDamagePosition` of a self splash), and its
//! attack check fails once it has (`SkillAttackState.CheckAttackable`), so it
//! returns to its idle state and hands the main skill back
//! (`PreemptiveSkillEnterIdleBehaviour`).

use super::*;

impl Simulation {
    /// The skill's `SkillStartAttackChecker.Check`: an around skill's
    /// `AroundSkillStartAttackChecker`, a support skill's
    /// `SupportSkillStartAttackChecker`, every other skill's none.
    pub(in crate::fight) fn may_start_attack(&mut self, skill_ref: SkillRef) -> bool {
        match (self.skill(skill_ref).kind, skill_ref.owner) {
            (SkillKind::Around, _) => self.around_may_start(skill_ref),
            _ if self.rocket_punch(skill_ref).is_some() => self.rocket_punch_may_start(skill_ref),
            (SkillKind::Support, FightActorRef::Unit(actor_id)) => self.support_gate(actor_id),
            (SkillKind::Support, FightActorRef::Building(_)) => false,
            _ => true,
        }
    }

    /// `FightSkill.IsPreemptive`: an around, a support or a rocket punch
    /// skill, or a permanent preemptive one.
    pub(in crate::fight) fn skill_is_preemptive(&self, skill_ref: SkillRef) -> bool {
        let SkillSlot::Extra(index) = skill_ref.slot else {
            return false;
        };
        let extra = &self.skills(skill_ref.owner).extras[index];
        matches!(extra.skill.kind, SkillKind::Around | SkillKind::Support)
            || extra.rules.preemptive.is_some()
            || extra.rules.rocket_punch.is_some()
    }

    /// Whether a preemptive skill that is not permanent takes the main
    /// skill's place as it leaves its idle state, and hands it back as it
    /// returns (`PreemptiveSkillExitIdleBehaviour`,
    /// `PreemptiveSkillEnterIdleBehaviour`).
    pub(in crate::fight) fn takes_main_place(&self, skill_ref: SkillRef) -> bool {
        matches!(
            self.skill(skill_ref).kind,
            SkillKind::Around | SkillKind::Support
        ) || self.rocket_punch(skill_ref).is_some()
    }

    /// A skill's rocket punch, if it is one.
    fn rocket_punch(&self, skill_ref: SkillRef) -> Option<&crate::rules::RocketPunch> {
        let SkillSlot::Extra(index) = skill_ref.slot else {
            return None;
        };
        self.skills(skill_ref.owner).extras[index]
            .rules
            .rocket_punch
            .as_ref()
    }

    /// `PreemptiveSkillStartAttackChecker.Check`: no preemptive skill
    /// running, since only a permanent one may start while one is set, the
    /// main skill at rest, and the skill's own target in its attack area.
    fn preemptive_may_start(&self, skill_ref: SkillRef) -> bool {
        let FightActorRef::Unit(actor_id) = skill_ref.owner else {
            return false;
        };
        let actor = &self.actors[&actor_id];
        if actor.skills.running_preemptive.is_some() || actor.skills.preemptive_active {
            return false;
        }
        self.main_skill_at_rest(actor_id)
            && self
                .skill(skill_ref)
                .attack_target()
                .is_some_and(|target| self.target_in_attack_range(skill_ref, target))
    }

    /// `RocketPunchAttackChecker.Check`: the preemptive checker's, a punch
    /// left of `triggerCount`, and the unit's life over its maximum, as
    /// `FPoint`s, at or below the condition for the punch it is at, the
    /// first condition for the first and the second for the second.
    fn rocket_punch_may_start(&self, skill_ref: SkillRef) -> bool {
        let (FightActorRef::Unit(actor_id), SkillSlot::Extra(index)) =
            (skill_ref.owner, skill_ref.slot)
        else {
            return false;
        };
        if !self.preemptive_may_start(skill_ref) {
            return false;
        }
        let actor = &self.actors[&actor_id];
        let extra = &actor.skills.extras[index];
        let Some(punch) = &extra.rules.rocket_punch else {
            return false;
        };
        if extra.punches >= punch.trigger_count {
            return false;
        }
        let max_life = actor.stats.max_life();
        if max_life <= 0 {
            return false;
        }
        let Ok(share_q32) = i64::try_from((i128::from(actor.life) << 32) / i128::from(max_life))
        else {
            return false;
        };
        let condition_q32 = usize::try_from(extra.punches)
            .ok()
            .and_then(|punch_index| punch.life_conditions.get(punch_index))
            .map_or(0, |&condition| crate::rules::readable_q32(condition));
        fpoint_less_or_equal(share_q32, condition_q32)
    }

    /// `AroundSkillStartAttackChecker.Check`.
    fn around_may_start(&self, skill_ref: SkillRef) -> bool {
        let FightActorRef::Unit(actor_id) = skill_ref.owner else {
            return false;
        };
        if !self.preemptive_may_start(skill_ref) {
            return false;
        }
        let actor = &self.actors[&actor_id];
        let Some(attacker) = self.skill_attacker(skill_ref) else {
            return false;
        };
        let AttackPath::Around {
            select_radius,
            target_count,
        } = attacker.attack.path
        else {
            return false;
        };
        // The enemy units that `FightTeam`'s unit quadtree answers for a square
        // of the select radius about the unit (`RectRange`), in its order:
        // each alive and of a type the skill takes, whose edge is within the
        // radius, `FightUtility.CalculateDistance3D` less its radius, counts,
        // up to the condition.
        let radius_q32 = crate::rules::metres_q32(select_radius);
        let own_y = space_to_q32(unit_height(actor.domain));
        let found = self
            .mech_quadtrees
            .iter()
            .filter(|(team, _)| **team != actor.placement.team)
            .flat_map(|(_, tree)| tree.query_square(actor.x_q32, actor.z_q32, radius_q32))
            .collect::<Vec<_>>();
        let count = found
            .into_iter()
            .filter_map(FightActorRef::unit_id)
            .filter_map(|id| self.actors.get(&id))
            .filter(|enemy| {
                enemy.alive()
                    && enemy.visibility == Visibility::Normal
                    && attacker.targets.accepts(enemy.domain)
                    && native_q32_magnitude_3d(
                        enemy.x_q32.saturating_sub(actor.x_q32),
                        space_to_q32(unit_height(enemy.domain)).saturating_sub(own_y),
                        enemy.z_q32.saturating_sub(actor.z_q32),
                    )
                    .saturating_sub(space_to_q32(enemy.rules.collision_radius()))
                        <= radius_q32
            })
            .take(usize::try_from(target_count).unwrap_or(usize::MAX))
            .count();
        u32::try_from(count).is_ok_and(|count| count >= target_count)
    }

    /// `PreemptiveSkillExitIdleBehaviour.Execute`: the skill is set as the
    /// unit's preemptive skill, the main skill locks
    /// (`ChangeToLockState`), and the skill searches its lock
    /// (`SearchLockTarget`), taking the motion as the main skill holds none.
    pub(in crate::fight) fn preemptive_leaves_idle(
        &mut self,
        skill_ref: SkillRef,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
    ) -> Result<()> {
        let (FightActorRef::Unit(actor_id), SkillSlot::Extra(index)) =
            (skill_ref.owner, skill_ref.slot)
        else {
            return Ok(());
        };
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        actor.skills.running_preemptive = Some(index);
        super::preemptive::lock(&mut actor.skills.main);
        self.search_skill_lock_target(skill_ref, target_search_order)?;
        Ok(())
    }

    /// `PreemptiveSkillEnterIdleBehaviour.Execute`: the unit's preemptive
    /// skill is removed and the main skill returns to its idle state
    /// (`ChangeToIdleState`), `SkillLockState.Exit` restarting its search
    /// timer.
    pub(in crate::fight) fn preemptive_enters_idle(&mut self, actor_id: u64) {
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        actor.skills.running_preemptive = None;
        super::preemptive::unlock(&mut actor.skills.main);
    }

    /// An around skill's blow: `DamageEffect` from its unit's own position,
    /// `SkillDamageProvider.CalculateDamagePosition` of a self splash, at
    /// the skill's splash, dealing the skill's damage, of the skill that
    /// struck.
    pub(in crate::fight) fn around_effect(
        &mut self,
        skill_ref: SkillRef,
        target: FightActorRef,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let FightActorRef::Unit(actor_id) = skill_ref.owner else {
            return Err(Error::new("a construction's around skill is not supported"));
        };
        let attacker = self
            .skill_attacker(skill_ref)
            .ok_or_else(|| Error::new("around skill owner is absent"))?;
        if !attacker.attack.self_splash || attacker.splash_radius <= 0 {
            return Err(Error::new(
                "an around skill that splashes about its target is not measured",
            ));
        }
        let (amount, splash_radius, crosses_shields) = (
            attacker.attack_damage,
            attacker.splash_radius,
            attacker.attack.crosses_shields,
        );
        let skill_slot = u16::try_from(self.skill_slot(skill_ref))
            .map_err(|_| Error::new("a skill slot is outside u16"))?;
        let actor = &self.actors[&actor_id];
        let hit = DamageHit {
            center_q32: (actor.x_q32, actor.z_q32),
            center_y_q32: space_to_q32(unit_height(actor.domain)),
            splash_radius,
            crosses_shields,
            ..DamageHit::of_skill(actor, skill_slot, (target, self.domain_of(target)), amount)
        };
        let struck = self.perform_damage(hit, events)?;
        self.record_ends(struck.ends, events);
        Ok(())
    }
}

/// `PreemptiveSkillStartAttackChecker.IsMainSkillIdleState` for a skill that
/// is not permanent: the main skill idle, or attacking between two blows
/// after its first (`performCount` above zero and no `currentController`).
///
/// A backswing that ends on this update has ended for the build: the Rhino of
/// `whirlwind-rhinos.yaml` whose first backswing runs out as its Whirlwind
/// checks starts the Whirlwind then. An idle main skill whose target is in
/// its attack area has started its attack in its own update
/// (`SkillIdleState.TryPerform`), before the Whirlwind checks: the same Rhino,
/// handed back its main skill, does not start its Whirlwind again as the main
/// skill takes its target.
impl Simulation {
    pub(in crate::fight) fn main_skill_at_rest(&self, actor_id: u64) -> bool {
        let main = &self.actors[&actor_id].skills.main;
        match main.state {
            SkillState::Idle { .. } => true,
            SkillState::Attack(Blow::Waiting) => main.performed(),
            SkillState::Attack(Blow::After { finish_step }) => finish_step <= self.step_now,
            _ => false,
        }
    }
}
