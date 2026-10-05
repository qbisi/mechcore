use super::*;
use crate::{data::Index, modifier::SecondaryDamage};

/// One hit, as the fight's damage pipeline reads it.
///
/// This is the build's `IDamageProvider` reduced to what this simulator uses:
/// who dealt it, how much, what it was aimed at, where its splash is measured
/// from and how far it reaches, and which domains it can touch. A direct
/// strike, a projectile arriving and a laser are all described as one, and
/// [`Simulation::damage_targets`] and [`Simulation::strike`] resolve every one
/// of them, which is the build's arrangement: `DamagePerformer` resolves the
/// damage of any provider — skills, projectiles, commander skills, mines,
/// explosions — against `FightActor`s, and a unit and a building are both.
#[derive(Debug, Clone, Copy)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "a hit's flags are its provider's, independent of each other"
)]
pub(in crate::fight) struct DamageHit {
    /// Its owner, `IDamageProvider.GetOwner`: none for a hit no object
    /// dealt, which is recorded under its team alone.
    pub(in crate::fight) source: Option<ObjectRef>,
    /// The team the hit is recorded under.
    pub(in crate::fight) source_team: u32,
    /// The team whose enemies it strikes: the attacker's own at the moment of
    /// impact, which a projectile reads from its owner rather than from the
    /// team it was released under.
    pub(in crate::fight) team: u32,
    /// Whose objects it strikes, `IDamageProvider.GetEffectTargetType`.
    pub(in crate::fight) effect: EffectTarget,
    pub(in crate::fight) amount: i64,
    /// The class of `IDamageProvider` that deals it, where
    /// `PerformHitTargetEffect` tests it.
    pub(in crate::fight) provider: Provider,
    /// The projectile that carried the hit, if one did.
    pub(in crate::fight) projectile: Option<ObjectRef>,
    /// The index of the skill that dealt it in its owner's skills.
    pub(in crate::fight) skill_slot: Option<u16>,
    /// What the attack was aimed at, `IDamageProvider.GetMainTarget`: none
    /// for a hit that only splashes.
    pub(in crate::fight) aimed: Option<FightActorRef>,
    /// Whether the aimed-at object is struck wherever it stands, rather than
    /// only if the splash reaches it. A direct strike always is; a projectile
    /// is when it locks its target.
    pub(in crate::fight) hits_aimed: bool,
    /// Where the splash is measured from, `FPoint` raw metres.
    pub(in crate::fight) center_q32: (i64, i64),
    /// How high that point is, `FPoint` raw metres: a shield holds the point
    /// or not in three dimensions.
    pub(in crate::fight) center_y_q32: i64,
    /// The shield the hit is for: a projectile that a shield took, or a blow
    /// at a unit its side's shield covers. With no splash the shield takes
    /// the whole of it; with one it is the splash's main shield.
    pub(in crate::fight) shield: Option<u64>,
    /// Whether it passes battlefield shields: the skill's
    /// `canCrossAdvancedShield`, or a performer that
    /// `IsInterceptByAdvancedEnergyShield` is false for.
    pub(in crate::fight) crosses_shields: bool,
    /// Whether its splash strikes buildings as well as units: a unit's and
    /// a missile's does, and a battle skill's reaches units alone.
    pub(in crate::fight) strikes_buildings: bool,
    pub(in crate::fight) splash_radius: i64,
    /// Its `IDamageProvider.GetDamageType` is `EDamageType.Fire`: it sets
    /// alight the oil its splash reaches.
    pub(in crate::fight) fire: bool,
    pub(in crate::fight) reach: Reach,
}

impl DamageHit {
    /// A skill's own hit, `SkillDamageProvider`'s: the unit that fired it owns
    /// it, it strikes the other side, and what it was aimed at is struck
    /// wherever that stands. It splashes and crosses shields as the unit's
    /// skill does, from where the unit stands, with no shield of its own.
    ///
    /// It touches only the domain of what it was aimed at, `aimed_domain`:
    /// `GetTargetType` of a skill that does not diffuse, and the simulator
    /// fights no skill that does, is whether the skill's attack target, or
    /// its lock, flies. A Melting Point's beam at a Phoenix splashes no
    /// Tarantula standing under it.
    pub(in crate::fight) fn of_skill(
        attacker: &Actor,
        skill_slot: u16,
        (aimed, aimed_domain): (FightActorRef, UnitDomain),
        amount: i64,
    ) -> Self {
        Self {
            source: Some(attacker.object_ref()),
            source_team: attacker.placement.team,
            team: attacker.placement.team,
            effect: EffectTarget::Opponent,
            amount,
            provider: Provider::Other,
            projectile: None,
            skill_slot: Some(skill_slot),
            aimed: Some(aimed),
            hits_aimed: true,
            center_q32: (attacker.x_q32, attacker.z_q32),
            center_y_q32: 0,
            shield: None,
            crosses_shields: attacker.rules.attack.crosses_shields,
            strikes_buildings: true,
            splash_radius: attacker.stats.splash_radius(),
            fire: usize::from(skill_slot) < attacker.skills.main_slots()
                && attacker.rules.attack.fire_damage,
            reach: Reach::Domain(aimed_domain),
        }
    }

    /// A projectile's hit where it stands, `FightProjectile`'s: the
    /// projectile owns it under the team it flies for, it strikes the other
    /// side, and what it was aimed at is struck wherever that stands only if
    /// the projectile locks it. It neither splashes nor crosses shields.
    pub(in crate::fight) fn of_projectile(
        projectile: &Projectile,
        aimed: FightActorRef,
        amount: i64,
        reach: Reach,
    ) -> Self {
        Self {
            source: Some(projectile.object_ref()),
            source_team: projectile.team,
            team: projectile.team,
            effect: EffectTarget::Opponent,
            amount,
            provider: Provider::Other,
            projectile: Some(projectile.object_ref()),
            skill_slot: None,
            aimed: Some(aimed),
            hits_aimed: projectile.lock_target,
            center_q32: (projectile.x_q32, projectile.z_q32),
            center_y_q32: projectile.y_q32,
            shield: None,
            crosses_shields: false,
            strikes_buildings: true,
            splash_radius: 0,
            fire: false,
            reach,
        }
    }

    /// `FightCalculator.IsInRange2D` of a bounds circle: the distance from
    /// the hit's centre, `FVector2.Distance` with `FPoint.RawSqrt`'s fast
    /// square root, less the circle's radius, no more than the range. The
    /// fast root errs by a few parts in a hundred thousand, which decides a
    /// unit standing on the edge of a splash.
    fn reaches(&self, x_q32: i64, z_q32: i64, radius_q32: i64, range_q32: i64) -> bool {
        let (center_x_q32, center_z_q32) = self.center_q32;
        native_q32_magnitude(
            x_q32.saturating_sub(center_x_q32),
            z_q32.saturating_sub(center_z_q32),
        )
        .saturating_sub(radius_q32)
            <= range_q32
    }

    /// Whether a splash of this radius takes a unit: one within it, and
    /// not underground, as `RangeTargetCalculator.CalculateRangeActors` asks
    /// `IsValidTarget(Stealth)` of each.
    fn splashes(&self, unit: &Actor, splash_q32: i64) -> bool {
        unit.visibility != Visibility::Hide
            && self.reaches(
                unit.x_q32,
                unit.z_q32,
                space_to_q32(unit.rules.collision_radius()),
                splash_q32,
            )
    }

    /// A hit no object deals, recorded under its team alone: it strikes both
    /// sides, of either domain, over a circle, and aims at nothing.
    pub(in crate::fight) fn unowned(
        team: u32,
        amount: i64,
        center_q32: (i64, i64),
        center_y_q32: i64,
        splash_radius: i64,
    ) -> Self {
        Self {
            source: None,
            source_team: team,
            team,
            effect: EffectTarget::Both,
            amount,
            provider: Provider::Other,
            projectile: None,
            skill_slot: None,
            aimed: None,
            hits_aimed: false,
            center_q32,
            center_y_q32,
            shield: None,
            crosses_shields: false,
            strikes_buildings: true,
            splash_radius,
            fire: false,
            reach: Reach::Targets(AttackTargets {
                ground: true,
                air: true,
            }),
        }
    }
}

/// The class of `IDamageProvider` behind a hit, where shared code tests it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::fight) enum Provider {
    /// `SupportUnitDamageProvider`: a summon's drop.
    SupportUnit,
    /// Any other.
    Other,
}

/// Whose objects a hit strikes: `DamagePerformer.PrepareRangeTargets` takes
/// the groups `GroupManager.GetOpponentGroups` answers for one, and every
/// group `GroupManager.GetGroups` does for the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::fight) enum EffectTarget {
    Opponent,
    Both,
}

impl EffectTarget {
    /// Whether a hit of this team's strikes an object of that one.
    const fn strikes(self, hitter: u32, team: u32) -> bool {
        match self {
            Self::Opponent => hitter != team,
            Self::Both => true,
        }
    }
}

/// Which units a hit can touch.
#[derive(Debug, Clone, Copy)]
pub(in crate::fight) enum Reach {
    /// The attacker's own `targets`: ground, air or both.
    Targets(AttackTargets),
    /// One domain only. `FightProjectile.Init` narrows a dual-domain skill to
    /// the actual target's domain, and `IDamageProvider.GetTargetType`
    /// preserves that choice for range damage.
    Domain(UnitDomain),
}

/// What one target took from a hit.
#[derive(Debug, Clone, Copy)]
pub(in crate::fight) struct Stroke {
    /// The life it actually lost, which is what a `damage` event records.
    pub(in crate::fight) actual: i64,
    /// The hit after every mitigation, before it is held to the life left:
    /// what the attacker's `DamageMax` counts.
    pub(in crate::fight) dealt: i64,
    /// The hit raised by what increases the target's damage taken, before
    /// anything reduces it: what the target's `DamageTaken` counts.
    pub(in crate::fight) taken: i64,
    /// Whether the target was no longer alive after it.
    pub(in crate::fight) killed: bool,
    /// Whether the target was alive when the hit reached it: the build skips
    /// a dead one before anything counts the hit.
    pub(in crate::fight) reached_alive: bool,
    /// Where a unit died, when this stroke killed it.
    pub(in crate::fight) death: Option<QVec3>,
    /// Where a building fell, when this stroke destroyed it.
    pub(in crate::fight) fallen: Option<QVec3>,
}

/// What a performed hit left for its caller to record.
///
/// Deaths and fallen buildings are handed back rather than recorded here
/// because each way of dealing damage records them in its own place in the
/// tick's events: a projectile records its own removal first.
#[derive(Debug, Default)]
pub(in crate::fight) struct Struck {
    /// Everything the hit struck, in the order it struck them, which is the
    /// list a hit's `DispatchHitDamageEvent` hands on.
    pub(in crate::fight) targets: Vec<FightActorRef>,
    pub(in crate::fight) deaths: Vec<(u64, QVec3)>,
    pub(in crate::fight) fallen: Vec<(u64, QVec3)>,
    /// Every death and fall together, in the order the hit struck them.
    pub(in crate::fight) ends: Vec<(FightActorRef, QVec3)>,
    /// The life it took in all, what `DamagePerformer.Perform` returns.
    pub(in crate::fight) lost: i64,
    /// The last shield it took energy from, which a recording names as the
    /// one that absorbed the projectile carrying the hit.
    pub(in crate::fight) shield: Option<u64>,
}

impl Reach {
    pub(in crate::fight) const fn touches(self, domain: UnitDomain) -> bool {
        match (self, domain) {
            (Self::Targets(targets), UnitDomain::Ground) => targets.ground,
            (Self::Targets(targets), UnitDomain::Air) => targets.air,
            (Self::Domain(UnitDomain::Ground), UnitDomain::Ground)
            | (Self::Domain(UnitDomain::Air), UnitDomain::Air) => true,
            (Self::Domain(_), _) => false,
        }
    }
}

impl Simulation {
    /// Whether an object flies (`IsFly`): a building stands on the ground.
    pub(in crate::fight) fn domain_of(&self, target: FightActorRef) -> UnitDomain {
        match target {
            FightActorRef::Unit(id) => self
                .actors
                .get(&id)
                .map_or(UnitDomain::Ground, |actor| actor.rules.domain),
            FightActorRef::Building(_) => UnitDomain::Ground,
        }
    }

    /// Every object one hit strikes, in the order it strikes them.
    ///
    /// The build's `DamagePerformer.PrepareRangeTargets`: what the hit was
    /// aimed at, if it strikes that wherever it stands, and every enemy its
    /// splash reaches. It is the one place this simulator decides who a hit
    /// lands on, which is why what it does not yet decide is refused here and
    /// nowhere else.
    ///
    /// # Errors
    ///
    /// Refuses a splash that has not been measured: one reaching a building
    /// from a hit aimed at a unit, and one reaching a unit from a hit aimed at
    /// a building.
    pub(in crate::fight) fn damage_targets(&self, hit: &DamageHit) -> Result<Vec<FightActorRef>> {
        let splash_q32 = space_to_q32(hit.splash_radius);
        if let Some(FightActorRef::Building(building_id)) = hit.aimed {
            let building = self
                .buildings
                .iter()
                .find(|building| building.building_id == building_id)
                .ok_or_else(|| Error::new("damage target building is absent"))?;
            let reached = hit.reaches(
                building.position.x,
                building.position.z,
                building.bounds_width / 2,
                splash_q32,
            );
            if hit.splash_radius == 0 || !hit.reach.touches(UnitDomain::Ground) {
                return Ok(if reached {
                    vec![FightActorRef::Building(building_id)]
                } else {
                    Vec::new()
                });
            }
            // A shot at a building splashes everything of the other side
            // around it, units and buildings alike, in the order the target
            // trees hold them: a Wraith's shot at one block of a wall reads 381
            // on that block and 381 on the next, whose edge is exactly its 8
            // metres of splash away, and an Arclight's shot at a block that
            // Crawlers are crossing reads the Crawlers and the block
            // interleaved, the block where the tree puts it.
            let struck = self
                .target_search_order()
                .into_iter()
                .filter(|(team, _)| hit.effect.strikes(hit.team, *team))
                .flat_map(|(_, candidates)| candidates)
                .filter(|candidate| match *candidate {
                    FightActorRef::Unit(unit_id) => {
                        let unit = &self.actors[&unit_id];
                        unit.alive()
                            && hit.reach.touches(unit.rules.domain)
                            && hit.splashes(unit, splash_q32)
                    }
                    FightActorRef::Building(other_id) => self
                        .buildings
                        .iter()
                        .find(|other| other.building_id == other_id)
                        .is_some_and(|other| {
                            building_alive(other)
                                && other.targetable
                                && hit.reaches(
                                    other.position.x,
                                    other.position.z,
                                    other.bounds_width / 2,
                                    splash_q32,
                                )
                        }),
                })
                .collect::<Vec<_>>();
            return Ok(struck);
        }
        // A shot at a unit takes what it was aimed at and, with a splash,
        // everything of the other side around it — buildings as well as units,
        // in the order the target trees hold them, and none underground
        // ([`DamageHit::splashes`]). An Arclight's shot at a
        // Crawler standing just in front of a wall reads the block between the
        // Crawlers around it, for the shot's full damage.
        let splash_reaches_buildings =
            hit.strikes_buildings && hit.splash_radius > 0 && hit.reach.touches(UnitDomain::Ground);
        let struck = self
            .target_search_order()
            .into_iter()
            .filter(|(team, _)| hit.effect.strikes(hit.team, *team))
            .flat_map(|(_, candidates)| candidates)
            .filter(|candidate_ref| match *candidate_ref {
                FightActorRef::Unit(candidate_id) => {
                    let candidate = &self.actors[&candidate_id];
                    candidate.alive()
                        && hit.reach.touches(candidate.rules.domain)
                        // `FightProjectile.Update` strikes what it aimed at
                        // only while `IsValidTarget(Stealth)`: a Wasp's shot
                        // that reaches a Sandworm as it burrows is spent.
                        // A hit with a splash strikes only what the splash
                        // reaches (`DamagePerformer.Perform` takes
                        // `PerformRangeEffect` over `PerformSingleEffect`): a
                        // Homing Missile landing its offset 20 metres from its
                        // target strikes nothing.
                        && ((hit.hits_aimed
                            && hit.splash_radius == 0
                            && Some(*candidate_ref) == hit.aimed
                            && candidate.visibility != Visibility::Hide)
                            || (hit.splash_radius > 0 && hit.splashes(candidate, splash_q32)))
                }
                FightActorRef::Building(building_id) => {
                    splash_reaches_buildings
                        && self
                            .buildings
                            .iter()
                            .find(|building| building.building_id == building_id)
                            .is_some_and(|building| {
                                building_alive(building)
                                    && building.targetable
                                    && hit.reaches(
                                        building.position.x,
                                        building.position.z,
                                        building.bounds_width / 2,
                                        splash_q32,
                                    )
                            })
                }
            })
            .collect::<Vec<_>>();
        Ok(struck)
    }

    /// `PerformHitTargetEffect` of a hit no object dealt, only a side: a
    /// fire's, or a buff's step on the unit it runs on. It takes the life, is
    /// counted for the side, and records the damage and any death.
    pub(in crate::fight) fn hit_with_no_object(
        &mut self,
        target: FightActorRef,
        team: u32,
        hit: (i64, bool),
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let stroke = self.strike(target, None, team, hit, Provider::Other)?;
        self.count_hit(None, team, target, &stroke)?;
        self.turned_unit_fell(target, &stroke);
        if stroke.actual > 0 {
            events.push(event(
                None,
                None,
                Some(team),
                Some(target.object_ref()),
                EventPayload::Damage {
                    amount: i32::try_from(stroke.actual)
                        .map_err(|_| Error::new("damage exceeds i32"))?,
                    skill_slot: None,
                },
            ));
        }
        if let Some(position) = stroke.death {
            self.record_ends(vec![(target, position)], events);
        }
        Ok(())
    }

    /// Takes one hit's damage off one target, unit or building alike.
    ///
    /// The one place this simulator takes life away. A unit remembers who hurt
    /// it and leaves the fight when it dies; a building stops being a target
    /// when it falls. `amplified` is `PerformHitTargetEffect`'s
    /// `isAmplifyDamageAffected`, and `provider` the class of what deals
    /// the hit.
    pub(in crate::fight) fn strike(
        &mut self,
        target: FightActorRef,
        source: Option<ObjectRef>,
        source_team: u32,
        (amount, amplified): (i64, bool),
        provider: Provider,
    ) -> Result<Stroke> {
        match target {
            FightActorRef::Unit(unit_id) => {
                let unit = self
                    .actors
                    .get_mut(&unit_id)
                    .ok_or_else(|| Error::new("damage target unit is absent"))?;
                // `PerformHitTargetEffect` scales the hit by the unit's rate on
                // damage taken before it takes any life, and counts it as taken
                // with the rate's increases alone; a hit that rate does not
                // affect is taken whole.
                let (taken, amount) = if amplified {
                    (
                        unit.stats.damage_taken_raised(amount)?,
                        unit.stats.damage_taken(amount)?,
                    )
                } else {
                    (amount, amount)
                };
                // Then the unit's damage reduction comes off it, though
                // never to below 1, and off a summon's drop only so far.
                let amount = reduced(amount, unit.stats.reduce_damage(), provider);
                let previous_life = unit.life;
                // `FightMech.OnHitted`: a shield with energy left takes the
                // hit, as much of it as it holds, and the unit loses no life;
                // what it took is what the hit dealt.
                if let Some(shield) = unit.shield.as_mut()
                    && amount >= 1
                    && shield.energy > 0
                {
                    let absorbed = shield.energy.min(amount);
                    shield.energy -= absorbed;
                    return Ok(Stroke {
                        actual: absorbed,
                        dealt: amount,
                        taken,
                        killed: false,
                        reached_alive: previous_life > 0,
                        death: None,
                        fallen: None,
                    });
                }
                unit.life = unit.life.saturating_sub(amount).max(0);
                let actual = previous_life - unit.life;
                if actual > 0 {
                    unit.last_damage_source = Some((source, source_team));
                }
                let death = (unit.life == 0).then(|| {
                    let position = QVec3 {
                        x: unit.x_q32,
                        y: space_to_q32(unit_height(unit.rules.domain)),
                        z: unit.z_q32,
                    };
                    unit.exit_fight_on_death();
                    position
                });
                // `DeadEffectSystem.OnActorDead` queues the unit's dead effect
                // for the module's update: an explosion explodes however its
                // unit died, once the unit has arrived
                // (`FightExplosionSkill.EnterFight`, `OnTravelFinished`).
                if death.is_some()
                    && previous_life > 0
                    && !self.actors[&unit_id].travelling
                    && self.explosion_of(unit_id).is_some()
                {
                    self.dead_explosions.push((unit_id, false));
                }
                Ok(Stroke {
                    actual,
                    dealt: amount,
                    taken,
                    killed: death.is_some(),
                    reached_alive: previous_life > 0,
                    death,
                    fallen: None,
                })
            }
            FightActorRef::Building(building_id) => {
                let (amount, taken) = if amplified {
                    self.construction_damage_taken(building_id, amount)?
                } else {
                    (amount, amount)
                };
                let building = self
                    .buildings
                    .iter_mut()
                    .find(|building| building.building_id == building_id)
                    .ok_or_else(|| Error::new("damage target building is absent"))?;
                let damage =
                    i32::try_from(amount).map_err(|_| Error::new("building damage exceeds i32"))?;
                let previous_life = building.life.current;
                building.life.current = building.life.current.saturating_sub(damage).max(0);
                let destroyed = previous_life > 0 && building.life.current == 0;
                if destroyed {
                    building.targetable = false;
                }
                let stroke = Stroke {
                    actual: i64::from(previous_life - building.life.current),
                    dealt: amount,
                    taken,
                    killed: building.life.current == 0,
                    reached_alive: previous_life > 0,
                    death: None,
                    fallen: destroyed.then_some(building.position),
                };
                // `FightActor.ReduceLife` hands a building it empties to
                // `DeadEffectSystem.OnActorDead`, which only queues it: its
                // `OnDead`, and a tower's loss with it, comes when that
                // module updates.
                if destroyed && self.is_tower(target) {
                    self.towers.fallen.push(building_id);
                }
                // An interceptor's building falling is `FightInterceptor.
                // OnDestroy`: it intercepts nothing from the next hit on.
                if destroyed {
                    self.lose_interceptor(building_id);
                }
                Ok(stroke)
            }
        }
    }

    /// Resolves one hit against everything it strikes.
    ///
    /// `damage` is recorded here, once per object that lost life, in the order
    /// the objects were struck. Deaths and fallen buildings are handed back:
    /// each way of dealing damage records them where it records them.
    pub(in crate::fight) fn perform_damage(
        &mut self,
        hit: DamageHit,
        events: &mut Vec<Event>,
    ) -> Result<Struck> {
        // `PerformSingleEffect` on a shield: it takes the hit, and no unit
        // does.
        if let Some(shield) = hit.shield
            && hit.splash_radius == 0
        {
            let mut struck = Struck::default();
            if self.hit_shield(shield, &hit, events)? > 0 {
                struck.shield = Some(shield);
            }
            return Ok(struck);
        }
        let mut targets = self.damage_targets(&hit)?;
        let mut shield = None;
        if hit.splash_radius > 0 && !hit.crosses_shields {
            for standing in self.shields_in_the_way(&hit, &mut targets) {
                if self.hit_shield(standing, &hit, events)? > 0 {
                    shield = Some(standing);
                }
            }
        }
        let mut struck = self.strike_targets(&hit, targets, events)?;
        struck.shield = shield;
        if let Some(secondary) = self.secondary_damage_of(&hit) {
            self.perform_secondary(&hit, secondary, &mut struck, events)?;
        }
        Ok(struck)
    }

    /// `DamagePerformer.PerformHitTargetsEffect` and `DispatchHitDamageEvent`:
    /// one hit on each of its targets in turn, and then its skill's hit
    /// effects.
    pub(in crate::fight) fn strike_targets(
        &mut self,
        hit: &DamageHit,
        targets: Vec<FightActorRef>,
        events: &mut Vec<Event>,
    ) -> Result<Struck> {
        let mut struck = Struck::default();
        for target in targets {
            // `DamagePerformer.PerformHitTargetEffect` hands a hit on to
            // `FightCalculator` only when it deals at least 1: one that deals
            // nothing, an Incendiary Bomb's, takes no life and is no hit
            // `ExpSystem.OnActorHitted` hears of, so its owner does not share
            // what the target is worth when it dies.
            if hit.amount < 1 {
                struck.targets.push(target);
                continue;
            }
            let stroke = self.strike(
                target,
                hit.source,
                hit.source_team,
                (hit.amount, true),
                hit.provider,
            )?;
            self.count_hit(hit.source, hit.source_team, target, &stroke)?;
            self.turned_unit_fell(target, &stroke);
            struck.targets.push(target);
            struck.lost += stroke.actual;
            if stroke.actual > 0 {
                events.push(event(
                    hit.projectile,
                    hit.source,
                    Some(hit.source_team),
                    Some(target.object_ref()),
                    EventPayload::Damage {
                        amount: i32::try_from(stroke.actual)
                            .map_err(|_| Error::new("damage exceeds i32"))?,
                        skill_slot: hit.skill_slot,
                    },
                ));
            }
            let id = match target {
                FightActorRef::Unit(id) | FightActorRef::Building(id) => id,
            };
            if let Some(position) = stroke.death {
                struck.deaths.push((id, position));
                struck.ends.push((target, position));
            }
            if let Some(position) = stroke.fallen {
                struck.fallen.push((id, position));
                struck.ends.push((target, position));
            }
        }
        self.dispatch_hit_damage(hit, struck.lost, events)?;
        // A hit that deals fire sets alight the oil its splash reaches.
        if hit.fire {
            let (x_q32, z_q32) = hit.center_q32;
            self.ignite_oils_hit((x_q32, z_q32, space_to_q32(hit.splash_radius)), hit.team)?;
        }
        Ok(struck)
    }

    /// `DamagePerformer.CheckSecondaryDamageApplied`: the second damage of the
    /// skill that dealt a hit, itself or through its projectile, unless its
    /// owner has died or its technologies are disabled: an Arclight's shell
    /// that lands after the Arclight fell deals no Shockwave. Only a unit's
    /// main skill is handed one (`SecondaryDamageIntensifyEffectProvider`).
    fn secondary_damage_of(&self, hit: &DamageHit) -> Option<SecondaryDamage> {
        let source = hit
            .source
            .filter(|source| source.kind == ObjectKind::Unit)?;
        let owner = self.actors.get(&source.id)?;
        (hit.skill_slot == Some(0) && owner.alive() && !owner.technology_disabled())
            .then_some(owner.placement.secondary_damage)
            .flatten()
    }

    /// `DamagePerformer.PerformSecondaryRangeEffect`, after a hit has struck
    /// what it strikes: every object of the other side within the second
    /// damage's range of where the hit landed, of the domain the hit was
    /// aimed at, less what the hit itself struck, takes the second damage.
    /// The damage is raised by the owner's tower buffs and then by each
    /// struck unit's damage taken where the row says the buffs reach it
    /// (`CalculateSecondaryDamageByAttackerBuff`,
    /// `CalculateSecondaryDamageByTargetBuff`), and `PerformHitTargetEffect`
    /// takes it with `isAmplifyDamageAffected` false, scaling it no further.
    /// An Arclight with Shockwave fells the
    /// Crawlers its shell lands among and deals 75 to every other Crawler
    /// within 30 metres.
    fn perform_secondary(
        &mut self,
        hit: &DamageHit,
        secondary: SecondaryDamage,
        struck: &mut Struck,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        if !self.shield.standing.is_empty() {
            return Err(Error::new(
                "a second damage lands in a fight with a battlefield shield, which is not \
                 measured",
            ));
        }
        let owner = hit.source.expect("a second damage has its unit").id;
        if self.actors[&owner].placement.lifesteal.is_some() {
            return Err(Error::new(
                "a second damage is dealt by a unit with lifesteal, and whether its hit \
                 hands life back is not measured",
            ));
        }
        let amount = if secondary.buffed {
            self.actors[&owner].stats.overlays.scaled_by_buffs_of(
                super::tower::SOURCE,
                Index::AttackDamage,
                secondary.damage,
            )
        } else {
            secondary.damage
        };
        let around = DamageHit {
            amount,
            splash_radius: secondary.splash_radius,
            hits_aimed: false,
            ..*hit
        };
        let targets = self
            .damage_targets(&around)?
            .into_iter()
            .filter(|target| secondary.hits_main_target || !struck.targets.contains(target))
            .collect::<Vec<_>>();
        for target in targets {
            let amount = match target {
                _ if !secondary.buffed => amount,
                FightActorRef::Unit(id) => self.actors[&id].stats.damage_taken(amount)?,
                FightActorRef::Building(id) => self.construction_damage_taken(id, amount)?.0,
            };
            if amount < 1 {
                continue;
            }
            let stroke = self.strike(
                target,
                hit.source,
                hit.source_team,
                (amount, false),
                hit.provider,
            )?;
            self.count_hit(hit.source, hit.source_team, target, &stroke)?;
            self.turned_unit_fell(target, &stroke);
            if stroke.actual > 0 {
                events.push(event(
                    hit.projectile,
                    hit.source,
                    Some(hit.source_team),
                    Some(target.object_ref()),
                    EventPayload::Damage {
                        amount: i32::try_from(stroke.actual)
                            .map_err(|_| Error::new("damage exceeds i32"))?,
                        skill_slot: hit.skill_slot,
                    },
                ));
            }
            let id = match target {
                FightActorRef::Unit(id) | FightActorRef::Building(id) => id,
            };
            if let Some(position) = stroke.death {
                struck.deaths.push((id, position));
                struck.ends.push((target, position));
            }
            if let Some(position) = stroke.fallen {
                struck.fallen.push((id, position));
                struck.ends.push((target, position));
            }
        }
        Ok(())
    }

    /// `IDamageProvider.DispatchHitDamageEvent` after a hit: a unit's skill,
    /// struck directly (`SkillDamageProvider`) or through its projectile
    /// (`FightProjectile`), hands the life the hit took in all to the skill's
    /// hit effects, `FightSkill.DispatchHitDamageEvent`. The one hit effect
    /// this simulator gives a skill is `LifeStealEffectProvider`'s. A hit no
    /// unit's skill dealt — a turret's, a mine's, a battle skill's — reaches
    /// no unit's skill.
    pub(in crate::fight) fn dispatch_hit_damage(
        &mut self,
        hit: &DamageHit,
        damage: i64,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        match hit.source {
            Some(owner) if owner.kind == ObjectKind::Unit && hit.skill_slot.is_some() => {
                self.steal_life(owner.id, damage, events)
            }
            _ => Ok(()),
        }
    }

    /// `LifeStealEffectProvider.PerformHitEffect`: the skill's owner, alive
    /// and short of its maximum life, takes back the whole part of the hit's
    /// damage times its `ILifeSteal`'s multiplier, through
    /// `FightMech.StealLife`, [`Self::add_life`].
    ///
    /// A source that `CanDisable` does nothing while the owner's
    /// technologies are disabled, which the provider reads as the hit's
    /// `isTechnologyDisabled`.
    fn steal_life(&mut self, owner_id: u64, damage: i64, events: &mut Vec<Event>) -> Result<()> {
        let Some(owner) = self.actors.get_mut(&owner_id) else {
            return Ok(());
        };
        let Some(lifesteal) = owner.placement.lifesteal else {
            return Ok(());
        };
        if lifesteal.can_disable && owner.technology_disabled() {
            return Ok(());
        }
        let max_life = owner.stats.max_life();
        if !owner.alive() || owner.life >= max_life {
            return Ok(());
        }
        let stolen = q32_mul(damage << 32, lifesteal.multiplier_q32) >> 32;
        if stolen < 1 {
            return Ok(());
        }
        self.add_life(owner_id, stolen, events)
    }

    /// `DamagePerformer.ProcessAdvancedEnergyShieldEffect` and the shields
    /// `PerformRangeEffect` then strikes. Every shield of the sides the hit
    /// strikes that does not hold the point it lands at takes its side's
    /// units and towers it covers out of the hit; the one covering what the
    /// hit was aimed at is its main shield. The main shield, and every other
    /// the splash reaches in the plane, take the hit, before any unit does.
    pub(in crate::fight) fn shields_in_the_way(
        &self,
        hit: &DamageHit,
        targets: &mut Vec<FightActorRef>,
    ) -> Vec<u64> {
        let (x_q32, z_q32) = hit.center_q32;
        let mut main = hit.shield;
        let mut listed = Vec::new();
        for shield in &self.shield.standing {
            if !shield.active
                || !hit.effect.strikes(hit.team, shield.team)
                || shield.contains(x_q32, hit.center_y_q32, z_q32)
            {
                continue;
            }
            let covered = |target: &FightActorRef| {
                self.fight_actor(*target)
                    .is_some_and(|actor| actor.team == shield.team)
                    && self.shield_holds(shield.id, *target)
            };
            if main.is_none() && hit.aimed.is_some_and(|aimed| covered(&aimed)) {
                main = Some(shield.id);
            }
            targets.retain(|target| !covered(target));
            listed.push(shield);
        }
        listed
            .into_iter()
            .filter(|shield| {
                Some(shield.id) == main
                    || native_q32_magnitude(
                        shield.x_q32.saturating_sub(x_q32),
                        shield.z_q32.saturating_sub(z_q32),
                    )
                    .saturating_sub(shield.radius_q32)
                        <= space_to_q32(hit.splash_radius)
            })
            .map(|shield| shield.id)
            .collect()
    }

    /// Records the units a hit killed, each credited to whoever last hurt it.
    pub(in crate::fight) fn record_deaths(
        &self,
        deaths: Vec<(u64, QVec3)>,
        events: &mut Vec<Event>,
    ) {
        for (dead_id, position) in deaths {
            let (source, source_team_id) = self.actors[&dead_id]
                .last_damage_source
                .map_or((None, None), |(source, team_id)| (source, Some(team_id)));
            events.push(event(
                Some(ObjectRef::new(ObjectKind::Unit, dead_id)),
                source,
                source_team_id,
                None,
                EventPayload::UnitDied { position },
            ));
        }
    }

    pub(in crate::fight) fn direct_effect(
        &mut self,
        actor_id: u64,
        target: FightActorRef,
        skill_slot: usize,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let amount = self.main_attack_damage(actor_id);
        let skill_ref = SkillRef::main(FightActorRef::Unit(actor_id));
        self.blow(skill_ref, skill_slot, target, amount, events)
    }

    /// An extra skill's strike: its own `DamageEffect`, dealing its own
    /// damage, of its own slot.
    pub(in crate::fight) fn extra_direct_effect(
        &mut self,
        skill_ref: SkillRef,
        target: FightActorRef,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let amount = self
            .skill_attacker(skill_ref)
            .ok_or_else(|| Error::new("an extra skill's owner is absent"))?
            .attack_damage;
        let skill_slot = self.skill_slot(skill_ref);
        self.blow(skill_ref, skill_slot, target, amount, events)
    }

    /// A blow dealing what a beam's damage effect deals: the main skill's
    /// `DamageEffect.Perform` with another amount.
    pub(in crate::fight) fn direct_effect_dealing(
        &mut self,
        actor_id: u64,
        target: FightActorRef,
        amount: i64,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let skill_ref = SkillRef::main(FightActorRef::Unit(actor_id));
        self.blow(skill_ref, 0, target, amount, events)
    }

    /// `DamagePerformer.Perform` of a skill's `SkillDamageProvider`: the
    /// splash is measured from what the blow struck, or from the skill's
    /// own unit for a skill that splashes about itself
    /// (`CalculateDamagePosition`), and a splash that diffuses grows from
    /// there over the updates to come (`PerformDiffusionRangeEffect`).
    fn blow(
        &mut self,
        skill_ref: SkillRef,
        skill_slot: usize,
        target: FightActorRef,
        amount: i64,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let actor_id = skill_ref
            .owner
            .unit_id()
            .ok_or_else(|| Error::new("a construction's strike is not supported"))?;
        let skill = self
            .skill_attacker(skill_ref)
            .ok_or_else(|| Error::new("a striking skill's owner is absent"))?;
        let (splash_radius, self_splash, diffusion) = (
            skill.splash_radius,
            skill.attack.self_splash,
            skill.attack.diffusion,
        );
        let attacker = &self.actors[&actor_id];
        let (center_q32, center_y_q32) = if self_splash && splash_radius > 0 {
            (
                (attacker.x_q32, attacker.z_q32),
                space_to_q32(unit_height(attacker.rules.domain)),
            )
        } else {
            let center_q32 = match target {
                FightActorRef::Unit(target_id) => {
                    let aimed = &self.actors[&target_id];
                    (aimed.x_q32, aimed.z_q32)
                }
                FightActorRef::Building(building_id) => self
                    .buildings
                    .iter()
                    .find(|building| building.building_id == building_id)
                    .map(|building| (building.position.x, building.position.z))
                    .ok_or_else(|| Error::new("direct attack target is absent"))?,
            };
            (center_q32, self.target_height_q32(target))
        };
        let shield = self.blow_shield(actor_id, target);
        if shield.is_some() && splash_radius > 0 {
            return Err(Error::new(
                "a splashing blow at a unit its side's shield covers is not measured",
            ));
        }
        // A blow is `SkillDamageProvider`'s, of the skill that struck.
        let hit = DamageHit {
            center_q32,
            center_y_q32,
            shield,
            splash_radius,
            ..DamageHit::of_skill(
                attacker,
                u16::try_from(skill_slot).expect("skill slot fits u16"),
                (target, self.domain_of(target)),
                amount,
            )
        };
        if let Some(diffusion) = diffusion.filter(|_| splash_radius > 0) {
            return self.start_diffusion(skill_ref, hit, diffusion);
        }
        let struck = self.perform_damage(hit, events)?;
        // A block a blow fells falls after every hit the tick resolves, as a
        // shot's does: the Crawlers of `wall-block.yaml` read three more blows
        // between the one that fells block 5 and `building_destroyed`. It
        // falls in its place among the tick's deaths, which the tick's end
        // moves there together.
        self.record_ends(struck.ends, events);
        Ok(())
    }

    /// A beam that a shield takes in place of its target.
    pub(in crate::fight) fn beam_at_shield(
        &mut self,
        actor_id: u64,
        target: FightActorRef,
        shield: u64,
        damage: i64,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let attacker = &self.actors[&actor_id];
        if attacker.stats.splash_radius() > 0 {
            return Err(Error::new(
                "a splashing beam at a unit its side's shield covers is not measured",
            ));
        }
        let hit = DamageHit {
            shield: Some(shield),
            crosses_shields: false,
            splash_radius: 0,
            ..DamageHit::of_skill(attacker, 0, (target, self.domain_of(target)), damage)
        };
        self.perform_damage(hit, events)?;
        Ok(())
    }

    /// `DamageEffect.Perform`'s shield: the one of the target's side that
    /// covers it, `FightSkill.IsActorProtectedByEnergyShield`, unless the
    /// blow crosses shields or the attacker stands inside that shield too.
    pub(in crate::fight) fn blow_shield(
        &self,
        actor_id: u64,
        target: FightActorRef,
    ) -> Option<u64> {
        self.search_target_shield(FightActorRef::Unit(actor_id), target)
    }

    /// How high a target stands, `FPoint` raw metres.
    pub(in crate::fight) fn target_height_q32(&self, target: FightActorRef) -> i64 {
        match target {
            FightActorRef::Unit(id) => self
                .actors
                .get(&id)
                .map_or(0, |actor| space_to_q32(unit_height(actor.rules.domain))),
            FightActorRef::Building(_) => 0,
        }
    }

    /// A hit's deaths and falls, in the order it struck them, among the
    /// tick's events: the tick's end moves every one of them, in that order,
    /// after the rest (`DeadEffectSystem.deadActors`).
    pub(in crate::fight) fn record_ends(
        &self,
        ends: Vec<(FightActorRef, QVec3)>,
        events: &mut Vec<Event>,
    ) {
        for (target, position) in ends {
            match target {
                FightActorRef::Unit(dead_id) => {
                    self.record_deaths(vec![(dead_id, position)], events);
                }
                FightActorRef::Building(building_id) => events.push(event(
                    Some(ObjectRef::new(ObjectKind::Building, building_id)),
                    None,
                    None,
                    None,
                    EventPayload::BuildingDestroyed { position },
                )),
            }
        }
    }

    /// What the `blow`th blow of a laser skill's attack deals: the main
    /// skill's from its unit's numbers, and an extra skill's with a damage
    /// rate from the unit's base damage at that rate, corrected by what
    /// reaches the main skill, which its `DataSet` holds too.
    fn laser_damage(&self, skill_ref: SkillRef, blow: usize) -> Result<i64> {
        let actor_id = skill_ref
            .owner
            .unit_id()
            .ok_or_else(|| Error::new("a construction's laser is not supported"))?;
        let actor = &self.actors[&actor_id];
        let against = self.attack_domain(self.skill(skill_ref).attack_target());
        let SkillSlot::Extra(index) = skill_ref.slot else {
            return Ok(actor.stats.laser_damage(&actor.rules, blow, against));
        };
        let rules = &actor.skills.extras[index].rules;
        let AttackPath::Laser { damage_multipliers } = &rules.attack.path else {
            return Err(Error::new("a laser blow from a skill that is no laser"));
        };
        if rules.damage_rate <= 0.0 {
            return Err(Error::new(
                "an extra laser skill without a damage rate is not measured",
            ));
        }
        Ok(actor.stats.ramped_laser_damage(
            actor.rules.attack.base_damage,
            damage_multipliers,
            (rules.damage_rate, blow),
            (0, against),
            false,
        ))
    }

    /// A beam's blow: the `blow`th of its skill's attack, from the skill
    /// `skill_ref` holds or the `member`th skill of its group, which the
    /// recording names by its own slot.
    pub(in crate::fight) fn laser_effect(
        &mut self,
        skill_ref: SkillRef,
        member: usize,
        blow: usize,
        target: FightActorRef,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let actor_id = skill_ref
            .owner
            .unit_id()
            .ok_or_else(|| Error::new("a construction's laser is not supported"))?;
        let damage = self.laser_damage(skill_ref, blow)?;
        let skill_slot = u16::try_from(self.skill_slot(skill_ref) + member)
            .map_err(|_| Error::new("a skill slot is outside u16"))?;
        let (attacker_ref, attacker_team) = {
            let attacker = &self.actors[&actor_id];
            (attacker.object_ref(), attacker.placement.team)
        };
        // A beam that splashes strikes as any other hit does, what it was
        // aimed at and everything of the other side around it, in the order
        // the target trees hold them: a Melting Point's beam at one Crawler
        // reads the Crawler beside it first.
        let splash_radius = self
            .skill_attacker(skill_ref)
            .ok_or_else(|| Error::new("laser owner is absent"))?
            .splash_radius;
        // A beam at a unit its side's shield covers strikes the shield, as a
        // blow does: `DamageEffect.Perform`.
        if let Some(shield) = self.blow_shield(actor_id, target) {
            if skill_ref.slot != SkillSlot::Main {
                return Err(Error::new(
                    "an extra skill's beam at a unit its side's shield covers is not measured",
                ));
            }
            return self.beam_at_shield(actor_id, target, shield, damage, events);
        }
        if splash_radius > 0 {
            let attacker = &self.actors[&actor_id];
            let center_q32 = match target {
                FightActorRef::Unit(target_id) => {
                    let aimed = &self.actors[&target_id];
                    (aimed.x_q32, aimed.z_q32)
                }
                FightActorRef::Building(building_id) => self
                    .buildings
                    .iter()
                    .find(|building| building.building_id == building_id)
                    .map(|building| (building.position.x, building.position.z))
                    .ok_or_else(|| Error::new("laser target is absent"))?,
            };
            let hit = DamageHit {
                center_q32,
                center_y_q32: self.target_height_q32(target),
                splash_radius,
                ..DamageHit::of_skill(
                    attacker,
                    skill_slot,
                    (target, self.domain_of(target)),
                    damage,
                )
            };
            let struck = self.perform_damage(hit, events)?;
            self.record_ends(struck.ends, events);
            return Ok(());
        }
        // A laser with no splash strikes one target, so it takes the
        // stroke without the range step. A unit it kills is recorded dead
        // before the damage, and a block it fells falls after it: the Steel
        // Balls of `wall-laser.yaml` read `damage` and then
        // `building_destroyed`. A beam that took no life records no damage,
        // as no other hit does.
        let stroke = self.strike(
            target,
            Some(attacker_ref),
            attacker_team,
            (damage, true),
            Provider::Other,
        )?;
        self.count_hit(Some(attacker_ref), attacker_team, target, &stroke)?;
        self.turned_unit_fell(target, &stroke);
        if let Some(position) = stroke.death {
            events.push(event(
                Some(target.object_ref()),
                Some(attacker_ref),
                Some(attacker_team),
                None,
                EventPayload::UnitDied { position },
            ));
        }
        if stroke.actual > 0 {
            events.push(event(
                None,
                Some(attacker_ref),
                Some(attacker_team),
                Some(target.object_ref()),
                EventPayload::Damage {
                    amount: i32::try_from(stroke.actual)
                        .map_err(|_| Error::new("laser damage exceeds i32"))?,
                    skill_slot: Some(skill_slot),
                },
            ));
        }
        // The beam is its skill's `SkillDamageProvider`, which hands what it
        // took to the skill's hit effects as any hit does.
        self.steal_life(actor_id, stroke.actual, events)?;
        // A building the beam fells is recorded at the end of the tick, as a
        // blow's and a projectile's are: in the tower-loss fight with two
        // lanes, the other lane's Steel Ball damages its tower between the
        // beam that fells the first and that tower's `building_destroyed`.
        // The tick's end moves it there, in its place among the deaths.
        if let Some(position) = stroke.fallen {
            events.push(event(
                Some(target.object_ref()),
                None,
                None,
                None,
                EventPayload::BuildingDestroyed { position },
            ));
        }
        Ok(())
    }
}

/// `PerformHitTargetEffect` on a unit, after the rates: a hit its damage
/// reduction would leave below 1 deals 1, and any other loses the whole
/// reduction, unless a summon's drop deals it.
fn reduced(amount: i64, reduction: i64, provider: Provider) -> i64 {
    if amount > 0 && amount - reduction < 1 {
        1
    } else if amount != 0 && provider != Provider::SupportUnit {
        amount - reduction
    } else {
        amount
    }
}
