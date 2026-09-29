use super::*;

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
    /// Where the splash is measured from, in space units.
    pub(in crate::fight) center: (i64, i64),
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
    pub(in crate::fight) splash_radius: i64,
    pub(in crate::fight) reach: Reach,
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
        let (center_x, center_z) = hit.center;
        if let Some(FightActorRef::Building(building_id)) = hit.aimed {
            let building = self
                .buildings
                .iter()
                .find(|building| building.building_id == building_id)
                .ok_or_else(|| Error::new("damage target building is absent"))?;
            let reached = magnitude(
                building_x(building).saturating_sub(center_x),
                building_z(building).saturating_sub(center_z),
            ) <= building_radius(building).saturating_add(hit.splash_radius);
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
                            && magnitude(
                                unit.x.saturating_sub(center_x),
                                unit.z.saturating_sub(center_z),
                            )
                            .saturating_sub(unit.rules.collision_radius())
                                <= hit.splash_radius
                    }
                    FightActorRef::Building(other_id) => self
                        .buildings
                        .iter()
                        .find(|other| other.building_id == other_id)
                        .is_some_and(|other| {
                            building_alive(other)
                                && other.targetable
                                && magnitude(
                                    building_x(other).saturating_sub(center_x),
                                    building_z(other).saturating_sub(center_z),
                                )
                                .saturating_sub(building_radius(other))
                                    <= hit.splash_radius
                        }),
                })
                .collect::<Vec<_>>();
            return Ok(struck);
        }
        // A shot at a unit takes what it was aimed at and, with a splash,
        // everything of the other side around it — buildings as well as units,
        // in the order the target trees hold them. An Arclight's shot at a
        // Crawler standing just in front of a wall reads the block between the
        // Crawlers around it, for the shot's full damage.
        let splash_reaches_buildings =
            hit.splash_radius > 0 && hit.reach.touches(UnitDomain::Ground);
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
                        && ((hit.hits_aimed && Some(*candidate_ref) == hit.aimed)
                            || (hit.splash_radius > 0
                                && magnitude(
                                    candidate.x.saturating_sub(center_x),
                                    candidate.z.saturating_sub(center_z),
                                )
                                .saturating_sub(candidate.rules.collision_radius())
                                    <= hit.splash_radius))
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
                                    && magnitude(
                                        building_x(building).saturating_sub(center_x),
                                        building_z(building).saturating_sub(center_z),
                                    )
                                    .saturating_sub(building_radius(building))
                                        <= hit.splash_radius
                            })
                }
            })
            .collect::<Vec<_>>();
        Ok(struck)
    }

    /// Takes one hit's damage off one target, unit or building alike.
    ///
    /// The one place this simulator takes life away. A unit remembers who hurt
    /// it and leaves the fight when it dies; a building stops being a target
    /// when it falls.
    pub(in crate::fight) fn strike(
        &mut self,
        target: FightActorRef,
        source: Option<ObjectRef>,
        source_team: u32,
        amount: i64,
    ) -> Result<Stroke> {
        match target {
            FightActorRef::Unit(unit_id) => {
                let unit = self
                    .actors
                    .get_mut(&unit_id)
                    .ok_or_else(|| Error::new("damage target unit is absent"))?;
                // `PerformHitTargetEffect` scales the hit by the unit's rate on
                // damage taken before it takes any life, and counts it as taken
                // with the rate's increases alone.
                let taken = unit.stats.damage_taken_raised(amount)?;
                let amount = unit.stats.damage_taken(amount)?;
                let previous_life = unit.life;
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
                let (amount, taken) = self.construction_damage_taken(building_id, amount)?;
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
                    self.fallen_towers.push(building_id);
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
        let mut struck = Struck::default();
        // `PerformSingleEffect` on a shield: it takes the hit, and no unit
        // does.
        if let Some(shield) = hit.shield
            && hit.splash_radius == 0
        {
            self.hit_shield(shield, &hit, events)?;
            return Ok(struck);
        }
        let mut targets = self.damage_targets(&hit)?;
        if hit.splash_radius > 0 && !hit.crosses_shields {
            for shield in self.shields_in_the_way(&hit, &mut targets) {
                self.hit_shield(shield, &hit, events)?;
            }
        }
        for target in targets {
            let stroke = self.strike(target, hit.source, hit.source_team, hit.amount)?;
            self.count_hit(hit.source, hit.source_team, target, &stroke)?;
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
        Ok(struck)
    }

    /// `DamagePerformer.ProcessAdvancedEnergyShieldEffect` and the shields
    /// `PerformRangeEffect` then strikes. Every shield of the sides the hit
    /// strikes that does not hold the point it lands at takes its side's
    /// units it covers out of the hit; the one covering what the hit was
    /// aimed at is its main shield. The main shield, and every other the
    /// splash reaches in the plane, take the hit, before any unit does.
    fn shields_in_the_way(&self, hit: &DamageHit, targets: &mut Vec<FightActorRef>) -> Vec<u64> {
        let (x_q32, z_q32) = (space_to_q32(hit.center.0), space_to_q32(hit.center.1));
        let mut main = hit.shield;
        let mut listed = Vec::new();
        for shield in &self.shields {
            if !hit.effect.strikes(hit.team, shield.team)
                || shield.contains(x_q32, hit.center_y_q32, z_q32)
            {
                continue;
            }
            let covered = |target: &FightActorRef| {
                matches!(target, FightActorRef::Unit(id)
                    if self.actors[id].placement.team == shield.team)
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
        let attacker = &self.actors[&actor_id];
        let center = match target {
            FightActorRef::Unit(target_id) => {
                let aimed = &self.actors[&target_id];
                (aimed.x, aimed.z)
            }
            FightActorRef::Building(building_id) => self
                .buildings
                .iter()
                .find(|building| building.building_id == building_id)
                .map(|building| (building_x(building), building_z(building)))
                .ok_or_else(|| Error::new("direct attack target is absent"))?,
        };
        let shield = self.blow_shield(actor_id, target);
        if shield.is_some() && attacker.rules.attack.splash_radius() > 0 {
            return Err(Error::new(
                "a splashing blow at a unit its side's shield covers is not measured",
            ));
        }
        let hit = DamageHit {
            source: Some(attacker.object_ref()),
            source_team: attacker.placement.team,
            team: attacker.placement.team,
            effect: EffectTarget::Opponent,
            amount: attacker.stats.attack_damage(),
            // A blow is `SkillDamageProvider`'s, of the skill that struck.
            projectile: None,
            skill_slot: Some(u16::try_from(skill_slot).expect("skill slot fits u16")),
            aimed: Some(target),
            hits_aimed: true,
            center,
            center_y_q32: self.target_height_q32(target),
            shield,
            crosses_shields: attacker.rules.attack.crosses_shields,
            splash_radius: attacker.rules.attack.splash_radius(),
            reach: Reach::Targets(attacker.rules.attack.targets),
        };
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
    fn beam_at_shield(
        &mut self,
        actor_id: u64,
        target: FightActorRef,
        shield: u64,
        damage: i64,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let attacker = &self.actors[&actor_id];
        if attacker.rules.attack.splash_radius() > 0 {
            return Err(Error::new(
                "a splashing beam at a unit its side's shield covers is not measured",
            ));
        }
        let hit = DamageHit {
            source: Some(attacker.object_ref()),
            source_team: attacker.placement.team,
            team: attacker.placement.team,
            effect: EffectTarget::Opponent,
            amount: damage,
            projectile: None,
            skill_slot: Some(0),
            aimed: Some(target),
            hits_aimed: true,
            center: (attacker.x, attacker.z),
            center_y_q32: 0,
            shield: Some(shield),
            crosses_shields: false,
            splash_radius: 0,
            reach: Reach::Targets(attacker.rules.attack.targets),
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

    pub(in crate::fight) fn laser_effect(
        &mut self,
        actor_id: u64,
        target: FightActorRef,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let (damage, attacker_ref, attacker_team) = {
            let attacker = &self.actors[&actor_id];
            (
                attacker.stats.laser_damage(
                    &attacker.rules,
                    usize::try_from(attacker.skill.attack_count).unwrap_or(0),
                ),
                attacker.object_ref(),
                attacker.placement.team,
            )
        };
        // A beam that splashes strikes as any other hit does, what it was
        // aimed at and everything of the other side around it, in the order
        // the target trees hold them: a Melting Point's beam at one Crawler
        // reads the Crawler beside it first.
        let splash_radius = self.actors[&actor_id].rules.attack.splash_radius();
        // A beam at a unit its side's shield covers strikes the shield, as a
        // blow does: `DamageEffect.Perform`.
        if let Some(shield) = self.blow_shield(actor_id, target) {
            return self.beam_at_shield(actor_id, target, shield, damage, events);
        }
        if splash_radius > 0 {
            let attacker = &self.actors[&actor_id];
            let center = match target {
                FightActorRef::Unit(target_id) => {
                    let aimed = &self.actors[&target_id];
                    (aimed.x, aimed.z)
                }
                FightActorRef::Building(building_id) => self
                    .buildings
                    .iter()
                    .find(|building| building.building_id == building_id)
                    .map(|building| (building_x(building), building_z(building)))
                    .ok_or_else(|| Error::new("laser target is absent"))?,
            };
            let hit = DamageHit {
                source: Some(attacker_ref),
                source_team: attacker_team,
                team: attacker_team,
                effect: EffectTarget::Opponent,
                amount: damage,
                projectile: None,
                skill_slot: Some(0),
                aimed: Some(target),
                hits_aimed: true,
                center,
                center_y_q32: self.target_height_q32(target),
                shield: None,
                crosses_shields: attacker.rules.attack.crosses_shields,
                splash_radius,
                reach: Reach::Targets(attacker.rules.attack.targets),
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
        let stroke = self.strike(target, Some(attacker_ref), attacker_team, damage)?;
        self.count_hit(Some(attacker_ref), attacker_team, target, &stroke)?;
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
                    skill_slot: Some(0),
                },
            ));
        }
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
