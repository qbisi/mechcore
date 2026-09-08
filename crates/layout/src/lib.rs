use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

mod grbr;
pub use grbr::{GrbrRoundTerrains, terrains_from_grbr_round};

/// Names the kind of document a file carries.
///
/// A layout is one of several documents this crate will define, and they share
/// most of their shape: a `turn` carries a partial layout beside its actions.
/// Structure alone therefore cannot say which one a file holds, so every
/// document names itself. The tag is a constant, not a version: it never needs
/// maintaining, and because it takes one value within a kind it cannot split
/// one state across two documents.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DocumentKind {
    Layout,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Layout {
    pub kind: DocumentKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1))]
    pub map_id: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<i32>,
    #[schemars(range(min = 1))]
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
    pub contraptions: Vec<ContraptionPlacement>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub airdrop_shields: Vec<Position>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub terrains: Vec<Terrain>,
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
    pub index: i32,
    pub position: Position,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exp: Option<i32>,
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
    pub index: i32,
    pub position: Position,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ContraptionPlacement {
    #[serde(rename = "type")]
    pub type_name: String,
    pub index: i32,
    pub position: Position,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BattleSkillDefinition {
    #[serde(rename = "type")]
    pub type_name: String,
    pub positions: Vec<Position>,
}

#[derive(
    Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord,
)]
#[serde(rename_all = "snake_case")]
pub enum TerrainType {
    Oil,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Terrain {
    #[serde(rename = "type")]
    pub terrain_type: TerrainType,
    pub control_points: Vec<Position>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub grid_rows: BTreeMap<u32, Vec<u32>>,
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
    pub research_center: ResearchCenter,
    pub energy_tower: EnergyTower,
    pub formations: Vec<Placement>,
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
const OIL_TERRAIN_RADIUS: i64 = 30;
const OIL_TERRAIN_GRID_SIZE: usize = 12;
const OIL_TERRAIN_GRID_MASK: u32 = (1 << OIL_TERRAIN_GRID_SIZE) - 1;
const LAYOUT_KIND: &str = "layout";
const OIL_TERRAIN_POINT_COUNT: u32 = 7;
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

    #[must_use]
    pub fn airdrop_shield_count(&self) -> usize {
        self.blue.airdrop_shields.len() + self.red.airdrop_shields.len()
    }

    #[must_use]
    pub fn terrain_count(&self) -> usize {
        self.blue.terrains.len() + self.red.terrains.len()
    }
}

impl Layout {
    /// Rewrites a layout into the one document that denotes its state.
    ///
    /// Two rules make up the normal form. Syntax equivalent to a public default
    /// is dropped, and every collection whose order carries no meaning is put in
    /// its defined order: indexed placements by deployment identity, technology
    /// and Officer IDs and retained airdrop shields ascending, and retained
    /// terrain by type and control points. `battle_skills` is the one exception,
    /// because release order is what it records.
    ///
    /// Applying this twice changes nothing the first pass did not already do.
    #[must_use]
    pub fn normalized(mut self) -> Self {
        for side in [&mut self.sides.blue, &mut self.sides.red] {
            side.techs.officers.sort_unstable();
            side.techs.units.sort_unstable();
            side.formations.sort_by_key(|formation| formation.index);
            side.constructions
                .sort_by_key(|construction| construction.index);
            side.contraptions
                .sort_by_key(|contraption| contraption.index);
            side.airdrop_shields
                .sort_unstable_by_key(|position| (position.x, position.y));
            side.terrains.sort_by_key(|terrain| {
                (
                    terrain.terrain_type,
                    terrain
                        .control_points
                        .iter()
                        .map(|position| (position.x, position.y))
                        .collect::<Vec<_>>(),
                )
            });
            for formation in &mut side.formations {
                if formation.level == Some(1) {
                    formation.level = None;
                }
                if formation.exp == Some(0) {
                    formation.exp = None;
                }
                if formation.rotated == Some(false) {
                    formation.rotated = None;
                }
                if formation.travelling == Some(false) {
                    formation.travelling = None;
                }
            }
            for terrain in &mut side.terrains {
                if terrain.grid_rows.len() == OIL_TERRAIN_POINT_COUNT as usize
                    && (0..OIL_TERRAIN_POINT_COUNT)
                        .all(|index| terrain.grid_rows.get(&index).is_some_and(Vec::is_empty))
                {
                    terrain.grid_rows.clear();
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
    if let Ok(header) = serde_yaml::from_slice::<DocumentHeader>(bytes) {
        require_layout_kind(header.kind.as_deref())?;
    }
    let layout: Layout =
        serde_yaml::from_slice(bytes).map_err(|error| format!("invalid layout YAML: {error}"))?;
    validate_embedded_categories(&layout)?;
    Ok(layout)
}

/// Reads only the discriminator, so a document of another kind is named as one.
///
/// Unknown fields are accepted here on purpose. Every document kind carries
/// fields a layout does not, and rejecting them at this level would answer
/// "which document is this?" with a complaint about one of its fields.
#[derive(Deserialize)]
struct DocumentHeader {
    #[serde(default)]
    kind: Option<String>,
}

/// Rejects a document that does not announce itself as a layout.
///
/// This runs before the document is deserialized, so a `turn` reaches the
/// reader as the wrong kind of document rather than as a layout with a strange
/// field. Without it the first difference in shape would be reported instead,
/// which says nothing about what the file actually is.
fn require_layout_kind(kind: Option<&str>) -> Result<(), String> {
    match kind {
        Some(LAYOUT_KIND) => Ok(()),
        Some(other) => Err(format!(
            "expected a {LAYOUT_KIND} document, found kind {other:?}"
        )),
        None => Err(format!(
            "document does not name its kind: a layout starts with `kind: {LAYOUT_KIND}`"
        )),
    }
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
/// Canonical layout YAML omits an unspecified `seed`, omits default-valued
/// optional syntax, puts every collection in the order [`Layout::normalized`]
/// defines, and ends with one newline. It does not preserve declaration order:
/// only `battle_skills`, whose order is its content, survives as written.
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
    let yaml = serde_yaml::to_string(&layout)
        .map_err(|error| format!("cannot serialize layout YAML: {error}"))?;
    Ok(collapse_placement_positions(&yaml))
}

/// Rewrites each placement's block-style `position` map onto one line.
///
/// A placement's coordinate pair is one value, so spending three lines on it
/// buries the fields that distinguish one placement from another. `serde_yaml`
/// has no per-field flow style, so the emitted document is folded afterwards.
/// Only the `position` key is folded: it always holds exactly `x` and `y`, so
/// the fold is total, whereas the lists of bare positions vary in length.
fn collapse_placement_positions(yaml: &str) -> String {
    let lines: Vec<&str> = yaml.lines().collect();
    let mut out = String::with_capacity(yaml.len());
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        if let Some(indent) = line
            .strip_suffix("position:")
            .filter(|indent| indent.chars().all(|character| character == ' ') && !indent.is_empty())
        {
            let field = |offset: usize, name: &str| {
                lines
                    .get(index + offset)
                    .and_then(|line| line.strip_prefix(indent))
                    .and_then(|line| line.strip_prefix("  "))
                    .and_then(|line| line.strip_prefix(name))
                    .filter(|value| {
                        value
                            .strip_prefix('-')
                            .unwrap_or(value)
                            .chars()
                            .all(|character| character.is_ascii_digit())
                            && !value.is_empty()
                    })
            };
            if let (Some(x), Some(y)) = (field(1, "x: "), field(2, "y: ")) {
                use std::fmt::Write as _;
                writeln!(out, "{indent}position: {{x: {x}, y: {y}}}")
                    .expect("writing to a String cannot fail");
                index += 3;
                continue;
            }
        }
        out.push_str(line);
        out.push('\n');
        index += 1;
    }
    out
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
    validate_placement_footprints("blue", &blue.formations)?;
    validate_placement_footprints("red", &red.formations)?;
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
    validate_techs(side_name, &side.techs)?;
    validate_side_modifiers(side_name, &side)?;
    let Side {
        techs,
        research_center,
        energy_tower,
        formations,
        constructions,
        contraptions,
        airdrop_shields,
        terrains,
        battle_skills,
    } = side;
    let formations = compile_formations(side_name, formations, round)?;
    let constructions = compile_constructions(side_name, constructions)?;
    let contraptions = compile_contraptions(side_name, contraptions)?;
    let airdrop_shields = compile_airdrop_shields(side_name, airdrop_shields)?;
    let terrains = compile_terrains(side_name, terrains)?;
    let battle_skills = compile_battle_skills(side_name, battle_skills)?;
    Ok(SidePlan {
        techs,
        research_center,
        energy_tower,
        formations,
        constructions,
        contraptions,
        airdrop_shields,
        terrains,
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
            let exp = exp.unwrap_or(0);
            let rotated = rotated.unwrap_or(false);
            let travelling = travelling.unwrap_or(false);
            if !(1..=9).contains(&level) {
                return Err(format!(
                    "side {side_name} formation type {type_name:?} at ({}, {}) level must be 1..=9",
                    position.x, position.y
                ));
            }
            if index < 0 {
                return Err(format!(
                    "side {side_name} formation type {type_name:?} at ({}, {}) index must be non-negative",
                    position.x, position.y
                ));
            }
            if exp < 0 {
                return Err(format!(
                    "side {side_name} formation type {type_name:?} at ({}, {}) exp must be non-negative",
                    position.x, position.y
                ));
            }
            if equipment.is_some_and(|id| id <= 0) {
                return Err(format!(
                    "side {side_name} formation type {type_name:?} at ({}, {}) equipment must be a positive integer",
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
            "side {side_name} formations must contain at least one valid unit"
        ));
    }
    validate_increasing_indices(side_name, "formation", &placements)?;
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
        let type_name = match terrain.terrain_type {
            TerrainType::Oil => "oil",
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
        if max_x + OIL_TERRAIN_RADIUS < BATTLEFIELD_MIN_X
            || min_x - OIL_TERRAIN_RADIUS > BATTLEFIELD_MAX_X
            || max_y + OIL_TERRAIN_RADIUS < BATTLEFIELD_MIN_Y
            || min_y - OIL_TERRAIN_RADIUS > BATTLEFIELD_MAX_Y
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

fn validate_unit_placement(
    side_name: &str,
    type_name: &str,
    position: Position,
    travelling: bool,
    round: i32,
) -> Result<(), String> {
    let in_ambush = i64::from(position.y) >= AMBUSH_MIN_Y;
    if travelling && !in_ambush {
        return Err(format!(
            "side {side_name} formation type {type_name:?} at ({}, {}) sets travelling=true outside the ambush zones",
            position.x, position.y
        ));
    }
    if !in_ambush {
        return Ok(());
    }
    if round == 1 {
        return Err(format!(
            "side {side_name} formation type {type_name:?} at ({}, {}) cannot occupy an ambush zone in round 1",
            position.x, position.y
        ));
    }
    if round == 2 && !travelling {
        return Err(format!(
            "side {side_name} formation type {type_name:?} at ({}, {}) must set travelling=true: a round 2 ambush unit is always a first flank deployment",
            position.x, position.y
        ));
    }
    Ok(())
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
        1_500_001 | 1_500_002 => Some("mobile_beacon"),
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
            "kind": "layout",
            "round": 1,
            "sides": {
                "blue": {
                    "formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}],
                    "battle_skills": [{"type": type_name, "positions": positions}]
                },
                "red": {
                    "formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}]
                }
            }
        })
    }

    fn layout_with_blue_terrains(terrains: &Value) -> Value {
        json!({
            "kind": "layout",
            "round": 1,
            "sides": {
                "blue": {
                    "formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}],
                    "terrains": terrains
                },
                "red": {
                    "formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}]
                }
            }
        })
    }

    #[test]
    fn a_document_of_another_kind_is_rejected_as_that_kind() {
        // The point of the discriminator is that this reads as "not a layout"
        // rather than as a layout with an unexpected field. A turn carries a
        // partial layout, so the shapes overlap and the first structural
        // difference would say nothing about what the file is.
        let turn = br"
kind: turn
round: 1
actions:
  - type: deploy
sides:
  blue:
    formations: [{type: marksman, index: 0, position: {x: 0, y: -50}}]
  red:
    formations: [{type: marksman, index: 0, position: {x: 0, y: -50}}]
";
        let error = parse_embedded_yaml(turn).unwrap_err();
        assert_eq!(error, "expected a layout document, found kind \"turn\"");

        let untagged = br"
round: 1
sides:
  blue:
    formations: [{type: marksman, index: 0, position: {x: 0, y: -50}}]
  red:
    formations: [{type: marksman, index: 0, position: {x: 0, y: -50}}]
";
        assert_eq!(
            parse_embedded_yaml(untagged).unwrap_err(),
            "document does not name its kind: a layout starts with `kind: layout`"
        );

        // The JSON entry point that MCP publishes answers the same way.
        assert_eq!(
            compile(&json!({"kind": "turn", "round": 1, "sides": {}})).unwrap_err(),
            "expected a layout document, found kind \"turn\""
        );

        // Malformed YAML still reports the syntax problem, not the kind.
        assert!(
            parse_embedded_yaml(b"kind: layout\nround: [\n")
                .unwrap_err()
                .starts_with("invalid layout YAML:")
        );
    }

    #[test]
    fn requires_a_positive_round() {
        let missing = compile(&json!({
            "kind": "layout",
            "sides": {
                "blue": {"formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}]},
                "red": {"formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}]}
            }
        }))
        .unwrap_err();
        assert!(missing.contains("missing field `round`"));

        let invalid = compile(&json!({
            "kind": "layout",
            "round": 0,
            "sides": {
                "blue": {"formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}]},
                "red": {"formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}]}
            }
        }))
        .unwrap_err();
        assert_eq!(invalid, "layout round must be at least 1");

        // The staging budget belongs to the executor, so the schema accepts a
        // round that no Training Ground run can reach.
        assert_eq!(
            compile(&json!({
                "kind": "layout",
                "round": 40,
                "sides": {
                    "blue": {"formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}]},
                    "red": {"formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}]}
                }
            }))
            .unwrap()
            .round,
            40
        );
    }

    #[test]
    fn normalization_reaches_its_fixed_point_in_one_pass() {
        let denormalized: Layout = serde_json::from_value(json!({
            "kind": "layout",
            "round": 2,
            "sides": {
                "blue": {
                    "techs": {"officers": [20039, 10014], "units": [10202, 10101]},
                    "formations": [
                        {"index": 4, "type": "marksman", "position": {"x": 40, "y": -50}, "level": 1},
                        {"index": 1, "type": "marksman", "position": {"x": 0, "y": -50},
                         "exp": 0, "rotated": false, "travelling": false}
                    ],
                    "contraptions": [
                        {"index": 3, "type": "shield", "position": {"x": 0, "y": -120}},
                        {"index": 2, "type": "shield", "position": {"x": 100, "y": -120}}
                    ],
                    "airdrop_shields": [{"x": 200, "y": 20}, {"x": -200, "y": 20}],
                    "terrains": [
                        {"type": "oil", "control_points": [{"x": 100, "y": 0}, {"x": 120, "y": 0}]},
                        {"type": "oil", "control_points": [{"x": -100, "y": 0}, {"x": -80, "y": 0}]}
                    ]
                },
                "red": {"formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}]}
            }
        }))
        .unwrap();

        let once = denormalized.normalized();
        assert_eq!(once.sides.blue.techs.officers, [10014, 20039]);
        assert_eq!(once.sides.blue.techs.units, [10101, 10202]);
        assert_eq!(
            once.sides
                .blue
                .formations
                .iter()
                .map(|formation| formation.index)
                .collect::<Vec<_>>(),
            [1, 4]
        );
        assert_eq!(
            once.sides
                .blue
                .contraptions
                .iter()
                .map(|contraption| contraption.index)
                .collect::<Vec<_>>(),
            [2, 3]
        );
        assert_eq!(
            once.sides.blue.airdrop_shields,
            [Position { x: -200, y: 20 }, Position { x: 200, y: 20 }]
        );
        assert_eq!(
            once.sides
                .blue
                .terrains
                .iter()
                .map(|terrain| terrain.control_points[0].x)
                .collect::<Vec<_>>(),
            [-100, 100]
        );
        assert!(once.sides.blue.formations[0].level.is_none());
        assert!(once.sides.blue.formations[0].exp.is_none());
        assert!(once.sides.blue.formations[0].rotated.is_none());
        assert!(once.sides.blue.formations[0].travelling.is_none());

        assert_eq!(once.clone().normalized(), once);
    }

    #[test]
    fn tracked_layouts_are_normal_and_normalize_idempotently() {
        let directory = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/layouts");
        let mut checked = 0;
        for entry in std::fs::read_dir(directory).expect("tracked layout directory") {
            let path = entry.expect("directory entry").path();
            if path.extension().is_none_or(|extension| extension != "yaml") {
                continue;
            }
            let bytes = std::fs::read(&path).expect("readable layout");
            let layout = parse_yaml(&bytes).unwrap_or_else(|error| {
                panic!("{} does not parse: {error}", path.display());
            });
            assert_eq!(
                layout.clone(),
                layout.clone().normalized(),
                "{} is not in normal form",
                path.display()
            );
            let once = layout.normalized();
            let twice = once.clone().normalized();
            assert_eq!(
                once,
                twice,
                "{} is not a normalization fixed point",
                path.display()
            );
            checked += 1;
        }
        assert!(checked > 0, "no tracked layouts were read from {directory}");
    }

    #[test]
    fn map_id_is_optional_positive_and_preserved() {
        let mut value = json!({"kind": "layout", "round": 1, "sides": {
            "blue": {"formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}]},
            "red": {"formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}]}
        }});
        assert_eq!(compile(&value).unwrap().map_id, None);
        for id in [1001, 1021] {
            value["map_id"] = json!(id);
            assert_eq!(compile(&value).unwrap().map_id, Some(id));
            let layout: Layout = serde_json::from_value(value.clone()).unwrap();
            let yaml = canonical_yaml(layout).unwrap();
            assert!(yaml.contains(&format!("map_id: {id}")));
        }
        for id in [
            json!(0),
            json!(-1),
            json!(1.5),
            json!("1021"),
            json!(2_147_483_648_i64),
        ] {
            value["map_id"] = id;
            assert!(compile(&value).is_err());
        }
    }

    #[test]
    fn leaves_an_unspecified_seed_absent_and_rejects_the_zero_sentinel() {
        let layout = |seed| {
            let mut value = json!({
                "kind": "layout",
                "round": 1,
                "sides": {
                    "blue": {"formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}]},
                    "red": {"formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}]}
                }
            });
            if let Some(seed) = seed {
                value["seed"] = json!(seed);
            }
            value
        };

        assert_eq!(compile(&layout(None)).unwrap().seed, None);
        assert_eq!(compile(&layout(Some(-17))).unwrap().seed, Some(-17));
        assert!(
            compile(&layout(Some(0)))
                .unwrap_err()
                .contains("native system-random request")
        );
    }

    #[test]
    fn compiles_and_counts_valid_oil_terrain_state() {
        let plan = compile(&layout_with_blue_terrains(&json!([
            {"type": "oil", "control_points": [{"x": -60, "y": 40}, {"x": 60, "y": 40}]},
            {"type": "oil", "control_points": [{"x": -60, "y": 40}, {"x": 60, "y": 40}], "grid_rows": {"0": [], "1": vec![0x0fff_u32; 12]}}
        ])))
        .unwrap();

        assert_eq!(plan.terrain_count(), 2);
        assert_eq!(plan.blue.terrains[0].terrain_type, TerrainType::Oil);
        assert!(plan.blue.terrains[0].grid_rows.is_empty());
        assert!(plan.blue.terrains[1].grid_rows[&0].is_empty());
        assert_eq!(plan.blue.terrains[1].grid_rows[&1], vec![0x0fff; 12]);
        assert!(plan.red.terrains.is_empty());
    }

    #[test]
    fn terrain_fields_and_grid_shape_are_fail_closed() {
        let unknown = compile(&layout_with_blue_terrains(&json!([
            {"type": "fire", "control_points": [{"x": 0, "y": 0}, {"x": 60, "y": 0}]}
        ])))
        .unwrap_err();
        assert!(unknown.contains("unknown variant `fire`, expected `oil`"));

        let outside = compile(&layout_with_blue_terrains(&json!([
            {"type": "oil", "control_points": [{"x": 431, "y": 0}, {"x": 500, "y": 0}]}
        ])))
        .unwrap_err();
        assert!(outside.contains("does not overlap the battlefield"));

        let wrong_count = compile(&layout_with_blue_terrains(&json!([
            {"type": "oil", "control_points": [{"x": 0, "y": 0}]}
        ])))
        .unwrap_err();
        assert!(wrong_count.contains("control_points must contain exactly two points"));

        let wrong_height = compile(&layout_with_blue_terrains(&json!([
            {"type": "oil", "control_points": [{"x": 0, "y": 0}, {"x": 60, "y": 0}], "grid_rows": {"1": vec![1_u32; 11]}}
        ])))
        .unwrap_err();
        assert!(wrong_height.contains("exactly 12 rows"));

        let outside_width = compile(&layout_with_blue_terrains(&json!([
            {"type": "oil", "control_points": [{"x": 0, "y": 0}, {"x": 60, "y": 0}], "grid_rows": {"1": vec![0x1000_u32; 12]}}
        ])))
        .unwrap_err();
        assert!(outside_width.contains("uses bits outside width 12"));

        let empty_grid = compile(&layout_with_blue_terrains(&json!([
            {"type": "oil", "control_points": [{"x": 0, "y": 0}, {"x": 60, "y": 0}], "grid_rows": {"1": vec![0_u32; 12]}}
        ])))
        .unwrap_err();
        assert!(empty_grid.contains("must activate at least one cell"));

        let outside_index = compile(&layout_with_blue_terrains(&json!([
            {"type": "oil", "control_points": [{"x": 0, "y": 0}, {"x": 60, "y": 0}], "grid_rows": {"7": []}}
        ])))
        .unwrap_err();
        assert!(outside_index.contains("point index 7 must be within 0..6"));
    }

    #[test]
    fn embedded_layout_keeps_placement_categories_explicit() {
        let error = parse_embedded_yaml(
            br"
kind: layout
round: 1
sides:
  blue:
    formations:
      - {type: defensive_wall, index: 0, position: {x: 140, y: -105}}
  red:
    formations:
      - {type: marksman, index: 0, position: {x: 0, y: -50}}
",
        )
        .unwrap_err();
        assert_eq!(
            error,
            "side blue formation type \"defensive_wall\" belongs in constructions"
        );
    }

    #[test]
    fn compiles_formations_in_declaration_order() {
        let plan = compile(&json!({
            "kind": "layout",
            "round": 3,
            "sides": {
                "blue": {"formations": [
                    {"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}},
                    {"index": 1, "type": "marksman", "position": {"x": -310, "y": 20}},
                    {"index": 2, "type": "arclight", "position": {"x": 310, "y": 20}, "travelling": true}
                ], "contraptions": [
                    {"index": 0, "type": "interceptor", "position": {"x": 5, "y": -85}}
                ]},
                "red": {"formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}]}
            }
        }))
        .unwrap();

        assert_eq!(plan.round, 3);
        let types = plan
            .blue
            .formations
            .iter()
            .map(|placement| placement.type_name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(types, ["marksman", "marksman", "arclight"]);
        assert!(!plan.blue.formations[1].travelling);
        assert!(plan.blue.formations[2].travelling);
        assert_eq!(plan.blue.contraptions[0].type_name, "interceptor");
    }

    #[test]
    fn enforces_round_rules_for_ambush_units() {
        let layout = |round, travelling| {
            json!({
                "kind": "layout",
                "round": round,
                "sides": {
                    "blue": {"formations": [
                        {"index": 0, "type": "marksman", "position": {"x": -310, "y": 20}, "travelling": travelling}
                    ]},
                    "red": {"formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}]}
                }
            })
        };

        assert!(
            compile(&layout(1, false))
                .unwrap_err()
                .contains("in round 1")
        );
        assert!(
            compile(&layout(1, true))
                .unwrap_err()
                .contains("in round 1")
        );
        assert!(
            compile(&layout(2, false))
                .unwrap_err()
                .contains("first flank deployment")
        );
        let omitted = compile(&json!({
            "kind": "layout",
            "round": 2,
            "sides": {
                "blue": {"formations": [{"index": 0, "type": "marksman", "position": {"x": -310, "y": 20}}]},
                "red": {"formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}]}
            }
        }))
        .unwrap_err();
        assert!(omitted.contains("first flank deployment"));
        assert!(compile(&layout(2, true)).unwrap().blue.formations[0].travelling);
        assert!(!compile(&layout(3, false)).unwrap().blue.formations[0].travelling);
    }

    #[test]
    fn rejects_round_2_ambush_fixture_that_omits_travelling() {
        let value = yaml_fixture(include_str!(
            "../tests/fixtures/invalid-round-2-ambush-travelling.yaml"
        ));
        let error = compile(&value).unwrap_err();
        assert_eq!(
            error,
            "side blue formation type \"marksman\" at (-310, 20) must set travelling=true: a round 2 ambush unit is always a first flank deployment"
        );
    }

    #[test]
    fn travelling_requires_an_ambush_position() {
        let main_error = compile(&json!({
            "kind": "layout",
            "round": 3,
            "sides": {
                "blue": {"formations": [
                    {"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}, "travelling": true}
                ]},
                "red": {"formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}]}
            }
        }))
        .unwrap_err();
        assert!(main_error.contains("travelling=true outside the ambush zones"));
    }

    #[test]
    fn ambush_unit_footprint_must_fit_one_flank_region() {
        let error = compile(&json!({
            "kind": "layout",
            "round": 3,
            "sides": {
                "blue": {"formations": [
                    {"index": 0, "type": "marksman", "position": {"x": -300, "y": 20}, "travelling": true}
                ]},
                "red": {"formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}]}
            }
        }))
        .unwrap_err();
        assert!(error.contains("must fit completely inside one ambush zone"));
    }

    #[test]
    fn ambush_region_orientation_changes_the_effective_unit_footprint() {
        let plan = compile(&json!({
            "kind": "layout",
            "round": 3,
            "sides": {
                "blue": {"formations": [
                    {"index": 0,
                        "type": "crawler", "position": {"x": 325, "y": 60},
                        "rotated": true, "travelling": true
                    }
                ]},
                "red": {"formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}]}
            }
        }))
        .unwrap();
        assert_eq!(
            placement_footprint(&plan.blue.formations[0]),
            Some((50, 20))
        );

        let error = compile(&json!({
            "kind": "layout",
            "round": 3,
            "sides": {
                "blue": {"formations": [
                    {"index": 0,
                        "type": "crawler", "position": {"x": 310, "y": 65},
                        "rotated": true, "travelling": true
                    }
                ]},
                "red": {"formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}]}
            }
        }))
        .unwrap_err();
        assert!(error.contains("footprint 50x20 requires center x≡5, y≡0"));
    }

    #[test]
    fn compiles_unit_defaults_for_both_sides() {
        let plan = compile(&json!({
            "kind": "layout",
            "round": 1,
            "sides": {
                "blue": {"formations": [{"index": 0,
                    "type": "marksman",
                    "position": {"x": -20, "y": -50}
                }]},
                "red": {"formations": [{"index": 0,
                    "type": "marksman",
                    "position": {"x": 20, "y": -180}
                }]}
            }
        }))
        .unwrap();

        assert_eq!(plan.blue.formations[0].level, Some(1));
        assert_eq!(plan.blue.formations[0].index, Some(0));
        assert_eq!(plan.blue.formations[0].exp, Some(0));
        assert!(!plan.blue.formations[0].rotated);
        assert_eq!(plan.blue.formations[0].equipment, None);
        assert_eq!(plan.blue.formations[0].type_name, "marksman");
        assert_eq!(plan.blue.formations[0].native, NativeFormation::Unit(2));
        assert_eq!(plan.red.formations[0].position, Position { x: 20, y: -180 });
        assert_eq!(plan.formation_count(), 2);
    }

    #[test]
    fn compiles_stable_unit_indices_and_experience() {
        let plan = compile(&json!({
            "kind": "layout",
            "round": 1,
            "sides": {
                "blue": {"formations": [
                    {"type": "marksman", "index": 0, "position": {"x": -20, "y": -50}, "exp": 7},
                    {"type": "marksman", "index": 2, "position": {"x": 20, "y": -50}}
                ]},
                "red": {"formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -180}}]}
            }
        }))
        .unwrap();

        assert_eq!(plan.blue.formations[0].index, Some(0));
        assert_eq!(plan.blue.formations[0].exp, Some(7));
        assert_eq!(plan.blue.formations[1].index, Some(2));
        assert_eq!(plan.blue.formations[1].exp, Some(0));

        let duplicate = compile(&json!({
            "kind": "layout",
            "round": 1,
            "sides": {
                "blue": {"formations": [
                    {"type": "marksman", "index": 1, "position": {"x": -20, "y": -50}},
                    {"type": "marksman", "index": 1, "position": {"x": 20, "y": -50}}
                ]},
                "red": {"formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -180}}]}
            }
        }))
        .unwrap_err();
        assert!(duplicate.contains("indices must be strictly increasing"));

        let negative_exp = compile(&json!({
            "kind": "layout",
            "round": 1,
            "sides": {
                "blue": {"formations": [
                    {"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}, "exp": -1}
                ]},
                "red": {"formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -180}}]}
            }
        }))
        .unwrap_err();
        assert!(negative_exp.contains("exp must be non-negative"));
    }

    #[test]
    fn compiles_one_equipment_for_a_unit() {
        let plan = compile(&json!({
            "kind": "layout",
            "round": 1,
            "sides": {
                "blue": {"formations": [{"index": 0,
                    "type": "marksman", "position": {"x": 0, "y": -50},
                    "equipment": 13_030_001
                }]},
                "red": {"formations": [{"index": 0,
                    "type": "marksman", "position": {"x": 0, "y": -50}
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
            "kind": "layout",
            "round": 1,
            "sides": {
                "blue": {
                    "formations": [{"index": 0, "type": "marksman", "position": {"x": 100, "y": -50}}],
                    "constructions": [{"index": 0, "type": "defensive_wall", "position": {"x": 0, "y": -55},
                     "equipment": 13_030_001}]
                },
                "red": {"formations": [{"index": 0,
                    "type": "marksman", "position": {"x": 0, "y": -50}
                }]}
            }
        }))
        .unwrap_err();

        assert!(error.contains("unknown field `equipment`"));
    }

    #[test]
    fn rejects_nonpositive_equipment_id() {
        let error = compile(&json!({
            "kind": "layout",
            "round": 1,
            "sides": {
                "blue": {"formations": [{"index": 0,
                    "type": "marksman", "position": {"x": 0, "y": -50}, "equipment": 0
                }]},
                "red": {"formations": [{"index": 0,
                    "type": "marksman", "position": {"x": 0, "y": -50}
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
            "kind": "layout",
            "round": 1,
            "sides": {
                "blue": {
                    "techs": {"officers": [30602], "units": [10202]},
                    "formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}]
                },
                "red": {
                    "formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}]
                }
            }
        }))
        .unwrap();
        assert_eq!(valid.blue.techs.officers, [30602]);
        assert_eq!(valid.blue.techs.units, [10202]);

        let error = compile(&json!({
            "kind": "layout",
            "round": 1,
            "sides": {
                "blue": {"techs": {"units": [10202, 10202]}, "formations": [{"index": 0,
                    "type": "marksman", "position": {"x": 0, "y": -50}
                }]},
                "red": {"formations": [{"index": 0,
                    "type": "marksman", "position": {"x": 0, "y": -50}
                }]}
            }
        }))
        .unwrap_err();
        assert_eq!(error, "side blue techs.units contains duplicate ID 10202");

        let error = compile(&json!({
            "kind": "layout",
            "round": 1,
            "sides": {
                "blue": {"techs": {"officers": [0]}, "formations": [{"index": 0,
                    "type": "marksman", "position": {"x": 0, "y": -50}
                }]},
                "red": {"formations": [{"index": 0,
                    "type": "marksman", "position": {"x": 0, "y": -50}
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
            "kind": "layout",
            "round": 1,
            "sides": {
                "blue": {
                    "research_center": {"attack_level": 3},
                    "formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}]
                },
                "red": {
                    "formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}]
                }
            }
        }))
        .unwrap_err();
        assert_eq!(error, "side blue research_center levels must each be 0..=2");
    }

    #[test]
    fn rejects_construction_types_in_formations() {
        let error = compile(&json!({
            "kind": "layout",
            "round": 1,
            "sides": {
                "blue": {"formations": [{"index": 0,
                    "type": "defensive_wall",
                    "position": {"x": 0, "y": -55}
                }]},
                "red": {"formations": [{"index": 0,
                    "type": "marksman", "position": {"x": 0, "y": -50}
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
            "kind": "layout",
            "round": 1,
            "sides": {
                "blue": {},
                "red": {"formations": [{"index": 0,
                    "type": "marksman", "position": {"x": 0, "y": -50}
                }]}
            }
        }))
        .unwrap_err();
        assert!(missing.contains("missing field `formations`"));

        let empty = compile(&json!({
            "kind": "layout",
            "round": 1,
            "sides": {
                "blue": {"formations": []},
                "red": {"formations": [{"index": 0,
                    "type": "marksman", "position": {"x": 0, "y": -50}
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
                "kind": "layout",
                "round": 1,
                "sides": {
                    "blue": {"formations": [
                        {"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}},
                        {"index": 1, "type": "arclight", "position": {"x": second_x, "y": -50}}
                    ]},
                    "red": {"formations": [{"index": 0,
                        "type": "marksman", "position": {"x": 0, "y": -50}
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
            "kind": "layout",
            "round": 1,
            "sides": {
                "blue": {"formations": [{"index": 0,
                    "type": "marksman", "position": {"x": 5, "y": -50}
                }]},
                "red": {"formations": [{"index": 0,
                    "type": "marksman", "position": {"x": 0, "y": -50}
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
        assert_eq!(battle_skill_type_from_id(1_500_001), Some("mobile_beacon"));
        assert_eq!(
            resolve_battle_skill_type(battle_skill_type_from_id(1_500_001).unwrap())
                .unwrap()
                .commander_skill_id,
            1_500_002
        );
        assert_eq!(battle_skill_type_from_id(0), None);
    }

    #[test]
    fn compiles_interceptor_and_reuses_formation_collision_validation() {
        let layout = |interceptor_y| {
            json!({
                "kind": "layout",
                "round": 1,
                "sides": {
                    "blue": {
                        "formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -100}}],
                        "contraptions": [{"index": 0, "type": "interceptor", "position": {"x": 5, "y": interceptor_y}}]
                    },
                    "red": {"formations": [{"index": 0,
                        "type": "marksman", "position": {"x": 0, "y": -50}
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
            "kind": "layout",
            "round": 1,
            "sides": {
                "blue": {
                    "formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -100}}],
                    "contraptions": [
                    {"index": 0, "type": "shield", "position": {"x": 1, "y": -101}},
                    {"index": 1, "type": "missile", "position": {"x": 1, "y": -101}}
                ]},
                "red": {"formations": [{"index": 0,
                    "type": "marksman", "position": {"x": 0, "y": -100}
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
    fn retained_airdrop_shields_are_their_own_collection() {
        let mut value = json!({"kind": "layout", "round": 2, "sides": {
            "blue": {"formations": [{"index": 0, "type":"marksman","position": {"x": 0, "y": -150}}],
                "contraptions": [{"index": 0, "type":"shield","position": {"x": 0, "y": -120}}],
                "airdrop_shields": [{"x":300,"y":20}, {"x":-300,"y":20}]},
            "red": {"formations": [{"index": 0, "type":"marksman","position": {"x": 0, "y": -150}}]}}});
        let plan = compile(&value).unwrap();
        assert_eq!(plan.blue.contraptions.len(), 1);
        assert_eq!(plan.airdrop_shield_count(), 2);
        assert_eq!(
            plan.blue.airdrop_shields,
            [Position { x: 300, y: 20 }, Position { x: -300, y: 20 }]
        );

        // A retained airdrop stands where it was released, not where a new
        // contraption could be placed, so only the battlefield bounds apply.
        value["sides"]["blue"]["airdrop_shields"][0]["x"] = json!(401);
        assert!(
            compile(&value)
                .unwrap_err()
                .contains("airdrop_shields[0] center (401, 20) is outside the battlefield")
        );
    }

    #[test]
    fn contraptions_reject_the_retired_isairdrop_field() {
        for kind in ["shield", "missile", "interceptor"] {
            let value = json!({"kind": "layout", "round":1,"sides":{
                "blue":{"formations":[{"index": 0, "type":"marksman","position": {"x": 0, "y": -150}}],
                    "contraptions":[{"index": 0, "type":kind,"position": {"x": 5, "y": -95},"isairdrop":false}]},
                "red":{"formations":[{"index": 0, "type":"marksman","position": {"x": 0, "y": -150}}]}}});
            assert!(compile(&value).unwrap_err().contains("unknown field"));
        }
    }

    #[test]
    fn shield_requires_its_complete_edge_inside_the_own_side() {
        let layout = |x, y| {
            json!({
                "kind": "layout",
                "round": 1,
                "sides": {
                    "blue": {
                        "formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -150}}],
                        "contraptions": [{"index": 0, "type": "shield", "position": {"x": x, "y": y}}]
                    },
                    "red": {"formations": [{"index": 0,
                        "type": "marksman", "position": {"x": 0, "y": -150}
                    }]}
                }
            })
        };

        compile(&layout(230, -80)).unwrap();
        compile(&layout(-230, -240)).unwrap();
        let error = compile(&layout(231, -79)).unwrap_err();
        assert_eq!(
            error,
            "side blue placement type \"shield\" at (231, -79) places its radius-70 edge outside the own-side deployment boundary: center must be within x=[-230,230], y=[-240,-80]"
        );
    }

    #[test]
    fn missile_requires_only_its_center_inside_the_own_side() {
        let layout = |x, y| {
            json!({
                "kind": "layout",
                "round": 1,
                "sides": {
                    "blue": {
                        "formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -150}}],
                        "contraptions": [{"index": 0, "type": "missile", "position": {"x": x, "y": y}}]
                    },
                    "red": {"formations": [{"index": 0,
                        "type": "marksman", "position": {"x": 0, "y": -150}
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
            "kind": "layout",
            "round": 1,
            "sides": {
                "blue": {
                    "formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -150}}],
                    "constructions": [
                    {"index": 0, "type": "rapid_fire_turret", "position": {"x": 140, "y": -60}},
                    {"index": 1, "type": "defensive_wall", "position": {"x": 140, "y": -105}},
                    {"index": 2, "type": "anti_armor_turret", "position": {"x": -140, "y": -60}},
                    {"index": 3, "type": "magnetic_barrier", "position": {"x": -165, "y": -105}}
                ]},
                "red": {"formations": [{"index": 0,
                    "type": "marksman", "position": {"x": 0, "y": -50}
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
                .map(|placement| placement.index)
                .collect::<Vec<_>>(),
            [Some(0), Some(1), Some(2), Some(3)]
        );
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
    fn construction_and_contraption_indices_are_required_and_ordered() {
        let layout = |constructions: Value, contraptions: Value| {
            json!({
                "kind": "layout",
                "round": 1,
                "sides": {
                    "blue": {
                        "formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -150}}],
                        "constructions": constructions,
                        "contraptions": contraptions
                    },
                    "red": {"formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}]}
                }
            })
        };
        let wall = |index: i32| json!({"index": index, "type": "defensive_wall", "position": {"x": 140, "y": -105}});
        let shield = |index: i32, x: i32| json!({"index": index, "type": "shield", "position": {"x": x, "y": -95}});

        let plan = compile(&layout(
            json!([wall(7)]),
            json!([shield(2, 5), shield(9, -105)]),
        ))
        .unwrap();
        assert_eq!(plan.blue.constructions[0].index, Some(7));
        assert_eq!(
            plan.blue
                .contraptions
                .iter()
                .map(|placement| placement.index)
                .collect::<Vec<_>>(),
            [Some(2), Some(9)]
        );

        // An index survives serialization, because it is the object's identity
        // rather than a position in the list.
        let definition: Layout =
            serde_json::from_value(layout(json!([wall(7)]), json!([]))).unwrap();
        let encoded = serde_json::to_value(definition).unwrap();
        assert_eq!(encoded["sides"]["blue"]["constructions"][0]["index"], 7);

        let missing = json!({"type": "defensive_wall", "position": {"x": 140, "y": -105}});
        assert!(
            compile(&layout(json!([missing]), json!([])))
                .unwrap_err()
                .contains("missing field `index`")
        );
        assert!(
            compile(&layout(json!([]), json!([shield(9, 5), shield(2, -105)])))
                .unwrap_err()
                .contains("contraption indices must be strictly increasing")
        );
        assert!(
            compile(&layout(json!([wall(-1)]), json!([])))
                .unwrap_err()
                .contains("construction index must be non-negative")
        );
    }

    #[test]
    fn compiles_position_targeted_battle_skills_in_document_order() {
        let plan = compile(&json!({
            "kind": "layout",
            "round": 1,
            "sides": {
                "blue": {
                    "formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}],
                    "battle_skills": [{
                        "type": "missile_strike",
                        "positions": [{"x": 55, "y": 60}]
                    }]
                },
                "red": {
                    "formations": [{"index": 0, "type": "fang", "position": {"x": -55, "y": -60}}],
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
            "kind": "layout",
            "round": 1,
            "sides": {
                "blue": {
                    "formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}]
                },
                "red": {
                    "formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}],
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
                "kind": "layout",
                "round": 1,
                "sides": {
                    "blue": {
                        "formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}],
                        "battle_skills": battle_skills
                    },
                    "red": {
                        "formations": [{"index": 0, "type": "marksman", "position": {"x": 0, "y": -50}}]
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
