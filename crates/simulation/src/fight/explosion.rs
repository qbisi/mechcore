//! An explosion skill's unit's death (`FightExplosionSkill`,
//! `DeadExplosiveController`), and a dead-explosion technology's
//! (`DeadExplosiveTech`).
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
//!
//! A `DeadExplosiveTech` is an `IDeadExplosive` its `DeadEffectProvider`
//! hands the same controller, so its unit's death strikes as an explosion
//! skill's does, with the technology's numbers: Final Blitz strikes with the
//! unit's maximum life (`explosiveDamageCondition` 1) within 48 metres of its
//! edge, its own side too, and leaves no fire.

use super::*;

/// What sets off a unit's death: one of its skills, an explosion, by its
/// slot, or a technology.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::fight) enum DeadBlast {
    Skill(SkillSlot),
    Technology,
}

impl Simulation {
    /// The explosion a unit's death sets off, if one of its skills is an
    /// explosion or a technology makes its death one.
    pub(in crate::fight) fn explosion_of(&self, actor_id: u64) -> Option<DeadBlast> {
        let actor = &self.actors[&actor_id];
        if actor.rules.explosion.is_some() {
            return Some(DeadBlast::Skill(SkillSlot::Main));
        }
        actor
            .skills
            .extras
            .iter()
            .position(|extra| extra.rules.explosion.is_some())
            .map(|index| DeadBlast::Skill(SkillSlot::Extra(index)))
            .or_else(|| {
                actor
                    .placement
                    .effects
                    .single
                    .dead_explosion
                    .is_some()
                    .then_some(DeadBlast::Technology)
            })
    }

    /// Whether the unit's death queues its explosion:
    /// `DeadEffectController.IsAvaliable` as the unit dies. An invincible
    /// unit's dead effect and that of a unit whose technologies are on go
    /// off, and a unit whose technologies are disabled has none that is a
    /// technology's (`IDeadEffect.IsTechnologyEffect`), as an extra skill's
    /// explosion is: a Fire Badger with Scorching Charge that dies switched
    /// off neither explodes nor burns, nor does a Rhino with Final Blitz.
    /// Whether a main skill's explosion is a technology's is not read; it is
    /// taken to go off.
    pub(in crate::fight) fn explodes_on_death(&self, actor_id: u64) -> bool {
        let actor = &self.actors[&actor_id];
        self.explosion_of(actor_id).is_some_and(|blast| {
            actor.invincible()
                || !actor.technology_disabled()
                || blast == DeadBlast::Skill(SkillSlot::Main)
        })
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
        let turned = unit.placement.team != unit.original_team;
        let position = QVec3 {
            x: unit.x_q32,
            y: space_to_q32(unit_height(unit.domain)),
            z: unit.z_q32,
        };
        self.dead_explosions.push((actor_id, true));
        // `DeadEffectSystem` calls a turned unit's `OnDead` however it died:
        // `TeamTranslationSystem.OnMechDead` takes it back to its side.
        if turned {
            self.turned_fallen.push(actor_id);
        }
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
        let Some(blast) = self.explosion_of(actor_id) else {
            return Ok(());
        };
        let actor = &self.actors[&actor_id];
        let Blast {
            damage,
            multiplier_q32,
            friendly_fire,
            splash_radius,
            skill_ref,
            dead_fire,
        } = blast_numbers(actor, actor_id, blast);
        // `DeadExplosiveDamageProvider.GetSplashRange`: the unit's radius and
        // the explosion's range.
        let splash_radius = splash_radius.saturating_add(actor.rules.collision_radius());
        let reach = Reach::Targets(
            self.skill_attacker(skill_ref)
                .map_or(actor.rules.attack.targets, |attacker| attacker.targets),
        );
        // `DeadExplosiveDamageProvider.GetTeamController`: the side the unit
        // was deployed on, a beam having turned it or not.
        let team = actor.original_team;
        let center_y_q32 = space_to_q32(unit_height(actor.domain));
        let (x_q32, z_q32) = (actor.x_q32, actor.z_q32);
        // `explosiveDamageCondition` 2: the life the unit had before it took
        // its own, which a unit any other blow killed never had; 1: its
        // maximum life as it died (`lifeGauge`'s maximum); 0: the skill's
        // attack damage, a Spider Mine's 2500 at level one.
        let base = match damage {
            crate::rules::ExplosionDamage::CurrentLife if suicide => actor.last_life_before_suicide,
            crate::rules::ExplosionDamage::CurrentLife => 0,
            crate::rules::ExplosionDamage::MaxLife => actor.stats.max_life(),
            crate::rules::ExplosionDamage::Attack => self
                .skill_attacker(skill_ref)
                .map_or(0, |attacker| attacker.attack_damage),
        };
        let amount = q32_mul(base << 32, multiplier_q32) >> 32;
        if amount > 0 {
            let hit = DamageHit {
                source: Some(actor.object_ref()),
                source_team: team,
                team,
                effect: if friendly_fire {
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
                fire: false,
                shield_damage: None,
                reach,
            };
            let struck = self.perform_damage(hit, events)?;
            // What it kills and fells comes after what the tick's shots did,
            // as `DeadEffectSystem` takes them after `ProjectileSystem`.
            let mut ends = Vec::new();
            self.record_ends(struck.ends, &mut ends);
            self.fallen_buildings.extend(ends);
        }
        // `RangeItemSystem.AddItem` under the unit's own side as it died,
        // `currentTeamController`: a turned unit's fire is its new side's.
        if let Some(fire) = dead_fire {
            let side = self.actors[&actor_id].placement.team;
            self.add_terrain(
                side,
                &format!("unit {actor_id}"),
                fire,
                (x_q32, center_y_q32, z_q32),
            )?;
        }
        Ok(())
    }
}

/// An explosion skill's explosion, its attack and the fire its unit's death
/// leaves: a main skill's leaves none.
fn explosion_skill(
    actor: &Actor,
    slot: SkillSlot,
) -> (
    &crate::rules::ExplosionConfig,
    &crate::rules::AttackConfig,
    Option<crate::layout::TerrainSpec>,
) {
    let (explosion, attack, dead_fire) = match slot {
        SkillSlot::Main => (actor.rules.explosion.as_ref(), &actor.rules.attack, None),
        SkillSlot::Extra(index) => {
            let extra = &actor.skills.extras[index];
            (
                extra.rules.explosion.as_ref(),
                &extra.rules.attack,
                extra.dead_fire,
            )
        }
    };
    (
        explosion.expect("an explosion skill has its explosion"),
        attack,
        dead_fire,
    )
}

/// What a unit's blast strikes with.
struct Blast {
    damage: crate::rules::ExplosionDamage,
    multiplier_q32: i64,
    friendly_fire: bool,
    /// Metres beyond the unit's radius, in space units.
    splash_radius: i64,
    /// The skill whose attacker answers its targets and its attack damage.
    skill_ref: SkillRef,
    dead_fire: Option<crate::layout::TerrainSpec>,
}

/// The numbers of an explosion skill's blast or a technology's.
fn blast_numbers(actor: &Actor, actor_id: u64, blast: DeadBlast) -> Blast {
    let owner = FightActorRef::Unit(actor_id);
    match blast {
        DeadBlast::Skill(slot) => {
            let (explosion, attack, dead_fire) = explosion_skill(actor, slot);
            Blast {
                damage: explosion.damage,
                multiplier_q32: crate::rules::metres_q32(explosion.damage_multiplier),
                friendly_fire: explosion.friendly_fire,
                splash_radius: attack.splash_radius(),
                skill_ref: SkillRef { owner, slot },
                dead_fire,
            }
        }
        // `DeadExplosiveTech.GetRange` and `GetDamageMultiplier` by the
        // unit's level, and `DeadExplosiveDamageProvider.GetTargetType` the
        // unit's own (`FightMech.GetAttackTargetType`).
        DeadBlast::Technology => {
            let source = actor
                .placement
                .effects
                .single
                .dead_explosion
                .as_ref()
                .expect("a technology's blast has its source");
            let level = actor.placement.level;
            Blast {
                damage: source.damage,
                multiplier_q32: super::burrow::level_value(&source.multiplier, level),
                friendly_fire: source.hits_allies,
                splash_radius: q32_to_space_rounded(super::burrow::level_value(
                    &source.range,
                    level,
                )),
                skill_ref: SkillRef {
                    owner,
                    slot: SkillSlot::Main,
                },
                dead_fire: None,
            }
        }
    }
}
