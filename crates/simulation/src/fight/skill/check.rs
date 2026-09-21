use super::*;

/// How far off the line of fire an enemy construction may stand and still take
/// the shot, in space units.
///
/// `docs/rules/constructions.md` carries the measurement: 94 decisions over
/// six unit types leave it in `[10.8, 11.8]` metres, and a Steel Ball of
/// `wall-laser.yaml`, closing on block 3 a few centimetres a tick, narrows it
/// to `[11.447, 11.507)` — it passes the block by at 11.507 for nine ticks and
/// takes it the tick after 11.447. 11.5 is the value inside that. It is not
/// the attacker's: a Fang of radius 2, a Marksman of 8, a Steel Ball of 6 and
/// a Wraith of 11 use the same one.
pub(in crate::fight) const WALL_IN_THE_WAY_WIDTH: i64 = 11_500;

impl Simulation {
    /// Asks again which construction, if any, stands in this actor's line of
    /// fire, and hands it to the weapons.
    ///
    /// The lock is left alone. `FightSkill.SearchAttackTarget` asks
    /// `WallConstructionTargetChecker` wherever a search settles and every
    /// tick the skill is idle, and a block that is no longer in the way stops
    /// being the attack target the next time it is asked. Nothing changes in a
    /// fight that places no enemy construction.
    pub(in crate::fight) fn search_attack_target(&mut self, actor_id: u64) {
        let found = match self.actors[&actor_id].skill.lock_target {
            // A search that already chose a building is not redirected: the
            // measurement is a wall taking the place of a unit.
            Some(target @ FightActorRef::Unit(_)) => self
                .wall_in_the_way(actor_id, target)
                .map(|building| (building, target)),
            _ => None,
        };
        self.actors
            .get_mut(&actor_id)
            .expect("actor identity is stable")
            .skill
            .in_the_way = found;
        self.refresh_group_walls(actor_id);
    }

    /// `SkillAttackableChecker.Check`: whether the skill can go on with its
    /// attack, deciding its lock and what it fires at again on the way.
    ///
    /// A live lock is kept and `SearchAttackTarget` asked again; a dead one
    /// is searched for, and a skill that cannot switch quickly fails if that
    /// changes what it fires at. What it fires at must then be in the attack
    /// area.
    ///
    /// A dead lock is always searched for here: the Marksman whose Crawler
    /// dies is switched onto the next one, and cools when that one is out of
    /// reach. A live lock the weapons still fire at is not yet held to the
    /// attack area here; the quick-switch and stale-target paths of
    /// `step_actor_with_target_order` still answer that, because what
    /// `SearchLockTarget` has to offer a lock that left the area depends on
    /// whether a search was prepared for it, which this mirror does not
    /// carry yet.
    pub(in crate::fight) fn check_attackable(
        &mut self,
        actor_id: u64,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
    ) -> Result<bool> {
        let lock = self.actors[&actor_id].skill.lock_target;
        let before = self.actors[&actor_id].skill.attack_target();
        if lock.is_some_and(|lock| self.fight_actor_is_alive(lock)) {
            self.search_attack_target(actor_id);
        } else {
            if !self.search_lock_target(actor_id, target_search_order)? {
                return Ok(false);
            }
            let actor = &self.actors[&actor_id];
            if !actor.rules.attack.quick_switch_target && actor.skill.attack_target() != before {
                return Ok(false);
            }
        }
        let after = self.actors[&actor_id].skill.attack_target();
        if after == before && lock == self.actors[&actor_id].skill.lock_target {
            return Ok(true);
        }
        let Some(target) = after else {
            return Ok(false);
        };
        Ok(self.target_in_attack_area(actor_id, target))
    }

    /// Whether `SkillAttackState` asks `CheckAttackable` on this update.
    ///
    /// It asks while no attack phase is running between two blows, and
    /// during the wait before a blow; not during the blow or its backswing.
    /// The capture of `wall-block.yaml` reads it: one Crawler's check fails on
    /// the update after its backswing ends, and another's during the wait
    /// before its next blow, each when a nearer block has come into its line.
    pub(in crate::fight) fn between_blows(&self, actor_id: u64, step: u64) -> bool {
        let actor = &self.actors[&actor_id];
        let waiting = actor.skill.pending.is_none()
            && actor.skill.projectile_pending_releases.is_empty()
            && actor
                .skill
                .backswing_finish_step
                .is_none_or(|finish| finish < step);
        let before = actor
            .skill
            .pending
            .is_some_and(|pending| step < pending.step);
        actor.skill.phase == FightSkillPhase::Attack && (waiting || before)
    }

    /// `SkillAttackState.CheckAttackable`: an attack on a building that has
    /// fallen is over; otherwise `FightSkill.CheckAttackable(true)`.
    ///
    /// The building test is the build's own. The state rejects a dead attack
    /// target of the building class before asking the checker, where a dead
    /// unit goes on to the checker and may be switched from: the Marksman of
    /// `crawlers-vs-marksman.yaml` stays attacking onto the next Crawler, and
    /// the one of `wall-line-of-fire.yaml` finishes when its block falls.
    pub(in crate::fight) fn attack_state_check_attackable(
        &mut self,
        actor_id: u64,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
    ) -> Result<bool> {
        if let Some(target @ FightActorRef::Building(_)) =
            self.actors[&actor_id].skill.attack_target()
            && !self.fight_actor_is_alive(target)
        {
            return Ok(false);
        }
        self.check_attackable(actor_id, target_search_order)
    }

    /// `SkillAttackState.Finish`: `StopAttack` drops the lock, the weapons
    /// keeping what they fired at; the skill then cools for its cooling time
    /// and enters `SkillIdleState` with its targets cleared.
    pub(in crate::fight) fn finish_attack(&mut self, actor_id: u64, step: u64) {
        let cooling_steps =
            native_time_units_to_steps(self.actors[&actor_id].rules.attack.cooling_time_units());
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        let fired_at = actor.skill.attack_target();
        // `MotionIdleState.Enter` publishes the stop once; a unit whose
        // motion is idle already keeps the point it stopped at.
        let entered_idle = actor.motion.state != MotionState::Idle;
        actor.motion.state = MotionState::Idle;
        actor.skill.drop_lock();
        actor.skill.laser_attack_count = 0;
        actor.skill.retarget_after_own_direct_kill = false;
        actor.skill.phase = FightSkillPhase::Idle;

        actor.skill.backswing_finish_step = None;
        actor.skill.pending = None;
        if entered_idle {
            actor.motion.next_target_x_q32 = actor.x_q32;
            actor.motion.next_target_z_q32 = actor.z_q32;
        }
        actor.motion.next_speed_q32 = 0;
        actor.motion.next_max_speed_q32 = space_to_q32(actor.stats.move_speed());
        if cooling_steps > 0 {
            actor.skill.cooling_hold = Some(step);
            actor.skill.cooling_candidate = fired_at;
        }
    }

    /// What `SearchLockTarget` answers in the middle of an update.
    ///
    /// The selector reads the positions the tick's query snapshot holds,
    /// unless the lock it replaces died during this very tick, when it reads
    /// where every candidate stands now; and a candidate that has itself
    /// just died is searched past with live positions.
    pub(in crate::fight) fn select_lock_replacement(
        &self,
        actor_id: u64,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
    ) -> Result<Option<FightActorRef>> {
        let died_this_tick = self.actors[&actor_id]
            .skill
            .mechanical_attack_target()
            .and_then(|target| self.fight_actor(target))
            .is_some_and(|target| target.query_alive && !target.alive);
        let selected =
            self.select_normal_target_with_order(actor_id, target_search_order, died_this_tick)?;
        if !died_this_tick
            && selected
                .and_then(|candidate| self.fight_actor(candidate))
                .is_some_and(|target| target.query_alive && !target.alive)
        {
            return self.select_normal_target_with_order(actor_id, target_search_order, true);
        }
        Ok(selected)
    }

    /// `SearchLockTarget` followed by `SearchAttackTarget`, as the checker
    /// runs them; no lock found clears the targets.
    pub(in crate::fight) fn search_lock_target(
        &mut self,
        actor_id: u64,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
    ) -> Result<bool> {
        let selected = self.select_lock_replacement(actor_id, target_search_order)?;
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        let Some(selected) = selected else {
            actor.skill.drop_lock();
            return Ok(false);
        };
        actor.skill.lock_target = Some(selected);
        self.search_attack_target(actor_id);
        Ok(true)
    }

    /// Enters `SkillIdleState` with `needClearTarget`, as a failed check does:
    /// the lock and the attack target are cleared, and the next update
    /// searches.
    pub(in crate::fight) fn enter_idle_clearing_targets(&mut self, actor_id: u64) {
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        actor.motion.state = MotionState::Idle;
        actor.skill.drop_lock();
        actor.skill.phase = FightSkillPhase::Idle;
        actor.skill.search_target_time = 0;
        actor.skill.backswing_finish_step = None;
        actor.skill.pending = None;
        actor.motion.next_target_x_q32 = actor.x_q32;
        actor.motion.next_target_z_q32 = actor.z_q32;
        actor.motion.next_speed_q32 = 0;
        actor.motion.next_max_speed_q32 = space_to_q32(actor.stats.move_speed());
    }

    /// Which enemy construction stands between this actor and its target.
    ///
    /// `docs/rules/constructions.md` states the rule and the readings behind
    /// it: of the enemy's constructions, the ones within reach edge to edge and
    /// within the width of the line of fire, the **nearest to the attacker** —
    /// not the nearest construction and not the one nearest the line.
    pub(in crate::fight) fn wall_in_the_way(
        &self,
        actor_id: u64,
        target: FightActorRef,
    ) -> Option<u64> {
        let actor = self.actors.get(&actor_id)?;
        let aimed = self.fight_actor(target)?;
        // A wall is considered when it is within reach edge to edge: the
        // attacker's range plus its own radius and the block's. A constant
        // allowance fits a Crawler and not a Wraith, which attacks a block 73.8
        // metres off with a reach of 60.
        let reach = space_to_q32(
            actor
                .stats
                .attack_range()
                .saturating_add(actor.rules.collision_radius()),
        );
        let width = space_to_q32(WALL_IN_THE_WAY_WIDTH);
        let mut nearest: Option<(i64, u64)> = None;
        for building in &self.buildings {
            if building.building_type_id != CONSTRUCTION_BUILDING_TYPE
                || building.team_id == actor.placement.team
                || !building_alive(building)
                || !building.targetable
            {
                continue;
            }
            let distance = native_q32_magnitude(
                building.position.x.saturating_sub(actor.x_q32),
                building.position.z.saturating_sub(actor.z_q32),
            );
            if distance > reach.saturating_add(building.bounds_width / 2) {
                continue;
            }
            if distance_to_segment_q32(
                (actor.x_q32, actor.z_q32),
                (aimed.x_q32, aimed.z_q32),
                (building.position.x, building.position.z),
            ) > width
            {
                continue;
            }
            if nearest.is_none_or(|(best, _)| distance < best) {
                nearest = Some((distance, building.building_id));
            }
        }
        nearest.map(|(_, building_id)| building_id)
    }
}
