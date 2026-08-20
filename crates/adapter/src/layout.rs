use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Layout {
    round: i32,
    sides: Sides,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Sides {
    blue: Side,
    red: Side,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Side {
    #[serde(default)]
    techs: Techs,
    #[serde(default)]
    research_center: ResearchCenter,
    #[serde(default)]
    energy_tower: EnergyTower,
    formations: Vec<Formation>,
    #[serde(default)]
    battle_skills: Vec<BattleSkillDefinition>,
}

#[derive(Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Techs {
    pub(crate) officers: Vec<i32>,
    pub(crate) units: Vec<i32>,
}

#[derive(Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
#[allow(clippy::struct_field_names)] // Field names are fixed by the public layout schema.
pub(crate) struct ResearchCenter {
    pub(crate) strength_level: i32,
    pub(crate) attack_level: i32,
    pub(crate) defense_level: i32,
}

#[derive(Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct EnergyTower {
    pub(crate) strength_level: i32,
    pub(crate) range_enhancement: bool,
    pub(crate) movement_enhancement: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Formation {
    #[serde(rename = "type")]
    type_name: String,
    x: i32,
    y: i32,
    level: Option<i32>,
    rotated: Option<bool>,
    equipment: Option<i32>,
    travelling: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BattleSkillDefinition {
    #[serde(rename = "type")]
    type_name: String,
    positions: Vec<Position>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NativeFormation {
    Unit(i32),
    Construction(i32),
    Contraption(i32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PlacementStage {
    PreActivation,
    Activation,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Position {
    pub(crate) x: i32,
    pub(crate) y: i32,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct BattleSkill {
    pub(crate) type_name: String,
    pub(crate) commander_skill_id: i32,
    pub(crate) positions: Vec<Position>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Placement {
    pub(crate) type_name: String,
    pub(crate) native: NativeFormation,
    pub(crate) position: Position,
    pub(crate) level: Option<i32>,
    pub(crate) rotated: bool,
    pub(crate) equipment: Option<i32>,
    pub(crate) travelling: bool,
    pub(crate) stage: PlacementStage,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct SidePlan {
    pub(crate) techs: Techs,
    pub(crate) research_center: ResearchCenter,
    pub(crate) energy_tower: EnergyTower,
    pub(crate) formations: Vec<Placement>,
    pub(crate) battle_skills: Vec<BattleSkill>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Plan {
    pub(crate) round: i32,
    pub(crate) blue: SidePlan,
    pub(crate) red: SidePlan,
}

const DEPLOYMENT_MIN_X: i64 = -300;
const DEPLOYMENT_MAX_X: i64 = 300;
const DEPLOYMENT_MIN_Y: i64 = -310;
const DEPLOYMENT_MAX_Y: i64 = -10;
const AMBUSH_LEFT_MIN_X: i64 = -360;
const AMBUSH_LEFT_MAX_X: i64 = -300;
const AMBUSH_RIGHT_MIN_X: i64 = 300;
const AMBUSH_RIGHT_MAX_X: i64 = 360;
const AMBUSH_MIN_Y: i64 = 10;
const AMBUSH_MAX_Y: i64 = 310;
const SHIELD_RADIUS: i64 = 70;
const BATTLEFIELD_MIN_X: i64 = -400;
const BATTLEFIELD_MAX_X: i64 = 400;
const BATTLEFIELD_MIN_Y: i64 = -350;
const BATTLEFIELD_MAX_Y: i64 = 350;
const ENEMY_TOWER_X: [i64; 2] = [-140, 140];
const ENEMY_TOWER_Y: i64 = 170;
const ENEMY_TOWER_PROTECTION_RANGE: i64 = 140;

#[derive(Clone, Copy)]
enum BattleSkillShape {
    Circle { radius: i64 },
    RandomCircle { outer_radius: i64, radius: i64 },
    Line { width: i64 },
    Path { width: i64 },
}

#[derive(Clone, Copy)]
enum BattleSkillMapRule {
    Overlap,
    Center,
    Contained,
}

#[derive(Clone, Copy)]
struct BattleSkillSpec {
    commander_skill_id: i32,
    positions: usize,
    shape: BattleSkillShape,
    map_rule: BattleSkillMapRule,
    tower_exclusion_radius: Option<i64>,
}

impl Plan {
    pub(crate) fn formation_count(&self) -> usize {
        self.blue.formations.len() + self.red.formations.len()
    }
}

pub(crate) fn compile(value: &Value) -> Result<Plan, String> {
    let layout: Layout = serde_json::from_value(value.clone())
        .map_err(|error| format!("invalid layout: {error}"))?;
    if layout.round <= 0 {
        return Err("layout round must be a positive integer".into());
    }
    let blue = compile_side("blue", layout.sides.blue, layout.round)?;
    let red = compile_side("red", layout.sides.red, layout.round)?;
    validate_formation_footprints("blue", &blue.formations)?;
    validate_formation_footprints("red", &red.formations)?;
    validate_formation_collisions(&blue.formations, &red.formations)?;
    Ok(Plan {
        round: layout.round,
        blue,
        red,
    })
}

fn compile_side(side_name: &str, side: Side, round: i32) -> Result<SidePlan, String> {
    validate_techs(side_name, &side.techs)?;
    validate_side_modifiers(side_name, &side)?;
    let Side {
        techs,
        research_center,
        energy_tower,
        formations,
        battle_skills,
    } = side;
    let formations = compile_formations(side_name, formations, round)?;
    let battle_skills = compile_battle_skills(side_name, battle_skills)?;
    Ok(SidePlan {
        techs,
        research_center,
        energy_tower,
        formations,
        battle_skills,
    })
}

#[allow(clippy::too_many_lines)]
fn compile_formations(
    side_name: &str,
    definitions: Vec<Formation>,
    round: i32,
) -> Result<Vec<Placement>, String> {
    let placements = definitions
        .into_iter()
        .map(|formation| {
            let Formation {
                type_name,
                x,
                y,
                level,
                rotated,
                equipment,
                travelling,
            } = formation;
            let position = Position { x, y };
            let native = resolve_type(&type_name).ok_or_else(|| {
                format!(
                    "side {side_name} formation type {type_name:?} at ({}, {}) is unknown",
                    position.x, position.y
                )
            })?;
            match native {
                NativeFormation::Unit(unit_id) => {
                    let level = level.unwrap_or(1);
                    let rotated = rotated.unwrap_or(false);
                    let travelling = travelling.unwrap_or(false);
                    if !(1..=9).contains(&level) {
                        return Err(format!(
                            "side {side_name} formation type {type_name:?} at ({}, {}) level must be 1..=9",
                            position.x, position.y
                        ));
                    }
                    if equipment.is_some_and(|id| id <= 0) {
                        return Err(format!(
                            "side {side_name} formation type {type_name:?} at ({}, {}) equipment must be a positive integer",
                            position.x, position.y
                        ));
                    }
                    let stage = unit_placement_stage(
                        side_name,
                        &type_name,
                        position,
                        travelling,
                        round,
                    )?;
                    Ok(Placement {
                        type_name,
                        native: NativeFormation::Unit(unit_id),
                        position,
                        level: Some(level),
                        rotated,
                        equipment,
                        travelling,
                        stage,
                    })
                }
                NativeFormation::Construction(id) => {
                    reject_unit_fields(
                        side_name,
                        &type_name,
                        position,
                        level,
                        rotated,
                        equipment,
                        travelling,
                    )?;
                    Ok(Placement {
                        type_name,
                        native: NativeFormation::Construction(id),
                        position,
                        level: None,
                        rotated: false,
                        equipment: None,
                        travelling: false,
                        stage: PlacementStage::Activation,
                    })
                }
                NativeFormation::Contraption(id) => {
                    reject_unit_fields(
                        side_name,
                        &type_name,
                        position,
                        level,
                        rotated,
                        equipment,
                        travelling,
                    )?;
                    Ok(Placement {
                        type_name,
                        native: NativeFormation::Contraption(id),
                        position,
                        level: None,
                        rotated: false,
                        equipment: None,
                        travelling: false,
                        stage: PlacementStage::Activation,
                    })
                }
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    if !placements
        .iter()
        .any(|placement| matches!(placement.native, NativeFormation::Unit(_)))
    {
        return Err(format!(
            "side {side_name} formations must contain at least one valid unit"
        ));
    }
    Ok(placements)
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

fn validate_formation_footprints(side_name: &str, placements: &[Placement]) -> Result<(), String> {
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
        let (width, height) = formation_footprint(placement)
            .ok_or_else(|| missing_footprint(side_name, placement))?;
        let x = i64::from(placement.position.x);
        let y = i64::from(placement.position.y);
        let min_x = x - width / 2;
        let max_x = x + width / 2;
        let min_y = y - height / 2;
        let max_y = y + height / 2;
        let required_x = grid_center_remainder(width).ok_or_else(|| {
            format!(
                "side {side_name} formation type {:?} at ({}, {}) has unsupported footprint width {width}",
                placement.type_name, placement.position.x, placement.position.y
            )
        })?;
        let required_y = grid_center_remainder(height).ok_or_else(|| {
            format!(
                "side {side_name} formation type {:?} at ({}, {}) has unsupported footprint height {height}",
                placement.type_name, placement.position.x, placement.position.y
            )
        })?;
        if x.rem_euclid(10) != required_x || y.rem_euclid(10) != required_y {
            return Err(format!(
                "side {side_name} formation type {:?} at ({}, {}) is off the native 10x10 grid: footprint {width}x{height} requires center x≡{required_x}, y≡{required_y} (mod 10)",
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
                    "side {side_name} formation type {:?} at ({}, {}) footprint {width}x{height} must fit completely inside one ambush zone: left x=[{AMBUSH_LEFT_MIN_X},{AMBUSH_LEFT_MAX_X}] or right x=[{AMBUSH_RIGHT_MIN_X},{AMBUSH_RIGHT_MAX_X}], y=[{AMBUSH_MIN_Y},{AMBUSH_MAX_Y}]",
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
                "side {side_name} formation type {:?} at ({}, {}) footprint {width}x{height} exceeds the main deployment boundary x=[{DEPLOYMENT_MIN_X},{DEPLOYMENT_MAX_X}], y=[{DEPLOYMENT_MIN_Y},{DEPLOYMENT_MAX_Y}]",
                placement.type_name, placement.position.x, placement.position.y
            ));
        }
    }
    Ok(())
}

fn validate_shield_position(side_name: &str, placement: &Placement) -> Result<(), String> {
    let min_x = DEPLOYMENT_MIN_X + SHIELD_RADIUS + 1;
    let max_x = DEPLOYMENT_MAX_X - SHIELD_RADIUS - 1;
    let min_y = DEPLOYMENT_MIN_Y + SHIELD_RADIUS + 1;
    let max_y = DEPLOYMENT_MAX_Y - SHIELD_RADIUS - 1;
    if position_within(placement.position, min_x, max_x, min_y, max_y) {
        Ok(())
    } else {
        Err(format!(
            "side {side_name} formation type {:?} at ({}, {}) places its radius-{SHIELD_RADIUS} edge outside the own-side deployment boundary: center must be within x=[{min_x},{max_x}], y=[{min_y},{max_y}]",
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
            "side {side_name} formation type {:?} at ({}, {}) is outside the own-side deployment boundary x=[{DEPLOYMENT_MIN_X},{DEPLOYMENT_MAX_X}], y=[{DEPLOYMENT_MIN_Y},{DEPLOYMENT_MAX_Y}]",
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

fn grid_center_remainder(extent: i64) -> Option<i64> {
    match extent.rem_euclid(20) {
        0 => Some(0),
        10 => Some(5),
        _ => None,
    }
}

fn validate_formation_collisions(blue: &[Placement], red: &[Placement]) -> Result<(), String> {
    let mut world = Vec::with_capacity(blue.len() + red.len());
    world.extend(
        blue.iter()
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
        red.iter()
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
            formation_footprint(left).ok_or_else(|| missing_footprint(left_side, left))?;
        for &(right_side, right, right_x, right_y) in &world[index + 1..] {
            let (right_width, right_height) =
                formation_footprint(right).ok_or_else(|| missing_footprint(right_side, right))?;
            let overlaps_x = (left_x - right_x).abs() * 2 < left_width + right_width;
            let overlaps_y = (left_y - right_y).abs() * 2 < left_height + right_height;
            if overlaps_x && overlaps_y {
                return Err(format!(
                    "formations collide: {left_side} type {:?} at ({}, {}) and {right_side} type {:?} at ({}, {})",
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
        "side {side_name} formation type {:?} at ({}, {}) has no deployment footprint",
        placement.type_name, placement.position.x, placement.position.y
    )
}

fn formation_footprint(placement: &Placement) -> Option<(i64, i64)> {
    match placement.native {
        NativeFormation::Unit(id) => {
            unit_footprint(id, placement.rotated ^ is_ambush_unit(placement))
        }
        NativeFormation::Construction(id) => construction_footprint(id),
        NativeFormation::Contraption(id) => contraption_footprint(id),
    }
}

fn is_ambush_unit(placement: &Placement) -> bool {
    matches!(placement.native, NativeFormation::Unit(_))
        && i64::from(placement.position.y) >= AMBUSH_MIN_Y
}

const fn unit_footprint(unit_id: i32, rotated: bool) -> Option<(i64, i64)> {
    let (width, height) = match unit_id {
        2 | 15 | 31 => (20, 20),
        5 | 14 | 18 | 19 | 21 | 24 | 26 => (30, 30),
        16 | 22 | 28 | 30 => (40, 20),
        1 | 3 | 4 | 23 | 27 => (40, 40),
        6 | 7 | 8 | 9 | 10 | 12 | 13 | 20 | 25 => (50, 20),
        11 => (50, 50),
        17 | 29 | 2002 => (70, 70),
        _ => return None,
    };
    Some(if rotated {
        (height, width)
    } else {
        (width, height)
    })
}

const fn construction_footprint(construction_id: i32) -> Option<(i64, i64)> {
    match construction_id {
        1 => Some((60, 10)),
        2 | 3 => Some((20, 20)),
        4 => Some((50, 10)),
        _ => None,
    }
}

const fn contraption_footprint(contraption_id: i32) -> Option<(i64, i64)> {
    match contraption_id {
        30001 => Some((30, 30)),
        _ => None,
    }
}

fn unit_placement_stage(
    side_name: &str,
    type_name: &str,
    position: Position,
    travelling: bool,
    round: i32,
) -> Result<PlacementStage, String> {
    let in_ambush = i64::from(position.y) >= AMBUSH_MIN_Y;
    if travelling && !in_ambush {
        return Err(format!(
            "side {side_name} formation type {type_name:?} at ({}, {}) sets travelling=true outside the ambush zones",
            position.x, position.y
        ));
    }
    if !in_ambush {
        return Ok(PlacementStage::Activation);
    }
    if round == 1 {
        return Err(format!(
            "side {side_name} formation type {type_name:?} at ({}, {}) cannot occupy an ambush zone in activation round 1",
            position.x, position.y
        ));
    }
    if round == 2 && !travelling {
        return Err(format!(
            "side {side_name} formation type {type_name:?} at ({}, {}) must set travelling=true in activation round 2",
            position.x, position.y
        ));
    }
    Ok(if travelling {
        PlacementStage::Activation
    } else {
        PlacementStage::PreActivation
    })
}

fn reject_unit_fields(
    side_name: &str,
    type_name: &str,
    position: Position,
    level: Option<i32>,
    rotated: Option<bool>,
    equipment: Option<i32>,
    travelling: Option<bool>,
) -> Result<(), String> {
    if level.is_some() || rotated.is_some() || equipment.is_some() || travelling.is_some() {
        Err(format!(
            "side {side_name} formation type {type_name:?} at ({}, {}) does not accept level, rotated, equipment, or travelling",
            position.x, position.y
        ))
    } else {
        Ok(())
    }
}

const fn resolve_type(type_name: &str) -> Option<NativeFormation> {
    match type_name.as_bytes() {
        b"fortress" => Some(NativeFormation::Unit(1)),
        b"marksman" => Some(NativeFormation::Unit(2)),
        b"vulcan" => Some(NativeFormation::Unit(3)),
        b"melting_point" => Some(NativeFormation::Unit(4)),
        b"rhino" => Some(NativeFormation::Unit(5)),
        b"wasp" => Some(NativeFormation::Unit(6)),
        b"mustang" => Some(NativeFormation::Unit(7)),
        b"steel_ball" => Some(NativeFormation::Unit(8)),
        b"fang" => Some(NativeFormation::Unit(9)),
        b"crawler" => Some(NativeFormation::Unit(10)),
        b"overlord" => Some(NativeFormation::Unit(11)),
        b"stormcaller" => Some(NativeFormation::Unit(12)),
        b"sledgehammer" => Some(NativeFormation::Unit(13)),
        b"hacker" => Some(NativeFormation::Unit(14)),
        b"arclight" => Some(NativeFormation::Unit(15)),
        b"phoenix" => Some(NativeFormation::Unit(16)),
        b"war_factory" => Some(NativeFormation::Unit(17)),
        b"wraith" => Some(NativeFormation::Unit(18)),
        b"scorpion" => Some(NativeFormation::Unit(19)),
        b"fire_badger" => Some(NativeFormation::Unit(20)),
        b"sabertooth" => Some(NativeFormation::Unit(21)),
        b"typhoon" => Some(NativeFormation::Unit(22)),
        b"sandworm" => Some(NativeFormation::Unit(23)),
        b"tarantula" => Some(NativeFormation::Unit(24)),
        b"phantom_ray" => Some(NativeFormation::Unit(25)),
        b"farseer" => Some(NativeFormation::Unit(26)),
        b"raiden" => Some(NativeFormation::Unit(27)),
        b"hound" => Some(NativeFormation::Unit(28)),
        b"abyss" => Some(NativeFormation::Unit(29)),
        b"void_eye" => Some(NativeFormation::Unit(30)),
        b"vortex" => Some(NativeFormation::Unit(31)),
        b"mountain" => Some(NativeFormation::Unit(2002)),
        b"defensive_wall" => Some(NativeFormation::Construction(1)),
        b"anti_armor_turret" => Some(NativeFormation::Construction(2)),
        b"rapid_fire_turret" => Some(NativeFormation::Construction(3)),
        b"magnetic_barrier" => Some(NativeFormation::Construction(4)),
        b"shield" => Some(NativeFormation::Contraption(10001)),
        b"missile" => Some(NativeFormation::Contraption(20001)),
        b"interceptor" => Some(NativeFormation::Contraption(30001)),
        _ => None,
    }
}

#[allow(clippy::too_many_lines)] // Keep the build-pinned public catalog one-to-one and auditable.
const fn resolve_battle_skill_type(type_name: &str) -> Option<BattleSkillSpec> {
    let spec = match type_name.as_bytes() {
        b"incendiary_bomb" => battle_skill_spec(
            100_002,
            2,
            BattleSkillShape::Line { width: 40 },
            BattleSkillMapRule::Overlap,
            None,
        ),
        b"electromagnetic_impact" => battle_skill_spec(
            200_001,
            1,
            BattleSkillShape::Circle { radius: 60 },
            BattleSkillMapRule::Overlap,
            None,
        ),
        b"electromagnetic_blast" => battle_skill_spec(
            200_002,
            1,
            BattleSkillShape::Circle { radius: 130 },
            BattleSkillMapRule::Overlap,
            None,
        ),
        b"photon_emission" => battle_skill_spec(
            200_003,
            1,
            BattleSkillShape::Circle { radius: 110 },
            BattleSkillMapRule::Overlap,
            None,
        ),
        b"missile_strike" => battle_skill_spec(
            300_001,
            1,
            BattleSkillShape::Circle { radius: 40 },
            BattleSkillMapRule::Overlap,
            None,
        ),
        b"orbital_bombardment" => battle_skill_spec(
            300_003,
            1,
            BattleSkillShape::RandomCircle {
                outer_radius: 130,
                radius: 30,
            },
            BattleSkillMapRule::Overlap,
            None,
        ),
        b"nuke" => battle_skill_spec(
            300_004,
            1,
            BattleSkillShape::Circle { radius: 100 },
            BattleSkillMapRule::Overlap,
            None,
        ),
        b"lightning_storm" => battle_skill_spec(
            300_005,
            1,
            BattleSkillShape::RandomCircle {
                outer_radius: 130,
                radius: 30,
            },
            BattleSkillMapRule::Overlap,
            None,
        ),
        b"ion_blast" => battle_skill_spec(
            300_006,
            2,
            BattleSkillShape::Line { width: 20 },
            BattleSkillMapRule::Overlap,
            None,
        ),
        b"orbital_javelin" => battle_skill_spec(
            300_007,
            1,
            BattleSkillShape::Circle { radius: 30 },
            BattleSkillMapRule::Overlap,
            None,
        ),
        b"sticky_oil_bomb" => battle_skill_spec(
            400_002,
            2,
            BattleSkillShape::Line { width: 30 },
            BattleSkillMapRule::Overlap,
            None,
        ),
        b"acid_blast" => battle_skill_spec(
            500_002,
            2,
            BattleSkillShape::Line { width: 40 },
            BattleSkillMapRule::Overlap,
            None,
        ),
        b"smoke_bomb" => battle_skill_spec(
            600_002,
            2,
            BattleSkillShape::Line { width: 50 },
            BattleSkillMapRule::Overlap,
            None,
        ),
        b"shield_airdrop" => battle_skill_spec(
            800_001,
            1,
            BattleSkillShape::Circle { radius: 70 },
            BattleSkillMapRule::Center,
            Some(70),
        ),
        b"underground_threat" => battle_skill_spec(
            1_200_001,
            1,
            BattleSkillShape::Circle { radius: 32 },
            BattleSkillMapRule::Contained,
            Some(32),
        ),
        b"rhino_assault" => battle_skill_spec(
            1_200_002,
            1,
            BattleSkillShape::Circle { radius: 20 },
            BattleSkillMapRule::Contained,
            Some(20),
        ),
        b"wasp_swarm" => battle_skill_spec(
            1_200_003,
            1,
            BattleSkillShape::Circle { radius: 25 },
            BattleSkillMapRule::Contained,
            Some(25),
        ),
        b"mobilize_battleship" => battle_skill_spec(
            1_200_004,
            1,
            BattleSkillShape::Circle { radius: 25 },
            BattleSkillMapRule::Contained,
            Some(25),
        ),
        b"vulcans_descent" => battle_skill_spec(
            1_200_005,
            1,
            BattleSkillShape::Circle { radius: 25 },
            BattleSkillMapRule::Contained,
            Some(25),
        ),
        b"mobile_beacon" => battle_skill_spec(
            1_500_002,
            3,
            BattleSkillShape::Path { width: 40 },
            BattleSkillMapRule::Contained,
            None,
        ),
        _ => return None,
    };
    Some(spec)
}

const fn battle_skill_spec(
    commander_skill_id: i32,
    positions: usize,
    shape: BattleSkillShape,
    map_rule: BattleSkillMapRule,
    tower_exclusion_radius: Option<i64>,
) -> BattleSkillSpec {
    BattleSkillSpec {
        commander_skill_id,
        positions,
        shape,
        map_rule,
        tower_exclusion_radius,
    }
}

fn validate_techs(side_name: &str, techs: &Techs) -> Result<(), String> {
    validate_unique_positive_ids(side_name, "techs.officers", &techs.officers)?;
    validate_unique_positive_ids(side_name, "techs.units", &techs.units)
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
    let research = &side.research_center;
    if !(0..=2).contains(&research.strength_level)
        || !(0..=2).contains(&research.attack_level)
        || !(0..=2).contains(&research.defense_level)
    {
        return Err(format!(
            "side {side_name} research_center levels must each be 0..=2"
        ));
    }
    if !(0..=2).contains(&side.energy_tower.strength_level) {
        return Err(format!(
            "side {side_name} energy_tower strength_level must be 0..=2"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn yaml_fixture(source: &str) -> Value {
        serde_yaml::from_str(source).unwrap()
    }

    fn layout_with_blue_battle_skill(type_name: &str, positions: impl serde::Serialize) -> Value {
        json!({
            "round": 1,
            "sides": {
                "blue": {
                    "formations": [{"type": "marksman", "x": 0, "y": -50}],
                    "battle_skills": [{"type": type_name, "positions": positions}]
                },
                "red": {
                    "formations": [{"type": "marksman", "x": 0, "y": -50}]
                }
            }
        })
    }

    #[test]
    fn requires_a_positive_activation_round() {
        let missing = compile(&json!({
            "sides": {
                "blue": {"formations": [{"type": "marksman", "x": 0, "y": -50}]},
                "red": {"formations": [{"type": "marksman", "x": 0, "y": -50}]}
            }
        }))
        .unwrap_err();
        assert!(missing.contains("missing field `round`"));

        let invalid = compile(&json!({
            "round": 0,
            "sides": {
                "blue": {"formations": [{"type": "marksman", "x": 0, "y": -50}]},
                "red": {"formations": [{"type": "marksman", "x": 0, "y": -50}]}
            }
        }))
        .unwrap_err();
        assert_eq!(invalid, "layout round must be a positive integer");
    }

    #[test]
    fn compiles_formations_into_pre_activation_and_activation_stages() {
        let plan = compile(&json!({
            "round": 3,
            "sides": {
                "blue": {"formations": [
                    {"type": "marksman", "x": 0, "y": -50},
                    {"type": "marksman", "x": -310, "y": 20},
                    {"type": "arclight", "x": 310, "y": 20, "travelling": true},
                    {"type": "interceptor", "x": 5, "y": -85}
                ]},
                "red": {"formations": [{"type": "marksman", "x": 0, "y": -50}]}
            }
        }))
        .unwrap();

        assert_eq!(plan.round, 3);
        assert_eq!(plan.blue.formations[0].stage, PlacementStage::Activation);
        assert_eq!(plan.blue.formations[1].stage, PlacementStage::PreActivation);
        assert!(!plan.blue.formations[1].travelling);
        assert_eq!(plan.blue.formations[2].stage, PlacementStage::Activation);
        assert!(plan.blue.formations[2].travelling);
        assert_eq!(plan.blue.formations[3].stage, PlacementStage::Activation);
    }

    #[test]
    fn enforces_activation_round_rules_for_ambush_units() {
        let layout = |round, travelling| {
            json!({
                "round": round,
                "sides": {
                    "blue": {"formations": [
                        {"type": "marksman", "x": -310, "y": 20, "travelling": travelling}
                    ]},
                    "red": {"formations": [{"type": "marksman", "x": 0, "y": -50}]}
                }
            })
        };

        assert!(
            compile(&layout(1, false))
                .unwrap_err()
                .contains("activation round 1")
        );
        assert!(
            compile(&layout(1, true))
                .unwrap_err()
                .contains("activation round 1")
        );
        assert!(
            compile(&layout(2, false))
                .unwrap_err()
                .contains("must set travelling=true")
        );
        let omitted = compile(&json!({
            "round": 2,
            "sides": {
                "blue": {"formations": [{"type": "marksman", "x": -310, "y": 20}]},
                "red": {"formations": [{"type": "marksman", "x": 0, "y": -50}]}
            }
        }))
        .unwrap_err();
        assert!(omitted.contains("must set travelling=true"));
        assert_eq!(
            compile(&layout(2, true)).unwrap().blue.formations[0].stage,
            PlacementStage::Activation
        );
        assert_eq!(
            compile(&layout(3, false)).unwrap().blue.formations[0].stage,
            PlacementStage::PreActivation
        );
    }

    #[test]
    fn travelling_is_unit_only_and_requires_an_ambush_position() {
        let main_error = compile(&json!({
            "round": 3,
            "sides": {
                "blue": {"formations": [
                    {"type": "marksman", "x": 0, "y": -50, "travelling": true}
                ]},
                "red": {"formations": [{"type": "marksman", "x": 0, "y": -50}]}
            }
        }))
        .unwrap_err();
        assert!(main_error.contains("travelling=true outside the ambush zones"));

        let construction_error = compile(&json!({
            "round": 3,
            "sides": {
                "blue": {"formations": [
                    {"type": "defensive_wall", "x": 0, "y": -55, "travelling": false},
                    {"type": "marksman", "x": 100, "y": -50}
                ]},
                "red": {"formations": [{"type": "marksman", "x": 0, "y": -50}]}
            }
        }))
        .unwrap_err();
        assert!(construction_error.contains("does not accept"));
        assert!(construction_error.contains("travelling"));
    }

    #[test]
    fn ambush_unit_footprint_must_fit_one_flank_region() {
        let error = compile(&json!({
            "round": 3,
            "sides": {
                "blue": {"formations": [
                    {"type": "marksman", "x": -300, "y": 20, "travelling": true}
                ]},
                "red": {"formations": [{"type": "marksman", "x": 0, "y": -50}]}
            }
        }))
        .unwrap_err();
        assert!(error.contains("must fit completely inside one ambush zone"));
    }

    #[test]
    fn ambush_region_orientation_changes_the_effective_unit_footprint() {
        let plan = compile(&json!({
            "round": 3,
            "sides": {
                "blue": {"formations": [
                    {
                        "type": "crawler", "x": 325, "y": 60,
                        "rotated": true, "travelling": true
                    }
                ]},
                "red": {"formations": [{"type": "marksman", "x": 0, "y": -50}]}
            }
        }))
        .unwrap();
        assert_eq!(
            formation_footprint(&plan.blue.formations[0]),
            Some((50, 20))
        );

        let error = compile(&json!({
            "round": 3,
            "sides": {
                "blue": {"formations": [
                    {
                        "type": "crawler", "x": 310, "y": 65,
                        "rotated": true, "travelling": true
                    }
                ]},
                "red": {"formations": [{"type": "marksman", "x": 0, "y": -50}]}
            }
        }))
        .unwrap_err();
        assert!(error.contains("footprint 50x20 requires center x≡5, y≡0"));
    }

    #[test]
    fn compiles_unit_defaults_for_both_sides() {
        let plan = compile(&json!({
            "round": 1,
            "sides": {
                "blue": {"formations": [{
                    "type": "marksman",
                    "x": -20, "y": -50
                }]},
                "red": {"formations": [{
                    "type": "marksman",
                    "x": 20, "y": -180
                }]}
            }
        }))
        .unwrap();

        assert_eq!(plan.blue.formations[0].level, Some(1));
        assert!(!plan.blue.formations[0].rotated);
        assert_eq!(plan.blue.formations[0].equipment, None);
        assert_eq!(plan.blue.formations[0].type_name, "marksman");
        assert_eq!(plan.blue.formations[0].native, NativeFormation::Unit(2));
        assert_eq!(plan.red.formations[0].position, Position { x: 20, y: -180 });
        assert_eq!(plan.formation_count(), 2);
    }

    #[test]
    fn compiles_one_equipment_for_a_unit() {
        let plan = compile(&json!({
            "round": 1,
            "sides": {
                "blue": {"formations": [{
                    "type": "marksman", "x": 0, "y": -50,
                    "equipment": 13_030_001
                }]},
                "red": {"formations": [{
                    "type": "marksman", "x": 0, "y": -50
                }]}
            }
        }))
        .unwrap();

        assert_eq!(plan.blue.formations[0].equipment, Some(13_030_001));
        assert_eq!(plan.red.formations[0].equipment, None);
    }

    #[test]
    fn rejects_equipment_for_non_unit_formations() {
        let error = compile(&json!({
            "round": 1,
            "sides": {
                "blue": {"formations": [
                    {"type": "defensive_wall", "x": 0, "y": -55,
                     "equipment": 13_030_001},
                    {"type": "marksman", "x": 100, "y": -50}
                ]},
                "red": {"formations": [{
                    "type": "marksman", "x": 0, "y": -50
                }]}
            }
        }))
        .unwrap_err();

        assert_eq!(
            error,
            "side blue formation type \"defensive_wall\" at (0, -55) does not accept level, rotated, equipment, or travelling"
        );
    }

    #[test]
    fn rejects_nonpositive_equipment_id() {
        let error = compile(&json!({
            "round": 1,
            "sides": {
                "blue": {"formations": [{
                    "type": "marksman", "x": 0, "y": -50, "equipment": 0
                }]},
                "red": {"formations": [{
                    "type": "marksman", "x": 0, "y": -50
                }]}
            }
        }))
        .unwrap_err();

        assert_eq!(
            error,
            "side blue formation type \"marksman\" at (0, -50) equipment must be a positive integer"
        );
    }

    #[test]
    fn validates_tech_ids() {
        let valid = compile(&json!({
            "round": 1,
            "sides": {
                "blue": {
                    "techs": {"officers": [30602], "units": [10202]},
                    "formations": [{"type": "marksman", "x": 0, "y": -50}]
                },
                "red": {
                    "formations": [{"type": "marksman", "x": 0, "y": -50}]
                }
            }
        }))
        .unwrap();
        assert_eq!(valid.blue.techs.officers, [30602]);
        assert_eq!(valid.blue.techs.units, [10202]);

        let error = compile(&json!({
            "round": 1,
            "sides": {
                "blue": {"techs": {"units": [10202, 10202]}, "formations": [{
                    "type": "marksman", "x": 0, "y": -50
                }]},
                "red": {"formations": [{
                    "type": "marksman", "x": 0, "y": -50
                }]}
            }
        }))
        .unwrap_err();
        assert_eq!(error, "side blue techs.units contains duplicate ID 10202");

        let error = compile(&json!({
            "round": 1,
            "sides": {
                "blue": {"techs": {"officers": [0]}, "formations": [{
                    "type": "marksman", "x": 0, "y": -50
                }]},
                "red": {"formations": [{
                    "type": "marksman", "x": 0, "y": -50
                }]}
            }
        }))
        .unwrap_err();
        assert_eq!(
            error,
            "side blue techs.officers[0] must be a positive integer"
        );
    }

    #[test]
    fn rejects_tower_levels_outside_the_runtime_catalog() {
        let error = compile(&json!({
            "round": 1,
            "sides": {
                "blue": {
                    "research_center": {"attack_level": 3},
                    "formations": [{"type": "marksman", "x": 0, "y": -50}]
                },
                "red": {
                    "formations": [{"type": "marksman", "x": 0, "y": -50}]
                }
            }
        }))
        .unwrap_err();
        assert_eq!(error, "side blue research_center levels must each be 0..=2");
    }

    #[test]
    fn rejects_fields_invalid_for_the_formation_type() {
        let error = compile(&json!({
            "round": 1,
            "sides": {
                "blue": {"formations": [{
                    "type": "defensive_wall",
                    "x": 0, "y": 0, "rotated": false
                }]},
                "red": {"formations": [{
                    "type": "marksman", "x": 0, "y": -50
                }]}
            }
        }))
        .unwrap_err();
        assert!(error.contains("does not accept level, rotated, equipment, or travelling"));
    }

    #[test]
    fn requires_nonempty_formations_for_both_sides() {
        let missing = compile(&json!({
            "round": 1,
            "sides": {
                "blue": {},
                "red": {"formations": [{
                    "type": "marksman", "x": 0, "y": -50
                }]}
            }
        }))
        .unwrap_err();
        assert!(missing.contains("missing field `formations`"));

        let empty = compile(&json!({
            "round": 1,
            "sides": {
                "blue": {"formations": []},
                "red": {"formations": [{
                    "type": "marksman", "x": 0, "y": -50
                }]}
            }
        }))
        .unwrap_err();
        assert!(empty.contains("at least one valid unit"));
    }

    #[test]
    fn rejects_positive_area_unit_overlap_but_allows_edge_contact() {
        let layout = |second_x| {
            json!({
                "round": 1,
                "sides": {
                    "blue": {"formations": [
                        {"type": "marksman", "x": 0, "y": -50},
                        {"type": "arclight", "x": second_x, "y": -50}
                    ]},
                    "red": {"formations": [{
                        "type": "marksman", "x": 0, "y": -50
                    }]}
                }
            })
        };

        let error = compile(&layout(10)).unwrap_err();
        assert!(error.contains("formations collide"));
        assert!(error.contains("marksman"));
        assert!(error.contains("arclight"));
        compile(&layout(20)).unwrap();
    }

    #[test]
    fn rejects_fixture_whose_center_is_inside_but_footprint_crosses_boundary() {
        let value = yaml_fixture(include_str!(
            "../tests/fixtures/invalid-footprint-boundary.yaml"
        ));
        let error = compile(&value).unwrap_err();
        assert_eq!(
            error,
            "side blue formation type \"sledgehammer\" at (285, -60) footprint 50x20 exceeds the main deployment boundary x=[-300,300], y=[-310,-10]"
        );
    }

    #[test]
    fn rejects_collision_fixture_and_names_both_units() {
        let value = yaml_fixture(include_str!(
            "../tests/fixtures/invalid-unit-collision.yaml"
        ));
        let error = compile(&value).unwrap_err();
        assert_eq!(
            error,
            "formations collide: blue type \"sledgehammer\" at (5, -100) and blue type \"crawler\" at (20, -95)"
        );
    }

    #[test]
    fn rejects_unit_construction_collision_and_names_both_formations() {
        let value = yaml_fixture(include_str!(
            "../tests/fixtures/invalid-unit-construction-collision.yaml"
        ));
        let error = compile(&value).unwrap_err();
        assert_eq!(
            error,
            "formations collide: blue type \"marksman\" at (140, -100) and blue type \"defensive_wall\" at (140, -105)"
        );
    }

    #[test]
    fn rejects_center_that_does_not_match_its_footprint_grid_class() {
        let error = compile(&json!({
            "round": 1,
            "sides": {
                "blue": {"formations": [{
                    "type": "marksman", "x": 5, "y": -50
                }]},
                "red": {"formations": [{
                    "type": "marksman", "x": 0, "y": -50
                }]}
            }
        }))
        .unwrap_err();
        assert_eq!(
            error,
            "side blue formation type \"marksman\" at (5, -50) is off the native 10x10 grid: footprint 20x20 requires center x≡0, y≡0 (mod 10)"
        );
    }

    #[test]
    fn derives_center_grid_class_from_footprint_dimension() {
        assert_eq!(grid_center_remainder(20), Some(0));
        assert_eq!(grid_center_remainder(40), Some(0));
        assert_eq!(grid_center_remainder(30), Some(5));
        assert_eq!(grid_center_remainder(50), Some(5));
        assert_eq!(grid_center_remainder(70), Some(5));
        assert_eq!(grid_center_remainder(25), None);
    }

    #[test]
    fn footprint_interface_covers_all_public_units_and_rotation() {
        let groups: &[(&[i32], (i64, i64))] = &[
            (&[2, 15, 31], (20, 20)),
            (&[5, 14, 18, 19, 21, 24, 26], (30, 30)),
            (&[16, 22, 28, 30], (40, 20)),
            (&[1, 3, 4, 23, 27], (40, 40)),
            (&[6, 7, 8, 9, 10, 12, 13, 20, 25], (50, 20)),
            (&[11], (50, 50)),
            (&[17, 29, 2002], (70, 70)),
        ];
        for &(unit_ids, (width, height)) in groups {
            for &unit_id in unit_ids {
                assert_eq!(unit_footprint(unit_id, false), Some((width, height)));
                assert_eq!(unit_footprint(unit_id, true), Some((height, width)));
            }
        }
        assert_eq!(groups.iter().map(|(ids, _)| ids.len()).sum::<usize>(), 32);
        assert_eq!(unit_footprint(32, false), None);
    }

    #[test]
    fn footprint_interface_covers_the_four_constructions() {
        assert_eq!(construction_footprint(1), Some((60, 10)));
        assert_eq!(construction_footprint(2), Some((20, 20)));
        assert_eq!(construction_footprint(3), Some((20, 20)));
        assert_eq!(construction_footprint(4), Some((50, 10)));
        assert_eq!(construction_footprint(5), None);
    }

    #[test]
    fn footprint_interface_covers_the_interceptor() {
        assert_eq!(contraption_footprint(30001), Some((30, 30)));
        assert_eq!(contraption_footprint(10001), None);
        assert_eq!(contraption_footprint(20001), None);
    }

    #[test]
    fn compiles_interceptor_and_reuses_formation_collision_validation() {
        let layout = |interceptor_y| {
            json!({
                "round": 1,
                "sides": {
                    "blue": {"formations": [
                        {"type": "marksman", "x": 0, "y": -100},
                        {"type": "interceptor", "x": 5, "y": interceptor_y}
                    ]},
                    "red": {"formations": [{
                        "type": "marksman", "x": 0, "y": -50
                    }]}
                }
            })
        };

        let error = compile(&layout(-95)).unwrap_err();
        assert_eq!(
            error,
            "formations collide: blue type \"marksman\" at (0, -100) and blue type \"interceptor\" at (5, -95)"
        );

        let plan = compile(&layout(-125)).unwrap();
        assert_eq!(
            plan.blue.formations[1].native,
            NativeFormation::Contraption(30001)
        );
        assert_eq!(plan.blue.formations[1].level, None);
        assert!(!plan.blue.formations[1].rotated);
    }

    #[test]
    fn shield_and_missile_skip_grid_and_collision_validation() {
        let plan = compile(&json!({
            "round": 1,
            "sides": {
                "blue": {"formations": [
                    {"type": "marksman", "x": 0, "y": -100},
                    {"type": "shield", "x": 1, "y": -101},
                    {"type": "missile", "x": 1, "y": -101}
                ]},
                "red": {"formations": [{
                    "type": "marksman", "x": 0, "y": -100
                }]}
            }
        }))
        .unwrap();

        assert_eq!(plan.formation_count(), 4);
        assert_eq!(
            plan.blue
                .formations
                .iter()
                .map(|placement| placement.native)
                .collect::<Vec<_>>(),
            [
                NativeFormation::Unit(2),
                NativeFormation::Contraption(10001),
                NativeFormation::Contraption(20001),
            ]
        );
    }

    #[test]
    fn shield_requires_its_complete_edge_inside_the_own_side() {
        let layout = |x, y| {
            json!({
                "round": 1,
                "sides": {
                    "blue": {"formations": [
                        {"type": "marksman", "x": 0, "y": -150},
                        {"type": "shield", "x": x, "y": y}
                    ]},
                    "red": {"formations": [{
                        "type": "marksman", "x": 0, "y": -150
                    }]}
                }
            })
        };

        compile(&layout(229, -81)).unwrap();
        compile(&layout(-229, -239)).unwrap();
        let error = compile(&layout(230, -80)).unwrap_err();
        assert_eq!(
            error,
            "side blue formation type \"shield\" at (230, -80) places its radius-70 edge outside the own-side deployment boundary: center must be within x=[-229,229], y=[-239,-81]"
        );
    }

    #[test]
    fn missile_requires_only_its_center_inside_the_own_side() {
        let layout = |x, y| {
            json!({
                "round": 1,
                "sides": {
                    "blue": {"formations": [
                        {"type": "marksman", "x": 0, "y": -150},
                        {"type": "missile", "x": x, "y": y}
                    ]},
                    "red": {"formations": [{
                        "type": "marksman", "x": 0, "y": -150
                    }]}
                }
            })
        };

        compile(&layout(300, -10)).unwrap();
        compile(&layout(-300, -310)).unwrap();
        let error = compile(&layout(301, -10)).unwrap_err();
        assert_eq!(
            error,
            "side blue formation type \"missile\" at (301, -10) is outside the own-side deployment boundary x=[-300,300], y=[-310,-10]"
        );
    }

    #[test]
    fn compiles_all_four_construction_types_in_document_order() {
        let plan = compile(&json!({
            "round": 1,
            "sides": {
                "blue": {"formations": [
                    {"type": "rapid_fire_turret", "x": 140, "y": -60},
                    {"type": "defensive_wall", "x": 140, "y": -105},
                    {"type": "anti_armor_turret", "x": -140, "y": -60},
                    {"type": "magnetic_barrier", "x": -165, "y": -105},
                    {"type": "marksman", "x": 0, "y": -150}
                ]},
                "red": {"formations": [{
                    "type": "marksman", "x": 0, "y": -50
                }]}
            }
        }))
        .unwrap();

        assert_eq!(plan.formation_count(), 6);
        assert_eq!(
            plan.blue
                .formations
                .iter()
                .map(|placement| placement.native)
                .collect::<Vec<_>>(),
            [
                NativeFormation::Construction(3),
                NativeFormation::Construction(1),
                NativeFormation::Construction(2),
                NativeFormation::Construction(4),
                NativeFormation::Unit(2),
            ]
        );
    }

    #[test]
    fn compiles_position_targeted_battle_skills_in_document_order() {
        let plan = compile(&json!({
            "round": 1,
            "sides": {
                "blue": {
                    "formations": [{"type": "marksman", "x": 0, "y": -50}],
                    "battle_skills": [{
                        "type": "missile_strike",
                        "positions": [{"x": 55, "y": 60}]
                    }]
                },
                "red": {
                    "formations": [{"type": "fang", "x": -55, "y": -60}],
                    "battle_skills": [{
                        "type": "mobile_beacon",
                        "positions": [
                            {"x": -55, "y": -60},
                            {"x": -105, "y": -90},
                            {"x": -105, "y": 20}
                        ]
                    }]
                }
            }
        }))
        .unwrap();

        assert_eq!(plan.blue.battle_skills[0].commander_skill_id, 300_001);
        assert_eq!(
            plan.red.battle_skills[0],
            BattleSkill {
                type_name: "mobile_beacon".into(),
                commander_skill_id: 1_500_002,
                positions: vec![
                    Position { x: -55, y: -60 },
                    Position { x: -105, y: -90 },
                    Position { x: -105, y: 20 },
                ],
            }
        );
    }

    #[test]
    fn battle_skill_circle_requires_only_map_overlap() {
        compile(&layout_with_blue_battle_skill(
            "electromagnetic_impact",
            json!([{"x": 460, "y": 0}]),
        ))
        .unwrap();

        let error = compile(&layout_with_blue_battle_skill(
            "electromagnetic_impact",
            json!([{"x": 461, "y": 0}]),
        ))
        .unwrap_err();
        assert!(error.contains("violates its battlefield map rule"));
    }

    #[test]
    fn random_circle_overlap_includes_the_sub_effect_radius() {
        compile(&layout_with_blue_battle_skill(
            "orbital_bombardment",
            json!([{"x": 560, "y": 0}]),
        ))
        .unwrap();

        let error = compile(&layout_with_blue_battle_skill(
            "orbital_bombardment",
            json!([{"x": 561, "y": 0}]),
        ))
        .unwrap_err();
        assert!(error.contains("violates its battlefield map rule"));
    }

    #[test]
    fn line_skill_requires_its_full_width_to_overlap_the_map() {
        compile(&layout_with_blue_battle_skill(
            "ion_blast",
            json!([{"x": -500, "y": 360}, {"x": 500, "y": 360}]),
        ))
        .unwrap();

        let error = compile(&layout_with_blue_battle_skill(
            "ion_blast",
            json!([{"x": -500, "y": 361}, {"x": 500, "y": 361}]),
        ))
        .unwrap_err();
        assert!(error.contains("violates its battlefield map rule"));

        let extreme = compile(&layout_with_blue_battle_skill(
            "ion_blast",
            json!([
                {"x": i32::MIN, "y": i32::MAX},
                {"x": i32::MAX, "y": i32::MAX}
            ]),
        ))
        .unwrap_err();
        assert!(extreme.contains("violates its battlefield map rule"));
    }

    #[test]
    fn shield_airdrop_requires_its_center_inside_the_map() {
        compile(&layout_with_blue_battle_skill(
            "shield_airdrop",
            json!([{"x": 400, "y": 0}]),
        ))
        .unwrap();

        let error = compile(&layout_with_blue_battle_skill(
            "shield_airdrop",
            json!([{"x": 401, "y": 0}]),
        ))
        .unwrap_err();
        assert!(error.contains("violates its battlefield map rule"));
    }

    #[test]
    fn mobile_beacon_requires_its_half_width_inside_the_map() {
        compile(&layout_with_blue_battle_skill(
            "mobile_beacon",
            json!([
                {"x": 380, "y": -100},
                {"x": 380, "y": 0},
                {"x": 380, "y": 100}
            ]),
        ))
        .unwrap();

        let error = compile(&layout_with_blue_battle_skill(
            "mobile_beacon",
            json!([
                {"x": 381, "y": -100},
                {"x": 380, "y": 0},
                {"x": 380, "y": 100}
            ]),
        ))
        .unwrap_err();
        assert!(error.contains("violates its battlefield map rule"));
    }

    #[test]
    fn summon_skills_use_their_effect_range_for_map_and_tower_clearance() {
        compile(&layout_with_blue_battle_skill(
            "rhino_assault",
            json!([{"x": 380, "y": 0}]),
        ))
        .unwrap();
        let map_error = compile(&layout_with_blue_battle_skill(
            "rhino_assault",
            json!([{"x": 381, "y": 0}]),
        ))
        .unwrap_err();
        assert!(map_error.contains("violates its battlefield map rule"));

        let rhino_error = compile(&layout_with_blue_battle_skill(
            "rhino_assault",
            json!([{"x": -300, "y": 170}]),
        ))
        .unwrap_err();
        assert!(rhino_error.contains("must be more than 160 m"));
        compile(&layout_with_blue_battle_skill(
            "rhino_assault",
            json!([{"x": -301, "y": 170}]),
        ))
        .unwrap();

        let underground_error = compile(&layout_with_blue_battle_skill(
            "underground_threat",
            json!([{"x": -312, "y": 170}]),
        ))
        .unwrap_err();
        assert!(underground_error.contains("must be more than 172 m"));
        compile(&layout_with_blue_battle_skill(
            "underground_threat",
            json!([{"x": -313, "y": 170}]),
        ))
        .unwrap();

        let wasp_error = compile(&layout_with_blue_battle_skill(
            "wasp_swarm",
            json!([{"x": -305, "y": 170}]),
        ))
        .unwrap_err();
        assert!(wasp_error.contains("must be more than 165 m"));
        compile(&layout_with_blue_battle_skill(
            "wasp_swarm",
            json!([{"x": -306, "y": 170}]),
        ))
        .unwrap();

        let red_error = compile(&json!({
            "round": 1,
            "sides": {
                "blue": {
                    "formations": [{"type": "marksman", "x": 0, "y": -50}]
                },
                "red": {
                    "formations": [{"type": "marksman", "x": 0, "y": -50}],
                    "battle_skills": [{
                        "type": "rhino_assault",
                        "positions": [{"x": -300, "y": 170}]
                    }]
                }
            }
        }))
        .unwrap_err();
        assert!(red_error.starts_with("side red battle skill"));
        assert!(red_error.contains("must be more than 160 m"));
    }

    #[test]
    fn shield_airdrop_uses_its_effect_range_for_tower_clearance() {
        let error = compile(&layout_with_blue_battle_skill(
            "shield_airdrop",
            json!([{"x": -350, "y": 170}]),
        ))
        .unwrap_err();
        assert!(error.contains("must be more than 210 m"));
        compile(&layout_with_blue_battle_skill(
            "shield_airdrop",
            json!([{"x": -351, "y": 170}]),
        ))
        .unwrap();
    }

    #[test]
    fn rejects_unknown_duplicate_and_wrong_length_battle_skills() {
        let layout = |battle_skills| {
            json!({
                "round": 1,
                "sides": {
                    "blue": {
                        "formations": [{"type": "marksman", "x": 0, "y": -50}],
                        "battle_skills": battle_skills
                    },
                    "red": {
                        "formations": [{"type": "marksman", "x": 0, "y": -50}]
                    }
                }
            })
        };

        let unknown = compile(&layout(json!([{
            "type": "not_a_skill", "positions": [{"x": 0, "y": 0}]
        }])))
        .unwrap_err();
        assert_eq!(
            unknown,
            "side blue battle skill type \"not_a_skill\" is unknown"
        );

        let wrong_length = compile(&layout(json!([{
            "type": "mobile_beacon", "positions": [{"x": 0, "y": 0}]
        }])))
        .unwrap_err();
        assert_eq!(
            wrong_length,
            "side blue battle skill type \"mobile_beacon\" requires 3 positions, got 1"
        );

        let duplicate = compile(&layout(json!([
            {"type": "missile_strike", "positions": [{"x": 0, "y": 0}]},
            {"type": "missile_strike", "positions": [{"x": 10, "y": 10}]}
        ])))
        .unwrap_err();
        assert_eq!(
            duplicate,
            "side blue battle skill type \"missile_strike\" is declared more than once"
        );
    }

    #[test]
    fn resolves_representative_public_formation_types() {
        let expected = [
            ("fortress", NativeFormation::Unit(1)),
            ("mountain", NativeFormation::Unit(2002)),
            ("defensive_wall", NativeFormation::Construction(1)),
            ("magnetic_barrier", NativeFormation::Construction(4)),
            ("shield", NativeFormation::Contraption(10001)),
            ("missile", NativeFormation::Contraption(20001)),
            ("interceptor", NativeFormation::Contraption(30001)),
        ];
        for (type_name, native) in expected {
            assert_eq!(resolve_type(type_name), Some(native));
        }
        assert_eq!(resolve_type("unit"), None);
    }
}
