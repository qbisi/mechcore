use mechcore_protocol::MAX_ACTIVATION_ROUND;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Layout {
    #[serde(default)]
    pub seed: i32,
    #[schemars(range(min = 1, max = 15))]
    pub round: i32,
    pub sides: Sides,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Sides {
    pub blue: Side,
    pub red: Side,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Side {
    #[serde(default, skip_serializing_if = "Techs::is_default")]
    pub techs: Techs,
    #[serde(default, skip_serializing_if = "ResearchCenter::is_default")]
    pub research_center: ResearchCenter,
    #[serde(default, skip_serializing_if = "EnergyTower::is_default")]
    pub energy_tower: EnergyTower,
    pub formations: Vec<Formation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub constructions: Vec<StaticPlacement>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contraptions: Vec<StaticPlacement>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub battle_skills: Vec<BattleSkillDefinition>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct Techs {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub officers: Vec<i32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub units: Vec<i32>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
#[allow(clippy::struct_field_names)] // Field names are fixed by the public layout schema.
pub struct ResearchCenter {
    pub strength_level: i32,
    pub attack_level: i32,
    pub defense_level: i32,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct EnergyTower {
    pub strength_level: i32,
    pub range_enhancement: bool,
    pub movement_enhancement: bool,
}

impl Techs {
    fn is_default(&self) -> bool {
        self == &Self::default()
    }
}

impl ResearchCenter {
    fn is_default(&self) -> bool {
        self == &Self::default()
    }
}

impl EnergyTower {
    fn is_default(&self) -> bool {
        self == &Self::default()
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Formation {
    #[serde(rename = "type")]
    pub type_name: String,
    pub x: i32,
    pub y: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rotated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub equipment: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub travelling: Option<bool>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StaticPlacement {
    #[serde(rename = "type")]
    pub type_name: String,
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BattleSkillDefinition {
    #[serde(rename = "type")]
    pub type_name: String,
    pub positions: Vec<Position>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeFormation {
    Unit(i32),
    Construction(i32),
    Contraption(i32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FormationSpec {
    native: NativeFormation,
    footprint: Option<(i64, i64)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlacementStage {
    PreActivation,
    Activation,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Position {
    pub x: i32,
    pub y: i32,
}

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
    pub level: Option<i32>,
    pub rotated: bool,
    pub equipment: Option<i32>,
    pub travelling: bool,
    pub stage: PlacementStage,
}

#[derive(Debug, PartialEq, Eq)]
pub struct SidePlan {
    pub techs: Techs,
    pub research_center: ResearchCenter,
    pub energy_tower: EnergyTower,
    pub formations: Vec<Placement>,
    pub constructions: Vec<Placement>,
    pub contraptions: Vec<Placement>,
    pub battle_skills: Vec<BattleSkill>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Plan {
    pub seed: i32,
    pub round: i32,
    pub blue: SidePlan,
    pub red: SidePlan,
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
    #[must_use]
    pub fn formation_count(&self) -> usize {
        self.blue.formations.len() + self.red.formations.len()
    }

    #[must_use]
    pub fn construction_count(&self) -> usize {
        self.blue.constructions.len() + self.red.constructions.len()
    }

    #[must_use]
    pub fn contraption_count(&self) -> usize {
        self.blue.contraptions.len() + self.red.contraptions.len()
    }
}

impl Layout {
    /// Removes syntax that is semantically equivalent to the public defaults.
    #[must_use]
    pub fn normalized(mut self) -> Self {
        for side in [&mut self.sides.blue, &mut self.sides.red] {
            for formation in &mut side.formations {
                if formation.level == Some(1) {
                    formation.level = None;
                }
                if formation.rotated == Some(false) {
                    formation.rotated = None;
                }
                if formation.travelling == Some(false) {
                    formation.travelling = None;
                }
            }
        }
        self
    }
}

/// Parses and validates one public layout YAML document.
///
/// # Errors
///
/// Returns an error when the YAML or shared layout contract is invalid.
pub fn parse_yaml(bytes: &[u8]) -> Result<Layout, String> {
    let layout = parse_embedded_yaml(bytes)?;
    compile_layout(layout.clone())?;
    Ok(layout)
}

/// Parses the shared layout structure used by an embedded MCFR member.
///
/// This level preserves optional-field gaps captured by Adapter. Consumers that execute the
/// layout call [`parse_yaml`] to additionally apply the complete gameplay legality rules.
///
/// # Errors
///
/// Returns an error when the YAML does not match the shared layout structure.
pub fn parse_embedded_yaml(bytes: &[u8]) -> Result<Layout, String> {
    let layout: Layout =
        serde_yaml::from_slice(bytes).map_err(|error| format!("invalid layout YAML: {error}"))?;
    validate_embedded_categories(&layout)?;
    Ok(layout)
}

fn validate_embedded_categories(layout: &Layout) -> Result<(), String> {
    for (side_name, side) in [("blue", &layout.sides.blue), ("red", &layout.sides.red)] {
        for formation in &side.formations {
            if resolve_unit_type(&formation.type_name).is_none() {
                let destination = if resolve_construction_type(&formation.type_name).is_some() {
                    "constructions"
                } else if resolve_contraption_type(&formation.type_name).is_some() {
                    "contraptions"
                } else {
                    return Err(format!(
                        "side {side_name} formation type {:?} is unknown",
                        formation.type_name
                    ));
                };
                return Err(format!(
                    "side {side_name} formation type {:?} belongs in {destination}",
                    formation.type_name,
                ));
            }
        }
        for construction in &side.constructions {
            if resolve_construction_type(&construction.type_name).is_none() {
                return Err(format!(
                    "side {side_name} construction type {:?} is unknown",
                    construction.type_name
                ));
            }
        }
        for contraption in &side.contraptions {
            if resolve_contraption_type(&contraption.type_name).is_none() {
                return Err(format!(
                    "side {side_name} contraption type {:?} is unknown",
                    contraption.type_name
                ));
            }
        }
    }
    Ok(())
}

/// Serializes a validated layout into the canonical YAML representation.
///
/// Canonical layout YAML always includes `seed`, preserves declaration order,
/// omits default-valued optional syntax, and ends with one newline.
///
/// # Errors
///
/// Returns an error when the layout is invalid or cannot be serialized.
pub fn canonical_yaml(layout: Layout) -> Result<String, String> {
    compile_layout(layout.clone())?;
    canonical_embedded_yaml(layout)
}

/// Serializes an embedded MCFR layout while preserving optional-field gaps.
///
/// # Errors
///
/// Returns an error when the layout cannot be serialized.
pub fn canonical_embedded_yaml(layout: Layout) -> Result<String, String> {
    validate_embedded_categories(&layout)?;
    let layout = layout.normalized();
    serde_yaml::to_string(&layout).map_err(|error| format!("cannot serialize layout YAML: {error}"))
}

/// Deserializes, validates, and normalizes a JSON layout into an execution plan.
///
/// # Errors
///
/// Returns an error when the JSON does not match [`Layout`] or violates any
/// shared static layout rule.
pub fn compile(value: &Value) -> Result<Plan, String> {
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
    if !(1..=MAX_ACTIVATION_ROUND).contains(&layout.round) {
        return Err(format!(
            "layout round must be within 1..={MAX_ACTIVATION_ROUND}"
        ));
    }
    let blue = compile_side("blue", layout.sides.blue, layout.round)?;
    let red = compile_side("red", layout.sides.red, layout.round)?;
    validate_placement_footprints("blue", &blue.formations)?;
    validate_placement_footprints("red", &red.formations)?;
    validate_placement_footprints("blue", &blue.constructions)?;
    validate_placement_footprints("red", &red.constructions)?;
    validate_placement_footprints("blue", &blue.contraptions)?;
    validate_placement_footprints("red", &red.contraptions)?;
    validate_placement_collisions(&blue, &red)?;
    Ok(Plan {
        seed: layout.seed,
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
        constructions,
        contraptions,
        battle_skills,
    } = side;
    let formations = compile_formations(side_name, formations, round)?;
    let constructions = compile_constructions(side_name, constructions)?;
    let contraptions = compile_contraptions(side_name, contraptions)?;
    let battle_skills = compile_battle_skills(side_name, battle_skills)?;
    Ok(SidePlan {
        techs,
        research_center,
        energy_tower,
        formations,
        constructions,
        contraptions,
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
            let spec = resolve_unit_type(&type_name).ok_or_else(|| {
                if resolve_construction_type(&type_name).is_some() {
                    return format!(
                        "side {side_name} formation type {type_name:?} at ({}, {}) belongs in constructions",
                        position.x, position.y
                    );
                }
                if resolve_contraption_type(&type_name).is_some() {
                    return format!(
                        "side {side_name} formation type {type_name:?} at ({}, {}) belongs in contraptions",
                        position.x, position.y
                    );
                }
                format!(
                    "side {side_name} formation type {type_name:?} at ({}, {}) is unknown",
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
            let stage = unit_placement_stage(side_name, &type_name, position, travelling, round)?;
            Ok(Placement {
                type_name,
                native: NativeFormation::Unit(unit_id),
                footprint: spec.footprint,
                position,
                level: Some(level),
                rotated,
                equipment,
                travelling,
                stage,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    if placements.is_empty() {
        return Err(format!(
            "side {side_name} formations must contain at least one valid unit"
        ));
    }
    Ok(placements)
}

fn compile_constructions(
    side_name: &str,
    definitions: Vec<StaticPlacement>,
) -> Result<Vec<Placement>, String> {
    definitions
        .into_iter()
        .map(|definition| {
            let StaticPlacement { type_name, x, y } = definition;
            let position = Position { x, y };
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
                level: None,
                rotated: false,
                equipment: None,
                travelling: false,
                stage: PlacementStage::Activation,
            })
        })
        .collect()
}

fn compile_contraptions(
    side_name: &str,
    definitions: Vec<StaticPlacement>,
) -> Result<Vec<Placement>, String> {
    definitions
        .into_iter()
        .map(|definition| {
            let StaticPlacement { type_name, x, y } = definition;
            let position = Position { x, y };
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
                level: None,
                rotated: false,
                equipment: None,
                travelling: false,
                stage: PlacementStage::Activation,
            })
        })
        .collect()
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
    let min_x = DEPLOYMENT_MIN_X + SHIELD_RADIUS + 1;
    let max_x = DEPLOYMENT_MAX_X - SHIELD_RADIUS - 1;
    let min_y = DEPLOYMENT_MIN_Y + SHIELD_RADIUS + 1;
    let max_y = DEPLOYMENT_MAX_Y - SHIELD_RADIUS - 1;
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

fn grid_center_remainder(extent: i64) -> Option<i64> {
    match extent.rem_euclid(20) {
        0 => Some(0),
        10 => Some(5),
        _ => None,
    }
}

fn validate_placement_collisions(blue: &SidePlan, red: &SidePlan) -> Result<(), String> {
    let mut world = Vec::with_capacity(
        blue.formations.len()
            + blue.constructions.len()
            + blue.contraptions.len()
            + red.formations.len()
            + red.constructions.len()
            + red.contraptions.len(),
    );
    world.extend(
        blue.formations
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
        red.formations
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

fn placement_footprint(placement: &Placement) -> Option<(i64, i64)> {
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
        && i64::from(placement.position.y) >= AMBUSH_MIN_Y
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

const fn formation_spec(native: NativeFormation, footprint: Option<(i64, i64)>) -> FormationSpec {
    FormationSpec { native, footprint }
}

const fn unit_spec(id: i32, width: i64, height: i64) -> FormationSpec {
    formation_spec(NativeFormation::Unit(id), Some((width, height)))
}

const fn construction_spec(id: i32, width: i64, height: i64) -> FormationSpec {
    formation_spec(NativeFormation::Construction(id), Some((width, height)))
}

const fn resolve_unit_type(type_name: &str) -> Option<FormationSpec> {
    match type_name.as_bytes() {
        b"fortress" => Some(unit_spec(1, 40, 40)),
        b"marksman" => Some(unit_spec(2, 20, 20)),
        b"vulcan" => Some(unit_spec(3, 40, 40)),
        b"melting_point" => Some(unit_spec(4, 40, 40)),
        b"rhino" => Some(unit_spec(5, 30, 30)),
        b"wasp" => Some(unit_spec(6, 50, 20)),
        b"mustang" => Some(unit_spec(7, 50, 20)),
        b"steel_ball" => Some(unit_spec(8, 50, 20)),
        b"fang" => Some(unit_spec(9, 50, 20)),
        b"crawler" => Some(unit_spec(10, 50, 20)),
        b"overlord" => Some(unit_spec(11, 50, 50)),
        b"stormcaller" => Some(unit_spec(12, 50, 20)),
        b"sledgehammer" => Some(unit_spec(13, 50, 20)),
        b"hacker" => Some(unit_spec(14, 30, 30)),
        b"arclight" => Some(unit_spec(15, 20, 20)),
        b"phoenix" => Some(unit_spec(16, 40, 20)),
        b"war_factory" => Some(unit_spec(17, 70, 70)),
        b"wraith" => Some(unit_spec(18, 30, 30)),
        b"scorpion" => Some(unit_spec(19, 30, 30)),
        b"fire_badger" => Some(unit_spec(20, 50, 20)),
        b"sabertooth" => Some(unit_spec(21, 30, 30)),
        b"typhoon" => Some(unit_spec(22, 40, 20)),
        b"sandworm" => Some(unit_spec(23, 40, 40)),
        b"tarantula" => Some(unit_spec(24, 30, 30)),
        b"phantom_ray" => Some(unit_spec(25, 50, 20)),
        b"farseer" => Some(unit_spec(26, 30, 30)),
        b"raiden" => Some(unit_spec(27, 40, 40)),
        b"hound" => Some(unit_spec(28, 40, 20)),
        b"abyss" => Some(unit_spec(29, 70, 70)),
        b"void_eye" => Some(unit_spec(30, 40, 20)),
        b"vortex" => Some(unit_spec(31, 20, 20)),
        b"mountain" => Some(unit_spec(2002, 70, 70)),
        _ => None,
    }
}

const fn resolve_construction_type(type_name: &str) -> Option<FormationSpec> {
    match type_name.as_bytes() {
        b"defensive_wall" => Some(construction_spec(1, 60, 10)),
        b"anti_armor_turret" => Some(construction_spec(2, 20, 20)),
        b"rapid_fire_turret" => Some(construction_spec(3, 20, 20)),
        b"magnetic_barrier" => Some(construction_spec(4, 50, 10)),
        _ => None,
    }
}

const fn resolve_contraption_type(type_name: &str) -> Option<FormationSpec> {
    match type_name.as_bytes() {
        b"shield" => Some(formation_spec(NativeFormation::Contraption(10001), None)),
        b"missile" => Some(formation_spec(NativeFormation::Contraption(20001), None)),
        b"interceptor" => Some(formation_spec(
            NativeFormation::Contraption(30001),
            Some((30, 30)),
        )),
        _ => None,
    }
}

/// Resolves a build-pinned native unit type ID to the public layout name and footprint.
#[must_use]
pub const fn unit_type_from_id(id: i32) -> Option<(&'static str, (i64, i64))> {
    match id {
        1 => Some(("fortress", (40, 40))),
        2 => Some(("marksman", (20, 20))),
        3 => Some(("vulcan", (40, 40))),
        4 => Some(("melting_point", (40, 40))),
        5 => Some(("rhino", (30, 30))),
        6 => Some(("wasp", (50, 20))),
        7 => Some(("mustang", (50, 20))),
        8 => Some(("steel_ball", (50, 20))),
        9 => Some(("fang", (50, 20))),
        10 => Some(("crawler", (50, 20))),
        11 => Some(("overlord", (50, 50))),
        12 => Some(("stormcaller", (50, 20))),
        13 => Some(("sledgehammer", (50, 20))),
        14 => Some(("hacker", (30, 30))),
        15 => Some(("arclight", (20, 20))),
        16 => Some(("phoenix", (40, 20))),
        17 => Some(("war_factory", (70, 70))),
        18 => Some(("wraith", (30, 30))),
        19 => Some(("scorpion", (30, 30))),
        20 => Some(("fire_badger", (50, 20))),
        21 => Some(("sabertooth", (30, 30))),
        22 => Some(("typhoon", (40, 20))),
        23 => Some(("sandworm", (40, 40))),
        24 => Some(("tarantula", (30, 30))),
        25 => Some(("phantom_ray", (50, 20))),
        26 => Some(("farseer", (30, 30))),
        27 => Some(("raiden", (40, 40))),
        28 => Some(("hound", (40, 20))),
        29 => Some(("abyss", (70, 70))),
        30 => Some(("void_eye", (40, 20))),
        31 => Some(("vortex", (20, 20))),
        2002 => Some(("mountain", (70, 70))),
        _ => None,
    }
}

/// Resolves a native construction type ID to the public layout name and footprint.
#[must_use]
pub const fn construction_type_from_id(id: i32) -> Option<(&'static str, (i64, i64))> {
    match id {
        1 => Some(("defensive_wall", (60, 10))),
        2 => Some(("anti_armor_turret", (20, 20))),
        3 => Some(("rapid_fire_turret", (20, 20))),
        4 => Some(("magnetic_barrier", (50, 10))),
        _ => None,
    }
}

/// Resolves a build-pinned native contraption type ID to the public layout name.
#[must_use]
pub const fn contraption_type_from_id(id: i32) -> Option<&'static str> {
    match id {
        10_001 => Some("shield"),
        20_001 => Some("missile"),
        30_001 => Some("interceptor"),
        _ => None,
    }
}

/// Resolves a build-pinned native commander-skill ID to the public layout name.
#[must_use]
pub const fn battle_skill_type_from_id(id: i32) -> Option<&'static str> {
    match id {
        100_002 => Some("incendiary_bomb"),
        200_001 => Some("electromagnetic_impact"),
        200_002 => Some("electromagnetic_blast"),
        200_003 => Some("photon_emission"),
        300_001 => Some("missile_strike"),
        300_003 => Some("orbital_bombardment"),
        300_004 => Some("nuke"),
        300_005 => Some("lightning_storm"),
        300_006 => Some("ion_blast"),
        300_007 => Some("orbital_javelin"),
        400_002 => Some("sticky_oil_bomb"),
        500_002 => Some("acid_blast"),
        600_002 => Some("smoke_bomb"),
        800_001 => Some("shield_airdrop"),
        1_200_001 => Some("underground_threat"),
        1_200_002 => Some("rhino_assault"),
        1_200_003 => Some("wasp_swarm"),
        1_200_004 => Some("mobilize_battleship"),
        1_200_005 => Some("vulcans_descent"),
        1_500_002 => Some("mobile_beacon"),
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
    fn requires_a_bounded_activation_round() {
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
        assert_eq!(invalid, "layout round must be within 1..=15");

        let invalid = compile(&json!({
            "round": 16,
            "sides": {
                "blue": {"formations": [{"type": "marksman", "x": 0, "y": -50}]},
                "red": {"formations": [{"type": "marksman", "x": 0, "y": -50}]}
            }
        }))
        .unwrap_err();
        assert_eq!(invalid, "layout round must be within 1..=15");
    }

    #[test]
    fn defaults_seed_to_zero_and_preserves_an_explicit_seed() {
        let layout = |seed| {
            let mut value = json!({
                "round": 1,
                "sides": {
                    "blue": {"formations": [{"type": "marksman", "x": 0, "y": -50}]},
                    "red": {"formations": [{"type": "marksman", "x": 0, "y": -50}]}
                }
            });
            if let Some(seed) = seed {
                value["seed"] = json!(seed);
            }
            value
        };

        assert_eq!(compile(&layout(None)).unwrap().seed, 0);
        assert_eq!(compile(&layout(Some(-17))).unwrap().seed, -17);
    }

    #[test]
    fn embedded_layout_keeps_placement_categories_explicit() {
        let error = parse_embedded_yaml(
            br#"
round: 1
sides:
  blue:
    formations:
      - {type: defensive_wall, x: 140, y: -105}
  red:
    formations:
      - {type: marksman, x: 0, y: -50}
"#,
        )
        .unwrap_err();
        assert_eq!(
            error,
            "side blue formation type \"defensive_wall\" belongs in constructions"
        );
    }

    #[test]
    fn compiles_formations_into_pre_activation_and_activation_stages() {
        let plan = compile(&json!({
            "round": 3,
            "sides": {
                "blue": {"formations": [
                    {"type": "marksman", "x": 0, "y": -50},
                    {"type": "marksman", "x": -310, "y": 20},
                    {"type": "arclight", "x": 310, "y": 20, "travelling": true}
                ], "contraptions": [
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
        assert_eq!(plan.blue.contraptions[0].stage, PlacementStage::Activation);
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
    fn travelling_requires_an_ambush_position() {
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
            placement_footprint(&plan.blue.formations[0]),
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
    fn constructions_reject_unit_only_fields() {
        let error = compile(&json!({
            "round": 1,
            "sides": {
                "blue": {
                    "formations": [{"type": "marksman", "x": 100, "y": -50}],
                    "constructions": [{"type": "defensive_wall", "x": 0, "y": -55,
                     "equipment": 13_030_001}]
                },
                "red": {"formations": [{
                    "type": "marksman", "x": 0, "y": -50
                }]}
            }
        }))
        .unwrap_err();

        assert!(error.contains("unknown field `equipment`"));
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
    fn rejects_construction_types_in_formations() {
        let error = compile(&json!({
            "round": 1,
            "sides": {
                "blue": {"formations": [{
                    "type": "defensive_wall",
                    "x": 0, "y": -55
                }]},
                "red": {"formations": [{
                    "type": "marksman", "x": 0, "y": -50
                }]}
            }
        }))
        .unwrap_err();
        assert_eq!(
            error,
            "side blue formation type \"defensive_wall\" at (0, -55) belongs in constructions"
        );
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
        assert!(error.contains("placements collide"));
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
            "side blue placement type \"sledgehammer\" at (285, -60) footprint 50x20 exceeds the main deployment boundary x=[-300,300], y=[-310,-10]"
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
            "placements collide: blue type \"sledgehammer\" at (5, -100) and blue type \"crawler\" at (20, -95)"
        );
    }

    #[test]
    fn rejects_unit_construction_collision_and_names_both_placements() {
        let value = yaml_fixture(include_str!(
            "../tests/fixtures/invalid-unit-construction-collision.yaml"
        ));
        let error = compile(&value).unwrap_err();
        assert_eq!(
            error,
            "placements collide: blue type \"marksman\" at (140, -100) and blue type \"defensive_wall\" at (140, -105)"
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
            "side blue placement type \"marksman\" at (5, -50) is off the native 10x10 grid: footprint 20x20 requires center x≡0, y≡0 (mod 10)"
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
    fn formation_specs_cover_all_public_unit_footprints() {
        let groups: &[(&[&str], (i64, i64))] = &[
            (&["marksman", "arclight", "vortex"], (20, 20)),
            (
                &[
                    "rhino",
                    "hacker",
                    "wraith",
                    "scorpion",
                    "sabertooth",
                    "tarantula",
                    "farseer",
                ],
                (30, 30),
            ),
            (&["phoenix", "typhoon", "hound", "void_eye"], (40, 20)),
            (
                &["fortress", "vulcan", "melting_point", "sandworm", "raiden"],
                (40, 40),
            ),
            (
                &[
                    "wasp",
                    "mustang",
                    "steel_ball",
                    "fang",
                    "crawler",
                    "stormcaller",
                    "sledgehammer",
                    "fire_badger",
                    "phantom_ray",
                ],
                (50, 20),
            ),
            (&["overlord"], (50, 50)),
            (&["war_factory", "abyss", "mountain"], (70, 70)),
        ];
        for &(type_names, footprint) in groups {
            for &type_name in type_names {
                let spec = resolve_unit_type(type_name).unwrap();
                assert!(matches!(spec.native, NativeFormation::Unit(_)));
                assert_eq!(spec.footprint, Some(footprint));
            }
        }
        assert_eq!(
            groups.iter().map(|(names, _)| names.len()).sum::<usize>(),
            32
        );
    }

    #[test]
    fn construction_specs_cover_the_four_constructions() {
        for (type_name, footprint) in [
            ("defensive_wall", (60, 10)),
            ("anti_armor_turret", (20, 20)),
            ("rapid_fire_turret", (20, 20)),
            ("magnetic_barrier", (50, 10)),
        ] {
            let spec = resolve_construction_type(type_name).unwrap();
            assert!(matches!(spec.native, NativeFormation::Construction(_)));
            assert_eq!(spec.footprint, Some(footprint));
        }
    }

    #[test]
    fn contraption_specs_cover_all_public_types() {
        assert_eq!(
            resolve_contraption_type("interceptor").unwrap().footprint,
            Some((30, 30))
        );
        assert_eq!(resolve_contraption_type("shield").unwrap().footprint, None);
        assert_eq!(resolve_contraption_type("missile").unwrap().footprint, None);
        assert_eq!(contraption_type_from_id(10_001), Some("shield"));
        assert_eq!(contraption_type_from_id(20_001), Some("missile"));
        assert_eq!(contraption_type_from_id(30_001), Some("interceptor"));
        assert_eq!(contraption_type_from_id(0), None);
    }

    #[test]
    fn battle_skill_reverse_catalog_matches_the_compiler_catalog() {
        for id in [
            100_002, 200_001, 200_002, 200_003, 300_001, 300_003, 300_004, 300_005, 300_006,
            300_007, 400_002, 500_002, 600_002, 800_001, 1_200_001, 1_200_002, 1_200_003,
            1_200_004, 1_200_005, 1_500_002,
        ] {
            let type_name = battle_skill_type_from_id(id).unwrap();
            assert_eq!(
                resolve_battle_skill_type(type_name)
                    .unwrap()
                    .commander_skill_id,
                id
            );
        }
        assert_eq!(battle_skill_type_from_id(0), None);
    }

    #[test]
    fn compiles_interceptor_and_reuses_formation_collision_validation() {
        let layout = |interceptor_y| {
            json!({
                "round": 1,
                "sides": {
                    "blue": {
                        "formations": [{"type": "marksman", "x": 0, "y": -100}],
                        "contraptions": [{"type": "interceptor", "x": 5, "y": interceptor_y}]
                    },
                    "red": {"formations": [{
                        "type": "marksman", "x": 0, "y": -50
                    }]}
                }
            })
        };

        let error = compile(&layout(-95)).unwrap_err();
        assert_eq!(
            error,
            "placements collide: blue type \"marksman\" at (0, -100) and blue type \"interceptor\" at (5, -95)"
        );

        let plan = compile(&layout(-125)).unwrap();
        assert_eq!(
            plan.blue.contraptions[0].native,
            NativeFormation::Contraption(30001)
        );
        assert_eq!(plan.blue.contraptions[0].level, None);
        assert!(!plan.blue.contraptions[0].rotated);
    }

    #[test]
    fn shield_and_missile_skip_grid_and_collision_validation() {
        let plan = compile(&json!({
            "round": 1,
            "sides": {
                "blue": {
                    "formations": [{"type": "marksman", "x": 0, "y": -100}],
                    "contraptions": [
                    {"type": "shield", "x": 1, "y": -101},
                    {"type": "missile", "x": 1, "y": -101}
                ]},
                "red": {"formations": [{
                    "type": "marksman", "x": 0, "y": -100
                }]}
            }
        }))
        .unwrap();

        assert_eq!(plan.formation_count(), 2);
        assert_eq!(plan.contraption_count(), 2);
        assert_eq!(
            plan.blue
                .contraptions
                .iter()
                .map(|placement| placement.native)
                .collect::<Vec<_>>(),
            [
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
                    "blue": {
                        "formations": [{"type": "marksman", "x": 0, "y": -150}],
                        "contraptions": [{"type": "shield", "x": x, "y": y}]
                    },
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
            "side blue placement type \"shield\" at (230, -80) places its radius-70 edge outside the own-side deployment boundary: center must be within x=[-229,229], y=[-239,-81]"
        );
    }

    #[test]
    fn missile_requires_only_its_center_inside_the_own_side() {
        let layout = |x, y| {
            json!({
                "round": 1,
                "sides": {
                    "blue": {
                        "formations": [{"type": "marksman", "x": 0, "y": -150}],
                        "contraptions": [{"type": "missile", "x": x, "y": y}]
                    },
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
            "side blue placement type \"missile\" at (301, -10) is outside the own-side deployment boundary x=[-300,300], y=[-310,-10]"
        );
    }

    #[test]
    fn compiles_all_four_construction_types_in_document_order() {
        let plan = compile(&json!({
            "round": 1,
            "sides": {
                "blue": {
                    "formations": [{"type": "marksman", "x": 0, "y": -150}],
                    "constructions": [
                    {"type": "rapid_fire_turret", "x": 140, "y": -60},
                    {"type": "defensive_wall", "x": 140, "y": -105},
                    {"type": "anti_armor_turret", "x": -140, "y": -60},
                    {"type": "magnetic_barrier", "x": -165, "y": -105}
                ]},
                "red": {"formations": [{
                    "type": "marksman", "x": 0, "y": -50
                }]}
            }
        }))
        .unwrap();

        assert_eq!(plan.formation_count(), 2);
        assert_eq!(plan.construction_count(), 4);
        assert_eq!(
            plan.blue
                .constructions
                .iter()
                .map(|placement| placement.native)
                .collect::<Vec<_>>(),
            [
                NativeFormation::Construction(3),
                NativeFormation::Construction(1),
                NativeFormation::Construction(2),
                NativeFormation::Construction(4),
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
        let formations = [
            ("fortress", NativeFormation::Unit(1)),
            ("mountain", NativeFormation::Unit(2002)),
        ];
        for (type_name, native) in formations {
            assert_eq!(
                resolve_unit_type(type_name).map(|spec| spec.native),
                Some(native)
            );
        }
        let constructions = [
            ("defensive_wall", NativeFormation::Construction(1)),
            ("magnetic_barrier", NativeFormation::Construction(4)),
        ];
        for (type_name, native) in constructions {
            assert_eq!(
                resolve_construction_type(type_name).map(|spec| spec.native),
                Some(native)
            );
        }
        let contraptions = [
            ("shield", NativeFormation::Contraption(10001)),
            ("missile", NativeFormation::Contraption(20001)),
            ("interceptor", NativeFormation::Contraption(30001)),
        ];
        for (type_name, native) in contraptions {
            assert_eq!(
                resolve_contraption_type(type_name).map(|spec| spec.native),
                Some(native)
            );
        }
        assert_eq!(resolve_unit_type("unit"), None);
        assert_eq!(resolve_construction_type("unit"), None);
        assert_eq!(resolve_contraption_type("unit"), None);
    }
}
