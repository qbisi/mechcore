//! `BuildingDamageController` and `ProjectileController`'s neutral crystals.

use super::*;

/// `ProjectileController.HitBuildingInfo`: a candidate and its first-check delay.
#[derive(Debug, Clone)]
pub(in crate::fight) struct HitBuilding {
    crystal: usize,
    delay: Option<i32>,
    updates: i32,
}

impl Simulation {
    /// `ProjectileController.GenerateTargetBuildings`, once on creation.
    pub(in crate::fight) fn generate_projectile_buildings(&self, projectile: &mut Projectile) {
        let start = (projectile.x_q32, projectile.z_q32);
        // The native query uses the actual target, before tracking offsets.
        let target = match projectile.target_kind {
            ObjectKind::Unit => FightActorRef::Unit(projectile.target),
            _ => FightActorRef::Building(projectile.target),
        };
        let Some(target) = self.fight_actor(target) else {
            return;
        };
        let end = (target.x_q32, target.z_q32);
        let delta = (end.0.wrapping_sub(start.0), end.1.wrapping_sub(start.1));
        // Keep the build's query centre: half the difference, without adding
        // the start. Root elements are returned even outside this rectangle.
        let center = (q32_div(delta.0, 2 * Q32_ONE), q32_div(delta.1, 2 * Q32_ONE));
        let half = (
            i64::midpoint(delta.0.abs(), 2 * Q32_ONE),
            i64::midpoint(delta.1.abs(), 2 * Q32_ONE),
        );
        let range = TargetActorRect {
            min_x: center.0 - half.0,
            max_x: center.0 + half.0,
            min_z: center.1 - half.1,
            max_z: center.1 + half.1,
        };
        projectile.hit_buildings = self
            .building_collisions
            .damage_tree
            .query_rect(range)
            .into_iter()
            .filter(|&index| {
                let crystal = &self.rvo.map_crystals[index];
                let point = (crystal.x_q32, crystal.z_q32);
                // LineRange.Contains(CircleRange) excludes either endpoint,
                // then uses its point/radius overload with width + radius.
                point != start
                    && point != end
                    && path_finding::line_range_contains(
                        start,
                        end,
                        10 * Q32_ONE + crystal.radius_q32,
                        point,
                    )
            })
            .map(|crystal| HitBuilding {
                crystal,
                delay: None,
                updates: 0,
            })
            .collect();
    }

    /// `ProjectileController.IsHitBuilding` and `OnHitBuilding`, after the
    /// projectile's move/impact and interceptor membership update. At most
    /// one candidate takes a hit per update, in the cached tree-query order.
    pub(in crate::fight) fn projectile_hit_building(
        &mut self,
        projectile: &mut Projectile,
        last_position: (i64, i64, i64),
    ) -> Result<()> {
        let position = (projectile.x_q32, projectile.y_q32, projectile.z_q32);
        for index in 0..projectile.hit_buildings.len() {
            let candidate = &mut projectile.hit_buildings[index];
            let crystal = self.rvo.map_crystals[candidate.crystal];
            if !crystal.alive {
                continue;
            }
            let center = (crystal.x_q32, 0, crystal.z_q32);
            let distance =
                native_q32_magnitude_3d(position.0 - center.0, position.1, position.2 - center.2);
            let delay = *candidate.delay.get_or_insert_with(|| {
                i32::try_from(
                    q32_div(
                        q32_div(distance, space_to_q32(projectile.speed)),
                        NATIVE_LOGIC_DELTA_Q32,
                    ) >> 32,
                )
                .unwrap_or(i32::MAX)
            });
            if candidate.updates < delay {
                candidate.updates += 1;
                continue;
            }
            let maximum_move = q32_mul(space_to_q32(projectile.speed), NATIVE_LOGIC_DELTA_Q32);
            if !fpoint_less_or_equal(distance, crystal.radius_q32)
                && (!fpoint_less_or_equal(distance, 2 * maximum_move)
                    || !line_cross_sphere(last_position, position, center, crystal.radius_q32))
            {
                continue;
            }
            let (strength, damage) = match &projectile.shooter {
                Shooter::Actor(owner) => {
                    let source = self
                        .skill_attacker(
                            self.skill_at_slot(*owner, usize::from(projectile.skill_slot)),
                        )
                        .ok_or_else(|| Error::new("projectile data source is absent"))?;
                    (source.attack.projectile_strength, source.attack_damage)
                }
                // FightLandMine.IProjectileDataSource.GetAttackStrength.
                Shooter::Missile(shot) => (i32::MAX, shot.damage),
            };
            if strength <= crystal.strength {
                return Err(Error::new(format!(
                    "projectile blocked by neutral map crystal is unimplemented (projectile {}, crystal {}, strength {strength} <= {})",
                    projectile.id, candidate.crystal, crystal.strength,
                )));
            }
            let key = candidate.crystal;
            projectile.hit_buildings.remove(index);
            let crystal = &mut self.rvo.map_crystals[key];
            crystal.life = crystal.life.saturating_sub(damage.max(0));
            if crystal.life <= 0 {
                crystal.alive = false;
                self.building_collisions.pending.push(key);
            }
            break;
        }
        Ok(())
    }
}

/// `FightUtility.IsLineCrossSphere`: both endpoint angles at most 90 degrees,
/// then the sine-derived perpendicular distance no greater than the radius.
fn line_cross_sphere(
    start: shield::Vector,
    end: shield::Vector,
    center: shield::Vector,
    radius: i64,
) -> bool {
    const RIGHT_ANGLE: i64 = 0x0477_D1A9 * 90;
    let sub = |a: shield::Vector, b: shield::Vector| (a.0 - b.0, a.1 - b.1, a.2 - b.2);
    let direction = sub(end, start);
    let from_start = sub(center, start);
    let angle = shield::angle_rad(from_start, direction);
    if !fpoint_less_or_equal(angle, RIGHT_ANGLE)
        || !fpoint_less_or_equal(
            shield::angle_rad(sub(center, end), sub(start, end)),
            RIGHT_ANGLE,
        )
    {
        return false;
    }
    let distance = q32_mul(
        native_q32_magnitude_3d(from_start.0, from_start.1, from_start.2),
        math::fpcs_sin_fastest(angle),
    );
    fpoint_less_or_equal(distance, radius)
}
