use super::*;

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
        if let FightActorRef::Building(building_id) = hit.aimed {
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
                return Ok(if reached { vec![hit.aimed] } else { Vec::new() });
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
                .filter(|(team, _)| *team != hit.team)
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
            .filter(|(team, _)| *team != hit.team)
            .flat_map(|(_, candidates)| candidates)
            .filter(|candidate_ref| match *candidate_ref {
                FightActorRef::Unit(candidate_id) => {
                    let candidate = &self.actors[&candidate_id];
                    candidate.alive()
                        && hit.reach.touches(candidate.rules.domain)
                        && ((hit.hits_aimed && *candidate_ref == hit.aimed)
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
        source: ObjectRef,
        source_team: u32,
        amount: i64,
    ) -> Result<Stroke> {
        match target {
            FightActorRef::Unit(unit_id) => {
                let unit = self
                    .actors
                    .get_mut(&unit_id)
                    .ok_or_else(|| Error::new("damage target unit is absent"))?;
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
                    death,
                    fallen: None,
                })
            }
            FightActorRef::Building(building_id) => {
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
                Ok(Stroke {
                    actual: i64::from(previous_life - building.life.current),
                    death: None,
                    fallen: destroyed.then_some(building.position),
                })
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
        for target in self.damage_targets(&hit)? {
            let stroke = self.strike(target, hit.source, hit.source_team, hit.amount)?;
            if stroke.actual > 0 {
                events.push(event(
                    None,
                    Some(hit.source),
                    Some(hit.source_team),
                    Some(target.object_ref()),
                    EventPayload::Damage {
                        amount: i32::try_from(stroke.actual)
                            .map_err(|_| Error::new("damage exceeds i32"))?,
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

    /// Records the units a hit killed, each credited to whoever last hurt it.
    pub(in crate::fight) fn record_deaths(
        &self,
        deaths: Vec<(u64, QVec3)>,
        events: &mut Vec<Event>,
    ) {
        for (dead_id, position) in deaths {
            let (source, source_team_id) = self.actors[&dead_id]
                .last_damage_source
                .map_or((None, None), |(source, team_id)| {
                    (Some(source), Some(team_id))
                });
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
        let hit = DamageHit {
            source: attacker.object_ref(),
            source_team: attacker.placement.team,
            team: attacker.placement.team,
            amount: attacker.stats.attack_damage(),
            aimed: target,
            hits_aimed: true,
            center,
            splash_radius: attacker.rules.attack.splash_radius(),
            reach: Reach::Targets(attacker.rules.attack.targets),
        };
        let struck = self.perform_damage(hit, events)?;
        self.record_deaths(struck.deaths, events);
        // A block a blow fells falls after every hit the tick resolves, as a
        // shot's does: the Crawlers of `wall-block.yaml` read three more blows
        // between the one that fells block 5 and `building_destroyed`.
        for (building_id, position) in struck.fallen {
            self.fallen_buildings.push(event(
                Some(ObjectRef::new(ObjectKind::Building, building_id)),
                None,
                None,
                None,
                EventPayload::BuildingDestroyed { position },
            ));
        }
        Ok(())
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
                attacker
                    .rules
                    .attack
                    .laser_damage(attacker.laser_attack_count),
                attacker.object_ref(),
                attacker.placement.team,
            )
        };
        self.actors
            .get_mut(&actor_id)
            .expect("actor identity is stable")
            .laser_attack_count += 1;
        // A laser strikes one target and has no splash, so it takes the
        // stroke without the range step. A unit it kills is recorded dead
        // before the damage, and a block it fells falls after it: the Steel
        // Balls of `wall-laser.yaml` read `damage` and then
        // `building_destroyed`. Damage is recorded even when none was dealt.
        let stroke = self.strike(target, attacker_ref, attacker_team, damage)?;
        if let Some(position) = stroke.death {
            events.push(event(
                Some(target.object_ref()),
                Some(attacker_ref),
                Some(attacker_team),
                None,
                EventPayload::UnitDied { position },
            ));
        }
        events.push(event(
            None,
            Some(attacker_ref),
            Some(attacker_team),
            Some(target.object_ref()),
            EventPayload::Damage {
                amount: i32::try_from(stroke.actual)
                    .map_err(|_| Error::new("laser damage exceeds i32"))?,
            },
        ));
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
