//! A side arm: an extra skill whose weapons are `WeaponMode.SideArm`
//! (`FightSkill.IsSideArmSkill`), which fires in turn with its unit's main
//! skill and at what stands about the main skill's lock.
//!
//! `FightSkill.EnterFight` hands the side arm to the main skill
//! (`SetSideArmSkill`). The main skill beginning a blow
//! (`SkillAttackController.PerformAttack`, `SetFireTurnsMark`) marks the side
//! arm's turn and the ticks it waits, `sideArmFireDelay` over the tick, which
//! the main skill's `FightSkill.Update` counts down before its state updates.
//! Neither skill begins a blow out of turn (`CanFireByTakeTurns`): the main
//! skill waits for the side arm to have begun one, which gives the turn
//! back, and the side arm for its turn and the wait. A side arm that cannot
//! take its turn gives it back (`TrySideArmResetFireMark`,
//! `ForceSideArmEndFireTurn`).
//!
//! A side arm searches with `SideArmSearchTargetController`, about the main
//! skill's lock, and searches again only when its own lock no longer suits
//! (`NeedRefreshSideArmTarget`), in its idle state and its attack state alike.

use super::*;
use crate::rules::{AttackTargets, WeaponMode};

impl Simulation {
    /// `FightSkill.IsSideArmSkill`.
    pub(in crate::fight) fn is_side_arm(&self, skill_ref: SkillRef) -> bool {
        self.skill_rules(skill_ref).weapons.mode == WeaponMode::SideArm
    }

    /// The side arm `FightSkill.EnterFight` hands the owner's main skill, if
    /// it has one: `HaveSideArmSkill`.
    fn side_arm(&self, owner: FightActorRef) -> Option<SkillRef> {
        let actor_id = owner.unit_id()?;
        self.actors[&actor_id]
            .skills
            .extras
            .iter()
            .position(|extra| extra.rules.attack.weapons.mode == WeaponMode::SideArm)
            .map(|index| SkillRef {
                owner,
                slot: SkillSlot::Extra(index),
            })
    }

    /// `FightSkill.Update` of a main skill with a side arm counts the side
    /// arm's wait down before its state updates.
    pub(in crate::fight) fn count_side_arm_fire_delay(&mut self, owner: FightActorRef) {
        if self.side_arm(owner).is_none() {
            return;
        }
        let main = self.skill_mut(SkillRef::main(owner));
        main.side_arm_fire_delay = main.side_arm_fire_delay.saturating_sub(1);
    }

    /// `FightSkill.CanFireByTakeTurns`: a side arm begins a blow in its turn
    /// once its wait is over; a main skill with a side arm, only while the
    /// turn is not the side arm's; any other skill whenever it may.
    pub(in crate::fight) fn can_fire_by_take_turns(&self, skill_ref: SkillRef) -> bool {
        let main = self.skill(SkillRef::main(skill_ref.owner));
        if self.is_side_arm(skill_ref) {
            return main.side_arm_fires && main.side_arm_fire_delay == 0;
        }
        !(skill_ref.slot == SkillSlot::Main
            && self.side_arm(skill_ref.owner).is_some()
            && main.side_arm_fires)
    }

    /// `FightSkill.SetFireTurnsMark`, as `SkillAttackController.PerformAttack`
    /// begins a blow: the main skill's hands the side arm its turn, to begin
    /// after `sideArmFireDelay` over the tick, the fraction dropped; the side
    /// arm's gives it back.
    pub(in crate::fight) fn set_fire_turns_mark(&mut self, skill_ref: SkillRef) {
        if self.is_side_arm(skill_ref) {
            self.end_side_arm_turn(skill_ref.owner);
            return;
        }
        if skill_ref.slot != SkillSlot::Main {
            return;
        }
        let Some(side_arm) = self.side_arm(skill_ref.owner) else {
            return;
        };
        let delay = self.skill_rules(side_arm).side_arm.map_or(0, |side_arm| {
            native_time_units_to_steps(side_arm.fire_delay_time_units())
        });
        let main = self.skill_mut(skill_ref);
        main.side_arm_fires = true;
        main.side_arm_fire_delay = delay;
    }

    /// `FightSkill.ForceSideArmEndFireTurn`: a side arm gives its turn back.
    pub(in crate::fight) fn force_side_arm_end_fire_turn(&mut self, skill_ref: SkillRef) {
        if self.is_side_arm(skill_ref) {
            self.end_side_arm_turn(skill_ref.owner);
        }
    }

    fn end_side_arm_turn(&mut self, owner: FightActorRef) {
        if self.side_arm(owner).is_none() {
            return;
        }
        let main = self.skill_mut(SkillRef::main(owner));
        main.side_arm_fires = false;
        main.side_arm_fire_delay = 0;
    }

    /// `FightSkill.TrySideArmResetFireMark`, which the side arm's idle and
    /// cooling states ask as they update: a side arm keeps its turn while it
    /// is enabled, neither cooling, reloading nor locked, its lock lives, and
    /// either it still waits or what it fires at is in its attack area (or
    /// it fires at nothing yet while its lock is); otherwise it gives it back.
    pub(in crate::fight) fn try_side_arm_reset_fire_mark(&mut self, skill_ref: SkillRef) {
        if !self.is_side_arm(skill_ref)
            || !self.skill(SkillRef::main(skill_ref.owner)).side_arm_fires
        {
            return;
        }
        let skill = self.skill(skill_ref);
        let lock = skill
            .lock_target
            .filter(|&lock| self.fight_actor_is_alive(lock));
        let able = !skill.disabled
            && !matches!(
                skill.state,
                SkillState::Cooling { .. } | SkillState::Reloading { .. } | SkillState::Locked
            );
        if able && let Some(lock) = lock {
            if self
                .skill(SkillRef::main(skill_ref.owner))
                .side_arm_fire_delay
                > 0
            {
                return;
            }
            if self.target_in_attack_area(skill_ref, lock) {
                let skill = self.skill(skill_ref);
                if skill.shield_target().is_some() {
                    if self.target_in_attack_range(skill_ref, lock) {
                        return;
                    }
                } else {
                    match skill.attack_target() {
                        None => return,
                        Some(target) if self.target_in_attack_area(skill_ref, target) => return,
                        Some(_) => {}
                    }
                }
            }
        }
        self.end_side_arm_turn(skill_ref.owner);
    }

    /// `SkillIdleState.TrySearchLockTarget` of a side arm, and what its idle
    /// state asks after it: it searches when its lock no longer suits, and
    /// then waits ten updates as any skill does, or counts down; asks whether
    /// it keeps its turn; and, not left idle, what it fires at about its lock
    /// (`SearchAttackTarget`).
    pub(in crate::fight) fn update_side_arm_idle_search(
        &mut self,
        skill_ref: SkillRef,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
    ) -> Result<()> {
        if self.skill(skill_ref).phase() != FightSkillPhase::Idle {
            return Ok(());
        }
        let searched = self.need_refresh_side_arm_target(skill_ref);
        if searched {
            self.skill_mut(skill_ref).searched_this_tick = true;
            self.search_skill_lock_target(skill_ref, target_search_order)?;
            self.skill_mut(skill_ref).search_target_time = SEARCH_TARGET_RESET_TICKS;
        } else {
            self.skill_mut(skill_ref).search_target_time -= 1;
        }
        self.try_side_arm_reset_fire_mark(skill_ref);
        let skill = self.skill(skill_ref);
        if searched && !skill.idle && skill.lock_target.is_some() {
            self.search_attack_target(skill_ref);
        }
        Ok(())
    }

    /// `FightSkill.SearchLockTarget` of a main skill with a side arm whose
    /// own lock is gone: it takes the side arm's live lock in place of a
    /// search (`TrySetSideArmTargetAsMainTarget`, inlined there).
    pub(in crate::fight) fn side_arm_lock_for_main(
        &self,
        skill_ref: SkillRef,
    ) -> Option<FightActorRef> {
        if skill_ref.slot != SkillSlot::Main {
            return None;
        }
        let side_arm = self.side_arm(skill_ref.owner)?;
        let lock = self.skill(skill_ref).lock_target;
        if lock.is_some_and(|lock| self.fight_actor_is_alive(lock)) {
            return None;
        }
        self.skill(side_arm)
            .lock_target
            .filter(|&side| self.fight_actor_is_alive(side) && Some(side) != lock)
    }

    /// `FightSkill.NeedRefreshSideArmTarget`: a side arm searches again when
    /// the main skill holds no live lock and it holds one, and when its own
    /// lock is dead or out of its attack range or angle.
    ///
    /// Its lock standing far from the main skill's is no reason: the build
    /// compares their distance with a range it reads through a helper the
    /// decompilation does not resolve (`0x264EE0`), and the game keeps a
    /// living lock in the side arm's area 49 metres from the main skill's,
    /// beyond the 40 of its search range, without asking its selector.
    pub(in crate::fight) fn need_refresh_side_arm_target(&self, skill_ref: SkillRef) -> bool {
        let lock = self.skill(skill_ref).lock_target;
        if !self
            .skill(SkillRef::main(skill_ref.owner))
            .lock_target
            .is_some_and(|anchor| self.fight_actor_is_alive(anchor))
        {
            return lock.is_some();
        }
        let Some(lock) = lock.filter(|&lock| self.fight_actor_is_alive(lock)) else {
            return true;
        };
        !self.target_in_attack_area(skill_ref, lock)
    }

    /// `SideArmSearchTargetController.PerformNormalSkillSearch`: about the
    /// main skill's lock, or the one it held last when it holds none, the
    /// side arm keeps a live lock of its own within its search range of it
    /// and in its attack area; otherwise it takes, of the other sides' units
    /// of the anchor's domain within that range of it
    /// (`RangeTargetCalculator.CalculateRangeTargets`), the anchor aside, those
    /// in its attack area, as its selector scores them; and with none, the
    /// anchor while it lives.
    pub(in crate::fight) fn select_side_arm_target(
        &self,
        skill_ref: SkillRef,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
    ) -> Option<FightActorRef> {
        let main = self.skill(SkillRef::main(skill_ref.owner));
        let anchor = main.lock_target.or(main.prev_lock_for_side_arm)?;
        let anchor_view = self.fight_actor(anchor)?;
        let anchor_alive = self.fight_actor_is_alive(anchor);
        let range = self.side_arm_search_range(skill_ref);
        let in_area = |target: FightActorRef| self.target_in_attack_area(skill_ref, target);
        if let Some(lock) = self.skill(skill_ref).lock_target
            && lock != anchor
            && self.fight_actor_is_alive(lock)
            && range > 0
            && self.within_of(lock, anchor, range)
            && in_area(lock)
        {
            return Some(lock);
        }
        if range <= 0 {
            return anchor_alive.then_some(anchor);
        }
        let source = self.skill_attacker(skill_ref)?;
        let fly = anchor_view.domain == crate::rules::UnitDomain::Air;
        let domains = AttackTargets {
            ground: !fly,
            air: fly,
        };
        let candidates = self
            .units_in_range(
                (anchor_view.x_q32, anchor_view.z_q32),
                space_to_q32(range),
                (domains, true),
                target_search_order,
            )
            .into_iter()
            .map(FightActorRef::Unit)
            .filter(|&candidate| {
                candidate != anchor
                    && self
                        .fight_actor(candidate)
                        .is_some_and(|view| view.team != source.team && view.searchable(false))
                    && in_area(candidate)
            });
        let mut scoring = Scoring::default();
        for candidate in candidates {
            let target = self.fight_actor(candidate)?;
            if let Some(score) = full_rotation_target_score_q32(
                source.query_x_q32,
                source.query_z_q32,
                source.radius,
                source.query_rotation_q32,
                target.x_q32,
                target.z_q32,
                target.radius,
                source
                    .score_offsets
                    .for_candidate(target.domain, target.visible),
                source.min_range,
                source.attack_range,
                source.rotation_window_q32,
            ) {
                scoring.consider(candidate, score, target.visible);
            }
        }
        scoring
            .chosen(|next| self.target_in_attack_range(skill_ref, next))
            .filter(|&chosen| self.fight_actor_is_alive(chosen))
            .or(anchor_alive.then_some(anchor))
    }

    /// The side arm's `sideArmSearchRange`, in space units.
    fn side_arm_search_range(&self, skill_ref: SkillRef) -> i64 {
        self.skill_rules(skill_ref)
            .side_arm
            .map_or(0, |side_arm| side_arm.search_range_units())
    }

    /// `FightTransform.Distance2D` of two actors within a range, by
    /// `FPoint.op_LessThanOrEqual`.
    fn within_of(&self, left: FightActorRef, right: FightActorRef, range: i64) -> bool {
        let (Some(left), Some(right)) = (self.fight_actor(left), self.fight_actor(right)) else {
            return false;
        };
        fpoint_less_or_equal(
            native_q32_magnitude(
                left.x_q32.saturating_sub(right.x_q32),
                left.z_q32.saturating_sub(right.z_q32),
            ),
            space_to_q32(range),
        )
    }
}
