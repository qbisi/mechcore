//! The layout document: the projection of a state onto what a fight simulates.
//!
//! `docs/layout.md` defines it. This module owns the document's shapes, its
//! parsing, and its normal form. Compiling one into an execution plan is
//! [`crate::compile`], and the catalogues both consult are [`crate::catalog`].

use crate::DocumentKind;
use crate::catalog::{resolve_construction_type, resolve_contraption_type, resolve_unit_type};
use crate::compile::compile_layout;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub(crate) const OIL_TERRAIN_GRID_SIZE: usize = 12;
pub(crate) const OIL_TERRAIN_GRID_MASK: u32 = (1 << OIL_TERRAIN_GRID_SIZE) - 1;
pub(crate) const LAYOUT_KIND: &str = "layout";
pub(crate) const OIL_TERRAIN_POINT_COUNT: u32 = 7;

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

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Position {
    pub x: i32,
    pub y: i32,
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
pub(crate) fn require_layout_kind(kind: Option<&str>) -> Result<(), String> {
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
pub(crate) fn collapse_placement_positions(yaml: &str) -> String {
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
