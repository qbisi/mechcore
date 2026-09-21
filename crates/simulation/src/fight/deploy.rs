use super::*;

pub(in crate::fight) fn generate_formation_positions(
    placement: &Placement,
    rules: &UnitConfig,
    seed: i32,
) -> Result<Vec<(i64, i64)>> {
    let members = i64::from(rules.formation.members);
    let (width, depth) = rules.formation_footprint_meters()?;
    let (width, depth) = if placement.rotated {
        (depth, width)
    } else {
        (width, depth)
    };
    let slot_size = rules.formation_slot_size_meters()?;
    let max_columns = width / slot_size;
    if max_columns <= 0 {
        return Err(Error::new(format!(
            "unit {:?} formation slot size exceeds its footprint width",
            rules.type_name
        )));
    }
    let rows = members.saturating_add(max_columns - 1) / max_columns;
    if rows <= 0 {
        return Err(Error::new(format!(
            "unit {:?} formation has no member rows",
            rules.type_name
        )));
    }
    let row_step = depth / rows;
    let column_step = width / max_columns;
    if row_step <= 0 || column_step <= 0 {
        return Err(Error::new(format!(
            "unit {:?} formation grid has a nonpositive native step",
            rules.type_name
        )));
    }

    let formation_seed = seed.wrapping_add(placement.formation_index);
    let mut layout_random = GrRandom::new(i64::from(formation_seed).cast_unsigned());
    let direction = if placement.team == 0 { 1_i64 } else { -1_i64 };
    let center_x_q32 = placement.world_x.saturating_mul(Q32_ONE);
    let center_z_q32 = placement.world_z.saturating_mul(Q32_ONE);
    let mut positions = Vec::with_capacity(usize::try_from(members).unwrap_or(usize::MAX));
    for row in 0..rows {
        let remaining = members.saturating_sub(i64::try_from(positions.len()).unwrap_or(i64::MAX));
        let columns = max_columns.min(remaining);
        let local_z_q32 = depth.saturating_mul(Q32_ONE) / 2
            - (row.saturating_mul(row_step).saturating_mul(Q32_ONE)
                + row_step.saturating_mul(Q32_ONE) / 2);
        let occupied_width = columns.saturating_mul(column_step);
        for column in 0..columns {
            let local_x_q32 = column.saturating_mul(column_step).saturating_mul(Q32_ONE)
                + column_step.saturating_mul(Q32_ONE) / 2
                - occupied_width.saturating_mul(Q32_ONE) / 2;
            let jitter_x = i64::from(layout_random.next_in_range(FORMATION_JITTER_RANGE_TENTHS))
                .saturating_mul(C0_1_RAW);
            let jitter_z = i64::from(layout_random.next_in_range(FORMATION_JITTER_RANGE_TENTHS))
                .saturating_mul(C0_1_RAW);
            positions.push((
                center_x_q32.saturating_add(
                    local_x_q32
                        .saturating_add(jitter_x)
                        .saturating_mul(direction),
                ),
                center_z_q32.saturating_add(
                    local_z_q32
                        .saturating_add(jitter_z)
                        .saturating_mul(direction),
                ),
            ));
        }
    }
    if positions.len() != usize::try_from(rules.formation.members).unwrap_or(usize::MAX) {
        return Err(Error::new(format!(
            "unit {:?} formation generation produced the wrong member count",
            rules.type_name
        )));
    }
    Ok(positions)
}

pub(in crate::fight) fn initialize_actors(
    layout: &CompiledLayout,
    configs: &UnitConfigs,
    seed: i32,
) -> Result<BTreeMap<u64, Actor>> {
    let mut initial = Vec::new();
    for placement in &layout.placements {
        let rules = configs.get(placement.type_name.as_str()).ok_or_else(|| {
            Error::new(format!(
                "unit type {:?} has no configuration",
                placement.type_name
            ))
        })?;
        for (x_q32, z_q32) in generate_formation_positions(placement, rules, seed)? {
            initial.push(Actor::at_generated_position(
                placement.clone(),
                rules.clone(),
                x_q32,
                z_q32,
            ));
        }
    }
    initial.sort_by_key(|actor| (actor.placement.team, actor.z, actor.x));
    for pair in initial.windows(2) {
        if pair[0].placement.team == pair[1].placement.team
            && pair[0].x == pair[1].x
            && pair[0].z == pair[1].z
        {
            return Err(Error::new(
                "two initial same-team units have equal world coordinates",
            ));
        }
    }

    let mut identities = IdentityAllocator::new();
    let mut formation_ids = BTreeMap::new();
    let mut actors = BTreeMap::new();
    for mut actor in initial {
        let unit_id = identities.allocate_object(ObjectKind::Unit)?.id;
        let formation_key = (actor.placement.team, actor.placement.formation_index);
        let formation_id = if let Some(id) = formation_ids.get(&formation_key) {
            *id
        } else {
            let id = identities.allocate_formation()?;
            formation_ids.insert(formation_key, id);
            id
        };
        actor.placement.unit_id = unit_id;
        actor.placement.formation_id = formation_id;
        actors.insert(unit_id, actor);
    }
    Ok(actors)
}

/// Every building the fight starts with: the map's own, and the ones this
/// layout's constructions place.
///
/// Identity is the capture's: a building's id is its place in `(team, type,
/// x, z)` order over both sources together, one-based, which is how a
/// recording numbers them and therefore the only numbering the two backends
/// can be compared under.
pub(in crate::fight) fn initialize_buildings(
    training_ground: &TrainingGroundConfig,
    constructions: &[ConstructionBuilding],
) -> Result<InitialBuildings> {
    let mut raw = training_ground
        .buildings
        .iter()
        .map(|building| RawBuilding {
            team_id: building.team_id,
            building_type_id: building.building_type_id,
            x: building.x(),
            z: building.z(),
            radius: building.radius(),
            life: building.life,
            collision_enabled: building.collision_enabled,
            searchable: true,
            collider_priority: None,
        })
        .collect::<Vec<_>>();
    raw.extend(constructions.iter().map(|building| RawBuilding {
        team_id: building.team,
        building_type_id: building.building_type_id,
        x: building.x,
        z: building.z,
        radius: building.radius,
        life: i64::from(building.life),
        // `BuildingData.EnableCollision` as the capture reads it, which is
        // true for a construction as it is for a tower. Whether the object is
        // an obstacle is a different question, and [`rvo_collides`] answers
        // it.
        collision_enabled: true,
        searchable: building.searchable,
        collider_priority: Some(building.collider_priority),
    }));

    let building_key = |building: &RawBuilding| {
        (
            building.team_id,
            building.building_type_id,
            building.x,
            building.z,
        )
    };
    let mut ordered = raw.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|building| building_key(building));
    let mut normalized_ids = BTreeMap::new();
    for (index, building) in ordered.into_iter().enumerate() {
        let id = u64::try_from(index)
            .map_err(|_| Error::new("building index overflow"))?
            .saturating_add(1);
        if normalized_ids.insert(building_key(building), id).is_some() {
            return Err(Error::new("two buildings stand in the same place"));
        }
    }
    let unsearchable = raw
        .iter()
        .filter(|building| !building.searchable)
        .map(|building| normalized_ids[&building_key(building)])
        .collect::<BTreeSet<_>>();
    let colliders = raw
        .iter()
        .filter_map(|building| {
            building
                .collider_priority
                .map(|priority| (normalized_ids[&building_key(building)], priority))
        })
        .collect::<BTreeMap<_, _>>();
    let states = raw
        .iter()
        .map(|building| {
            let building_id = normalized_ids[&building_key(building)];
            Ok(BuildingState {
                building_id,
                team_id: building.team_id,
                building_type_id: building.building_type_id,
                position: point(building.x, building.z),
                bounds_width: space_to_q32(building.radius.saturating_mul(2)),
                bounds_height: space_to_q32(building.radius.saturating_mul(2)),
                life: GaugeI32 {
                    current: i32::try_from(building.life)
                        .map_err(|_| Error::new("building life exceeds i32"))?,
                    maximum: i32::try_from(building.life)
                        .map_err(|_| Error::new("building life exceeds i32"))?,
                },
                available: true,
                targetable: building.life > 0,
                collision_enabled: building.collision_enabled,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(InitialBuildings {
        states,
        unsearchable,
        colliders,
    })
}

/// Whether a building takes part in RVO as a tower, which is not the same as
/// whether its data says collision is enabled.
///
/// The map's towers push every unit around. A construction takes part too,
/// but on its own collider layer and only for the other side, which
/// [`Simulation::construction_colliders`] carries: every construction is
/// `BuildingType.Special` and only the map's own two towers are anything
/// else, so the type is what separates them here.
pub(in crate::fight) const fn rvo_collides(building: &BuildingState) -> bool {
    building.collision_enabled && building.building_type_id != CONSTRUCTION_BUILDING_TYPE
}

impl Simulation {
    pub(in crate::fight) fn initialize_presearch_targets(&mut self) -> Result<()> {
        let actor_ids = self.actors.keys().copied().collect::<Vec<_>>();
        // Build 2259 PresearchTargetController::CalculateCountPerTime returns
        // ceil(mech_count / 10). SearchTarget assigns the zero-based batch
        // ordinal to the main FightSkill search controller before selecting
        // its initial target.
        let count_per_time = actor_ids.len().div_ceil(10).max(1);
        // The selector answers whatever stands nearest, and a building is an
        // answer: a Defensive Wall in front of a deployment is what the other
        // side presearches, which is what it does in the game.
        let target_search_order = self.target_search_order();
        let selections = actor_ids
            .iter()
            .map(|&actor_id| {
                Ok((
                    actor_id,
                    self.select_normal_target_with_order(actor_id, &target_search_order, false)?,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        for (ordinal, (actor_id, target)) in selections.into_iter().enumerate() {
            self.actors
                .get_mut(&actor_id)
                .expect("initial actor identity is stable")
                .fight_skill_search_target_time = i32::try_from(ordinal / count_per_time)
                .expect("presearch batch ordinal is at most nine");
            let Some(target) = target else {
                continue;
            };
            let view = self
                .fight_actor(target)
                .ok_or_else(|| Error::new("presearch chose a target that is not on the board"))?;
            let target_rotation_q32 = direction_degrees_q32_raw(
                view.x_q32.saturating_sub(self.actors[&actor_id].x_q32),
                view.z_q32.saturating_sub(self.actors[&actor_id].z_q32),
            );
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("initial actor identity is stable");
            actor.lock_target = Some(target);
            actor.set_body_rotation(target_rotation_q32);
            actor.aim_rotation = actor.body_rotation;
            actor.set_weapon_rotation(target_rotation_q32);
            self.search_attack_target(actor_id);
        }
        Ok(())
    }
}
