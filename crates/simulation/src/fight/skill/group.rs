use super::*;

impl Simulation {
    /// Asks each grouped slot which construction stands between the actor and
    /// the unit that slot was allocated.
    ///
    /// Every slot of a Wraith takes the block its core took, eight ticks later,
    /// when the group allocates its children. With one enemy unit the slots'
    /// lines are the core's, so whether several blocks in reach would be
    /// shared out among the slots is not something any recording has shown;
    /// the build's `CheckWallConstructionForGroupedSkill` keeps a list of walls
    /// already checked, which suggests it might, and nothing here assumes so.
    pub(in crate::fight) fn refresh_group_walls(&mut self, actor_id: u64) {
        let slots = self.actors[&actor_id].skill.group_skill_targets.clone();
        if slots.is_empty() {
            return;
        }
        let found = slots
            .iter()
            .map(|slot| {
                slot.and_then(|unit| {
                    self.wall_in_the_way(actor_id, FightActorRef::Unit(unit))
                        .map(|building| (building, unit))
                })
            })
            .collect::<Vec<_>>();
        self.actors
            .get_mut(&actor_id)
            .expect("actor identity is stable")
            .skill
            .group_in_the_way = found;
    }

    pub(in crate::fight) fn rank_group_unit_targets_with_order(
        &self,
        actor_id: u64,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
        use_live_candidate_positions: bool,
    ) -> Result<Vec<u64>> {
        let source = self
            .actors
            .get(&actor_id)
            .ok_or_else(|| Error::new("target selector source actor is absent"))?;
        let mut scored = Vec::new();
        let mut ordinal = 0_usize;
        for (&team, candidates) in target_search_order {
            if team == source.placement.team {
                continue;
            }
            for &candidate in candidates {
                let FightActorRef::Unit(candidate_id) = candidate else {
                    continue;
                };
                let Some(candidate_actor) = self.actors.get(&candidate_id) else {
                    continue;
                };
                let candidate_alive = if use_live_candidate_positions {
                    candidate_actor.alive()
                } else {
                    candidate_actor.target_query_alive
                };
                if !candidate_alive || !source.rules.attack.accepts(candidate_actor.rules.domain) {
                    continue;
                }
                let (candidate_x_q32, candidate_z_q32) = if use_live_candidate_positions {
                    (candidate_actor.x_q32, candidate_actor.z_q32)
                } else {
                    (
                        candidate_actor.target_query_x_q32,
                        candidate_actor.target_query_z_q32,
                    )
                };
                let Some(score) = normal_visible_full_rotation_target_score_q32(
                    source.target_query_x_q32,
                    source.target_query_z_q32,
                    source.rules.collision_radius(),
                    source.body_rotation_q32,
                    candidate_x_q32,
                    candidate_z_q32,
                    candidate_actor.rules.collision_radius(),
                    source.rules.attack.min_range(),
                    source.stats.attack_range(),
                ) else {
                    continue;
                };
                // PerformGroupedSkillSearch walks OpponentController.GetActors
                // and lets the selector score every alive, type-valid actor.
                // Range is checked later by the individual FightSkill state.
                scored.push((score, ordinal, candidate_id));
                ordinal = ordinal.saturating_add(1);
            }
        }
        scored.sort_by_key(|&(score, order, _)| (score, order));
        Ok(scored
            .into_iter()
            .map(|(_, _, candidate_id)| candidate_id)
            .collect())
    }

    #[allow(clippy::too_many_lines)]
    pub(in crate::fight) fn update_group_skill_targets(
        &mut self,
        actor_id: u64,
        step: u64,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
    ) -> Result<()> {
        let actor = &self.actors[&actor_id];
        if actor.rules.attack.weapons.mode != WeaponMode::Group
            || actor.motion.state != MotionState::Attacking
        {
            return Ok(());
        }
        let needs_initial_targets = actor.skill.group_skill_targets.iter().any(Option::is_none);
        let all_targets_empty = actor.skill.group_skill_targets.iter().all(Option::is_none);
        let core_was_empty = actor
            .skill
            .group_skill_targets
            .first()
            .is_some_and(Option::is_none);
        let has_dead_target = actor
            .skill
            .group_skill_targets
            .iter()
            .flatten()
            .any(|target_id| !self.actors.get(target_id).is_some_and(Actor::alive));
        let core_has_entered_attack = actor.skill.phase() == FightSkillPhase::Attack
            || matches!(
                actor.skill.phase(),
                FightSkillPhase::Prepare { finish_step }
                    if finish_step <= step.saturating_add(1)
            );
        if needs_initial_targets && !core_has_entered_attack {
            return Ok(());
        }
        if !needs_initial_targets && !has_dead_target {
            return Ok(());
        }
        let ranked = self
            .rank_group_unit_targets_with_order(actor_id, target_search_order, has_dead_target)
            .map_err(|error| Error::new(format!("logic step {step} actor {actor_id}: {error}")))?;
        let core_ranked = ranked
            .iter()
            .copied()
            .filter(|target_id| {
                let target = &self.actors[target_id];
                let (target_x_q32, target_z_q32) = if has_dead_target {
                    (target.x_q32, target.z_q32)
                } else {
                    (target.target_query_x_q32, target.target_query_z_q32)
                };
                native_q32_magnitude(
                    target_x_q32.saturating_sub(actor.target_query_x_q32),
                    target_z_q32.saturating_sub(actor.target_query_z_q32),
                )
                .saturating_sub(space_to_q32(actor.rules.collision_radius()))
                .saturating_sub(space_to_q32(target.rules.collision_radius()))
                    <= space_to_q32(actor.stats.attack_range())
            })
            .collect::<Vec<_>>();
        // Slots hold the units they were allocated; a construction in a slot's
        // way is asked for afterwards. So the core is seeded with the unit the
        // mech is locked on, never with the block its weapons are firing at.
        let current_target = actor
            .skill
            .mechanical_lock_target()
            .and_then(FightActorRef::unit_id);
        let prepare_steps = native_time_units_to_steps(actor.rules.attack.prepare_time_units());
        let allow_same_target = actor
            .rules
            .attack
            .weapons
            .allow_same_target
            .unwrap_or(false);
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        if all_targets_empty
            && actor
                .skill
                .group_skill_targets
                .first()
                .is_some_and(Option::is_none)
        {
            actor.skill.group_skill_targets[0] = current_target;
        }
        let previous_targets = actor.skill.group_skill_targets.clone();
        let core_needs_replacement = previous_targets
            .first()
            .copied()
            .flatten()
            .is_none_or(|target_id| !core_ranked.contains(&target_id));
        let mut used = actor
            .skill
            .group_skill_targets
            .iter()
            .flatten()
            .copied()
            .filter(|target_id| ranked.contains(target_id))
            .collect::<std::collections::BTreeSet<_>>();
        let no_unused_global_target = ranked.iter().all(|target_id| used.contains(target_id));
        let mut formal_target_changes = BTreeMap::new();
        let mut redistributed_indices = std::collections::BTreeSet::new();
        if core_was_empty
            && !all_targets_empty
            && let Some(global_replacement) = ranked
                .iter()
                .copied()
                .find(|target_id| !used.contains(target_id))
            && !core_ranked.contains(&global_replacement)
            && let Some(donor_target) = core_ranked.iter().copied().find(|target_id| {
                actor
                    .skill
                    .group_skill_targets
                    .iter()
                    .skip(1)
                    .flatten()
                    .any(|current| current == target_id)
            })
            && let Some(donor_index) = actor
                .skill
                .group_skill_targets
                .iter()
                .position(|target| *target == Some(donor_target))
        {
            actor.skill.group_skill_targets[0] = Some(donor_target);
            actor.skill.group_skill_targets[donor_index] = Some(global_replacement);
            let prepare_ready_step = step.saturating_add(prepare_steps).saturating_add(1);
            actor.skill.group_skill_prepare_ready_steps[0] = prepare_ready_step;
            actor.skill.group_skill_prepare_ready_steps[donor_index] = prepare_ready_step;
            used.insert(global_replacement);
            formal_target_changes.insert(0, Some(donor_target));
            formal_target_changes.insert(donor_index, Some(global_replacement));
            redistributed_indices.insert(0);
            redistributed_indices.insert(donor_index);
        }
        let mut replacement_indices =
            (0..actor.skill.group_skill_targets.len()).collect::<Vec<_>>();
        if !all_targets_empty && !core_needs_replacement {
            replacement_indices.sort_by_key(|&index| {
                let target_is_invalid =
                    previous_targets[index].is_none_or(|target_id| !ranked.contains(&target_id));
                let follows_intervening_attack = target_is_invalid
                    && actor.skill.group_skill_next_attack_steps[index] == step.saturating_add(1)
                    && (1..index).any(|earlier_index| {
                        let earlier_is_invalid = previous_targets[earlier_index]
                            .is_none_or(|target_id| !ranked.contains(&target_id));
                        earlier_is_invalid
                            && ((earlier_index + 1)..index).any(|middle_index| {
                                previous_targets[middle_index]
                                    .is_some_and(|target_id| ranked.contains(&target_id))
                                    && actor.skill.group_skill_next_attack_steps[middle_index] > 0
                                    && actor.skill.group_skill_next_attack_steps[middle_index]
                                        <= step
                            })
                    });
                (!follows_intervening_attack, index)
            });
        }
        for index in replacement_indices {
            if redistributed_indices.contains(&index) {
                continue;
            }
            let candidates = if index == 0 && !core_was_empty {
                &core_ranked
            } else {
                &ranked
            };
            let retained = actor.skill.group_skill_targets[index]
                .filter(|target_id| candidates.contains(target_id));
            if retained.is_some() {
                continue;
            }
            let replacement = candidates
                .iter()
                .copied()
                .find(|target_id| !used.contains(target_id))
                .or_else(|| {
                    (allow_same_target && (index != 0 || no_unused_global_target))
                        .then(|| candidates.first().copied())
                        .flatten()
                });
            actor.skill.group_skill_targets[index] = replacement;
            formal_target_changes.insert(index, replacement);
            if let Some(target_id) = replacement {
                used.insert(target_id);
                if index != 0 && previous_targets[index].is_none() {
                    actor.skill.group_pending_releases.push((
                        index,
                        PendingRelease {
                            step: step.saturating_add(prepare_steps).saturating_add(1),
                            target: FightActorRef::Unit(target_id),
                        },
                    ));
                }
            }
        }
        if let Some((_, target_id)) = formal_target_changes.last_key_value() {
            actor.skill.lock_target = target_id.map(FightActorRef::Unit);
        }
        Ok(())
    }

    pub(in crate::fight) fn refresh_group_skill_attack_interval(
        &mut self,
        actor_id: u64,
        skill_index: usize,
        step: u64,
    ) -> Result<()> {
        // What a recording reads as the unit's current interval is its core
        // skill's, so a child slot's draw does not replace it.
        let core_interval = self.actors[&actor_id].skill.current_attack_interval;
        let sampled_step = self.sample_actor_attack_interval(actor_id, step)?;
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("group skill owner identity is stable");
        if skill_index != 0 {
            actor.skill.current_attack_interval = core_interval;
        }
        let next_attack_step = actor
            .skill
            .group_skill_next_attack_steps
            .get_mut(skill_index)
            .ok_or_else(|| Error::new("group skill index is absent"))?;
        *next_attack_step = sampled_step;
        Ok(())
    }
}
