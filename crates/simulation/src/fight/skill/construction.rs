use super::*;

impl Simulation {
    /// `SkillManager.Update` for a construction: the skill's state, moved on
    /// as a unit's is, by an owner whose attack no motion starts.
    ///
    /// - `SkillIdleState`: with no lock, a dead one, or its search timer run
    ///   out, the skill searches, and otherwise counts the timer down; then,
    ///   with a lock in reach and the weapon within the attack angle of it,
    ///   `TryStartAttack` enters the attack. A state is not updated on the
    ///   tick it is entered, so the first shot leaves on the tick after.
    /// - `SkillAttackState`: a lock that died is replaced by the next target,
    ///   the skill switching quickly as its row says; a lock out of reach, or
    ///   none, finishes the attack. A shot is due once its interval is up and
    ///   the weapon faces the target, and its interval is drawn for it from
    ///   the team's stream as a unit's is; with the attack point at zero it
    ///   leaves at once.
    /// - With the magazine empty, the next update of either state enters
    ///   `SkillReloadingState`, which keeps the lock, lasts the reload, and
    ///   hands the skill back to idle, full.
    pub(in crate::fight) fn update_construction_skill(
        &mut self,
        building_id: u64,
        step: u64,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let construction = &self.constructions[&building_id];
        let state = construction.skill.state;
        let empty = construction.attack.magazine.is_some() && construction.rounds == 0;
        let lock = construction.skill.lock_target;
        let lock_alive = lock.is_some_and(|target| self.fight_actor_is_alive(target));
        match state {
            SkillState::Reloading { finish_step } => {
                let construction = self.construction_mut(building_id);
                if step >= finish_step {
                    construction.rounds = construction
                        .attack
                        .magazine
                        .map_or(u32::MAX, |magazine| magazine.capacity);
                    // `SkillReloadingState.Exit` resets the search timer, and
                    // the idle it hands the lock to keeps it: the Rapid-Fire
                    // Turret of `rapid-fire-head-on.yaml` fires its 11th shot
                    // at the Crawler it fired its 10th at, where a search
                    // from its weapon would have taken another.
                    construction.skill.state = SkillState::Idle { ready_step: None };
                    construction.skill.search_target_time = SEARCH_TARGET_RESET_TICKS;
                }
                Ok(())
            }
            _ if empty => {
                let construction = self.construction_mut(building_id);
                let reload_steps = construction.attack.magazine.map_or(0, |magazine| {
                    native_time_units_to_steps(magazine.reload_time_units())
                });
                construction.skill.state = SkillState::Reloading {
                    finish_step: step.saturating_add(reload_steps),
                };
                Ok(())
            }
            SkillState::Attack(_) => {
                // `SkillAttackableChecker.Check`: a lock that died is replaced
                // on the spot, and one that left the attack area is searched
                // again (`CheckWhenLoseTarget`); either search ignores the
                // timer. A skill that ends with nothing in its area finishes
                // the attack, and with no cooling that is idle, lock cleared.
                let target = if lock_alive
                    && lock.is_some_and(|target| {
                        self.construction_reaches_target(building_id, target)
                            && self.construction_faces_target(building_id, target)
                    }) {
                    lock
                } else {
                    self.select_construction_target(building_id, target_search_order)
                };
                let Some(target) =
                    target.filter(|&target| self.construction_reaches_target(building_id, target))
                else {
                    let construction = self.construction_mut(building_id);
                    construction.skill.drop_lock();
                    construction.skill.set_phase(FightSkillPhase::Idle);
                    return Ok(());
                };
                let faces = self.construction_faces_target(building_id, target);
                let construction = self.construction_mut(building_id);
                construction.skill.lock_target = Some(target);
                // `SkillAttackState.Update` counts the timer down and never
                // searches on it.
                construction.skill.search_target_time -= 1;
                if step < construction.skill.next_attack_step || !faces {
                    return Ok(());
                }
                self.construction_shot(building_id, step, target, events)
            }
            SkillState::Idle { .. } | SkillState::Cooling { .. } | SkillState::Prepare { .. } => {
                // `SkillIdleState.CanStartSearchTarget`: no lock, a dead one,
                // or the timer run out.
                let searches = !lock_alive || construction.skill.search_target_time <= 0;
                if searches {
                    let found = self.select_construction_target(building_id, target_search_order);
                    let construction = self.construction_mut(building_id);
                    construction.skill.lock_target = found;
                    construction.skill.search_target_time = SEARCH_TARGET_RESET_TICKS;
                } else {
                    self.construction_mut(building_id).skill.search_target_time -= 1;
                }
                let Some(target) = self.constructions[&building_id].skill.lock_target else {
                    return Ok(());
                };
                if self.construction_reaches_target(building_id, target)
                    && self.construction_faces_target(building_id, target)
                {
                    // `SkillIdleState.Exit` resets the timer.
                    let construction = self.construction_mut(building_id);
                    construction.skill.set_phase(FightSkillPhase::Attack);
                    construction.skill.search_target_time = SEARCH_TARGET_RESET_TICKS;
                }
                Ok(())
            }
        }
    }

    fn construction_mut(&mut self, building_id: u64) -> &mut Construction {
        self.constructions
            .get_mut(&building_id)
            .expect("construction identity is stable")
    }

    /// `SkillAttackState.TryPerformAttack` and the shot it performs.
    fn construction_shot(
        &mut self,
        building_id: u64,
        step: u64,
        target: FightActorRef,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let construction = &self.constructions[&building_id];
        let team = construction.team;
        let interval_steps = native_time_units_to_steps(construction.attack.interval_time_units());
        let offset_steps =
            native_time_units_to_steps(construction.attack.interval_offset_time_units());
        let attack_point_steps =
            native_time_units_to_steps(construction.attack.attack_point_time_units());
        let random = self
            .team_random
            .get_mut(&team)
            .ok_or_else(|| Error::new("construction team random stream is absent"))?;
        let construction = self
            .constructions
            .get_mut(&building_id)
            .expect("construction identity is stable");
        construction.skill.schedule_blow(
            random,
            step,
            interval_steps,
            offset_steps,
            attack_point_steps,
            target,
        );
        construction.rounds = construction.rounds.saturating_sub(1);
        let launch = construction.launch();
        let pending = construction
            .skill
            .pending()
            .expect("a blow was just scheduled");
        if pending.step > step {
            return Err(Error::new(
                "a construction skill with an attack point is not supported",
            ));
        }
        construction.skill.set_pending(None);
        self.launch_at(launch, pending.target, events)
    }

    /// Puts a shot in flight at a target where it stands now.
    pub(in crate::fight) fn launch_at(
        &mut self,
        launch: Launch,
        target: FightActorRef,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let view = self
            .fight_actor(target)
            .ok_or_else(|| Error::new("projectile target is absent"))?;
        let target_y = match target {
            FightActorRef::Unit(id) => unit_height(self.actors[&id].rules.domain),
            FightActorRef::Building(_) => 0,
        };
        self.launch_projectile(
            launch,
            target.kind(),
            target.id(),
            (
                q32_to_space_rounded(view.x_q32),
                target_y,
                q32_to_space_rounded(view.z_q32),
            ),
            (view.x_q32, view.z_q32),
            view.radius,
            0,
            0,
            events,
        )
    }
}
