use super::*;

/// Every building a fight starts with, and what a unit may do about each.
pub(in crate::fight) struct InitialBuildings {
    /// The interceptors, each side's in the order its layout releases them.
    pub(in crate::fight) interceptors: Vec<Interceptor>,
    pub(in crate::fight) states: Vec<BuildingState>,
    /// The ones a unit looking for a target may not find.
    pub(in crate::fight) unsearchable: BTreeSet<u64>,
    /// Each construction's RVO collider priority.
    pub(in crate::fight) colliders: BTreeMap<u64, i32>,
    /// The constructions their own side passes through.
    pub(in crate::fight) passable_constructions: BTreeSet<u64>,
    /// What each tower's fall writes on its side.
    pub(in crate::fight) tower_losses: BTreeMap<u64, TowerLoss>,
    /// The constructions a tower's loss reaches.
    pub(in crate::fight) tower_buffed_constructions: BTreeSet<u64>,
    /// Each construction's side and group, by building.
    pub(in crate::fight) construction_groups: BTreeMap<u64, (u32, usize)>,
    /// The experience each building's destruction hands out, by building.
    pub(in crate::fight) building_exp: BTreeMap<u64, i64>,
}

/// One building before it is given an identity, from either source.
#[derive(Debug, Clone, Copy)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "each flag is a separate answer of the building's row"
)]
pub(in crate::fight) struct RawBuilding {
    pub(in crate::fight) team_id: u32,
    pub(in crate::fight) building_type_id: u32,
    pub(in crate::fight) x: i64,
    pub(in crate::fight) z: i64,
    pub(in crate::fight) radius: i64,
    pub(in crate::fight) life: i64,
    pub(in crate::fight) collision_enabled: bool,
    pub(in crate::fight) searchable: bool,
    /// A construction's `pathfinding_collider_priority`; none for a tower.
    pub(in crate::fight) collider_priority: Option<i32>,
    /// A tower's loss, its buff and how many ticks it lasts; none for a
    /// construction.
    pub(in crate::fight) loss: Option<(u32, u32)>,
    /// Whether a tower's loss reaches this construction.
    pub(in crate::fight) tower_buff: bool,
    /// Whether its own side passes through it: a construction whose row
    /// answers `IsEnableBlock`.
    pub(in crate::fight) enable_block: bool,
    /// The construction a block belongs to; none for a tower.
    pub(in crate::fight) group: Option<usize>,
    /// The experience its destruction hands out.
    pub(in crate::fight) exp: i64,
}

/// Construction groups, building experience and tower-buffed constructions.
type BuildingGroups = (
    BTreeMap<u64, (u32, usize)>,
    BTreeMap<u64, i64>,
    BTreeSet<u64>,
);

/// Each construction block's side and construction, what each building's
/// destruction hands out, and the constructions a tower's loss reaches, by
/// building.
fn construction_groups(raw: &[RawBuilding], id_of: impl Fn(&RawBuilding) -> u64) -> BuildingGroups {
    (
        raw.iter()
            .filter_map(|building| Some((id_of(building), (building.team_id, building.group?))))
            .collect(),
        raw.iter()
            .map(|building| (id_of(building), building.exp))
            .collect(),
        raw.iter()
            .filter(|building| building.tower_buff)
            .map(&id_of)
            .collect(),
    )
}

pub(in crate::fight) fn generate_formation_positions(
    placement: &Placement,
    rules: &UnitConfig,
    seed: i32,
) -> Result<Vec<(i64, i64)>> {
    let members = i64::from(rules.formation.members);
    let (width, depth) = rules.formation_footprint_meters()?;
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
    // `MechPositionManager.CalculateMechLocalPosition` counts the grid once,
    // on the unrotated footprint, and lays the rotated grid on the swapped
    // footprint with the two counts exchanged: its rows are the unrotated
    // columns. Counting again on the swapped footprint would stand a Fang
    // in two columns of nine rather than three of six.
    let (width, depth, rows, max_columns) = if placement.rotated {
        (depth, width, max_columns, rows)
    } else {
        (width, depth, rows, max_columns)
    };
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

/// The map's own buildings, each tower at its side's strengthen level: in the
/// order the map lists a side's towers, which is the order its levels are
/// written in.
fn map_buildings(
    towers: &TowersConfig,
    tower_levels: &BTreeMap<u32, Vec<u8>>,
) -> Result<Vec<RawBuilding>> {
    let mut seen = BTreeMap::<u32, usize>::new();
    towers
        .buildings
        .iter()
        .map(|building| {
            let position = seen.entry(building.team_id).or_default();
            let level = tower_levels
                .get(&building.team_id)
                .and_then(|levels| levels.get(*position))
                .copied()
                .unwrap_or(0);
            *position += 1;
            Ok(RawBuilding {
                team_id: building.team_id,
                building_type_id: building.building_type_id,
                x: building.x(),
                z: building.z(),
                radius: building.radius(),
                life: building.life + towers.life_added(level)?,
                collision_enabled: building.collision_enabled,
                searchable: true,
                collider_priority: None,
                loss: Some((towers.loss_buff(level)?, towers.loss_ticks(level)?)),
                tower_buff: false,
                enable_block: false,
                group: None,
                exp: building.exp,
            })
        })
        .collect()
}

/// A neutral crystal of the map in the RVO tree: an immovable agent on its
/// own collider priority's layer with its radius for both of its radii. None
/// is ever destroyed here, so it is in the tree for the whole fight.
#[derive(Debug, Clone, Copy)]
pub(in crate::fight) struct MapCrystal {
    pub(in crate::fight) x_q32: i64,
    pub(in crate::fight) z_q32: i64,
    pub(in crate::fight) radius_q32: i64,
    pub(in crate::fight) collider_priority: i32,
}

/// The lowest collider priority a neutral crystal takes part in movement
/// with. Every crystal of priority 1 is left out of the RVO tree: with the
/// Training Ground's nineteen that overlap a deployment region in the tree,
/// 29 of the pinned fights part from the game, and with none they all hold.
/// On every standard 1v1 map a crystal's priority is above 1 exactly when its
/// life and its strength are, so which of the three the build asks is not
/// separated; `docs/rules/map.md` says so.
const CRYSTAL_MIN_COLLIDER_PRIORITY: i32 = 2;

/// A crystal stands where `CrystalElement.GetPosition` puts it: its map
/// position as a `MapVector`, whose `FVector2` constructor keeps each
/// coordinate's integer part, which for an `FPoint` is its floor.
const fn whole_metres(raw: i64) -> i64 {
    raw & !0xFFFF_FFFF
}

/// The map's crystals that take part in movement, in the order the map lists
/// them.
pub(in crate::fight) fn map_crystals(map: &[MapBuilding]) -> Vec<MapCrystal> {
    map.iter()
        .filter_map(|building| match *building {
            MapBuilding::Tower { .. } => None,
            MapBuilding::Crystal {
                x,
                z,
                radius,
                collider_priority,
            } => (collider_priority >= CRYSTAL_MIN_COLLIDER_PRIORITY).then_some(MapCrystal {
                x_q32: whole_metres(x),
                z_q32: whole_metres(z),
                radius_q32: radius,
                collider_priority,
            }),
        })
        .collect()
}

/// A construction's block as the fight builds it.
fn construction_building(building: &ConstructionBuilding) -> RawBuilding {
    RawBuilding {
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
        loss: None,
        tower_buff: building.tower_buff,
        enable_block: building.enable_block,
        group: Some(building.group),
        exp: i64::from(building.exp),
    }
}

/// An interceptor as the fight builds it: `BuildingType.Special` as a
/// construction is, a building of its side that units may shoot and that
/// stands on its collider layer, but one that belongs to no construction and
/// that no tower's loss is read on.
fn interceptor_building(building: &InterceptorBuilding) -> RawBuilding {
    RawBuilding {
        team_id: building.team,
        building_type_id: CONSTRUCTION_BUILDING_TYPE,
        x: building.x,
        z: building.z,
        radius: building.radius,
        life: i64::from(building.life),
        collision_enabled: true,
        searchable: true,
        collider_priority: Some(building.collider_priority),
        loss: None,
        tower_buff: false,
        enable_block: false,
        group: None,
        exp: i64::from(building.exp),
    }
}

/// Every building the fight starts with: the map's own, and the ones this
/// layout's constructions place.
///
/// Identity is the capture's: a building's id is its place in `(team, type,
/// x, z)` order over both sources together, one-based, which is how a
/// recording numbers them and therefore the only numbering the two backends
/// can be compared under.
///
/// A side's towers take their strengthen levels in the order the map lists
/// them, which is `BuildingManager.buildings`' order and the key
/// `tower_strengthen_levels` is written in: a level adds its life and chooses
/// the buff the tower's loss writes.
pub(in crate::fight) fn initialize_buildings(
    towers: &TowersConfig,
    constructions: &[ConstructionBuilding],
    interceptors: &[InterceptorBuilding],
    tower_levels: &BTreeMap<u32, Vec<u8>>,
) -> Result<InitialBuildings> {
    let mut raw = map_buildings(towers, tower_levels)?;
    let placed_interceptors = raw.len() + constructions.len();
    raw.extend(constructions.iter().map(construction_building));
    raw.extend(interceptors.iter().map(interceptor_building));

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
    let passable_constructions = raw
        .iter()
        .filter(|building| building.enable_block)
        .map(|building| normalized_ids[&building_key(building)])
        .collect::<BTreeSet<_>>();
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
    let tower_losses = raw
        .iter()
        .filter_map(|building| {
            building.loss.map(|(buff_id, ticks)| {
                (
                    normalized_ids[&building_key(building)],
                    TowerLoss {
                        team: building.team_id,
                        buff_id,
                        ticks,
                    },
                )
            })
        })
        .collect();
    let (construction_groups, building_exp, tower_buffed_constructions) =
        construction_groups(&raw, |building| normalized_ids[&building_key(building)]);
    let interceptors = raw[placed_interceptors..]
        .iter()
        .zip(interceptors)
        .map(|(building, placed)| Interceptor::new(normalized_ids[&building_key(building)], placed))
        .collect();
    Ok(InitialBuildings {
        interceptors,
        states,
        unsearchable,
        colliders,
        passable_constructions,
        tower_losses,
        tower_buffed_constructions,
        construction_groups,
        building_exp,
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
        // The build's PresearchTargetController::CalculateCountPerTime returns
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
                    self.select_normal_target_with_order(
                        FightActorRef::Unit(actor_id),
                        &target_search_order,
                        false,
                    )?,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        for (ordinal, (actor_id, target)) in selections.into_iter().enumerate() {
            self.actors
                .get_mut(&actor_id)
                .expect("initial actor identity is stable")
                .skill
                .search_target_time = i32::try_from(ordinal / count_per_time)
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
            actor.skill.lock_target = Some(target);
            actor.skill.set_mech_lock(Some(target));
            actor.set_body_rotation(target_rotation_q32);
            actor.aim_rotation = actor.body_rotation;
            actor.set_weapon_rotation(target_rotation_q32);
            self.search_attack_target(FightActorRef::Unit(actor_id));
        }
        // A weapon fixed to the body enters the fight with the body's
        // rotation.
        for actor in self.actors.values_mut() {
            let rotation_q32 = actor.body_rotation_q32;
            if let Some(group) = &mut actor.skill.group {
                group.sibling_weapon_rotations_q32.fill(rotation_q32);
            }
        }
        Ok(())
    }
}
