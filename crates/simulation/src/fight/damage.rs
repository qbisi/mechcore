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
    /// What it deals a shield it strikes in place of `amount`, where its
    /// provider's `IDamageModifier.IsChangeHitEnergyShieldDamage` says so: a
    /// battlefield shield, or a unit's own shield with energy left
    /// (`DamagePerformer.CalculateHitEnergyShieldDamage`). Electromagnetic
    /// Barrage's shells deal nothing but 6000 to a shield.
    pub(in crate::fight) shield_damage: Option<i64>,
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
            provider: Provider::Skill {
                melee: attacker.skill_melee(skill_slot),
            },
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
            shield_damage: None,
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
            provider: Provider::Projectile,
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
            shield_damage: None,
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
            shield_damage: None,
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
    /// `FightProjectile`.
    Projectile,
    /// `SkillDamageProvider`, of a skill whose attack is melee or not.
    Skill { melee: bool },
    /// Any other.
    Other,
}

impl Provider {
    /// `IDamageProvider.GetAttackDistanceType` is `remote`: a projectile's
    /// answers it, a skill's does unless the skill is melee
    /// (`2 - IsMelee`), and every other answers `None`.
    const fn remote(self) -> bool {
        matches!(self, Self::Projectile | Self::Skill { melee: false })
    }
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

impl Struck {
    /// Takes on another hit's deaths and falls after its own: those a hit
    /// effect's hits dealt.
    pub(in crate::fight) fn absorb_ends(&mut self, other: Self) {
        self.deaths.extend(other.deaths);
        self.fallen.extend(other.fallen);
        self.ends.extend(other.ends);
    }
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
                .map_or(UnitDomain::Ground, |actor| actor.domain),
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
                            && hit.reach.touches(unit.domain)
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
                        && hit.reach.touches(candidate.domain)
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
        for (struck, stroke) in self.strike(target, None, team, hit, Provider::Other, events)? {
            self.count_hit(None, team, struck, &stroke)?;
            self.turned_unit_fell(struck, &stroke);
            if stroke.actual > 0 {
                events.push(event(
                    None,
                    None,
                    Some(team),
                    Some(struck.object_ref()),
                    EventPayload::Damage {
                        amount: i32::try_from(stroke.actual)
                            .map_err(|_| Error::new("damage exceeds i32"))?,
                        skill_slot: None,
                    },
                ));
            }
            if let Some(position) = stroke.death {
                self.record_ends(vec![(struck, position)], events);
            }
        }
        Ok(())
    }

    /// Takes one hit's damage off one target, unit or building alike, and
    /// answers what each object it reached lost: the target alone, or every
    /// member alive of a group that shares it, in the group's order.
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
        hit: (i64, bool),
        provider: Provider,
        events: &mut Vec<Event>,
    ) -> Result<Vec<(FightActorRef, Stroke)>> {
        // `FightCalculator.PerformHitTargetEffect`'s first hit on a unit a
        // group shares damage with hands its damage, after the unit's rates,
        // reduction and stealth, to `CalculateGroupDamage`: the whole part of
        // it over the group's count, dead members counted, to each member
        // alive in the group's order as a hit of its own, which nothing
        // raises, reduces or shares again. A dead member's part is lost.
        if let FightActorRef::Unit(unit_id) = target
            && self.actors.get(&unit_id).is_some_and(Actor::alive)
            && let Some(members) = self.sharing_group(unit_id)
        {
            let (_, amount) = self.first_hit(unit_id, hit, provider)?;
            let count = i64::try_from(members.len()).map_err(|_| Error::new("group too large"))?;
            let share = math::q32_div(amount << 32, count << 32) >> 32;
            let mut strokes = Vec::with_capacity(members.len());
            for member in members {
                if !self.actors[&member].alive() {
                    continue;
                }
                let stroke = self.land(member, (source, source_team), (share, share), events)?;
                self.after_hit(FightActorRef::Unit(member), source, &stroke, events)?;
                strokes.push((FightActorRef::Unit(member), stroke));
            }
            return Ok(strokes);
        }
        let stroke = self.strike_target(target, source, source_team, hit, provider, events)?;
        self.after_hit(target, source, &stroke, events)?;
        Ok(vec![(target, stroke)])
    }

    /// `FightMech.OnHitted` raises `OnMechBeHit` once the hit took what it
    /// took, the hit's owner its `damageSourceOwner`.
    fn after_hit(
        &mut self,
        target: FightActorRef,
        source: Option<ObjectRef>,
        stroke: &Stroke,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        if let (FightActorRef::Unit(id), Some(attacker)) = (target, source)
            && stroke.reached_alive
        {
            self.on_mech_be_hit(id, attacker, events)?;
        }
        Ok(())
    }

    /// `PerformHitTargetEffect` on a first hit at a unit: it scales the hit
    /// by the unit's rate on damage taken before it takes any life, and
    /// counts it as taken with the rate's increases alone, a hit that rate
    /// does not affect taken whole, and a remote hit takes the unit's rate on
    /// remote hits; then the unit's damage reduction comes
    /// off it, though never to below 1, and off a summon's drop only so far;
    /// a unit in stealth loses none of it, though it counts as taken.
    /// Answers what it counts as taken and what it deals.
    fn first_hit(
        &self,
        unit_id: u64,
        (amount, amplified): (i64, bool),
        provider: Provider,
    ) -> Result<(i64, i64)> {
        let unit = self
            .actors
            .get(&unit_id)
            .ok_or_else(|| Error::new("damage target unit is absent"))?;
        let (taken, amount) = if amplified {
            (
                unit.stats.damage_taken_raised(amount)?,
                unit.stats.damage_taken(amount)?,
            )
        } else {
            (amount, amount)
        };
        // A remote hit then takes the unit's rate on remote hits.
        let amount = if provider.remote() {
            unit.stats.remote_damage_taken(amount)
        } else {
            amount
        };
        let amount = if unit.visibility == Visibility::Stealth {
            0
        } else {
            reduced(amount, unit.stats.reduce_damage(), provider)
        };
        Ok((taken, amount))
    }

    /// What `FightMech.OnHitted` and `FightActor.ReduceLife` do with a hit
    /// on a unit, once its damage is known. A shield with energy left takes
    /// the hit, as much of it as it holds, and the unit loses no life; what
    /// it took is what the hit dealt. A unit in stealth loses none of it.
    fn land(
        &mut self,
        unit_id: u64,
        (source, source_team): (Option<ObjectRef>, u32),
        (amount, taken): (i64, i64),
        events: &mut Vec<Event>,
    ) -> Result<Stroke> {
        let unit = self
            .actors
            .get_mut(&unit_id)
            .ok_or_else(|| Error::new("damage target unit is absent"))?;
        let previous_life = unit.life;
        if let Some(shield) = unit.shield.as_mut()
            && amount >= 1
            && shield.available()
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
        if unit.visibility != Visibility::Stealth {
            unit.life = unit.life.saturating_sub(amount).max(0);
        }
        let actual = previous_life - unit.life;
        if actual > 0 {
            unit.last_damage_source = Some((source, source_team));
        }
        let killed = unit.life == 0 && previous_life > 0;
        // `ReduceLife` invokes `OnLifeChange` as soon as it took life,
        // before the unit's death is handled.
        if actual > 0 {
            self.on_life_change(unit_id, events)?;
        }
        let unit = &self.actors[&unit_id];
        let death = (unit.life == 0).then(|| QVec3 {
            x: unit.x_q32,
            y: space_to_q32(unit_height(unit.domain)),
            z: unit.z_q32,
        });
        if killed {
            self.on_actor_dead(unit_id);
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

    fn strike_target(
        &mut self,
        target: FightActorRef,
        source: Option<ObjectRef>,
        source_team: u32,
        (amount, amplified): (i64, bool),
        provider: Provider,
        events: &mut Vec<Event>,
    ) -> Result<Stroke> {
        match target {
            FightActorRef::Unit(unit_id) => {
                let (taken, amount) = self.first_hit(unit_id, (amount, amplified), provider)?;
                self.land(unit_id, (source, source_team), (amount, taken), events)
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

    /// `FightActor.OnLifeChange`, which `ReduceLife` invokes once it took
    /// life from a unit: its `GetDamage` buff sources, and
    /// `StealthTechSystem`.
    fn on_life_change(&mut self, unit_id: u64, events: &mut Vec<Event>) -> Result<()> {
        self.add_damaged_buffs(unit_id, events)?;
        self.stealth_on_life_change(unit_id);
        Ok(())
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
        if hit.splash_radius > 0 {
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

    /// What an extra skill's hit deals a shield in place of its damage: its
    /// technology's `energyShieldDamage`, the skill's damage modifier
    /// (`ExtraWeaponTech.ChangeHitEnergyShieldDamage`).
    pub(in crate::fight) fn skill_shield_damage(&self, skill_ref: SkillRef) -> Option<i64> {
        let SkillSlot::Extra(index) = skill_ref.slot else {
            return None;
        };
        self.skills(skill_ref.owner)
            .extras
            .get(index)
            .and_then(|extra| extra.rules.shield_damage)
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
            // `DamagePerformer.PerformHitTargetEffect` hands the target to the
            // hit's pre-hit effects first, and a target they destroyed takes
            // nothing more of it.
            let culled = self.cull(hit, target, events)?;
            let (carrier, skill_slot) = if culled.is_some() {
                (None, None)
            } else {
                (hit.projectile, hit.skill_slot)
            };
            let strokes = if let Some(stroke) = culled {
                vec![(target, stroke)]
            } else {
                // `DamagePerformer.PerformHitTargetEffect` hands a hit on to
                // `FightCalculator` only when it deals at least 1: one that deals
                // nothing, an Incendiary Bomb's, takes no life and is no hit
                // `ExpSystem.OnActorHitted` hears of, so its owner does not share
                // what the target is worth when it dies.
                // A unit whose own shield has energy left takes what the hit
                // deals a shield, before what it deals is asked
                // (`CalculateHitEnergyShieldDamage`).
                let amount = match (hit.shield_damage, target) {
                    (Some(damage), FightActorRef::Unit(id))
                        if self.actors[&id]
                            .shield
                            .as_ref()
                            .is_some_and(PersonalShield::available) =>
                    {
                        damage
                    }
                    _ => hit.amount,
                };
                if amount < 1 {
                    struck.targets.push(target);
                    continue;
                }
                self.strike(
                    target,
                    hit.source,
                    hit.source_team,
                    (amount, true),
                    hit.provider,
                    events,
                )?
            };
            struck.targets.push(target);
            for (reached, stroke) in strokes {
                self.count_hit(hit.source, hit.source_team, reached, &stroke)?;
                self.turned_unit_fell(reached, &stroke);
                struck.lost += stroke.actual;
                if stroke.actual > 0 {
                    events.push(event(
                        carrier,
                        hit.source,
                        Some(hit.source_team),
                        Some(reached.object_ref()),
                        EventPayload::Damage {
                            amount: i32::try_from(stroke.actual)
                                .map_err(|_| Error::new("damage exceeds i32"))?,
                            skill_slot,
                        },
                    ));
                }
                let id = match reached {
                    FightActorRef::Unit(id) | FightActorRef::Building(id) => id,
                };
                if let Some(position) = stroke.death {
                    struck.deaths.push((id, position));
                    struck.ends.push((reached, position));
                }
                if let Some(position) = stroke.fallen {
                    struck.fallen.push((id, position));
                    struck.ends.push((reached, position));
                }
            }
        }
        let effects = self.dispatch_hit_damage(hit, &struck.targets, struck.lost, events)?;
        struck.absorb_ends(effects);
        // A hit that deals fire sets alight the oil its splash reaches.
        if hit.fire {
            let (x_q32, z_q32) = hit.center_q32;
            self.ignite_oils_hit((x_q32, z_q32, space_to_q32(hit.splash_radius)), hit.team)?;
        }
        Ok(struck)
    }

    /// `DeadLineEffectProvider.PerformPreHitEffect`, a pre-hit effect of the
    /// main skill of a unit with a dead-line technology, itself or through
    /// its projectile (`FightProjectile` hands its pre-hit on to its skill's
    /// `SkillDamageProvider`): a unit it strikes, alive and at or under the
    /// line at the skill's owner's level, is destroyed, unless its own shield
    /// has energy left and the technology does not ignore shields, or the
    /// owner's technologies are disabled. `FightActor.ReduceLife` takes its
    /// whole life as a suicide's, which neither a shield nor stealth stops and
    /// no reduction lessens, and `OnActorHitted` charges it with the line.
    /// The hit it makes names no damage provider, so its `damage` names
    /// neither a projectile nor a skill.
    fn cull(
        &mut self,
        hit: &DamageHit,
        target: FightActorRef,
        events: &mut Vec<Event>,
    ) -> Result<Option<Stroke>> {
        let (Some(source), Some(slot), FightActorRef::Unit(unit_id)) =
            (hit.source, hit.skill_slot, target)
        else {
            return Ok(None);
        };
        let Some(owner) = self
            .actors
            .get(&source.id)
            .filter(|_| source.kind == ObjectKind::Unit)
        else {
            return Ok(None);
        };
        let Some(line) = owner.placement.effects.dead_line else {
            return Ok(None);
        };
        if usize::from(slot) >= owner.skills.main_slots() || owner.technology_disabled() {
            return Ok(None);
        }
        let unit = &self.actors[&unit_id];
        // `PerformPreHitEffect` asks the shield's `isActive` and its energy,
        // not whether it is enabled: one switched off with energy left
        // still keeps the line off.
        let shielded = unit.shield.as_ref().is_some_and(|shield| shield.energy > 0);
        if (shielded && !line.ignores_shield) || !unit.alive() || unit.life > line.life {
            return Ok(None);
        }
        let unit = self
            .actors
            .get_mut(&unit_id)
            .expect("actor identity is stable");
        let life = unit.life;
        unit.life = 0;
        unit.last_damage_source = Some((hit.source, hit.source_team));
        let death = QVec3 {
            x: unit.x_q32,
            y: space_to_q32(unit_height(unit.domain)),
            z: unit.z_q32,
        };
        self.on_life_change(unit_id, events)?;
        self.on_actor_dead(unit_id);
        Ok(Some(Stroke {
            actual: life,
            dealt: life,
            taken: line.life,
            killed: true,
            reached_alive: true,
            death: Some(death),
            fallen: None,
        }))
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
            .then_some(owner.placement.effects.secondary_damage)
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
        let owner = hit.source.expect("a second damage has its unit").id;
        if self.actors[&owner].placement.effects.lifesteal.is_some() {
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
        // `PerformSecondaryRangeEffect`: `PrepareRangeTargets` takes what
        // the battlefield shields cover out, as a splash's targets, then
        // what the hit struck goes unless the row strikes it again
        // (`CanMainTargetBeHit`), and the shields its splash reaches take
        // the damage before any unit does (`PerformSecondarySplashShieldDamage`,
        // `PerformHitAdvancedEndergyShieldEffect`).
        let mut targets = self.damage_targets(&around)?;
        let shields = self.shields_in_the_way(&around, &mut targets);
        targets.retain(|target| secondary.hits_main_target || !struck.targets.contains(target));
        for shield in shields {
            self.hit_shield(shield, &around, events)?;
        }
        for target in targets {
            let amount = match target {
                _ if !secondary.buffed => amount,
                FightActorRef::Unit(id) => self.actors[&id].stats.damage_taken(amount)?,
                FightActorRef::Building(id) => self.construction_damage_taken(id, amount)?.0,
            };
            if amount < 1 {
                continue;
            }
            let strokes = self.strike(
                target,
                hit.source,
                hit.source_team,
                (amount, false),
                hit.provider,
                events,
            )?;
            for (reached, stroke) in strokes {
                self.count_hit(hit.source, hit.source_team, reached, &stroke)?;
                self.turned_unit_fell(reached, &stroke);
                if stroke.actual > 0 {
                    events.push(event(
                        hit.projectile,
                        hit.source,
                        Some(hit.source_team),
                        Some(reached.object_ref()),
                        EventPayload::Damage {
                            amount: i32::try_from(stroke.actual)
                                .map_err(|_| Error::new("damage exceeds i32"))?,
                            skill_slot: hit.skill_slot,
                        },
                    ));
                }
                let id = match reached {
                    FightActorRef::Unit(id) | FightActorRef::Building(id) => id,
                };
                if let Some(position) = stroke.death {
                    struck.deaths.push((id, position));
                    struck.ends.push((reached, position));
                }
                if let Some(position) = stroke.fallen {
                    struck.fallen.push((id, position));
                    struck.ends.push((reached, position));
                }
            }
        }
        Ok(())
    }

    /// `IDamageProvider.DispatchHitDamageEvent` after a hit: a unit's skill,
    /// struck directly (`SkillDamageProvider`) or through its projectile
    /// (`FightProjectile`), hands what the hit struck and the life it took in
    /// all to the skill's hit effects, `FightSkill.DispatchHitDamageEvent`:
    /// `LifeStealEffectProvider`'s, `TeamWreckageRecoveryManager`'s, a
    /// buff source's `BuffCycleController` and `KillExplosionEffectProvider`'s,
    /// in that order, which no recording holds two of. Answers the deaths and
    /// falls the hit effects dealt.
    /// A hit no unit's skill dealt — a turret's, a mine's, a battle skill's —
    /// reaches no unit's skill.
    pub(in crate::fight) fn dispatch_hit_damage(
        &mut self,
        hit: &DamageHit,
        targets: &[FightActorRef],
        damage: i64,
        events: &mut Vec<Event>,
    ) -> Result<Struck> {
        match (hit.source, hit.skill_slot) {
            (Some(owner), Some(slot)) if owner.kind == ObjectKind::Unit => {
                self.steal_life(owner.id, damage, events)?;
                self.record_wreckage_hit(owner.id, slot, targets);
                let center = (hit.center_q32.0, hit.center_y_q32, hit.center_q32.1);
                self.add_hit_buffs(owner.id, slot, (targets, center), events)?;
                let ends = self.explode_kills(owner.id, slot, targets, events)?;
                // `PerformMainSkillHitted`, when the hit is the main skill's.
                let skill = self.skill_at_slot(FightActorRef::Unit(owner.id), usize::from(slot));
                if skill.slot == SkillSlot::Main {
                    self.leave_main_fire(skill, targets, center)?;
                    if let Some(actor) = self.actors.get_mut(&owner.id) {
                        actor.reset_stacks_on_main_hit()?;
                    }
                }
                Ok(ends)
            }
            _ => Ok(Struck::default()),
        }
    }

    /// `FireIntensifyEffectProvider.PerformHitEffect`, a hit effect of the
    /// main skill alone (`SkillDataModifier.AvaliableCheck` on
    /// `mainSkillEffect`, the rows setting no `extraSkillEffect`): a first
    /// hit, not a second damage's, leaves the unit's fire
    /// (`GroundFireController.GetFireMech`) through `RangeItemSystem.AddItem`
    /// under the unit's side, at the point the hit landed. A source that
    /// `CanDisable` leaves none while the unit's technologies are disabled.
    ///
    /// The point is the hit's when it struck nothing, when the skill has no
    /// lock (and so no shield it fires at), or when its lock stands on the
    /// side of the first unit it struck. Otherwise the
    /// provider takes the point on the shield the skill fires at
    /// (`FightUtility.GetAttackPositionOnEnergyShield`), or the lock's own
    /// position for a skill that locks its target, which is not measured.
    fn leave_main_fire(
        &mut self,
        skill: SkillRef,
        targets: &[FightActorRef],
        center: (i64, i64, i64),
    ) -> Result<()> {
        let FightActorRef::Unit(owner) = skill.owner else {
            return Ok(());
        };
        let Some(actor) = self.actors.get(&owner) else {
            return Ok(());
        };
        let Some(fire) = actor.placement.effects.main_fire else {
            return Ok(());
        };
        if actor.technology_disabled() {
            return Ok(());
        }
        let team = actor.placement.team;
        if let Some(&first) = targets.first() {
            let side = |target| self.fight_actor(target).map(|view| view.team);
            let lock = self.skill(skill).lock_target;
            if lock.is_some_and(|lock| side(lock) != side(first)) {
                return Err(Error::new(format!(
                    "unit {owner}'s fire lands where its lock does not stand on the side \
                     of what its hit struck, which is not measured"
                )));
            }
        }
        self.add_terrain(team, &format!("unit {owner}"), fire, center)
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
        let Some(lifesteal) = owner.placement.effects.lifesteal else {
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
    /// hit was aimed at, when the hit strikes it, is its main shield. The main shield, and every other
    /// the splash reaches in the plane, take the hit, before any unit does.
    /// A hit that crosses shields takes nothing out and has no main shield.
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
            // A hit that crosses shields (`CanCrossAdvancedEnergyShield`)
            // leaves what they cover in it, and still strikes every shield its
            // splash reaches: a Rhino's Whirlwind beside an enemy shield
            // strikes the shield and the units in it alike.
            if !hit.crosses_shields {
                // Only what the hit strikes can name its main shield: a
                // Stormcaller's shell landing 38 metres short of the unit it
                // was fired at, inside a shield, leaves that shield alone.
                if main.is_none()
                    && hit
                        .aimed
                        .is_some_and(|aimed| targets.contains(&aimed) && covered(&aimed))
                {
                    main = Some(shield.id);
                }
                targets.retain(|target| !covered(target));
            }
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
        let shield = self.blow_shield(actor_id, target);
        let attacker = &self.actors[&actor_id];
        let (center_q32, center_y_q32) = if let Some(shield) = shield {
            self.shield_hit_center(skill_ref.owner, target, shield)
        } else if self_splash && splash_radius > 0 {
            (
                (attacker.x_q32, attacker.z_q32),
                space_to_q32(unit_height(attacker.domain)),
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
        // A blow is `SkillDamageProvider`'s, of the skill that struck.
        let hit = DamageHit {
            center_q32,
            center_y_q32,
            shield,
            splash_radius,
            shield_damage: self.skill_shield_damage(skill_ref),
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
        (skill_ref, skill_slot): (SkillRef, u16),
        target: FightActorRef,
        shield: u64,
        damage: i64,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let actor_id = skill_ref
            .owner
            .unit_id()
            .ok_or_else(|| Error::new("a construction's beam is not supported"))?;
        let splash_radius = self
            .skill_attacker(skill_ref)
            .ok_or_else(|| Error::new("beam owner is absent"))?
            .splash_radius;
        let (center_q32, center_y_q32) = self.shield_hit_center(skill_ref.owner, target, shield);
        let attacker = &self.actors[&actor_id];
        let hit = DamageHit {
            center_q32,
            center_y_q32,
            shield: Some(shield),
            crosses_shields: false,
            splash_radius,
            // `CalculateHitEnergyShieldDamage`: an extra skill's own damage
            // to shields, where its technology sets one.
            shield_damage: self.skill_shield_damage(skill_ref),
            ..DamageHit::of_skill(
                attacker,
                skill_slot,
                (target, self.domain_of(target)),
                damage,
            )
        };
        let struck = self.perform_damage(hit, events)?;
        self.record_ends(struck.ends, events);
        Ok(())
    }

    /// `SkillDamageProvider.CalculateDamagePosition` of a skill's hit for a
    /// shield, which it asks before anything else, a self splash included:
    /// where the way from its target out to its owner leaves the shield
    /// (`FightUtility.GetAttackPositionOnEnergyShieldOuter`). A splash
    /// strikes from there, the shield its main one: the units the shield
    /// covers are kept out, and what stands outside it within the splash is
    /// struck. A hit without a splash is the shield's alone, wherever it is.
    fn shield_hit_center(
        &self,
        owner: FightActorRef,
        target: FightActorRef,
        shield: u64,
    ) -> ((i64, i64), i64) {
        let inside = self.position_3d(target);
        let outside = self.position_3d(owner);
        let (x_q32, y_q32, z_q32) = match (inside, outside) {
            (Some(inside), Some(outside)) => self.shield_entry_point(shield, inside, outside),
            _ => (0, 0, 0),
        };
        ((x_q32, z_q32), y_q32)
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

    /// The same for the skill `skill_ref` holds, which asks with its own
    /// attack (`FightSkill.IsActorProtectedByEnergyShield`): an extra skill
    /// crosses shields, and reaches one, as its own row says.
    pub(in crate::fight) fn skill_blow_shield(
        &self,
        skill_ref: SkillRef,
        target: FightActorRef,
    ) -> Option<u64> {
        let attacker = self.skill_attacker(skill_ref)?;
        if attacker.attack.crosses_shields {
            return None;
        }
        let shield = self.target_energy_shield(skill_ref.owner, target, attacker.shield_range())?;
        (!self.shield_holds(shield, skill_ref.owner)).then_some(shield)
    }

    /// How high a target stands, `FPoint` raw metres.
    pub(in crate::fight) fn target_height_q32(&self, target: FightActorRef) -> i64 {
        match target {
            FightActorRef::Unit(id) => self
                .actors
                .get(&id)
                .map_or(0, |actor| space_to_q32(unit_height(actor.domain))),
            FightActorRef::Building(_) => 0,
        }
    }

    /// `DeadEffectSystem.OnActorDead` of a unit a hit killed: it queues the
    /// unit's dead effect for the module's update, an explosion exploding
    /// however its unit died once the unit has arrived
    /// (`FightExplosionSkill.EnterFight`, `OnTravelFinished`), and its
    /// `OnDead`, whose `BuffManager.OnMechDead` lets a buff it runs summon.
    fn on_actor_dead(&mut self, unit_id: u64) {
        self.dead_exits.push(unit_id);
        if !self.actors[&unit_id].travelling && self.explodes_on_death(unit_id) {
            self.dead_explosions.push((unit_id, false));
        }
        self.support.dying.push(unit_id);
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
    #[allow(clippy::too_many_lines)]
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
        if let Some(shield) = self.skill_blow_shield(skill_ref, target) {
            return self.beam_at_shield((skill_ref, skill_slot), target, shield, damage, events);
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
        // as no other hit does. A blow that deals nothing strikes nothing:
        // `DamagePerformer.PerformHitTargetEffect` returns before
        // `FightCalculator` when it deals under 1, so `ExpSystem.
        // OnActorHitted` never lists the beam's owner among the target's
        // attackers.
        if damage < 1 {
            return Ok(());
        }
        let strokes = self.strike(
            target,
            Some(attacker_ref),
            attacker_team,
            (damage, true),
            Provider::Skill {
                melee: self.actors[&actor_id].skill_melee(skill_slot),
            },
            events,
        )?;
        let (mut lost, mut fallen) = (0, Vec::new());
        for (reached, stroke) in strokes {
            self.count_hit(Some(attacker_ref), attacker_team, reached, &stroke)?;
            self.turned_unit_fell(reached, &stroke);
            if let Some(position) = stroke.death {
                events.push(event(
                    Some(reached.object_ref()),
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
                    Some(reached.object_ref()),
                    EventPayload::Damage {
                        amount: i32::try_from(stroke.actual)
                            .map_err(|_| Error::new("laser damage exceeds i32"))?,
                        skill_slot: Some(skill_slot),
                    },
                ));
            }
            lost += stroke.actual;
            fallen.extend(stroke.fallen.map(|position| (reached, position)));
        }
        // The beam is its skill's `SkillDamageProvider`, which hands what it
        // took to the skill's hit effects as any hit does.
        self.steal_life(actor_id, lost, events)?;
        // A building the beam fells is recorded at the end of the tick, as a
        // blow's and a projectile's are: in the tower-loss fight with two
        // lanes, the other lane's Steel Ball damages its tower between the
        // beam that fells the first and that tower's `building_destroyed`.
        // The tick's end moves it there, in its place among the deaths.
        for (reached, position) in fallen {
            events.push(event(
                Some(reached.object_ref()),
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
