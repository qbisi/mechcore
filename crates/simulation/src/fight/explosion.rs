//! An explosion skill's unit's death (`FightExplosionSkill`,
//! `DeadExplosiveController`).
//!
//! `FightExplosionSkill.EnterFight` hands its unit to `DeadEffectSystem` with
//! the skill's data as its dead effect, so the unit explodes however it dies.
//! The skill's own blow is `SuicideEffect`: the unit takes its own life, and
//! `FightActor.lastLifeBeforeSuicide` keeps what it had. When
//! `DeadEffectSystem` updates, `DeadExplosiveController.PerformDeadEffect`
//! strikes everything within the skill's splash of where the unit stood with
//! that life times the skill's multiplier (`DeadExplosiveDamageProvider`,
//! `explosiveDamageCondition` 2), its own side too where the skill enables
//! friendly fire, and leaves a fire there (`RangeItemSystem.AddItem`).

use super::*;

impl Simulation {
    /// The explosion a unit's death sets off, if one of its skills is an
    /// explosion: its extra skill's index.
    pub(in crate::fight) fn explosion_of(&self, actor_id: u64) -> Option<usize> {
        self.actors[&actor_id]
            .skills
            .extras
            .iter()
            .position(|extra| extra.rules.explosion.is_some())
    }

    /// `SuicideEffect.Perform`: the unit's blow takes its whole life, from
    /// itself. Who last hurt it keeps the credit for its death: the blow is
    /// its own. Its agent still moves on this update, as it was moving
    /// (`RVOControllerFixed` updates after the unit), and it dies, explodes
    /// and burns where that leaves it.
    pub(in crate::fight) fn suicide(
        &mut self,
        skill_ref: SkillRef,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let actor_id = skill_ref
            .owner
            .unit_id()
            .ok_or_else(|| Error::new("a construction's suicide is not supported"))?;
        let unit = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        if !unit.alive() {
            return Ok(());
        }
        unit.last_life_before_suicide = unit.life;
        unit.life = 0;
        let position = QVec3 {
            x: unit.x_q32,
            y: space_to_q32(unit_height(unit.rules.domain)),
            z: unit.z_q32,
        };
        self.dead_explosions.push((actor_id, true));
        // `ExpSystem.OnActorHitted` of a hit with no side hands nothing out.
        self.record_deaths(vec![(actor_id, position)], events);
        Ok(())
    }

    /// Whether a unit took its own life on this update, and still moves.
    pub(in crate::fight) fn suicided_this_tick(&self, actor_id: u64) -> bool {
        self.dead_explosions.contains(&(actor_id, true))
    }

    /// `DeadEffectSystem.Update` for the units with an explosion that died
    /// this tick, in the order they died: one its explosion kills explodes in
    /// turn, on the same update.
    pub(in crate::fight) fn step_dead_explosions(&mut self, events: &mut Vec<Event>) -> Result<()> {
        let mut next = 0;
        while next < self.dead_explosions.len() {
            let (actor_id, suicide) = self.dead_explosions[next];
            next += 1;
            self.explode(actor_id, suicide, events)?;
        }
        self.dead_explosions.clear();
        Ok(())
    }

    /// `DeadExplosiveController.PerformDeadEffect` for one unit.
    fn explode(&mut self, actor_id: u64, suicide: bool, events: &mut Vec<Event>) -> Result<()> {
        if suicide {
            // It died where its last move left it.
            let unit = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            let (x_q32, z_q32) = (unit.x_q32, unit.z_q32);
            unit.exit_fight_on_death();
            let object = Some(ObjectRef::new(ObjectKind::Unit, actor_id));
            if let Some(EventPayload::UnitDied { position }) = events
                .iter_mut()
                .find(|event| {
                    event.subject == object
                        && matches!(event.payload, EventPayload::UnitDied { .. })
                })
                .map(|event| &mut event.payload)
            {
                position.x = x_q32;
                position.z = z_q32;
            }
        }
        let Some(index) = self.explosion_of(actor_id) else {
            return Ok(());
        };
        let actor = &self.actors[&actor_id];
        let extra = &actor.skills.extras[index];
        let explosion = extra
            .rules
            .explosion
            .clone()
            .expect("an explosion skill has its explosion");
        // `DeadExplosiveDamageProvider.GetSplashRange`: the unit's radius and
        // the explosion's range.
        let splash_radius = extra
            .rules
            .attack
            .splash_radius()
            .saturating_add(actor.rules.collision_radius());
        let reach = Reach::Targets(extra.rules.attack.targets);
        let dead_fire = extra.dead_fire;
        let team = actor.placement.team;
        let center_y_q32 = space_to_q32(unit_height(actor.rules.domain));
        let (x_q32, z_q32) = (actor.x_q32, actor.z_q32);
        // `explosiveDamageCondition` 2: the life the unit had before it took
        // its own, which a unit any other blow killed never had.
        let life = if suicide {
            actor.last_life_before_suicide
        } else {
            0
        };
        let amount = q32_mul(
            life << 32,
            crate::rules::metres_q32(explosion.damage_multiplier),
        ) >> 32;
        if amount > 0 {
            let hit = DamageHit {
                source: Some(actor.object_ref()),
                source_team: team,
                team,
                effect: if explosion.friendly_fire {
                    EffectTarget::Both
                } else {
                    EffectTarget::Opponent
                },
                amount,
                provider: Provider::Other,
                projectile: None,
                skill_slot: None,
                aimed: None,
                hits_aimed: false,
                center_q32: (x_q32, z_q32),
                center_y_q32,
                shield: None,
                crosses_shields: false,
                strikes_buildings: true,
                splash_radius,
                reach,
            };
            let struck = self.perform_damage(hit, events)?;
            // What it kills and fells comes after what the tick's shots did,
            // as `DeadEffectSystem` takes them after `ProjectileSystem`.
            let mut ends = Vec::new();
            self.record_ends(struck.ends, &mut ends);
            self.fallen_buildings.extend(ends);
        }
        // A fight one side has already lost adds no terrain: the explosion that
        // fells its last unit leaves no fire, nor does that unit's own death.
        if let Some(fire) = dead_fire
            && [0_u32, 1].into_iter().all(|team| self.standing(team))
        {
            self.add_terrain(
                team,
                &format!("unit {actor_id}"),
                fire,
                (x_q32, center_y_q32, z_q32),
            )?;
        }
        Ok(())
    }
}
