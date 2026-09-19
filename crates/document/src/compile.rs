//! Compiles a layout into an execution plan, and refuses one that cannot stand.
//!
//! The rules here are geometric rather than syntactic: footprints, deployment
//! regions, collisions between placements, and where a battle skill may land.
//! They are pinned to one build through [`crate::catalog`].

use crate::catalog::{
    BattleSkillMapRule, BattleSkillShape, BattleSkillSpec, NativeFormation, resolve_battle_skill_type,
    resolve_construction_type, resolve_contraption_type, resolve_unit_type,
};
use crate::layout::{
    AMBUSH_LEFT_MAX_X, AMBUSH_LEFT_MIN_X, AMBUSH_MAX_Y, AMBUSH_MIN_Y, AMBUSH_RIGHT_MAX_X,
    AMBUSH_RIGHT_MIN_X, BattleSkillDefinition, ContraptionPlacement,
    FIGHT_VISIBLE_ENERGY_TOWER_SKILLS, Layout, MAX_TOWER_STRENGTHEN_LEVEL, OIL_TERRAIN_GRID_MASK,
    OIL_TERRAIN_GRID_SIZE, OIL_TERRAIN_POINT_COUNT, Position, Region, Side, StaticPlacement,
    TOWER_COUNT, Techs, Terrain, TerrainType, UnitPlacement, require_layout_kind,
};
use serde_json::Value;
#[derive(Debug, PartialEq, Eq)]
pub struct BattleSkill {
    pub type_name: String,
    pub commander_skill_id: i32,
    pub positions: Vec<Position>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Placement {
    pub type_name: String,
    pub native: NativeFormation,
    pub footprint: Option<(i64, i64)>,
    pub position: Position,
    pub index: Option<i32>,
    pub level: Option<i32>,
    pub exp: Option<i32>,
    pub rotated: bool,
    pub equipment: Option<i32>,
    pub travelling: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub struct SidePlan {
    pub techs: Techs,
    pub energy_tower_skills: Vec<i32>,
    pub tower_strengthen_levels: Vec<i32>,
    pub units: Vec<Placement>,
    pub constructions: Vec<Placement>,
    pub contraptions: Vec<Placement>,
    pub airdrop_shields: Vec<Position>,
    pub terrains: Vec<Terrain>,
    pub battle_skills: Vec<BattleSkill>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Plan {
    pub map_id: Option<i32>,
    pub seed: Option<i32>,
    pub round: i32,
    pub blue: SidePlan,
    pub red: SidePlan,
}

const DEPLOYMENT_MIN_X: i64 = -300;
const DEPLOYMENT_MAX_X: i64 = 300;
const DEPLOYMENT_MIN_Y: i64 = -310;
const DEPLOYMENT_MAX_Y: i64 = -10;
const SHIELD_RADIUS: i64 = 70;
const BATTLEFIELD_MIN_X: i64 = -400;
const BATTLEFIELD_MAX_X: i64 = 400;
const BATTLEFIELD_MIN_Y: i64 = -350;
const BATTLEFIELD_MAX_Y: i64 = 350;
const OIL_TERRAIN_RADIUS: i64 = 30;
const ENEMY_TOWER_X: [i64; 2] = [-140, 140];
const ENEMY_TOWER_Y: i64 = 170;
const ENEMY_TOWER_PROTECTION_RANGE: i64 = 140;

impl Plan {
    #[must_use]
    pub fn unit_count(&self) -> usize {
        self.blue.units.len() + self.red.units.len()
    }

    #[must_use]
    pub fn construction_count(&self) -> usize {
        self.blue.constructions.len() + self.red.constructions.len()
    }

    #[must_use]
    pub fn contraption_count(&self) -> usize {
        self.blue.contraptions.len() + self.red.contraptions.len()
    }

    #[must_use]
    pub fn airdrop_shield_count(&self) -> usize {
        self.blue.airdrop_shields.len() + self.red.airdrop_shields.len()
    }

    #[must_use]
    pub fn terrain_count(&self) -> usize {
        self.blue.terrains.len() + self.red.terrains.len()
    }
}

/// Deserializes, validates, and normalizes a JSON layout into an execution plan.
///
/// # Errors
///
/// Returns an error when the JSON does not match [`Layout`] or violates any
/// shared static layout rule.
pub fn compile(value: &Value) -> Result<Plan, String> {
    match value.get("kind") {
        Some(Value::String(kind)) => require_layout_kind(Some(kind))?,
        // A non-string `kind` is a type error, which serde reports better.
        Some(_) => {}
        None => require_layout_kind(None)?,
    }
    let layout: Layout = serde_json::from_value(value.clone())
        .map_err(|error| format!("invalid layout: {error}"))?;
    compile_layout(layout)
}

/// Validates and normalizes a parsed public layout into an execution plan.
///
/// # Errors
///
/// Returns an error when the layout violates any shared static layout rule.
pub fn compile_layout(layout: Layout) -> Result<Plan, String> {
    if layout.round < 1 {
        return Err("layout round must be at least 1".to_owned());
    }
    if layout.seed == Some(0) {
        return Err(
            "layout seed 0 is the native system-random request, not a match seed: omit the field \
             to ask for a generated seed"
                .to_owned(),
        );
    }
    let blue = compile_side("blue", layout.sides.blue, layout.round)?;
    let red = compile_side("red", layout.sides.red, layout.round)?;
    validate_placement_footprints("blue", &blue.units)?;
    validate_placement_footprints("red", &red.units)?;
    validate_placement_footprints("blue", &blue.constructions)?;
    validate_placement_footprints("red", &red.constructions)?;
    validate_placement_footprints("blue", &blue.contraptions)?;
    validate_placement_footprints("red", &red.contraptions)?;
    validate_placement_collisions(&blue, &red)?;
    if layout.map_id.is_some_and(|id| id <= 0) {
        return Err("layout map_id must be positive".into());
    }
    Ok(Plan {
        map_id: layout.map_id,
        seed: layout.seed,
        round: layout.round,
        blue,
        red,
    })
}

fn compile_side(side_name: &str, side: Side, round: i32) -> Result<SidePlan, String> {
    validate_techs(side_name, &side)?;
    validate_side_modifiers(side_name, &side)?;
    let Side {
        officers,
        techs,
        blueprints,
        energy_tower_skills,
        tower_strengthen_levels,
        units,
        constructions,
        contraptions,
        airdrop_shields,
        terrains,
        battle_skills,
    } = side;
    let units = compile_units(side_name, units, round)?;
    let constructions = compile_constructions(side_name, constructions)?;
    let contraptions = compile_contraptions(side_name, contraptions)?;
    let airdrop_shields = compile_airdrop_shields(side_name, airdrop_shields)?;
    let terrains = compile_terrains(side_name, terrains)?;
    let battle_skills = compile_battle_skills(side_name, battle_skills)?;
    // A chain blueprint is applied as the officer it hands out, which is what
    // a fight reads.
    let mut officers = officers;
    officers.extend(
        blueprints
            .iter()
            .filter_map(|blueprint| crate::catalog::chain_officer(*blueprint)),
    );
    officers.sort_unstable();
    Ok(SidePlan {
        techs: Techs {
            officers,
            units: techs,
        },
        energy_tower_skills,
        tower_strengthen_levels,
        units,
        constructions,
        contraptions,
        airdrop_shields,
        terrains,
        battle_skills,
    })
}

#[allow(clippy::too_many_lines)]
fn compile_units(
    side_name: &str,
    definitions: Vec<UnitPlacement>,
    round: i32,
) -> Result<Vec<Placement>, String> {
    let placements = definitions
        .into_iter()
        .map(|formation| {
            let UnitPlacement {
                type_name,
                index,
                position,
                level,
                exp,
                rotated,
                equipment,
                travelling,
            } = formation;
            let spec = resolve_unit_type(&type_name).ok_or_else(|| {
                if resolve_construction_type(&type_name).is_some() {
                    return format!(
                        "side {side_name} unit type {type_name:?} at ({}, {}) belongs in constructions",
                        position.x, position.y
                    );
                }
                if resolve_contraption_type(&type_name).is_some() {
                    return format!(
                        "side {side_name} unit type {type_name:?} at ({}, {}) belongs in contraptions",
                        position.x, position.y
                    );
                }
                format!(
                    "side {side_name} unit type {type_name:?} at ({}, {}) is unknown",
                    position.x, position.y
                )
            })?;
            let NativeFormation::Unit(unit_id) = spec.native else {
                unreachable!("unit resolver returned non-unit placement")
            };
            let level = level.unwrap_or(1);
            let rotated = rotated.unwrap_or(false);
            let travelling = travelling.unwrap_or(false);
            if !(1..=9).contains(&level) {
                return Err(format!(
                    "side {side_name} unit type {type_name:?} at ({}, {}) level must be 1..=9",
                    position.x, position.y
                ));
            }
            // The native call takes the current experience. The maximum is
            // the level's full bar, which the document states and the table
            // decides, so the two have to agree.
            let exp = match exp {
                None => 0,
                Some(exp) => {
                    let full = crate::experience::full(&type_name, level);
                    if full != Some(exp.maximum) {
                        return Err(format!(
                            "side {side_name} unit type {type_name:?} at ({}, {}) exp \
                             maximum {} is not its level {level} bar {full:?}",
                            position.x, position.y, exp.maximum
                        ));
                    }
                    exp.current
                }
            };
            if index < 0 {
                return Err(format!(
                    "side {side_name} unit type {type_name:?} at ({}, {}) index must be non-negative",
                    position.x, position.y
                ));
            }
            if exp < 0 {
                return Err(format!(
                    "side {side_name} unit type {type_name:?} at ({}, {}) exp must be non-negative",
                    position.x, position.y
                ));
            }
            if equipment.is_some_and(|id| id <= 0) {
                return Err(format!(
                    "side {side_name} unit type {type_name:?} at ({}, {}) equipment must be a positive integer",
                    position.x, position.y
                ));
            }
            validate_unit_placement(side_name, &type_name, position, travelling, round)?;
            Ok(Placement {
                type_name,
                native: NativeFormation::Unit(unit_id),
                footprint: spec.footprint,
                position,
                index: Some(index),
                level: Some(level),
                exp: Some(exp),
                rotated,
                equipment,
                travelling,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    if placements.is_empty() {
        return Err(format!(
            "side {side_name} units must contain at least one valid unit"
        ));
    }
    validate_increasing_indices(side_name, "unit", &placements)?;
    Ok(placements)
}

/// A deployment index is a cross-round identity allocated in deployment order,
/// so a collection is in normal form only when it is sorted by it. Indices are
/// never reused, so a sold or destroyed object leaves a permanent gap.
fn validate_increasing_indices(
    side_name: &str,
    kind: &str,
    placements: &[Placement],
) -> Result<(), String> {
    let mut previous = None;
    for placement in placements {
        let index = placement
            .index
            .expect("indexed placements carry their index");
        if index < 0 {
            return Err(format!(
                "side {side_name} {kind} index must be non-negative, got {index}"
            ));
        }
        if previous.is_some_and(|previous| index <= previous) {
            return Err(format!(
                "side {side_name} {kind} indices must be strictly increasing in declaration order; found {index} after {}",
                previous.expect("checked as some")
            ));
        }
        previous = Some(index);
    }
    Ok(())
}

fn compile_constructions(
    side_name: &str,
    definitions: Vec<StaticPlacement>,
) -> Result<Vec<Placement>, String> {
    definitions
        .into_iter()
        .map(|definition| {
            let StaticPlacement {
                type_name,
                index,
                position,
            } = definition;
            let spec = resolve_construction_type(&type_name).ok_or_else(|| {
                format!(
                    "side {side_name} construction type {type_name:?} at ({}, {}) is unknown",
                    position.x, position.y
                )
            })?;
            Ok(Placement {
                type_name,
                native: spec.native,
                footprint: spec.footprint,
                position,
                index: Some(index),
                level: None,
                exp: None,
                rotated: false,
                equipment: None,
                travelling: false,
            })
        })
        .collect::<Result<Vec<_>, String>>()
        .and_then(|placements| {
            validate_increasing_indices(side_name, "construction", &placements)?;
            Ok(placements)
        })
}

fn compile_contraptions(
    side_name: &str,
    definitions: Vec<ContraptionPlacement>,
) -> Result<Vec<Placement>, String> {
    definitions
        .into_iter()
        .map(|definition| {
            let ContraptionPlacement {
                type_name,
                index,
                position,
            } = definition;
            let spec = resolve_contraption_type(&type_name).ok_or_else(|| {
                format!(
                    "side {side_name} contraption type {type_name:?} at ({}, {}) is unknown",
                    position.x, position.y
                )
            })?;
            Ok(Placement {
                type_name,
                native: spec.native,
                footprint: spec.footprint,
                position,
                index: Some(index),
                level: None,
                exp: None,
                rotated: false,
                equipment: None,
                travelling: false,
            })
        })
        .collect::<Result<Vec<_>, String>>()
        .and_then(|placements| {
            validate_increasing_indices(side_name, "contraption", &placements)?;
            Ok(placements)
        })
}

/// A retained Shield Airdrop is an existing world object, not a contraption
/// release, so it only has to stand on the battlefield.
fn compile_airdrop_shields(
    side_name: &str,
    shields: Vec<Position>,
) -> Result<Vec<Position>, String> {
    for (shield_index, &position) in shields.iter().enumerate() {
        if !position_within(
            position,
            BATTLEFIELD_MIN_X,
            BATTLEFIELD_MAX_X,
            BATTLEFIELD_MIN_Y,
            BATTLEFIELD_MAX_Y,
        ) {
            return Err(format!(
                "side {side_name} airdrop_shields[{shield_index}] center ({}, {}) is outside the battlefield",
                position.x, position.y
            ));
        }
    }
    Ok(shields)
}

fn compile_terrains(side_name: &str, terrains: Vec<Terrain>) -> Result<Vec<Terrain>, String> {
    for (terrain_index, terrain) in terrains.iter().enumerate() {
        let type_name = terrain_type_name(terrain.terrain_type);
        // A terrain's own document says where it is and how much of it is
        // left. How wide each point is and how many points a release expands
        // into belong to the skill that made it, and only one of them is
        // measured, so the rest are refused here rather than checked against
        // the wrong numbers. A state may still carry one: a recording that
        // holds it is described, and a plan is what cannot be built from it.
        let Some(radius) = terrain_radius(terrain.terrain_type) else {
            return Err(format!(
                "side {side_name} terrain[{terrain_index}] type {type_name:?} has no measured \
                 point radius or count in this build, so it cannot be compiled into a plan"
            ));
        };
        if terrain.control_points.len() != 2 {
            return Err(format!(
                "side {side_name} terrain[{terrain_index}] type {type_name:?} control_points must contain exactly two points"
            ));
        }
        let min_x = terrain
            .control_points
            .iter()
            .map(|position| i64::from(position.x))
            .min()
            .expect("two control points");
        let max_x = terrain
            .control_points
            .iter()
            .map(|position| i64::from(position.x))
            .max()
            .expect("two control points");
        let min_y = terrain
            .control_points
            .iter()
            .map(|position| i64::from(position.y))
            .min()
            .expect("two control points");
        let max_y = terrain
            .control_points
            .iter()
            .map(|position| i64::from(position.y))
            .max()
            .expect("two control points");
        if max_x + radius < BATTLEFIELD_MIN_X
            || min_x - radius > BATTLEFIELD_MAX_X
            || max_y + radius < BATTLEFIELD_MIN_Y
            || min_y - radius > BATTLEFIELD_MAX_Y
        {
            return Err(format!(
                "side {side_name} terrain[{terrain_index}] type {type_name:?} path does not overlap the battlefield"
            ));
        }
        if terrain.grid_rows.is_empty() {
            continue;
        }
        for (&point_index, rows) in &terrain.grid_rows {
            if point_index >= OIL_TERRAIN_POINT_COUNT {
                return Err(format!(
                    "side {side_name} terrain[{terrain_index}] type {type_name:?} grid_rows point index {point_index} must be within 0..{}",
                    OIL_TERRAIN_POINT_COUNT - 1
                ));
            }
            if rows.is_empty() {
                continue;
            }
            if rows.len() != OIL_TERRAIN_GRID_SIZE {
                return Err(format!(
                    "side {side_name} terrain[{terrain_index}] type {type_name:?} grid_rows[{point_index}] must be empty or contain exactly {OIL_TERRAIN_GRID_SIZE} rows"
                ));
            }
            for (row_index, &row) in rows.iter().enumerate() {
                if row & !OIL_TERRAIN_GRID_MASK != 0 {
                    return Err(format!(
                        "side {side_name} terrain[{terrain_index}] type {type_name:?} grid_rows[{point_index}][{row_index}] uses bits outside width {OIL_TERRAIN_GRID_SIZE}"
                    ));
                }
            }
            if rows.iter().all(|&row| row == 0) {
                return Err(format!(
                    "side {side_name} terrain[{terrain_index}] type {type_name:?} grid_rows[{point_index}] must activate at least one cell"
                ));
            }
        }
    }
    Ok(terrains)
}

/// The public word for one terrain.
pub(crate) const fn terrain_type_name(terrain: TerrainType) -> &'static str {
    match terrain {
        TerrainType::Fire => "fire",
        TerrainType::Oil => "oil",
        TerrainType::Fog => "fog",
        TerrainType::Acid => "acid",
        TerrainType::RecoveryZone => "recovery_zone",
    }
}

/// How wide one of a terrain's points is, when this build has been measured
/// for it.
///
/// The radius is the producing skill's `subEffectRange`, and the point count
/// its `subEffectCount`. `docs/rules/battle_skill.md` carries the first for
/// every skill and the second for none, so only the terrain whose count was
/// read out of a snapshot can be bounded, which is Sticky Oil Bomb's.
const fn terrain_radius(terrain: TerrainType) -> Option<i64> {
    match terrain {
        TerrainType::Oil => Some(OIL_TERRAIN_RADIUS),
        _ => None,
    }
}

fn compile_battle_skills(
    side_name: &str,
    definitions: Vec<BattleSkillDefinition>,
) -> Result<Vec<BattleSkill>, String> {
    let mut skills = Vec::with_capacity(definitions.len());
    for definition in definitions {
        let BattleSkillDefinition {
            type_name,
            positions,
        } = definition;
        let spec = resolve_battle_skill_type(&type_name).ok_or_else(|| {
            format!("side {side_name} battle skill type {type_name:?} is unknown")
        })?;
        if positions.len() != spec.positions {
            return Err(format!(
                "side {side_name} battle skill type {type_name:?} requires {} positions, got {}",
                spec.positions,
                positions.len()
            ));
        }
        if skills
            .iter()
            .any(|skill: &BattleSkill| skill.commander_skill_id == spec.commander_skill_id)
        {
            return Err(format!(
                "side {side_name} battle skill type {type_name:?} is declared more than once"
            ));
        }
        validate_battle_skill_positions(side_name, &type_name, &positions, spec)?;
        skills.push(BattleSkill {
            type_name,
            commander_skill_id: spec.commander_skill_id,
            positions,
        });
    }
    Ok(skills)
}

fn validate_battle_skill_positions(
    side_name: &str,
    type_name: &str,
    positions: &[Position],
    spec: BattleSkillSpec,
) -> Result<(), String> {
    let valid_map_position = match spec.map_rule {
        BattleSkillMapRule::Overlap => battle_skill_overlaps_map(positions, spec.shape),
        BattleSkillMapRule::Center => positions.iter().all(|&position| {
            position_within(
                position,
                BATTLEFIELD_MIN_X,
                BATTLEFIELD_MAX_X,
                BATTLEFIELD_MIN_Y,
                BATTLEFIELD_MAX_Y,
            )
        }),
        BattleSkillMapRule::Contained => {
            let margin = battle_skill_margin(spec.shape);
            positions.iter().all(|&position| {
                position_within(
                    position,
                    BATTLEFIELD_MIN_X + margin,
                    BATTLEFIELD_MAX_X - margin,
                    BATTLEFIELD_MIN_Y + margin,
                    BATTLEFIELD_MAX_Y - margin,
                )
            })
        }
    };
    if !valid_map_position {
        return Err(format!(
            "side {side_name} battle skill type {type_name:?} at {positions:?} violates its battlefield map rule within x=[{BATTLEFIELD_MIN_X},{BATTLEFIELD_MAX_X}], y=[{BATTLEFIELD_MIN_Y},{BATTLEFIELD_MAX_Y}]"
        ));
    }

    if let Some(radius) = spec.tower_exclusion_radius {
        let minimum_distance = ENEMY_TOWER_PROTECTION_RANGE + radius;
        let minimum_distance_squared = i128::from(minimum_distance).pow(2);
        for &position in positions {
            let x = i64::from(position.x);
            let y = i64::from(position.y);
            for &tower_x in &ENEMY_TOWER_X {
                let dx = i128::from(x - tower_x);
                let dy = i128::from(y - ENEMY_TOWER_Y);
                if dx * dx + dy * dy <= minimum_distance_squared {
                    return Err(format!(
                        "side {side_name} battle skill type {type_name:?} at ({}, {}) must be more than {minimum_distance} m from enemy tower center ({tower_x}, {ENEMY_TOWER_Y})",
                        position.x, position.y
                    ));
                }
            }
        }
    }
    Ok(())
}

const fn battle_skill_margin(shape: BattleSkillShape) -> i64 {
    match shape {
        BattleSkillShape::Circle { radius } => radius,
        BattleSkillShape::RandomCircle {
            outer_radius,
            radius,
        } => outer_radius + radius,
        BattleSkillShape::Line { width } | BattleSkillShape::Path { width } => width / 2,
    }
}

fn battle_skill_overlaps_map(positions: &[Position], shape: BattleSkillShape) -> bool {
    match shape {
        BattleSkillShape::Circle { radius } => positions
            .iter()
            .any(|&position| circle_overlaps_map(position, radius)),
        BattleSkillShape::RandomCircle {
            outer_radius,
            radius,
        } => positions
            .iter()
            .any(|&position| circle_overlaps_map(position, outer_radius + radius)),
        BattleSkillShape::Line { width } | BattleSkillShape::Path { width } => positions
            .windows(2)
            .any(|segment| thick_segment_overlaps_map(segment[0], segment[1], width / 2)),
    }
}

fn circle_overlaps_map(position: Position, radius: i64) -> bool {
    let x = i64::from(position.x);
    let y = i64::from(position.y);
    let nearest_x = x.clamp(BATTLEFIELD_MIN_X, BATTLEFIELD_MAX_X);
    let nearest_y = y.clamp(BATTLEFIELD_MIN_Y, BATTLEFIELD_MAX_Y);
    let dx = i128::from(x - nearest_x);
    let dy = i128::from(y - nearest_y);
    dx * dx + dy * dy <= i128::from(radius).pow(2)
}

fn thick_segment_overlaps_map(start: Position, end: Position, radius: i64) -> bool {
    let start = (i64::from(start.x), i64::from(start.y));
    let end = (i64::from(end.x), i64::from(end.y));
    let corners = [
        (BATTLEFIELD_MIN_X, BATTLEFIELD_MIN_Y),
        (BATTLEFIELD_MIN_X, BATTLEFIELD_MAX_Y),
        (BATTLEFIELD_MAX_X, BATTLEFIELD_MAX_Y),
        (BATTLEFIELD_MAX_X, BATTLEFIELD_MIN_Y),
    ];
    if point_in_battlefield(start)
        || point_in_battlefield(end)
        || corners
            .iter()
            .copied()
            .zip(corners.iter().copied().cycle().skip(1))
            .take(4)
            .any(|(left, right)| segments_intersect(start, end, left, right))
    {
        return true;
    }

    let radius_squared = i128::from(radius).pow(2);
    point_to_map_distance_squared(start) <= radius_squared
        || point_to_map_distance_squared(end) <= radius_squared
        || corners
            .iter()
            .copied()
            .any(|corner| point_within_segment_distance(corner, start, end, radius_squared))
}

fn point_in_battlefield((x, y): (i64, i64)) -> bool {
    (BATTLEFIELD_MIN_X..=BATTLEFIELD_MAX_X).contains(&x)
        && (BATTLEFIELD_MIN_Y..=BATTLEFIELD_MAX_Y).contains(&y)
}

fn point_to_map_distance_squared((x, y): (i64, i64)) -> i128 {
    let nearest_x = x.clamp(BATTLEFIELD_MIN_X, BATTLEFIELD_MAX_X);
    let nearest_y = y.clamp(BATTLEFIELD_MIN_Y, BATTLEFIELD_MAX_Y);
    let dx = i128::from(x - nearest_x);
    let dy = i128::from(y - nearest_y);
    dx * dx + dy * dy
}

fn point_within_segment_distance(
    point: (i64, i64),
    start: (i64, i64),
    end: (i64, i64),
    radius_squared: i128,
) -> bool {
    let dx = i128::from(end.0 - start.0);
    let dy = i128::from(end.1 - start.1);
    let offset_x = i128::from(point.0 - start.0);
    let offset_y = i128::from(point.1 - start.1);
    let length_squared = dx * dx + dy * dy;
    if length_squared == 0 {
        return offset_x * offset_x + offset_y * offset_y <= radius_squared;
    }
    let projection = offset_x * dx + offset_y * dy;
    if projection <= 0 {
        return offset_x * offset_x + offset_y * offset_y <= radius_squared;
    }
    if projection >= length_squared {
        let end_x = i128::from(point.0 - end.0);
        let end_y = i128::from(point.1 - end.1);
        return end_x * end_x + end_y * end_y <= radius_squared;
    }
    let perpendicular = offset_x * dy - offset_y * dx;
    perpendicular
        .checked_mul(perpendicular)
        .is_some_and(|distance| distance <= radius_squared * length_squared)
}

fn segments_intersect(
    first_start: (i64, i64),
    first_end: (i64, i64),
    second_start: (i64, i64),
    second_end: (i64, i64),
) -> bool {
    let first_left = cross(first_start, first_end, second_start);
    let first_right = cross(first_start, first_end, second_end);
    let second_left = cross(second_start, second_end, first_start);
    let second_right = cross(second_start, second_end, first_end);
    if first_left == 0 && point_on_segment(second_start, first_start, first_end)
        || first_right == 0 && point_on_segment(second_end, first_start, first_end)
        || second_left == 0 && point_on_segment(first_start, second_start, second_end)
        || second_right == 0 && point_on_segment(first_end, second_start, second_end)
    {
        return true;
    }
    first_left.signum() != first_right.signum() && second_left.signum() != second_right.signum()
}

fn cross(origin: (i64, i64), left: (i64, i64), right: (i64, i64)) -> i128 {
    i128::from(left.0 - origin.0) * i128::from(right.1 - origin.1)
        - i128::from(left.1 - origin.1) * i128::from(right.0 - origin.0)
}

fn point_on_segment(point: (i64, i64), start: (i64, i64), end: (i64, i64)) -> bool {
    (start.0.min(end.0)..=start.0.max(end.0)).contains(&point.0)
        && (start.1.min(end.1)..=start.1.max(end.1)).contains(&point.1)
}

fn validate_placement_footprints(side_name: &str, placements: &[Placement]) -> Result<(), String> {
    for placement in placements {
        match placement.native {
            NativeFormation::Contraption(10001) => {
                validate_shield_position(side_name, placement)?;
                continue;
            }
            NativeFormation::Contraption(20001) => {
                validate_missile_position(side_name, placement)?;
                continue;
            }
            _ => {}
        }
        let (width, height) = placement_footprint(placement)
            .ok_or_else(|| missing_footprint(side_name, placement))?;
        let x = i64::from(placement.position.x);
        let y = i64::from(placement.position.y);
        let min_x = x - width / 2;
        let max_x = x + width / 2;
        let min_y = y - height / 2;
        let max_y = y + height / 2;
        let required_x = grid_center_remainder(width).ok_or_else(|| {
            format!(
                "side {side_name} placement type {:?} at ({}, {}) has unsupported footprint width {width}",
                placement.type_name, placement.position.x, placement.position.y
            )
        })?;
        let required_y = grid_center_remainder(height).ok_or_else(|| {
            format!(
                "side {side_name} placement type {:?} at ({}, {}) has unsupported footprint height {height}",
                placement.type_name, placement.position.x, placement.position.y
            )
        })?;
        if x.rem_euclid(10) != required_x || y.rem_euclid(10) != required_y {
            return Err(format!(
                "side {side_name} placement type {:?} at ({}, {}) is off the native 10x10 grid: footprint {width}x{height} requires center x≡{required_x}, y≡{required_y} (mod 10)",
                placement.type_name, placement.position.x, placement.position.y,
            ));
        }
        if is_ambush_unit(placement) {
            let inside_left = rectangle_within(
                min_x,
                max_x,
                min_y,
                max_y,
                AMBUSH_LEFT_MIN_X,
                AMBUSH_LEFT_MAX_X,
                AMBUSH_MIN_Y,
                AMBUSH_MAX_Y,
            );
            let inside_right = rectangle_within(
                min_x,
                max_x,
                min_y,
                max_y,
                AMBUSH_RIGHT_MIN_X,
                AMBUSH_RIGHT_MAX_X,
                AMBUSH_MIN_Y,
                AMBUSH_MAX_Y,
            );
            if !inside_left && !inside_right {
                return Err(format!(
                    "side {side_name} placement type {:?} at ({}, {}) footprint {width}x{height} must fit completely inside one ambush zone: left x=[{AMBUSH_LEFT_MIN_X},{AMBUSH_LEFT_MAX_X}] or right x=[{AMBUSH_RIGHT_MIN_X},{AMBUSH_RIGHT_MAX_X}], y=[{AMBUSH_MIN_Y},{AMBUSH_MAX_Y}]",
                    placement.type_name, placement.position.x, placement.position.y
                ));
            }
        } else if !rectangle_within(
            min_x,
            max_x,
            min_y,
            max_y,
            DEPLOYMENT_MIN_X,
            DEPLOYMENT_MAX_X,
            DEPLOYMENT_MIN_Y,
            DEPLOYMENT_MAX_Y,
        ) {
            return Err(format!(
                "side {side_name} placement type {:?} at ({}, {}) footprint {width}x{height} exceeds the main deployment boundary x=[{DEPLOYMENT_MIN_X},{DEPLOYMENT_MAX_X}], y=[{DEPLOYMENT_MIN_Y},{DEPLOYMENT_MAX_Y}]",
                placement.type_name, placement.position.x, placement.position.y
            ));
        }
    }
    Ok(())
}

fn validate_shield_position(side_name: &str, placement: &Placement) -> Result<(), String> {
    let min_x = DEPLOYMENT_MIN_X + SHIELD_RADIUS;
    let max_x = DEPLOYMENT_MAX_X - SHIELD_RADIUS;
    let min_y = DEPLOYMENT_MIN_Y + SHIELD_RADIUS;
    let max_y = DEPLOYMENT_MAX_Y - SHIELD_RADIUS;
    if position_within(placement.position, min_x, max_x, min_y, max_y) {
        Ok(())
    } else {
        Err(format!(
            "side {side_name} placement type {:?} at ({}, {}) places its radius-{SHIELD_RADIUS} edge outside the own-side deployment boundary: center must be within x=[{min_x},{max_x}], y=[{min_y},{max_y}]",
            placement.type_name, placement.position.x, placement.position.y
        ))
    }
}

fn validate_missile_position(side_name: &str, placement: &Placement) -> Result<(), String> {
    if position_within(
        placement.position,
        DEPLOYMENT_MIN_X,
        DEPLOYMENT_MAX_X,
        DEPLOYMENT_MIN_Y,
        DEPLOYMENT_MAX_Y,
    ) {
        Ok(())
    } else {
        Err(format!(
            "side {side_name} placement type {:?} at ({}, {}) is outside the own-side deployment boundary x=[{DEPLOYMENT_MIN_X},{DEPLOYMENT_MAX_X}], y=[{DEPLOYMENT_MIN_Y},{DEPLOYMENT_MAX_Y}]",
            placement.type_name, placement.position.x, placement.position.y
        ))
    }
}

fn position_within(position: Position, min_x: i64, max_x: i64, min_y: i64, max_y: i64) -> bool {
    let x = i64::from(position.x);
    let y = i64::from(position.y);
    (min_x..=max_x).contains(&x) && (min_y..=max_y).contains(&y)
}

#[allow(clippy::too_many_arguments)]
const fn rectangle_within(
    min_x: i64,
    max_x: i64,
    min_y: i64,
    max_y: i64,
    bound_min_x: i64,
    bound_max_x: i64,
    bound_min_y: i64,
    bound_max_y: i64,
) -> bool {
    min_x >= bound_min_x && max_x <= bound_max_x && min_y >= bound_min_y && max_y <= bound_max_y
}

pub(crate) fn grid_center_remainder(extent: i64) -> Option<i64> {
    match extent.rem_euclid(20) {
        0 => Some(0),
        10 => Some(5),
        _ => None,
    }
}

fn validate_placement_collisions(blue: &SidePlan, red: &SidePlan) -> Result<(), String> {
    let mut world = Vec::with_capacity(
        blue.units.len()
            + blue.constructions.len()
            + blue.contraptions.len()
            + red.units.len()
            + red.constructions.len()
            + red.contraptions.len(),
    );
    world.extend(
        blue.units
            .iter()
            .chain(&blue.constructions)
            .chain(&blue.contraptions)
            .filter(|placement| participates_in_collision(placement))
            .map(|placement| {
                (
                    "blue",
                    placement,
                    i64::from(placement.position.x),
                    i64::from(placement.position.y),
                )
            }),
    );
    world.extend(
        red.units
            .iter()
            .chain(&red.constructions)
            .chain(&red.contraptions)
            .filter(|placement| participates_in_collision(placement))
            .map(|placement| {
                (
                    "red",
                    placement,
                    -i64::from(placement.position.x),
                    -i64::from(placement.position.y),
                )
            }),
    );

    for (index, &(left_side, left, left_x, left_y)) in world.iter().enumerate() {
        let (left_width, left_height) =
            placement_footprint(left).ok_or_else(|| missing_footprint(left_side, left))?;
        for &(right_side, right, right_x, right_y) in &world[index + 1..] {
            let (right_width, right_height) =
                placement_footprint(right).ok_or_else(|| missing_footprint(right_side, right))?;
            let overlaps_x = (left_x - right_x).abs() * 2 < left_width + right_width;
            let overlaps_y = (left_y - right_y).abs() * 2 < left_height + right_height;
            if overlaps_x && overlaps_y {
                return Err(format!(
                    "placements collide: {left_side} type {:?} at ({}, {}) and {right_side} type {:?} at ({}, {})",
                    left.type_name,
                    left.position.x,
                    left.position.y,
                    right.type_name,
                    right.position.x,
                    right.position.y
                ));
            }
        }
    }
    Ok(())
}

const fn participates_in_collision(placement: &Placement) -> bool {
    !matches!(
        placement.native,
        NativeFormation::Contraption(10001 | 20001)
    )
}

fn missing_footprint(side_name: &str, placement: &Placement) -> String {
    format!(
        "side {side_name} placement type {:?} at ({}, {}) has no deployment footprint",
        placement.type_name, placement.position.x, placement.position.y
    )
}

pub(crate) fn placement_footprint(placement: &Placement) -> Option<(i64, i64)> {
    let (width, height) = placement.footprint?;
    Some(
        if matches!(placement.native, NativeFormation::Unit(_))
            && (placement.rotated ^ is_ambush_unit(placement))
        {
            (height, width)
        } else {
            (width, height)
        },
    )
}

fn is_ambush_unit(placement: &Placement) -> bool {
    matches!(placement.native, NativeFormation::Unit(_))
        && Region::of(placement.position).is_flank()
}

fn validate_unit_placement(
    side_name: &str,
    type_name: &str,
    position: Position,
    travelling: bool,
    round: i32,
) -> Result<(), String> {
    let in_ambush = Region::of(position).is_flank();
    if travelling && !in_ambush {
        return Err(format!(
            "side {side_name} unit type {type_name:?} at ({}, {}) sets travelling=true outside the ambush zones",
            position.x, position.y
        ));
    }
    if !in_ambush {
        return Ok(());
    }
    if round == 1 {
        return Err(format!(
            "side {side_name} unit type {type_name:?} at ({}, {}) cannot occupy an ambush zone in round 1",
            position.x, position.y
        ));
    }
    if round == 2 && !travelling {
        return Err(format!(
            "side {side_name} unit type {type_name:?} at ({}, {}) must set travelling=true: a round 2 ambush unit is always a first flank deployment",
            position.x, position.y
        ));
    }
    Ok(())
}

fn validate_techs(side_name: &str, side: &Side) -> Result<(), String> {
    // An Officer may repeat. Some Officer cards can be taken again, and taking
    // one twice stacks it rather than doing nothing: two copies of Advanced
    // Offensive Tactics are +60% damage, which a fight plainly sees. A unit
    // technology has no second copy to hold.
    for (index, &id) in side.officers.iter().enumerate() {
        if id <= 0 {
            return Err(format!(
                "side {side_name} officers[{index}] must be a positive integer"
            ));
        }
        // A chain's officer is stated by its blueprint, once.
        if crate::catalog::chain_blueprint(id).is_some() {
            return Err(format!(
                "side {side_name} officers[{index}] is a chain blueprint's officer; \
                 name its blueprint in blueprints instead"
            ));
        }
    }
    validate_unique_positive_ids(side_name, "techs", &side.techs)?;
    validate_unique_positive_ids(side_name, "blueprints", &side.blueprints)?;
    for &blueprint in &side.blueprints {
        if crate::catalog::chain_officer(blueprint).is_none() {
            return Err(format!(
                "side {side_name} blueprints holds {blueprint}, which is not an \
                 enhancement chain; a layout lists only the chains a fight sees"
            ));
        }
    }
    // A chain's second level replaces its first.
    for (first, second) in [(4, 401), (5, 501)] {
        if side.blueprints.contains(&first) && side.blueprints.contains(&second) {
            return Err(format!(
                "side {side_name} blueprints holds both levels of one chain"
            ));
        }
    }
    Ok(())
}

fn validate_unique_positive_ids(side_name: &str, field: &str, ids: &[i32]) -> Result<(), String> {
    for (index, &id) in ids.iter().enumerate() {
        if id <= 0 {
            return Err(format!(
                "side {side_name} {field}[{index}] must be a positive integer"
            ));
        }
        if ids[..index].contains(&id) {
            return Err(format!(
                "side {side_name} {field} contains duplicate ID {id}"
            ));
        }
    }
    Ok(())
}

fn validate_side_modifiers(side_name: &str, side: &Side) -> Result<(), String> {
    let skills = &side.energy_tower_skills;
    for (index, &skill) in skills.iter().enumerate() {
        if !FIGHT_VISIBLE_ENERGY_TOWER_SKILLS.contains(&skill) {
            return Err(format!(
                "side {side_name} energy_tower_skills[{index}] is {skill}, which no fight can see: a layout carries {FIGHT_VISIBLE_ENERGY_TOWER_SKILLS:?}"
            ));
        }
        if skills[..index].contains(&skill) {
            return Err(format!(
                "side {side_name} energy_tower_skills contains duplicate ID {skill}"
            ));
        }
    }

    let levels = &side.tower_strengthen_levels;
    if !levels.is_empty() && levels.len() != TOWER_COUNT {
        return Err(format!(
            "side {side_name} tower_strengthen_levels must hold {TOWER_COUNT} levels, one per fixed tower, or none at all"
        ));
    }
    for (index, &level) in levels.iter().enumerate() {
        if !(0..=MAX_TOWER_STRENGTHEN_LEVEL).contains(&level) {
            return Err(format!(
                "side {side_name} tower_strengthen_levels[{index}] must be 0..={MAX_TOWER_STRENGTHEN_LEVEL}"
            ));
        }
    }
    Ok(())
}
