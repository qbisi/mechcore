//! The layout document: the projection of a state onto what a fight simulates.
//!
//! `docs/spec/document/layout.md` defines it. This module owns the document's shapes, its
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

/// How many fixed towers a side's building manager holds.
///
/// A 1v1 side holds exactly this many fixed towers, which `docs/spec/document/state.md`
/// states.
///
/// A position in that list is a key, not a name. `tower_strengthen_levels` is
/// keyed by it, the same key `PAD_StrengthenTower.Index` uses, so a layout and
/// a state say the levels the same way and a level is always applied to the
/// tower it was read from.
///
/// The two sides do not agree on which position is which tower. A live capture
/// of round 7 of the TUFF replay reads `BuildingData.BuildingType` at each
/// position and finds `[EnergyTower, ResearchCenter]` for blue against
/// `[ResearchCenter, EnergyTower]` for red, on map 1021. `docs/spec/document/state.md`
/// carries the measurement and the reason: a side's buildings are appended in
/// the order its own territory lists them, and the two territories are mirror
/// images, so the order is map data per side rather than a property of the
/// build.
///
/// So no constant names a position and no code may assume one. The adapter
/// checks only that a side holds one tower of each kind, and keys every level
/// by the position it was found at.
pub const TOWER_COUNT: usize = 2;

/// The highest level a tower can be strengthened to.
///
/// `config/economy.yaml` prices levels 1 through 4.
pub const MAX_TOWER_STRENGTHEN_LEVEL: i32 = 4;

/// The Energy Tower skill that widens ranged attack range.
pub const RANGE_ENHANCEMENT_SKILL: i32 = 5;
/// The Energy Tower skill that raises movement speed.
pub const MOVEMENT_ENHANCEMENT_SKILL: i32 = 6;

/// The Energy Tower skills whose effect a fight can see.
///
/// The others buy supply or discount a round's shopping, which a layout does
/// not carry, so a projection drops them rather than recording them here.
pub const FIGHT_VISIBLE_ENERGY_TOWER_SKILLS: [i32; 2] =
    [RANGE_ENHANCEMENT_SKILL, MOVEMENT_ENHANCEMENT_SKILL];

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Layout {
    pub kind: DocumentKind,
    /// The build whose tables this document is written against, which a
    /// document stating nothing inherits from the binary that reads it.
    #[serde(default = "crate::economy::this_build")]
    pub game_build: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1))]
    pub map_id: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<i32>,
    #[schemars(range(min = 1))]
    pub round: i32,
    pub blue: Side,
    pub red: Side,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Side {
    /// The officers the side holds, by name, a multiset in ascending ID. A
    /// chain blueprint's officer is not one: `blueprints` states it.
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        with = "crate::names::officer::many"
    )]
    #[schemars(with = "Vec<String>")]
    pub officers: Vec<i32>,
    /// Unit technologies, written grouped by the unit type they belong to.
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        with = "crate::names::technologies"
    )]
    #[schemars(with = "BTreeMap<String, Vec<String>>")]
    pub techs: Vec<i32>,
    /// The Research Center's enhancement chains the side holds, by name: the
    /// blueprints a fight sees. [`crate::catalog::CHAIN_BLUEPRINTS`] lists them.
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        with = "crate::names::blueprint::many"
    )]
    #[schemars(with = "Vec<String>")]
    pub blueprints: Vec<i32>,
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        with = "crate::names::energy_tower_skill::many"
    )]
    #[schemars(with = "Vec<String>")]
    pub energy_tower_skills: Vec<i32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tower_strengthen_levels: Vec<i32>,
    pub units: Vec<UnitPlacement>,
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

/// What a compiled side holds of officers and technologies, as the adapter and
/// the simulator apply them: every officer, a chain blueprint's among them, and
/// every unit technology.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Techs {
    pub officers: Vec<i32>,
    pub units: Vec<i32>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct UnitPlacement {
    #[serde(rename = "name")]
    pub type_name: String,
    pub index: i32,
    pub position: Position,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exp: Option<Experience>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rotated: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::names::equipment::option"
    )]
    #[schemars(with = "Option<String>")]
    pub equipment: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub travelling: Option<bool>,
}

/// A formation's experience within its level, as a gauge written
/// `current/maximum`, such as `124/450`.
///
/// `maximum` is the level's full bar, which [`crate::experience::full`] reads
/// out of the build's table. It is there to be read: a document shows how far
/// the bar has to go without sending its reader to the table. The bar is full
/// when `current` reaches `maximum`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Experience {
    pub current: i32,
    pub maximum: i32,
}

impl std::fmt::Display for Experience {
    fn fmt(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(formatter, "{}/{}", self.current, self.maximum)
    }
}

impl std::str::FromStr for Experience {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        let malformed = || format!("experience {text:?} is not current/maximum");
        let (current, maximum) = text.split_once('/').ok_or_else(malformed)?;
        Ok(Self {
            current: current.trim().parse().map_err(|_| malformed())?,
            maximum: maximum.trim().parse().map_err(|_| malformed())?,
        })
    }
}

impl Serialize for Experience {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Experience {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

impl JsonSchema for Experience {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Experience".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "description": "Experience within the formation's level, written current/maximum, where maximum is the level's full bar.",
            "type": "string",
            "pattern": "^-?[0-9]+/[0-9]+$"
        })
    }
}

impl Experience {
    /// The gauge of a formation of this type and level holding `current`, or
    /// nothing when it holds none.
    ///
    /// # Errors
    ///
    /// Returns an error when the experience table has no bar for the type and
    /// level.
    pub fn of(current: i32, type_name: &str, level: i32) -> Result<Option<Self>, String> {
        if current == 0 {
            return Ok(None);
        }
        let maximum = crate::experience::full(type_name, level).ok_or_else(|| {
            format!("the experience table has no level {level} bar for {type_name:?}")
        })?;
        Ok(Some(Self { current, maximum }))
    }

    /// Whether the bar is full.
    #[must_use]
    pub fn is_full(self) -> bool {
        self.current >= self.maximum
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StaticPlacement {
    #[serde(rename = "name")]
    pub type_name: String,
    pub index: i32,
    pub position: Position,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ContraptionPlacement {
    #[serde(rename = "name")]
    pub type_name: String,
    pub index: i32,
    pub position: Position,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BattleSkillDefinition {
    #[serde(rename = "name")]
    pub type_name: String,
    pub positions: Vec<Position>,
}

#[derive(
    Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord,
)]

/// A battlefield area a commander skill leaves behind.
///
/// These are the build's own `RangeItemType` names, less `FogSand`, which a
/// unit technology makes rather than a skill and which no side's skill panel
/// therefore retains. One name is one substance, not one skill: a skill is what
/// produced the area and [`crate::catalog::terrain_type_from_skill`] is the
/// mapping, so a build that gave a second skill the same substance would not
/// need a second name here.
///
/// Under build 2259's standard 1v1 rules only `Oil` is ever read back, because
/// only the Sticky Oil Bomb lasts two rounds and every other area is gone
/// before the round that would record it opens. The rest are carried so that a
/// recording holding one is described rather than refused.
#[serde(rename_all = "snake_case")]
pub enum TerrainType {
    Fire,
    Oil,
    Fog,
    Acid,
    RecoveryZone,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Terrain {
    #[serde(rename = "name")]
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

/// The two flank rectangles, in the side's own local frame.
///
/// They share their `y` span and differ only in `x`, and each is a deployment
/// region of its own rather than two halves of one ambush zone. [`Region`] is
/// what reads them.
pub(crate) const AMBUSH_LEFT_MIN_X: i64 = -360;
pub(crate) const AMBUSH_LEFT_MAX_X: i64 = -300;
pub(crate) const AMBUSH_RIGHT_MIN_X: i64 = 300;
pub(crate) const AMBUSH_RIGHT_MAX_X: i64 = 360;
pub(crate) const AMBUSH_MIN_Y: i64 = 10;
pub(crate) const AMBUSH_MAX_Y: i64 = 310;

/// Which of a side's three deployment regions a position lies in.
///
/// The game does not read a coordinate where it decides a flank deployment. It
/// asks its territory which region holds a position, and the main deployment
/// half and the two flank rectangles are three separate regions. That is the
/// distinction [`crate::transition::step`] turns on when it moves a formation, and it is why the
/// two flanks are told apart here rather than lumped together as "ambush":
/// crossing from one flank to the other is a change of region like any other.
///
/// The three regions do not cover the plane. A position outside all of them is
/// refused by [`crate::compile`] before it can reach a plan, so this
/// classification is defined over positions a layout may state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Region {
    /// The side's own deployment half, `x=[-300,300], y=[-310,-10]`.
    Main,
    /// `x=[-360,-300], y=[10,310]`.
    LeftFlank,
    /// `x=[300,360], y=[10,310]`.
    RightFlank,
}

impl Region {
    /// Which region holds a position.
    #[must_use]
    pub fn of(position: Position) -> Self {
        if i64::from(position.y) < AMBUSH_MIN_Y {
            Self::Main
        } else if position.x < 0 {
            Self::LeftFlank
        } else {
            Self::RightFlank
        }
    }

    /// Whether this region is one of the two flanks.
    ///
    /// Arriving in one is what puts a formation in the travelling set; the
    /// main half is where a formation is when it is not. The game reaches the
    /// same answer by asking whether the region is the side's own main one,
    /// and the two agree because a side's territory holds no other region.
    #[must_use]
    pub fn is_flank(self) -> bool {
        !matches!(self, Self::Main)
    }
}

impl Layout {
    /// Rewrites a layout into the one document that denotes its state.
    ///
    /// Two rules make up the normal form. Syntax equivalent to a public default
    /// is dropped, and every collection whose order carries no meaning is put in
    /// its defined order: indexed placements by deployment identity, technology
    /// and Officer IDs and Energy Tower skills and retained airdrop shields
    /// ascending, and retained terrain by type and control points.
    /// `battle_skills` is the one exception, because release order is what it
    /// records, and `tower_strengthen_levels` is not a collection whose order is
    /// free: it is keyed by building-manager position, so an all-zero list is
    /// dropped as a default rather than sorted.
    ///
    /// Applying this twice changes nothing the first pass did not already do.
    #[must_use]
    pub fn normalized(mut self) -> Self {
        for side in [&mut self.blue, &mut self.red] {
            side.officers.sort_unstable();
            side.techs.sort_unstable();
            side.blueprints.sort_unstable();
            side.energy_tower_skills.sort_unstable();
            if side.tower_strengthen_levels.iter().all(|level| *level == 0) {
                side.tower_strengthen_levels.clear();
            }
            side.units.sort_by_key(|formation| formation.index);
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
            for formation in &mut side.units {
                if formation.level == Some(1) {
                    formation.level = None;
                }
                if formation.exp.is_some_and(|exp| exp.current == 0) {
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
    crate::economy::require_this_build(&layout.game_build)?;
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
    for (side_name, side) in [("blue", &layout.blue), ("red", &layout.red)] {
        for formation in &side.units {
            if resolve_unit_type(&formation.type_name).is_none() {
                let destination = if resolve_construction_type(&formation.type_name).is_some() {
                    "constructions"
                } else if resolve_contraption_type(&formation.type_name).is_some() {
                    "contraptions"
                } else {
                    return Err(format!(
                        "side {side_name} unit type {:?} is unknown",
                        formation.type_name
                    ));
                };
                return Err(format!(
                    "side {side_name} unit type {:?} belongs in {destination}",
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
    let value = serde_yaml::to_value(&layout)
        .map_err(|error| format!("cannot serialize layout YAML: {error}"))?;
    crate::spelling::document(&value)
}
