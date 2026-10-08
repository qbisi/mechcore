use super::*;

#[derive(Debug, Clone)]
pub(in crate::fight) struct Projectile {
    pub(in crate::fight) id: u64,
    pub(in crate::fight) team: u32,
    /// Who released it.
    pub(in crate::fight) shooter: Shooter,
    /// The owner's skill that released it.
    pub(in crate::fight) skill_slot: u16,
    pub(in crate::fight) target_kind: ObjectKind,
    pub(in crate::fight) target: u64,
    pub(in crate::fight) x: i64,
    pub(in crate::fight) y: i64,
    pub(in crate::fight) z: i64,
    pub(in crate::fight) x_q32: i64,
    pub(in crate::fight) y_q32: i64,
    pub(in crate::fight) z_q32: i64,
    pub(in crate::fight) cached_target_x: i64,
    pub(in crate::fight) cached_target_y: i64,
    pub(in crate::fight) cached_target_z: i64,
    pub(in crate::fight) cached_target_x_q32: i64,
    pub(in crate::fight) cached_target_y_q32: i64,
    pub(in crate::fight) cached_target_z_q32: i64,
    pub(in crate::fight) cached_target_radius: i64,
    pub(in crate::fight) speed: i64,
    /// What is left of its life, which only an interceptor takes.
    pub(in crate::fight) life: i64,
    pub(in crate::fight) max_life: i64,
    pub(in crate::fight) interceptible: bool,
    /// The interceptors whose reach it is in, as it last moved.
    pub(in crate::fight) sources: Vec<u64>,
    /// The interceptors locked on it.
    pub(in crate::fight) locked_by: Vec<u64>,
    pub(in crate::fight) lock_target: bool,
    /// Where a projectile that follows its target lands relative to it.
    pub(in crate::fight) offset_x_q32: i64,
    pub(in crate::fight) offset_z_q32: i64,
    /// The height a projectile still climbs to before it flies at its
    /// target, if it has not reached it.
    pub(in crate::fight) climb_to_q32: Option<i64>,
    /// `ProjectileController.inEnergyShields`: the enemy shields that held it
    /// as it was made, which it passes through for the rest of its flight.
    pub(in crate::fight) spawn_shields: Vec<u64>,
    /// The shield that took it, once one has.
    pub(in crate::fight) absorbed_by: Option<u64>,
    /// `FightProjectile.moveDirection`: how far and which way its last move
    /// that went anywhere took it; zero before it has moved.
    pub(in crate::fight) move_direction: (i64, i64, i64),
    /// `FightProjectile.moveRange`: its data source's attack range and its
    /// target's radius, set as it is made (`FightProjectile.Init`). The data
    /// source is the releasing skill, or a missile's row, whose range is its
    /// trigger range.
    pub(in crate::fight) move_range_q32: i64,
}

/// What released a projectile.
#[derive(Debug, Clone)]
pub(in crate::fight) enum Shooter {
    /// A unit, or a construction whose skill fires.
    Actor(FightActorRef),
    /// A missile, `FightLandMine`: nothing owns the projectile, and its hit is
    /// the missile's own.
    Missile(MissileShot),
}

impl Shooter {
    /// The unit or construction that released it, if one did.
    pub(in crate::fight) const fn actor(&self) -> Option<FightActorRef> {
        match self {
            Self::Actor(owner) => Some(*owner),
            Self::Missile(_) => None,
        }
    }
}

impl Projectile {
    pub(in crate::fight) fn object_ref(&self) -> ObjectRef {
        ObjectRef::new(ObjectKind::Projectile, self.id)
    }

    pub(in crate::fight) fn snapshot(&self) -> ProjectileState {
        ProjectileState {
            projectile_id: self.id,
            team_id: self.team,
            owner: self.shooter.actor().map(FightActorRef::object_ref),
            position: QVec3 {
                x: self.x_q32,
                y: self.y_q32,
                z: self.z_q32,
            },
            target: Some(ObjectRef::new(self.target_kind, self.target)),
            cached_target_position: QVec3 {
                x: self.cached_target_x_q32,
                y: self.cached_target_y_q32,
                z: self.cached_target_z_q32,
            },
            cached_target_radius: space_to_q32(self.cached_target_radius),
            move_range: self.move_range_q32,
            life: GaugeI32 {
                current: i32::try_from(self.life).expect("projectile life fits i32"),
                maximum: i32::try_from(self.max_life).expect("projectile life fits i32"),
            },
            spawn_containing_shields: self
                .spawn_shields
                .iter()
                .map(|&id| ObjectRef::new(ObjectKind::Shield, id))
                .collect(),
        }
    }
}

/// How far a projectile travels in one update.
fn projectile_step_q32(projectile: &Projectile) -> i64 {
    q32_mul(
        space_to_q32(projectile.speed),
        Q32_ONE.saturating_mul(LOGIC_TICK_TIME_UNITS.cast_signed())
            / TIME_UNITS_PER_SECOND.cast_signed(),
    )
}

impl Simulation {
    #[allow(clippy::similar_names)] // Paired fixed-point x/z components are intentionally parallel.
    #[allow(clippy::too_many_lines)]
    pub(in crate::fight) fn step_projectiles(&mut self, events: &mut Vec<Event>) -> Result<()> {
        let mut retained = Vec::with_capacity(self.projectiles.len());
        // The build's ProjectileSystem keeps registration order in its List,
        // but Update walks that list from Count - 1 down to zero. Preserve the
        // list order after this reverse update pass so later ticks use the
        // same stable registration sequence.
        for mut projectile in std::mem::take(&mut self.projectiles).into_iter().rev() {
            // A projectile that climbs first rises straight up at its speed,
            // neither following its target nor landing, until it stands at
            // its height or above: a Farseer's shot climbs 7 metres a tick to
            // 63 for its 60, and flies at the Rhino from the tick after.
            if let Some(height_q32) = projectile.climb_to_q32 {
                let dy_q32 = height_q32.saturating_sub(projectile.y_q32);
                let distance_q32 = native_q32_magnitude_3d(0, dy_q32, 0);
                let move_q32 = projectile_step_q32(&projectile);
                if distance_q32 > 0 {
                    let reciprocal = q32_div(Q32_ONE, distance_q32);
                    let climbed = q32_mul(q32_mul(dy_q32, reciprocal), move_q32);
                    projectile.y_q32 = projectile.y_q32.saturating_add(climbed);
                    if climbed != 0 {
                        projectile.move_direction = (0, climbed, 0);
                    }
                }
                projectile.y = q32_to_space_rounded(projectile.y_q32);
                if projectile.y_q32 >= height_q32 {
                    projectile.climb_to_q32 = None;
                }
                // Climbing, it is not yet among any interceptor's: the
                // Farseer's shot of `farseer-climbs.yaml` takes no hit until
                // it flies.
                retained.push(projectile);
                continue;
            }
            if projectile.target_kind == ObjectKind::Unit
                && projectile.lock_target
                && let Some(target) = self
                    .actors
                    .get(&projectile.target)
                    .filter(|actor| actor.alive())
            {
                projectile.cached_target_x_q32 =
                    target.x_q32.saturating_add(projectile.offset_x_q32);
                projectile.cached_target_z_q32 =
                    target.z_q32.saturating_add(projectile.offset_z_q32);
                projectile.cached_target_x = q32_to_space_rounded(projectile.cached_target_x_q32);
                projectile.cached_target_y = unit_height(target.rules.domain);
                projectile.cached_target_z = q32_to_space_rounded(projectile.cached_target_z_q32);
                projectile.cached_target_y_q32 = space_to_q32(projectile.cached_target_y);
                projectile.cached_target_radius = target.rules.collision_radius();
            }
            if !self.within_owner_reach(&projectile) {
                self.leave_interceptors(&projectile);
                events.push(spent_on_nothing(&projectile));
                continue;
            }
            let dx_q32 = projectile
                .cached_target_x_q32
                .saturating_sub(projectile.x_q32);
            let dz_q32 = projectile
                .cached_target_z_q32
                .saturating_sub(projectile.z_q32);
            let dy_q32 = projectile
                .cached_target_y_q32
                .saturating_sub(projectile.y_q32);
            let distance_q32 = native_q32_magnitude_3d(dx_q32, dy_q32, dz_q32);
            if distance_q32 < space_to_q32(projectile.cached_target_radius) {
                self.leave_interceptors(&projectile);
                // `CheckIsHitEnergyShield` on arrival: an enemy shield that
                // holds it now and did not as it was made takes it even
                // though it did not move, at the point a step back along its
                // way meets the shield's surface. A Shield Airdrop landing
                // over a missile's landing point on that tick takes the
                // missile.
                self.absorb_on_arrival(&mut projectile, distance_q32);
                self.impact(&projectile, events)?;
            } else {
                let (last_x_q32, last_y_q32, last_z_q32) =
                    (projectile.x_q32, projectile.y_q32, projectile.z_q32);
                let step_q32 = projectile_step_q32(&projectile);
                if distance_q32 > 0 {
                    let move_q32 = step_q32.min(distance_q32);
                    let reciprocal = q32_div(Q32_ONE, distance_q32);
                    projectile.x_q32 = projectile
                        .x_q32
                        .saturating_add(q32_mul(q32_mul(dx_q32, reciprocal), move_q32));
                    projectile.y_q32 = projectile
                        .y_q32
                        .saturating_add(q32_mul(q32_mul(dy_q32, reciprocal), move_q32));
                    projectile.z_q32 = projectile
                        .z_q32
                        .saturating_add(q32_mul(q32_mul(dz_q32, reciprocal), move_q32));
                }
                // `FightProjectile.Move` keeps the way it went as its
                // `moveDirection`, when it went anywhere.
                let moved = (
                    projectile.x_q32.wrapping_sub(last_x_q32),
                    projectile.y_q32.wrapping_sub(last_y_q32),
                    projectile.z_q32.wrapping_sub(last_z_q32),
                );
                if moved != (0, 0, 0) {
                    projectile.move_direction = moved;
                }
                projectile.x = q32_to_space_rounded(projectile.x_q32);
                projectile.y = q32_to_space_rounded(projectile.y_q32);
                projectile.z = q32_to_space_rounded(projectile.z_q32);
                // `CheckIsHitEnergyShield`: where `Move` has put it, the first
                // enemy shield that holds it and did not as it was made takes
                // it, at the point it crossed the shield's surface.
                if let Some(shield) = self.absorbing_shield(&projectile) {
                    let (x_q32, y_q32, z_q32) = self.shield_entry_point(
                        shield,
                        (projectile.x_q32, projectile.y_q32, projectile.z_q32),
                        (last_x_q32, last_y_q32, last_z_q32),
                    );
                    projectile.x_q32 = x_q32;
                    projectile.y_q32 = y_q32;
                    projectile.z_q32 = z_q32;
                    projectile.x = q32_to_space_rounded(x_q32);
                    projectile.y = q32_to_space_rounded(y_q32);
                    projectile.z = q32_to_space_rounded(z_q32);
                    projectile.absorbed_by = Some(shield);
                    self.leave_interceptors(&projectile);
                    self.impact(&projectile, events)?;
                    continue;
                }
                // `Move` has put it where it is now, and
                // `UpdateIsInInterceptSources` reads it there.
                self.track_interceptors(&mut projectile);
                retained.push(projectile);
            }
        }
        retained.reverse();
        self.projectiles = retained;
        Ok(())
    }

    /// `CheckIsHitEnergyShield` as a projectile arrives: the shield that
    /// holds it, if any, takes it where a step back along its way meets the
    /// shield's surface.
    fn absorb_on_arrival(&self, projectile: &mut Projectile, distance_q32: i64) {
        let Some(shield) = self.absorbing_shield(projectile) else {
            return;
        };
        let inside = (projectile.x_q32, projectile.y_q32, projectile.z_q32);
        let outside = self.arrival_outside_point(projectile, distance_q32);
        let (x_q32, y_q32, z_q32) = self.shield_entry_point(shield, inside, outside);
        projectile.x_q32 = x_q32;
        projectile.y_q32 = y_q32;
        projectile.z_q32 = z_q32;
        projectile.x = q32_to_space_rounded(x_q32);
        projectile.y = q32_to_space_rounded(y_q32);
        projectile.z = q32_to_space_rounded(z_q32);
        projectile.absorbed_by = Some(shield);
    }

    /// `CheckIsHitEnergyShield`'s `lastPosition` for a projectile that has
    /// not moved this update: a step back along its `moveDirection`, the
    /// step its `maxMoveDistancePerCount` or the distance left to its target
    /// when that is less and not zero; where its owner stands, at its own
    /// height, when it has no direction, and where it is for a missile's,
    /// which no one owns.
    fn arrival_outside_point(&self, projectile: &Projectile, distance_q32: i64) -> (i64, i64, i64) {
        let (x, y, z) = (projectile.x_q32, projectile.y_q32, projectile.z_q32);
        if projectile.move_direction == (0, 0, 0) {
            return match projectile.shooter {
                Shooter::Actor(owner) => self
                    .fight_actor(owner)
                    .map_or((x, y, z), |view| (view.x_q32, y, view.z_q32)),
                Shooter::Missile(_) => (x, y, z),
            };
        }
        let step_q32 = projectile_step_q32(projectile);
        let step_q32 = if distance_q32 == 0 {
            step_q32
        } else {
            step_q32.min(distance_q32)
        };
        let (dx, dy, dz) = projectile.move_direction;
        let back = super::shield::scale(super::shield::normalized((-dx, -dy, -dz)), step_q32);
        (
            x.saturating_add(back.0),
            y.saturating_add(back.1),
            z.saturating_add(back.2),
        )
    }

    /// `FightProjectile.Update`'s `IsInRange3D`: a projectile that locks its
    /// target lands only while it stands within `CalculateMaxMoveDistance` of
    /// its owner, edge to edge in three dimensions: the owner's radius and the
    /// projectile's `moveRange`, taken across the height between the two when
    /// the owner and its target fly at different heights. The owner is asked
    /// where it stood last, dead or alive. A Mustang's shot at a Crawler
    /// running away is spent on nothing once it is farther from the Mustang
    /// than that; a Stormcaller's shells, which lock nothing, are not asked.
    fn within_owner_reach(&self, projectile: &Projectile) -> bool {
        let (Shooter::Actor(owner), move_range_q32, true) = (
            &projectile.shooter,
            projectile.move_range_q32,
            projectile.lock_target,
        ) else {
            return true;
        };
        let Some(view) = self.fight_actor(*owner) else {
            return true;
        };
        let owner_height = match owner {
            FightActorRef::Unit(id) => unit_height(self.actors[id].rules.domain),
            FightActorRef::Building(_) => 0,
        };
        let target_height = q32_to_space_rounded(projectile.cached_target_y_q32);
        let radius_q32 = space_to_q32(view.radius);
        let reach_q32 = radius_q32.saturating_add(move_range_q32);
        let rise_q32 = space_to_q32(target_height.saturating_sub(owner_height));
        let reach_q32 = if rise_q32 == 0 {
            reach_q32
        } else {
            fpcs_sqrt_fastest(
                q32_mul(reach_q32, reach_q32).saturating_add(q32_mul(rise_q32, rise_q32)),
            )
        };
        native_q32_magnitude_3d(
            projectile.x_q32.saturating_sub(view.x_q32),
            projectile.y_q32.saturating_sub(space_to_q32(owner_height)),
            projectile.z_q32.saturating_sub(view.z_q32),
        )
        .saturating_sub(radius_q32)
            <= reach_q32
    }

    #[allow(clippy::too_many_lines)]
    pub(in crate::fight) fn impact(
        &mut self,
        projectile: &Projectile,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let (aimed, reach) = if projectile.target_kind == ObjectKind::Building {
            // A building stands on the ground, and a projectile narrows a
            // dual-domain skill to its target's domain.
            (
                FightActorRef::Building(projectile.target),
                Reach::Domain(UnitDomain::Ground),
            )
        } else {
            let target_domain = self
                .actors
                .get(&projectile.target)
                .ok_or_else(|| Error::new("projectile target is absent"))?
                .rules
                .domain;
            (
                FightActorRef::Unit(projectile.target),
                Reach::Domain(target_domain),
            )
        };
        let (struck, source) = match &projectile.shooter {
            Shooter::Actor(owner) => (
                self.actor_hit(
                    projectile,
                    self.skill_at_slot(*owner, usize::from(projectile.skill_slot)),
                    aimed,
                    reach,
                    events,
                )?,
                // Its removal names the side the owner stands on as it lands,
                // as the recording last saw it: an owner already dead is
                // named on the side it fired from.
                Some((
                    owner.object_ref(),
                    self.fight_actor(*owner)
                        .filter(|owner| owner.alive)
                        .map_or(projectile.team, |owner| owner.team),
                )),
            ),
            Shooter::Missile(shot) => (
                self.missile_hit(projectile, shot, aimed, reach, events)?,
                None,
            ),
        };
        events.push(event(
            Some(projectile.object_ref()),
            source.map(|(source, _)| source),
            source.map(|(_, team)| team),
            Some(aimed.object_ref()),
            EventPayload::ProjectileRemoved {
                position: QVec3 {
                    x: projectile.x_q32,
                    y: projectile.y_q32,
                    z: projectile.z_q32,
                },
                intercepted: false,
                absorbed_by: struck
                    .shield
                    .map(|shield| ObjectRef::new(ObjectKind::Shield, shield)),
            },
        ));
        // Deaths and falls wait for every shot the tick resolves, and then
        // come in the order the shots struck them: an Arclight's splash that
        // kills nine Crawlers and fells a block between them reads the block's
        // fall between their deaths, and a tick that lands two shots on a
        // wall reads both removals before the block falls.
        for (target, position) in struck.ends {
            match target {
                FightActorRef::Unit(dead_id) => {
                    let mut death = Vec::new();
                    self.record_deaths(vec![(dead_id, position)], &mut death);
                    self.fallen_buildings.extend(death);
                }
                FightActorRef::Building(building_id) => self.fallen_buildings.push(event(
                    Some(ObjectRef::new(ObjectKind::Building, building_id)),
                    None,
                    None,
                    None,
                    EventPayload::BuildingDestroyed { position },
                )),
            }
        }
        Ok(())
    }

    /// A unit's or a construction's projectile landing.
    fn actor_hit(
        &mut self,
        projectile: &Projectile,
        skill_ref: SkillRef,
        aimed: FightActorRef,
        reach: Reach,
        events: &mut Vec<Event>,
    ) -> Result<super::damage::Struck> {
        let splash_radius = self
            .skill_attacker(skill_ref)
            .map(|attacker| attacker.splash_radius)
            .ok_or_else(|| Error::new("projectile owner is absent"))?;
        // A projectile carries no damage of its own: it takes its owner's as
        // the owner has it when it lands. The Fangs of the two-tower fight
        // whose debuff ends, or who die, while a shot is in the air land it
        // for the full 63.
        let amount = self
            .skill_attacker(skill_ref)
            .ok_or_else(|| Error::new("projectile owner is absent"))?
            .attack_damage;
        let crosses_shields = self
            .skill_attacker(skill_ref)
            .is_some_and(|attacker| attacker.attack.crosses_shields);
        let fire = self
            .skill_attacker(skill_ref)
            .is_some_and(|attacker| attacker.attack.fire_damage);
        // A shot that took no shield on its way and lands without a splash
        // is still taken by the shield of its target's side that covers the
        // target, and neither is when the one who fired it stands inside that
        // shield: `DamagePerformer.Perform` then strikes the target.
        let mut shield = projectile.absorbed_by;
        if shield.is_none() && splash_radius == 0 && !crosses_shields {
            shield = self.shield_around(aimed);
        }
        if splash_radius == 0
            && let Some(covering) = shield
            && self.shield_holds(covering, skill_ref.owner)
        {
            shield = None;
        }
        // A unit's projectile is owned by the unit, and strikes for the side
        // the unit was on as it released it: `ProjectileSystem.Create` hands
        // the projectile's controller that side (`ProjectileController.Init`),
        // which a beam turning the unit while the shot flies does not change.
        let hit = DamageHit {
            source: Some(skill_ref.owner.object_ref()),
            skill_slot: Some(projectile.skill_slot),
            shield,
            crosses_shields,
            splash_radius,
            fire,
            shield_damage: self.skill_shield_damage(skill_ref),
            ..DamageHit::of_projectile(projectile, aimed, amount, reach)
        };
        // A projectile in simulated motion (`isSimulateMode`) that lands on a
        // target already dead does nothing, splash included: a Fire Badger's
        // or a Typhoon's shot at a Crawler another shot killed while it flew
        // leaves the Crawlers around it untouched, and so does a Fire
        // Badger's at a wall block that fell. Any other projectile still
        // strikes where it lands, as an Arclight's does.
        let simulated = self.skill_attacker(skill_ref).is_some_and(|attacker| {
            matches!(
                attacker.attack.path,
                crate::rules::AttackPath::Projectile {
                    simulated_motion: true,
                    ..
                }
            )
        });
        let target = match projectile.target_kind {
            ObjectKind::Building => FightActorRef::Building(projectile.target),
            _ => FightActorRef::Unit(projectile.target),
        };
        let lands_on_nothing = simulated
            && !self
                .fight_actor(target)
                .is_some_and(|view| view.alive && view.visible);
        let center = (hit.center_q32.0, hit.center_y_q32, hit.center_q32.1);
        let struck = if lands_on_nothing {
            super::damage::Struck::default()
        } else {
            let struck = self.perform_damage(hit, events)?;
            self.extra_hit_effect(skill_ref, &struck.targets, center, events)?;
            struck
        };
        Ok(struck)
    }

    /// `ExtraSkillProvider.PerformHitEffect`: an extra skill whose row names
    /// a buff writes it on every unit the hit struck, from the skill's unit
    /// (`BuffSystem.AddBuff`), and one whose row leaves a terrain leaves it
    /// where the hit lands, of its side, through `RangeItemSystem.AddItem`: a
    /// fire is the unit's own (`GroundFireController.GetFireMech`), any other
    /// the technology's.
    pub(in crate::fight) fn extra_hit_effect(
        &mut self,
        skill_ref: SkillRef,
        struck: &[FightActorRef],
        center: (i64, i64, i64),
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let (FightActorRef::Unit(id), SkillSlot::Extra(index)) = (skill_ref.owner, skill_ref.slot)
        else {
            return Ok(());
        };
        let Some(actor) = self.actors.get(&id) else {
            return Ok(());
        };
        let team = actor.placement.team;
        let extra = &actor.skills.extras[index];
        let (buff, terrain) = (extra.buff, extra.terrain);
        let name = format!("unit {id}");
        if let Some(buff) = buff {
            let units = struck
                .iter()
                .filter_map(|target| target.unit_id())
                .collect::<Vec<_>>();
            self.write_skill_buff(
                (&name, team),
                Some(ObjectRef::new(ObjectKind::Unit, id)),
                &buff,
                &units,
                events,
            )?;
        }
        if let Some(terrain) = terrain {
            self.add_terrain(team, &name, terrain, center)?;
        }
        Ok(())
    }
}

/// `ProjectileController.Release` with no damage performed.
fn spent_on_nothing(projectile: &Projectile) -> Event {
    let source = projectile.shooter.actor().map(FightActorRef::object_ref);
    event(
        Some(projectile.object_ref()),
        source,
        source.map(|_| projectile.team),
        Some(ObjectRef::new(projectile.target_kind, projectile.target)),
        EventPayload::ProjectileRemoved {
            position: QVec3 {
                x: projectile.x_q32,
                y: projectile.y_q32,
                z: projectile.z_q32,
            },
            intercepted: false,
            absorbed_by: None,
        },
    )
}
