//! `IterationHitEffectProvider`: a technology whose unit's hits jump on to
//! another enemy.
//!
//! An `IterationHitTech` answers `IIterationHit`, Chain's: within 60 metres,
//! preferring 25, after 0.2 seconds, once, for a quarter of the skill's
//! damage. Its provider, a `SingleEffectProvider`, is a hit effect of the
//! main skill (`IHitEffectPerformer.PerformHitEffect`). A first hit, not a
//! second damage's, records everything it struck as passed over by its
//! skill's jumps (`FightMechIgnoreTargetManager.AddIgnoreBySkillHit`), and,
//! unless a jump of the same skill is already under way
//! (`IterationEffectSystem.IsIteration`), starts one from where the hit
//! landed (`HitEffectControl.Init`, `Perform`): a `GRTimer` of the delay,
//! which strikes nothing yet. When it fires (`OnTimerOver`) it draws an
//! enemy unit at random, from the owner's side's stream, among those within
//! the preferred range of the point that its records do not pass over, or
//! failing any, within the range (`GetTarget`): fully visible, of either
//! domain, its edge within the range. It strikes the one drawn with the
//! skill's damage now times the rate to the jump's number, the `FPoint`
//! product's whole part, as the owner's hit (`DamagePerformer.Perform`,
//! `HitEffectControl` as its provider), which the skill's hit effects then
//! take as a first hit; and jumps again from it until it has jumped its
//! count. With none drawn, or its count done, it ends
//! (`OnIterationEnd`): the skill stops attacking in the records, and once no
//! skill of the unit is, every record goes. A jump at a unit a battlefield
//! shield covers that does not cover the owner is refused, which is not
//! measured. `docs/rules/technology_effects.md` states the rule.

use super::*;

/// One `HitEffectControl` under way: the timer of its next jump.
#[derive(Debug, Clone)]
pub(in crate::fight) struct ChainJump {
    owner: u64,
    /// The skill's index among its owner's skills.
    slot: u16,
    /// `teamControllerCache`: the owner's side as the chain started.
    team: u32,
    /// `triggerPos`, where it jumps from.
    from_q32: (i64, i64),
    /// `index`: the jump's place in the chain, from none.
    index: u32,
    /// `curIterationCount`: the jumps started.
    started: u32,
    /// `GRTimer.CurrentTime` and `Interval`.
    time: u64,
    interval: u64,
}

/// `FightMechIgnoreTargetManager` of a unit: what each skill's hits struck,
/// in the order the skills first struck, and the skills whose chain is
/// under way.
#[derive(Debug, Clone, Default)]
pub(in crate::fight) struct ChainRecords {
    struck: Vec<(u16, Vec<FightActorRef>)>,
    attacking: Vec<u16>,
}

impl ChainRecords {
    /// `AddIgnoreBySkillHit`: a skill's first record makes it one that
    /// attacks.
    fn add(&mut self, slot: u16, target: FightActorRef) {
        if let Some((_, list)) = self.struck.iter_mut().find(|(skill, _)| *skill == slot) {
            list.push(target);
        } else {
            self.struck.push((slot, vec![target]));
            self.attacking.push(slot);
        }
    }

    /// `IsContains`: whether any skill's record holds it.
    fn contains(&self, target: FightActorRef) -> bool {
        self.struck.iter().any(|(_, list)| list.contains(&target))
    }

    /// `ClearIgnoreByHitEffectEnd`: the skill no longer attacks, and once
    /// none does every record goes (`ClearTargetRecord`).
    fn end(&mut self, slot: u16) {
        self.attacking.retain(|&skill| skill != slot);
        if self.attacking.is_empty() {
            self.struck.clear();
        }
    }
}

impl Simulation {
    /// `IterationHitEffectProvider.PerformHitEffect` of a first hit of the
    /// unit's skill at `slot` on `targets`, landed at `point`.
    pub(in crate::fight) fn start_chain(
        &mut self,
        owner: u64,
        slot: u16,
        targets: &[FightActorRef],
        point: (i64, i64),
    ) {
        let Some(actor) = self.actors.get_mut(&owner) else {
            return;
        };
        let Some(source) = actor.placement.effects.single.chain else {
            return;
        };
        if (source.can_disable && actor.technology_disabled())
            || usize::from(slot) >= actor.skills.main_slots()
            || targets.is_empty()
        {
            return;
        }
        for &target in targets {
            actor.chain_records.add(slot, target);
        }
        let team = actor.placement.team;
        if self
            .chains
            .iter()
            .any(|jump| jump.owner == owner && jump.slot == slot)
        {
            return;
        }
        self.chains.push(ChainJump {
            owner,
            slot,
            team,
            from_q32: point,
            index: 0,
            started: 1,
            time: 0,
            interval: seconds_q32_to_steps(source.delay_q32),
        });
    }

    /// The `GRTimerManager` update of every chain's timer, in the order the
    /// chains started: each one due jumps (`OnTimerOver`).
    pub(in crate::fight) fn update_chains(&mut self, events: &mut Vec<Event>) -> Result<()> {
        let mut index = 0;
        while index < self.chains.len() {
            let jump = &mut self.chains[index];
            jump.time += 1;
            if jump.time < jump.interval {
                index += 1;
                continue;
            }
            // The control stays in `IterationEffectSystem` while it strikes:
            // its own hit, which the skill's hit effects take, finds a chain
            // of the skill under way and starts none.
            let jump = jump.clone();
            match self.jump(&jump, events)? {
                Some(next) => {
                    self.chains[index] = next;
                    index += 1;
                }
                None => {
                    self.chains.remove(index);
                }
            }
        }
        Ok(())
    }

    /// `HitEffectControl.OnTimerOver`: the jump drawn and struck, and the
    /// next one started, or the chain ended.
    fn jump(&mut self, jump: &ChainJump, events: &mut Vec<Event>) -> Result<Option<ChainJump>> {
        let source = self.actors[&jump.owner]
            .placement
            .effects
            .single
            .chain
            .expect("a chain under way has its source");
        let preferred_q32 = source.preferred_range_q32.min(source.select_range_q32);
        let Some(target) = self.chain_target(jump, source.select_range_q32, preferred_q32)? else {
            self.end_chain(jump);
            return Ok(None);
        };
        let aimed = FightActorRef::Unit(target);
        if let Some(shield) = self.shield_around(aimed)
            && !self.shield_holds(shield, FightActorRef::Unit(jump.owner))
        {
            return Err(Error::new(format!(
                "unit {}'s chain jumps at unit {target}, which shield {shield} covers, and a \
                 jump into a battlefield shield is not measured",
                jump.owner
            )));
        }
        let owner = &self.actors[&jump.owner];
        let skill = self.skill_at_slot(FightActorRef::Unit(jump.owner), usize::from(jump.slot));
        let damage = self
            .skill_attacker(skill)
            .map_or(0, |attacker| attacker.attack_damage);
        let mut rate_q32 = 1_i64 << 32;
        for _ in 0..=jump.index {
            rate_q32 = q32_mul(rate_q32, source.damage_rate_q32);
        }
        let amount = q32_mul(damage << 32, rate_q32) >> 32;
        let struck_unit = &self.actors[&target];
        let (x_q32, z_q32) = (struck_unit.x_q32, struck_unit.z_q32);
        let hit = DamageHit {
            source: Some(owner.object_ref()),
            source_team: jump.team,
            team: jump.team,
            effect: EffectTarget::Opponent,
            amount,
            provider: Provider::Other,
            projectile: None,
            skill_slot: Some(jump.slot),
            aimed: Some(aimed),
            hits_aimed: true,
            center_q32: (x_q32, z_q32),
            center_y_q32: space_to_q32(unit_height(struck_unit.domain)),
            shield: None,
            crosses_shields: false,
            strikes_buildings: false,
            splash_radius: 0,
            fire: false,
            shield_damage: None,
            reach: Reach::Domain(struck_unit.domain),
        };
        let struck = self.perform_damage(hit, events)?;
        self.record_ends(struck.ends, events);
        if source.count == 0 {
            return Ok(None);
        }
        if jump.started < source.count {
            return Ok(Some(ChainJump {
                from_q32: (x_q32, z_q32),
                index: jump.index + 1,
                started: jump.started + 1,
                time: 0,
                ..jump.clone()
            }));
        }
        self.end_chain(jump);
        Ok(None)
    }

    /// `HitEffectControl.GetTarget`: the enemy units within `range` of the
    /// point, their edge as `RangeTargetCalculator.CalculateRangeTargets`
    /// measures it, fully visible, of either domain, in the order each side's
    /// objects are searched; of them those within `preferred` the owner's
    /// records do not pass over, or failing any those within `range`, and
    /// one of them drawn from the chain's side's stream (`RandomElementSync`).
    fn chain_target(
        &mut self,
        jump: &ChainJump,
        range_q32: i64,
        preferred_q32: i64,
    ) -> Result<Option<u64>> {
        let order = self.target_search_order();
        let records = &self.actors[&jump.owner].chain_records;
        let within = |limit_q32: i64| {
            order
                .iter()
                .filter(|(team, _)| **team != jump.team)
                .flat_map(|(_, candidates)| candidates)
                .filter_map(|&candidate| {
                    let FightActorRef::Unit(id) = candidate else {
                        return None;
                    };
                    let actor = self.actors.get(&id)?;
                    let distance = native_q32_magnitude(
                        actor.x_q32.saturating_sub(jump.from_q32.0),
                        actor.z_q32.saturating_sub(jump.from_q32.1),
                    )
                    .saturating_sub(space_to_q32(actor.rules.collision_radius()));
                    (actor.alive()
                        && actor.placement.team != jump.team
                        && actor.visibility == Visibility::Normal
                        && fpoint_less_or_equal(distance, limit_q32)
                        && !records.contains(candidate))
                    .then_some(id)
                })
                .collect::<Vec<_>>()
        };
        let mut drawn = within(preferred_q32);
        if drawn.is_empty() {
            drawn = within(range_q32);
        }
        if drawn.is_empty() {
            return Ok(None);
        }
        let last = i32::try_from(drawn.len() - 1).map_err(|_| Error::new("too many targets"))?;
        let index = self.side_random(jump.team)?.next_between_inclusive(0, last);
        Ok(Some(
            drawn[usize::try_from(index).expect("a draw within the list is an index")],
        ))
    }

    /// `HitEffectControl.OnIterationEnd`.
    fn end_chain(&mut self, jump: &ChainJump) {
        if let Some(actor) = self.actors.get_mut(&jump.owner) {
            actor.chain_records.end(jump.slot);
        }
    }
}
