use super::*;

#[derive(Debug, Clone)]
pub(in crate::fight) struct Projectile {
    pub(in crate::fight) id: u64,
    pub(in crate::fight) team: u32,
    pub(in crate::fight) owner: u64,
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
    pub(in crate::fight) damage: i64,
    pub(in crate::fight) life: i64,
    pub(in crate::fight) lock_target: bool,
}

impl Projectile {
    pub(in crate::fight) fn object_ref(&self) -> ObjectRef {
        ObjectRef::new(ObjectKind::Projectile, self.id)
    }

    pub(in crate::fight) fn snapshot(&self) -> ProjectileState {
        ProjectileState {
            projectile_id: self.id,
            team_id: self.team,
            owner: Some(ObjectRef::new(ObjectKind::Unit, self.owner)),
            position: QVec3 {
                x: self.x_q32,
                y: self.y_q32,
                z: self.z_q32,
            },
            orientation: 0,
            target: Some(ObjectRef::new(self.target_kind, self.target)),
            cached_target_position: QVec3 {
                x: self.cached_target_x_q32,
                y: self.cached_target_y_q32,
                z: self.cached_target_z_q32,
            },
            cached_target_radius: space_to_q32(self.cached_target_radius),
            released: false,
            life: GaugeI32 {
                current: i32::try_from(self.life).expect("projectile life fits i32"),
                maximum: i32::try_from(self.life).expect("projectile life fits i32"),
            },
            spawn_containing_shields: Vec::new(),
        }
    }
}

impl Simulation {
    #[allow(clippy::similar_names)] // Paired fixed-point x/z components are intentionally parallel.
    pub(in crate::fight) fn step_projectiles(&mut self, events: &mut Vec<Event>) -> Result<()> {
        let mut retained = Vec::with_capacity(self.projectiles.len());
        // Build 2259's ProjectileSystem keeps registration order in its List,
        // but Update walks that list from Count - 1 down to zero. Preserve the
        // list order after this reverse update pass so later ticks use the
        // same stable registration sequence.
        for mut projectile in std::mem::take(&mut self.projectiles).into_iter().rev() {
            if projectile.target_kind == ObjectKind::Unit
                && projectile.lock_target
                && let Some(target) = self
                    .actors
                    .get(&projectile.target)
                    .filter(|actor| actor.alive())
            {
                projectile.cached_target_x = target.x;
                projectile.cached_target_y = unit_height(target.rules.domain);
                projectile.cached_target_z = target.z;
                projectile.cached_target_x_q32 = target.x_q32;
                projectile.cached_target_y_q32 = space_to_q32(projectile.cached_target_y);
                projectile.cached_target_z_q32 = target.z_q32;
                projectile.cached_target_radius = target.rules.collision_radius();
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
                self.impact(&projectile, events)?;
            } else {
                let step_q32 = q32_mul(
                    space_to_q32(projectile.speed),
                    Q32_ONE.saturating_mul(LOGIC_TICK_TIME_UNITS.cast_signed())
                        / TIME_UNITS_PER_SECOND.cast_signed(),
                );
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
                projectile.x = q32_to_space_rounded(projectile.x_q32);
                projectile.y = q32_to_space_rounded(projectile.y_q32);
                projectile.z = q32_to_space_rounded(projectile.z_q32);
                retained.push(projectile);
            }
        }
        retained.reverse();
        self.projectiles = retained;
        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    pub(in crate::fight) fn impact(
        &mut self,
        projectile: &Projectile,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let owner = self
            .actors
            .get(&projectile.owner)
            .ok_or_else(|| Error::new("projectile owner is absent"))?;
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
        let hit = DamageHit {
            source: ObjectRef::new(ObjectKind::Unit, projectile.owner),
            source_team: projectile.team,
            team: owner.placement.team,
            amount: projectile.damage,
            aimed,
            hits_aimed: projectile.lock_target,
            center: (projectile.x, projectile.z),
            splash_radius: owner.rules.attack.splash_radius(),
            reach,
        };
        let struck = self.perform_damage(hit, events)?;
        events.push(event(
            Some(projectile.object_ref()),
            Some(hit.source),
            Some(projectile.team),
            Some(aimed.object_ref()),
            EventPayload::ProjectileRemoved {
                position: QVec3 {
                    x: projectile.x_q32,
                    y: projectile.y_q32,
                    z: projectile.z_q32,
                },
                intercepted: false,
                absorbed_by: None,
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
}
