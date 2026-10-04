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
    // `MechPositionManager` lays the grid out facing +z and turns it with the
    // formation: a quarter turn stands it across a flank.
    let turn = |x: i64, z: i64| -> Result<(i64, i64)> {
        match placement.rotation {
            0 => Ok((x, z)),
            90_000 => Ok((z, x.saturating_neg())),
            180_000 => Ok((x.saturating_neg(), z.saturating_neg())),
            270_000 => Ok((z.saturating_neg(), x)),
            other => Err(Error::new(format!(
                "unit {:?} formation faces {other} millidegrees, which is no \
                 deployment facing",
                rules.type_name
            ))),
        }
    };
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
            let (offset_x, offset_z) = turn(
                local_x_q32.saturating_add(jitter_x),
                local_z_q32.saturating_add(jitter_z),
            )?;
            positions.push((
                center_x_q32.saturating_add(offset_x),
                center_z_q32.saturating_add(offset_z),
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
    let flanked = flanked_sides(&layout.placements);
    let mut initial = Vec::new();
    for placement in &layout.placements {
        let rules = configs.get(placement.type_name.as_str()).ok_or_else(|| {
            Error::new(format!(
                "unit type {:?} has no configuration",
                placement.type_name
            ))
        })?;
        for (x_q32, z_q32) in generate_formation_positions(placement, rules, seed)? {
            let mut actor =
                Actor::at_generated_position(placement.clone(), rules.clone(), x_q32, z_q32);
            let facing = attack_facing(placement, x_q32, z_q32, &flanked);
            if facing != placement.rotation {
                actor.face(mdeg_to_degrees_q32(facing));
            }
            initial.push(actor);
        }
    }
    // A side's units take their identities in ascending world z, then x, by
    // the raw positions: two units a few raw units apart in z keep that
    // order even where their millimetres agree.
    initial.sort_by_key(|actor| (actor.placement.team, actor.z_q32, actor.x_q32));
    for pair in initial.windows(2) {
        if pair[0].placement.team == pair[1].placement.team
            && pair[0].x_q32 == pair[1].x_q32
            && pair[0].z_q32 == pair[1].z_q32
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

/// A side's main deployment region, `x=[-300,300), y=[-310,-10)` in its own
/// frame, whole metres, half open as `MapRect.Contains` is; its towers stand
/// at `x = ±TOWER_X, y = TOWER_Y`, `TOWER_SIZE` square.
const MAIN_MIN_X: i64 = -300;
const MAIN_MAX_X: i64 = 300;
const MAIN_MIN_Y: i64 = -310;
const MAIN_MAX_Y: i64 = -10;
const TOWER_X: i64 = 140;
const TOWER_Y: i64 = -170;
const TOWER_SIZE: i64 = 20;
/// `PlayerTerritory.MAIN_DEFENSE_AREA_HEIGHT_OFFSET`.
const DEFENSE_AREA_HEIGHT_OFFSET: i64 = 150;

/// Which of each side's two defence regions hold something, as
/// `(team, left)`: a side's left, in its own frame, holds something when the
/// other side has a formation on the flank beside it.
///
/// `TerritoryManager.PrepareDefenseRegion` gives each of a side's two
/// defence areas the region beside it, the other side's flank there, and
/// `MapRegion.IsDefenseRegionEmpty` asks whether it holds anything. A side's
/// left is beside the other's right flank, and its right beside the other's
/// left one.
fn flanked_sides(placements: &[Placement]) -> BTreeSet<(u32, bool)> {
    let mut flanked = BTreeSet::new();
    for placement in placements {
        let (x, z) = if placement.team == 0 {
            (placement.world_x, placement.world_z)
        } else {
            (-placement.world_x, -placement.world_z)
        };
        let position = mechcore_document::Position {
            x: i32::try_from(x).unwrap_or(i32::MAX),
            y: i32::try_from(z).unwrap_or(i32::MAX),
        };
        let other = u32::from(placement.team == 0);
        match mechcore_document::Region::of(position) {
            mechcore_document::Region::Main => {}
            mechcore_document::Region::LeftFlank => {
                flanked.insert((other, false));
            }
            mechcore_document::Region::RightFlank => {
                flanked.insert((other, true));
            }
        }
    }
    flanked
}

/// `PlayerTerritory.GetAttackFacing`, which `TerritoryManager.RefreshMechDiretion`
/// turns each unit to as the fight starts: a unit of the main region standing
/// in one of its side's two defence areas, whose defence region holds
/// something, faces a quarter turn towards that side, left or right of its
/// formation; any other faces as its formation does.
///
/// It reads the unit's own position, in whole metres, so the members of one
/// formation can face two ways. `CreateLeftDefenseAreaLocal` makes the left
/// area the main region from its left edge to the left tower's right edge and
/// from its back edge to `DEFENSE_AREA_HEIGHT_OFFSET` short of its front, and
/// `DefenseArea.Contains` takes of it what lies no further forward than the
/// tower's front edge, beyond which a triangle is added that a standard map's
/// towers leave empty: their front edge is the area's. The right area mirrors
/// the left.
fn attack_facing(
    placement: &Placement,
    x_q32: i64,
    z_q32: i64,
    flanked: &BTreeSet<(u32, bool)>,
) -> i64 {
    let main = if placement.team == 0 { 0 } else { 180_000 };
    if placement.rotation != main {
        return placement.rotation;
    }
    // A `MapVector` keeps each coordinate's floor, and the main region's
    // bound is held in world coordinates.
    let (world_x, world_z) = (x_q32 >> 32, z_q32 >> 32);
    let (x, y) = if placement.team == 0 {
        (world_x, world_z)
    } else {
        (-world_x, -world_z)
    };
    let (min_z, max_z) = if placement.team == 0 {
        (MAIN_MIN_Y, MAIN_MAX_Y)
    } else {
        (-MAIN_MAX_Y, -MAIN_MIN_Y)
    };
    if !(MAIN_MIN_X..MAIN_MAX_X).contains(&world_x) || !(min_z..max_z).contains(&world_z) {
        return main;
    }
    let back = MAIN_MIN_Y;
    let front = back + (MAIN_MAX_Y - MAIN_MIN_Y) - DEFENSE_AREA_HEIGHT_OFFSET;
    if !(back..front).contains(&y) || y > TOWER_Y + TOWER_SIZE / 2 {
        return main;
    }
    let inner = TOWER_X - TOWER_SIZE / 2;
    if (MAIN_MIN_X..-inner).contains(&x) && flanked.contains(&(placement.team, true)) {
        (main - 90_000).rem_euclid(360_000)
    } else if (inner..MAIN_MAX_X).contains(&x) && flanked.contains(&(placement.team, false)) {
        (main + 90_000).rem_euclid(360_000)
    } else {
        main
    }
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

/// The ground a formation stands on: its footprint, turned with the
/// formation, around its centre, as `(x_min, x_max, z_min, z_max)` Q32.32.
pub(in crate::fight) fn formation_ground(
    placement: &Placement,
    rules: &UnitConfig,
) -> Result<(i64, i64, i64, i64)> {
    let (width, depth) = rules.formation_footprint_meters()?;
    let across = placement.rotated != matches!(placement.rotation, 90_000 | 270_000);
    let (width, depth) = if across {
        (depth, width)
    } else {
        (width, depth)
    };
    let (x, z) = (
        placement.world_x.saturating_mul(Q32_ONE),
        placement.world_z.saturating_mul(Q32_ONE),
    );
    let (half_width, half_depth) = (
        width.saturating_mul(Q32_ONE) / 2,
        depth.saturating_mul(Q32_ONE) / 2,
    );
    Ok((
        x.saturating_sub(half_width),
        x.saturating_add(half_width),
        z.saturating_sub(half_depth),
        z.saturating_add(half_depth),
    ))
}

/// The ground every formation of the layout stands on.
pub(in crate::fight) fn formation_grounds(
    layout: &CompiledLayout,
    configs: &UnitConfigs,
) -> Result<Vec<(i64, i64, i64, i64)>> {
    layout
        .placements
        .iter()
        .map(|placement| {
            let rules = configs.get(&placement.type_name).ok_or_else(|| {
                Error::new(format!(
                    "unit type {:?} has no configuration",
                    placement.type_name
                ))
            })?;
            formation_ground(placement, rules)
        })
        .collect()
}

/// The map's crystals that take part in movement, in the order the map lists
/// them. A crystal a formation stands on is not among them: its circle
/// reaching into the formation's footprint, strictly, leaves it out of the
/// fight, and one whose circle only touches the footprint's edge stays.
pub(in crate::fight) fn map_crystals(
    map: &[MapBuilding],
    grounds: &[(i64, i64, i64, i64)],
) -> Vec<MapCrystal> {
    let stood_on = |x: i64, z: i64, radius: i64| {
        grounds.iter().any(|&(x_min, x_max, z_min, z_max)| {
            let dx = x.clamp(x_min, x_max).saturating_sub(x);
            let dz = z.clamp(z_min, z_max).saturating_sub(z);
            i128::from(dx) * i128::from(dx) + i128::from(dz) * i128::from(dz)
                < i128::from(radius) * i128::from(radius)
        })
    };
    map.iter()
        .filter_map(|building| match *building {
            MapBuilding::Tower { .. } => None,
            MapBuilding::Crystal {
                x,
                z,
                radius,
                collider_priority,
            } => (collider_priority >= CRYSTAL_MIN_COLLIDER_PRIORITY
                && !stood_on(whole_metres(x), whole_metres(z), radius))
            .then_some(MapCrystal {
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
                .skills
                .main
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
            actor.skills.main.lock_target = Some(target);
            actor.skills.main.set_mech_lock(Some(target));
            // A unit with a command faces its point rather than its lock.
            if actor.command.is_none() {
                actor.set_body_rotation(target_rotation_q32);
                actor.aim_rotation = actor.body_rotation;
                actor.set_weapon_rotation(target_rotation_q32);
            }
            // A travelling unit keeps the lock its presearch found, and its
            // attack target is searched by the first update of its own.
            if actor.travelling {
                continue;
            }
            self.search_attack_target(SkillRef::main(FightActorRef::Unit(actor_id)));
        }
        // A weapon fixed to the body enters the fight with the body's
        // rotation.
        for actor in self.actors.values_mut() {
            let rotation_q32 = actor.body_rotation_q32;
            if let Some(group) = &mut actor.skills.main.group {
                group.sibling_weapon_rotations_q32.fill(rotation_q32);
            }
        }
        Ok(())
    }
}

impl Simulation {
    /// `TerritoryManager.RefreshConstructionDirection` as the fight starts:
    /// `UnitDirectionCalculator.Calculate` turns each construction that fires
    /// onto the best of the targets it finds of the other side, scored as its
    /// skill's selector scores them from where the construction stands and
    /// points. It finds the other side's towers, the constructions a search
    /// finds, and its legacy units; a unit that joined its side during the
    /// round is not among them.
    pub(in crate::fight) fn face_constructions_at_fight_start(
        &mut self,
        legacy_units: &BTreeMap<u32, i32>,
        delivered: &BTreeSet<(u32, i32)>,
    ) {
        let ids = self
            .constructions
            .iter()
            .filter(|(_, construction)| construction.searches)
            .map(|(&id, _)| id)
            .collect::<Vec<_>>();
        for id in ids {
            let Some(source) = self.attacker(FightActorRef::Building(id)) else {
                continue;
            };
            let mut best: Option<(i64, i64, i64)> = None;
            let mut consider = |x_q32: i64, z_q32: i64, radius: i64| {
                // Nothing is hidden before the fight starts.
                let Some(score) = full_rotation_target_score_q32(
                    source.x_q32,
                    source.z_q32,
                    source.radius,
                    source.query_rotation_q32,
                    x_q32,
                    z_q32,
                    radius,
                    true,
                    source.attack.min_range(),
                    source.attack_range,
                    source.rotation_window_q32,
                ) else {
                    return;
                };
                if best.is_none_or(|(least, _, _)| score < least) {
                    best = Some((score, x_q32, z_q32));
                }
            };
            for building in &self.buildings {
                let found = self.is_tower(FightActorRef::Building(building.building_id))
                    || (building.targetable
                        && !self.unsearchable_buildings.contains(&building.building_id));
                if building.team_id != source.team
                    && found
                    && building_alive(building)
                    && source.attack.accepts(UnitDomain::Ground)
                {
                    consider(
                        building.position.x,
                        building.position.z,
                        building_radius(building),
                    );
                }
            }
            for actor in self.actors.values() {
                let legacy = legacy_units
                    .get(&actor.placement.team)
                    .is_some_and(|&legacy_index| actor.placement.formation_index < legacy_index);
                if actor.placement.team != source.team
                    && legacy
                    && actor.alive()
                    && source.attack.accepts(actor.rules.domain)
                {
                    // A squad an officer delivered as the round opened still
                    // stands at the origin for this: the selector scores it
                    // there and `CalculateMechDirection` turns towards its
                    // `FightTransform.recordPosition`, which the delivery
                    // leaves unset.
                    let (x_q32, z_q32) = if delivered
                        .contains(&(actor.placement.team, actor.placement.formation_index))
                    {
                        (0, 0)
                    } else {
                        (actor.x_q32, actor.z_q32)
                    };
                    consider(x_q32, z_q32, actor.rules.collision_radius());
                }
            }
            let Some((_, tx, tz)) = best else {
                continue;
            };
            let bearing = direction_degrees_q32_raw(
                tx.saturating_sub(source.x_q32),
                tz.saturating_sub(source.z_q32),
            );
            self.skill_mut(SkillRef::main(FightActorRef::Building(id)))
                .turn_weapons_towards(bearing, 360_i64 << 32);
        }
    }
}

/// The order a fight updates its deployed units in: each side's units by
/// `FightUtility.PositionComparer` on where they spawned, world `z` unless
/// `FPoint`'s tolerant inequality finds two within 43 raw of each other, and
/// world `x` then. Identities follow `z` strictly, so two units a few raw
/// units apart in `z` update in the order their `x` gives, not their
/// identities': recorded in replays 201370830 and 67152171, round 1, where two
/// such units draw their first intervals, and act each tick, in that order.
/// The comparison is no total order, which `sort_by` may refuse, so each unit
/// is inserted after every unit it does not precede.
pub(in crate::fight) fn update_order(actors: &BTreeMap<u64, Actor>) -> Vec<u64> {
    const TOLERANCE: u64 = 43;
    let order = |left: &Actor, right: &Actor| {
        left.placement
            .team
            .cmp(&right.placement.team)
            .then_with(|| {
                if left.z_q32.abs_diff(right.z_q32) > TOLERANCE {
                    left.z_q32.cmp(&right.z_q32)
                } else {
                    left.x_q32.cmp(&right.x_q32)
                }
            })
    };
    let mut sorted: Vec<u64> = Vec::with_capacity(actors.len());
    for (&id, actor) in actors {
        let at = sorted
            .iter()
            .rposition(|placed| order(&actors[placed], actor) != Ordering::Greater)
            .map_or(0, |index| index + 1);
        sorted.insert(at, id);
    }
    sorted
}
