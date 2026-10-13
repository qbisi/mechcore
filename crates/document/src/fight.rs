//! A fight writes its settlement on the layout objects it belongs to.
//!
//! `docs/spec/document/fight.md` defines the embedded result fields, optional
//! trajectory and assertions. [`project`] recovers the starting layout;
//! [`parse_yaml`] validates that layout and each result's bounds.

use crate::layout::{
    BattleSkillEntry, BattleSkillFields, BattleSkillRelease, Experience, Layout, Position,
    SHIELD_AIRDROP_SKILL, STICKY_OIL_BOMB_SKILL, Side, Standing, StaticPlacement, UnitPlacement,
};
use crate::{DocumentKind, compile::compile_layout};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const FIGHT_KIND: &str = "fight";

/// A fight document, as the code holds it: the layout's fields, with each
/// result on the object it is about.
///
/// Settlement is embedded on each object; [`WrittenFight`] pairs the optional
/// trajectory fields while keeping assertions independent.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(try_from = "WrittenFight", into = "WrittenFight")]
pub struct Fight {
    pub kind: FightKind,
    /// The build whose tables this document is written against, which a
    /// document stating nothing inherits from the binary that reads it.
    pub game_build: String,
    pub map_id: Option<i32>,
    /// The match seed the fight was fought with: a result is one seed's.
    pub seed: i32,
    pub round: i32,
    /// Who fought it, and so what it may be checked against.
    pub source: Source,
    /// The fight's ticks and trajectory hash, which a document states
    /// together or not at all.
    pub trajectory: Option<Trajectory>,
    /// What the fixture claims about how the fight went, each a query of its
    /// recording and the rows the game's recording answered.
    pub asserts: Vec<Assert>,
    pub blue: FightSide,
    pub red: FightSide,
}

/// One claim about a fight's recording: a query, as `mechcore query` reads
/// one, and the rows the game's recording of the fight answered it with.
///
/// The rows are the game's, recorded by `verify --backend game --update`,
/// and an assert whose rows nobody recorded states none. Whether the claim
/// is bound to a tick or only to the order of what happened is the query's
/// to say, by what it selects and orders by.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Assert {
    /// The SQL the recording is asked.
    pub sql: String,
    /// What the game's recording answered, row by row, each cell as the
    /// query answers it; absent until it is recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rows: Option<Vec<Vec<serde_json::Value>>>,
}

impl Eq for Assert {}

/// A fight's logical ticks, the recording's `tick_count`, and its trajectory
/// hash.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Trajectory {
    pub ticks: u32,
    pub hash: FightHash,
}

impl JsonSchema for Fight {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Fight".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        WrittenFight::json_schema(generator)
    }
}

/// The embedded fight document and its optional trajectory.
#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct WrittenFight {
    kind: FightKind,
    /// Who fought it, and so what it may be checked against.
    source: Source,
    #[serde(
        default = "crate::economy::this_build",
        skip_serializing_if = "crate::economy::is_this_build"
    )]
    game_build: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1))]
    map_id: Option<i32>,
    /// The match seed the fight was fought with: a result is one seed's.
    seed: i32,
    #[schemars(range(min = 1))]
    round: i32,
    blue: FightSide,
    red: FightSide,
    /// What the fixture claims about how the fight went.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    asserts: Vec<Assert>,
    /// The fight's logical ticks, stated with `hash` or not at all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1))]
    ticks: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    hash: Option<FightHash>,
}

impl TryFrom<WrittenFight> for Fight {
    type Error = String;

    fn try_from(written: WrittenFight) -> Result<Self, String> {
        let trajectory = match (written.ticks, written.hash) {
            (Some(ticks), Some(hash)) => Some(Trajectory { ticks, hash }),
            (None, None) => None,
            _ => return Err("a fight states ticks and hash together, or neither".to_owned()),
        };
        let fight = Self {
            kind: written.kind,
            game_build: written.game_build,
            map_id: written.map_id,
            seed: written.seed,
            round: written.round,
            source: written.source,
            trajectory,
            asserts: written.asserts,
            blue: written.blue,
            red: written.red,
        };
        Ok(fight)
    }
}

impl From<Fight> for WrittenFight {
    fn from(fight: Fight) -> Self {
        let (ticks, hash) = fight.trajectory.map_or((None, None), |trajectory| {
            (Some(trajectory.ticks), Some(trajectory.hash))
        });
        Self {
            kind: fight.kind,
            source: fight.source,
            game_build: fight.game_build,
            map_id: fight.map_id,
            seed: fight.seed,
            round: fight.round,
            blue: fight.blue,
            red: fight.red,
            asserts: fight.asserts,
            ticks,
            hash,
        }
    }
}

/// A fight document names itself `fight`, and nothing else.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FightKind {
    Fight,
}

/// Who fought the fight whose result a document states: the recording's
/// `producer`. The game fights a layout and a native replay's round the same
/// way, and the two agree tick for tick, so both are `game`.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// The game, through the Adapter: evidence of what the game does.
    Game,
    /// The simulator's own run: a valid document, never evidence.
    Simulator,
}

impl Source {
    /// The name the document writes.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Game => "game",
            Self::Simulator => "simulator",
        }
    }
}

/// A trajectory hash and the profile that defines it,
/// `docs/spec/mcfr/mcfr.md`'s `hash_profile` and `result_hash`, written as
/// one string: the profile's number, a colon, and 64 lowercase hex digits
/// (`23:e4ed…`).
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(try_from = "String", into = "String")]
pub struct FightHash {
    pub profile: String,
    /// 64 lowercase hex digits.
    pub result: String,
}

impl TryFrom<String> for FightHash {
    type Error = String;

    fn try_from(written: String) -> Result<Self, String> {
        let (profile, result) = written
            .split_once(':')
            .ok_or_else(|| format!("fight hash {written:?} is not <profile>:<hex>"))?;
        Ok(Self {
            profile: profile.to_owned(),
            result: result.to_owned(),
        })
    }
}

impl From<FightHash> for String {
    fn from(hash: FightHash) -> Self {
        format!("{}:{}", hash.profile, hash.result)
    }
}

impl JsonSchema for FightHash {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "FightHash".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "pattern": "^[0-9]+:[0-9a-f]{64}$",
        })
    }
}

/// One side of a fight: the layout's side, what the fight took off its
/// reactor core, and the result on each of its objects.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FightSide {
    /// Complete settlement, required even when zero.
    #[schemars(range(min = 0))]
    pub core_damage: i32,
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
    pub units: Vec<FightUnit>,
    /// The index of each squad an officer delivered as the round opened that
    /// the side recovered during the deployment, in ascending order: the fight
    /// sees nothing of it, and a layout replay recovers it by this index.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recovered: Vec<i32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub constructions: Vec<StaticPlacement>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contraptions: Vec<FightContraption>,
    /// The side's battle skills: what earlier releases left standing, then
    /// this round's releases in release order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub battle_skills: Vec<FightBattleSkill>,
}

/// A unit and its beginning/final/maximum experience.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FightUnit {
    #[serde(rename = "name")]
    pub type_name: String,
    pub index: i32,
    pub position: Position,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exp: Option<FightExperience>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rotated: Option<bool>,
    /// What the formation wears, in the order it was fitted. Build 2.0 lets a
    /// formation wear more than one when its side's officers add slots.
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        with = "crate::names::equipment::many"
    )]
    #[schemars(with = "Vec<String>")]
    pub equipment: Vec<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub travelling: Option<bool>,
    /// How the unit came to its side; `legacy`, the default, is not written.
    #[serde(default, skip_serializing_if = "crate::layout::UnitSource::is_legacy")]
    pub source: crate::layout::UnitSource,
}

/// A unit's experience across a fight.
///
/// `before` and `maximum` are the layout's `current` and the level's bar;
/// `after` is what the unit ends the fight holding. A full bar takes no further share, so
/// `after` never passes `maximum`, and a fight only adds experience, so it
/// never falls below `before`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FightExperience {
    pub before: i32,
    pub after: i32,
    pub maximum: i32,
}

impl std::fmt::Display for FightExperience {
    fn fmt(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(formatter, "{}/{}/{}", self.before, self.after, self.maximum)
    }
}

impl std::str::FromStr for FightExperience {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        let malformed = || format!("experience {text:?} is not begin/final/max");
        let terms = text
            .split('/')
            .map(|term| term.trim().parse::<i32>().map_err(|_| malformed()))
            .collect::<Result<Vec<_>, _>>()?;
        let [before, after, maximum] = terms[..] else {
            return Err(malformed());
        };
        Ok(Self {
            before,
            after,
            maximum,
        })
    }
}

impl Serialize for FightExperience {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for FightExperience {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

impl JsonSchema for FightExperience {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "FightExperience".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "description": "Experience within the formation's level across the fight, written begin/final/max, where maximum is the level's full bar.",
            "type": "string",
            "pattern": "^-?[0-9]+/-?[0-9]+/[0-9]+$"
        })
    }
}

/// A layout contraption, and whether it still stands when the fight ends.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FightContraption {
    #[serde(rename = "name")]
    pub type_name: String,
    pub index: i32,
    pub position: Position,
    /// Written only as `false`.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub retained: bool,
}

/// One `battle_skills` entry of a fight: the layout's entry and what the
/// fight left of it.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(try_from = "FightBattleSkillFields", into = "FightBattleSkillFields")]
pub enum FightBattleSkill {
    Release(FightRelease),
    Standing(FightStanding),
}

/// A release this round, and what its product is when the fight ends.
///
/// Only a Shield Airdrop's shield and a Sticky Oil Bomb's area can outlive
/// the round, so only those two releases carry a result. `retained` says
/// whether anything is left at all; an oil area that is left says in
/// `grid_rows` which of its seven points survive and how much of each, as a
/// standing area does, and one with no `grid_rows` is whole.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FightRelease {
    pub release: BattleSkillRelease,
    pub retained: bool,
    pub grid_rows: BTreeMap<u32, Vec<u32>>,
}

/// An object an earlier release left standing, and whether it still stands
/// when the fight ends.
///
/// Only a standing shield carries a result. A standing oil area is in its
/// second and last round, so it never carries on and `retained` is always
/// `true` for one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FightStanding {
    pub standing: Standing,
    pub retained: bool,
}

/// How a fight's `battle_skills` entry is written: the layout's fields, and
/// the result fields beside them.
#[derive(Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct FightBattleSkillFields {
    name: String,
    /// This round's release, at these positions in order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    positions: Option<Vec<Position>>,
    /// What an earlier round's release left standing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    standing: Option<Standing>,
    /// What a Sticky Oil Bomb released this round leaves, keyed by
    /// generated-point index.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    grid_rows: BTreeMap<u32, Vec<u32>>,
    /// Whether a shield or oil area is left when the fight ends; written only
    /// as `false`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    retained: Option<bool>,
}

/// The commander skill a battle-skill name stands for, when it names one.
fn skill_of(name: &str) -> Option<i32> {
    crate::catalog::resolve_battle_skill_type(name).map(|skill| skill.commander_skill_id)
}

impl TryFrom<FightBattleSkillFields> for FightBattleSkill {
    type Error = String;

    fn try_from(fields: FightBattleSkillFields) -> Result<Self, String> {
        let FightBattleSkillFields {
            name,
            positions,
            standing,
            grid_rows,
            retained,
        } = fields;
        let entry = BattleSkillEntry::try_from(BattleSkillFields {
            name: name.clone(),
            positions,
            standing,
        })?;
        match entry {
            BattleSkillEntry::Standing(standing) => {
                if !grid_rows.is_empty() {
                    return Err(format!(
                        "battle skill {name}'s standing entry carries grid_rows beside its \
                         standing object: only a sticky_oil_bomb release carries them as a result"
                    ));
                }
                if retained.is_some() && !matches!(standing, Standing::Shield { .. }) {
                    return Err(format!(
                        "battle skill {name}'s standing area carries no result: oil lasts two \
                         rounds and a standing area is in its second"
                    ));
                }
                Ok(Self::Standing(FightStanding {
                    standing,
                    retained: retained.unwrap_or(true),
                }))
            }
            BattleSkillEntry::Release(release) => {
                let skill = skill_of(&name);
                let leaves = matches!(skill, Some(SHIELD_AIRDROP_SKILL | STICKY_OIL_BOMB_SKILL));
                if retained.is_some() && !leaves {
                    return Err(format!(
                        "battle skill {name}'s release leaves nothing that outlives the round, \
                         so it carries no retained"
                    ));
                }
                if !grid_rows.is_empty() && skill != Some(STICKY_OIL_BOMB_SKILL) {
                    return Err(format!(
                        "battle skill {name}'s release carries grid_rows: only a sticky_oil_bomb \
                         release leaves an area"
                    ));
                }
                if !grid_rows.is_empty() && retained == Some(false) {
                    return Err(format!(
                        "battle skill {name}'s release states both grid_rows and retained: false"
                    ));
                }
                Ok(Self::Release(FightRelease {
                    release,
                    retained: retained.unwrap_or(true),
                    grid_rows,
                }))
            }
        }
    }
}

impl From<FightBattleSkill> for FightBattleSkillFields {
    fn from(entry: FightBattleSkill) -> Self {
        let (layout, grid_rows, retained) = match entry {
            FightBattleSkill::Release(release) => (
                BattleSkillEntry::Release(release.release),
                release.grid_rows,
                release.retained,
            ),
            FightBattleSkill::Standing(standing) => (
                BattleSkillEntry::Standing(standing.standing),
                BTreeMap::new(),
                standing.retained,
            ),
        };
        let BattleSkillFields {
            name,
            positions,
            standing,
        } = layout.into();
        Self {
            name,
            positions,
            standing,
            grid_rows,
            retained: (!retained).then_some(false),
        }
    }
}

impl FightBattleSkill {
    /// The layout entry, without the result.
    #[must_use]
    pub fn entry(&self) -> BattleSkillEntry {
        match self {
            Self::Release(release) => BattleSkillEntry::Release(release.release.clone()),
            Self::Standing(standing) => BattleSkillEntry::Standing(standing.standing.clone()),
        }
    }
}

const fn yes() -> bool {
    true
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_true(value: &bool) -> bool {
    *value
}

/// The layout a fight starts from: embedded results, assertions and trajectory
/// dropped, and experience reduced to its beginning/maximum gauge.
///
/// Every layout field is copied unchanged, `seed` included, and a unit's
/// experience is the one it starts with, nothing when that is `0`. The
/// projection of a fight in normal form is a layout in normal form.
#[must_use]
pub fn project(fight: &Fight) -> Layout {
    Layout {
        kind: DocumentKind::Layout,
        game_build: fight.game_build.clone(),
        map_id: fight.map_id,
        seed: Some(fight.seed),
        round: fight.round,
        blue: project_side(&fight.blue),
        red: project_side(&fight.red),
    }
}

fn project_side(side: &FightSide) -> Side {
    Side {
        officers: side.officers.clone(),
        techs: side.techs.clone(),
        blueprints: side.blueprints.clone(),
        energy_tower_skills: side.energy_tower_skills.clone(),
        tower_strengthen_levels: side.tower_strengthen_levels.clone(),
        recovered: side.recovered.clone(),
        units: side
            .units
            .iter()
            .map(|unit| UnitPlacement {
                type_name: unit.type_name.clone(),
                index: unit.index,
                position: unit.position,
                level: unit.level,
                exp: unit
                    .exp
                    .filter(|exp| exp.before != 0)
                    .map(|exp| Experience {
                        current: exp.before,
                        maximum: exp.maximum,
                    }),
                rotated: unit.rotated,
                equipment: unit.equipment.clone(),
                travelling: unit.travelling,
                source: unit.source,
            })
            .collect(),
        constructions: side.constructions.clone(),
        contraptions: side
            .contraptions
            .iter()
            .map(|contraption| crate::layout::ContraptionPlacement {
                type_name: contraption.type_name.clone(),
                index: contraption.index,
                position: contraption.position,
            })
            .collect(),
        battle_skills: side
            .battle_skills
            .iter()
            .map(FightBattleSkill::entry)
            .collect(),
    }
}

impl Fight {
    /// Rewrites a fight into the one document that denotes it.
    ///
    /// The layout part takes the layout's normal form, collection orders and
    /// dropped defaults alike, and each result field follows the object it
    /// sits on. A unit's `exp` is dropped when both `before` and `after` are
    /// `0`, and a release's `grid_rows` that says every point survives whole
    /// is dropped as an absent one says the same. `retained: true` is not
    /// written; core damage is always explicit, including zero.
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
            let (mut standing, releases): (Vec<_>, Vec<_>) =
                std::mem::take(&mut side.battle_skills)
                    .into_iter()
                    .partition(|entry| matches!(entry, FightBattleSkill::Standing(_)));
            standing.sort_by_cached_key(|entry| match entry {
                FightBattleSkill::Standing(standing) => standing.standing.sort_key(),
                FightBattleSkill::Release(_) => unreachable!("partitioned out"),
            });
            side.battle_skills = standing.into_iter().chain(releases).collect();
            for formation in &mut side.units {
                if formation.level == Some(1) {
                    formation.level = None;
                }
                if formation
                    .exp
                    .is_some_and(|exp| exp.before == 0 && exp.after == 0)
                {
                    formation.exp = None;
                }
                if formation.rotated == Some(false) {
                    formation.rotated = None;
                }
                if formation.travelling == Some(false) {
                    formation.travelling = None;
                }
            }
            for entry in &mut side.battle_skills {
                match entry {
                    FightBattleSkill::Standing(FightStanding {
                        standing: Standing::Oil(area),
                        ..
                    }) => area.normalize_grid_rows(),
                    FightBattleSkill::Release(release) => {
                        crate::layout::normalize_grid_rows(&mut release.grid_rows);
                    }
                    FightBattleSkill::Standing(_) => {}
                }
            }
        }
        self
    }
}

/// Parses and validates one fight YAML document.
///
/// The document has to name itself `fight`, be written against this build,
/// project onto a layout that compiles, and hold a result that fits its
/// layout: see [`validate`].
///
/// # Errors
///
/// Returns an error when the YAML, its projection or its result is invalid.
pub fn parse_yaml(bytes: &[u8]) -> Result<Fight, String> {
    if let Ok(header) = serde_yaml::from_slice::<Header>(bytes) {
        require_fight_kind(header.kind.as_deref())?;
    }
    let fight: Fight =
        serde_yaml::from_slice(bytes).map_err(|error| format!("invalid fight YAML: {error}"))?;
    validate(&fight)?;
    Ok(fight)
}

/// Checks a fight: its build, its projection, and its result.
///
/// # Errors
///
/// Names the first rule the fight breaks.
pub fn validate(fight: &Fight) -> Result<(), String> {
    crate::economy::require_this_build(&fight.game_build)?;
    compile_layout(project(fight))?;
    validate_trajectory(fight)?;
    for (side_name, side) in [("blue", &fight.blue), ("red", &fight.red)] {
        validate_side(side_name, side)?;
    }
    Ok(())
}

/// Serializes a fight in normal form.
///
/// # Errors
///
/// Returns an error when the fight is invalid or cannot be serialized.
pub fn canonical_yaml(fight: Fight) -> Result<String, String> {
    validate(&fight)?;
    let value = serde_yaml::to_value(fight.normalized())
        .map_err(|error| format!("cannot serialize fight YAML: {error}"))?;
    crate::spelling::fight(&value)
}

#[derive(Deserialize)]
struct Header {
    #[serde(default)]
    kind: Option<String>,
}

fn require_fight_kind(kind: Option<&str>) -> Result<(), String> {
    match kind {
        Some(FIGHT_KIND) => Ok(()),
        Some(other) => Err(format!(
            "expected a {FIGHT_KIND} document, found kind {other:?}"
        )),
        None => Err(format!(
            "document does not name its kind: a fight starts with `kind: {FIGHT_KIND}`"
        )),
    }
}

/// A fight's `ticks` and `hash` in form.
///
/// The profile is checked for form only: the profile the MCFR crate
/// computes lives in a crate that reads this one, not one this crate reads.
fn validate_trajectory(fight: &Fight) -> Result<(), String> {
    let Some(Trajectory { ticks, hash }) = &fight.trajectory else {
        return Ok(());
    };
    if *ticks == 0 {
        return Err("fight ticks must be at least 1".to_owned());
    }
    if hash.profile.is_empty() || !hash.profile.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(format!(
            "fight hash profile {:?} is not a profile number",
            hash.profile
        ));
    }
    if hash.result.len() != 64
        || !hash
            .result
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(format!(
            "fight hash result {:?} is not 64 lowercase hex digits",
            hash.result
        ));
    }
    Ok(())
}

fn validate_side(side_name: &str, side: &FightSide) -> Result<(), String> {
    if side.core_damage < 0 {
        return Err(format!("side {side_name} core_damage must be non-negative"));
    }
    for unit in &side.units {
        let Some(exp) = unit.exp else { continue };
        let at = format!(
            "side {side_name} unit {:?} index {}",
            unit.type_name, unit.index
        );
        let level = unit.level.unwrap_or(1);
        let full = crate::experience::full(&unit.type_name, level);
        if full != Some(exp.maximum) {
            return Err(format!(
                "{at} exp maximum {} is not its level {level} bar {full:?}",
                exp.maximum
            ));
        }
        if exp.before < 0 {
            return Err(format!("{at} exp before must be non-negative"));
        }
        if exp.after < exp.before {
            return Err(format!(
                "{at} exp after {} is below before {}: a fight only adds experience",
                exp.after, exp.before
            ));
        }
        if exp.after > exp.maximum {
            return Err(format!(
                "{at} exp after {} passes its bar {}: a full bar takes no further share",
                exp.after, exp.maximum
            ));
        }
    }
    for (entry_index, entry) in side.battle_skills.iter().enumerate() {
        FightBattleSkill::try_from(FightBattleSkillFields::from(entry.clone()))?;
        if let FightBattleSkill::Release(release) = entry {
            crate::compile::validate_grid_rows(
                &format!(
                    "side {side_name} battle_skills[{entry_index}] {}",
                    release.release.type_name
                ),
                &release.grid_rows,
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = r"kind: fight
source: game
seed: 4242
round: 3
blue:
  core_damage: 0
  officers: [extended_range_marksman]
  units:
  - {name: marksman, index: 0, position: {x: 0, y: -50}, exp: 12/170/650}
  contraptions:
  - {name: interceptor, index: 0, position: {x: 5, y: -95}, retained: false}
  battle_skills:
  - {name: shield_airdrop, standing: {position: {x: -150, y: -150}}, retained: false}
red:
  core_damage: 37
  units:
  - {name: arclight, index: 0, position: {x: 0, y: -100}, exp: 0/40/750}
  battle_skills:
  - {name: sticky_oil_bomb, positions: [{x: -30, y: 150}, {x: 60, y: 150}], grid_rows: {2: [], 3: [], 4: []}}
ticks: 870
hash: 23:380d721bf2aa581622f521e4386160a0b5eedfb16ffed7b477b7e288c31534ef
";

    fn example() -> Fight {
        parse_yaml(EXAMPLE.as_bytes()).unwrap()
    }
    fn edited(from: &str, to: &str) -> Result<Fight, String> {
        assert!(EXAMPLE.contains(from), "{from}");
        parse_yaml(EXAMPLE.replacen(from, to, 1).as_bytes())
    }

    #[test]
    fn the_worked_example_is_in_normal_form() {
        let fight = example();
        assert_eq!(fight.source, Source::Game);
        assert_eq!(fight.blue.core_damage, 0);
        assert_eq!(fight.red.core_damage, 37);
        assert_eq!(
            fight.blue.units[0].exp,
            Some(FightExperience {
                before: 12,
                after: 170,
                maximum: 650
            })
        );
        assert_eq!(
            fight.red.units[0].exp,
            Some(FightExperience {
                before: 0,
                after: 40,
                maximum: 750
            })
        );
        assert!(!fight.blue.contraptions[0].retained);
        assert_eq!(canonical_yaml(fight.clone()).unwrap(), EXAMPLE);
        assert_eq!(
            parse_yaml(canonical_yaml(fight.clone()).unwrap().as_bytes()).unwrap(),
            fight
        );
    }

    #[test]
    fn projection_keeps_only_the_starting_layout() {
        let projected = project(&example());
        assert_eq!(
            projected.blue.units[0].exp,
            Some(Experience {
                current: 12,
                maximum: 650
            })
        );
        let text = crate::layout::canonical_yaml(projected).unwrap();
        assert!(!text.contains("outcome"));
        assert!(!text.contains("retained"));
        assert!(!text.contains("170/650"));
        assert!(crate::layout::parse_yaml(text.as_bytes()).is_ok());
    }

    #[test]
    fn results_use_native_indices_including_gaps() {
        let text = EXAMPLE
            .replacen("name: marksman, index: 0", "name: marksman, index: 7", 1)
            .replacen(
                "name: interceptor, index: 0",
                "name: interceptor, index: 9",
                1,
            );
        let fight = parse_yaml(text.as_bytes()).unwrap();
        assert_eq!(fight.blue.units[0].index, 7);
        assert_eq!(fight.blue.units[0].exp.unwrap().after, 170);
        assert_eq!(fight.blue.contraptions[0].index, 9);
        assert!(!fight.blue.contraptions[0].retained);
        assert_eq!(canonical_yaml(fight).unwrap(), text);
    }

    #[test]
    fn settlement_requires_both_core_damages_and_accepts_zero() {
        for text in [
            EXAMPLE.replace("  core_damage: 0\n", ""),
            EXAMPLE.replace("  core_damage: 37\n", ""),
        ] {
            assert!(
                parse_yaml(text.as_bytes())
                    .unwrap_err()
                    .contains("core_damage")
            );
        }
        assert!(edited("core_damage: 37", "core_damage: 0").is_ok());
        assert!(
            edited("core_damage: 37", "core_damage: -1")
                .unwrap_err()
                .contains("non-negative")
        );
        assert!(
            parse_yaml(
                format!(
                    "{EXAMPLE}outcome: {{blue: {{core_damage: 0}}, red: {{core_damage: 37}}}}\n"
                )
                .as_bytes()
            )
            .is_err()
        );
    }

    #[test]
    fn experience_states_begin_final_and_the_level_bar() {
        for (to, message) in [
            ("12/651/650", "full bar"),
            ("12/170/651", "not its level"),
            ("12/11/650", "only adds"),
            ("12/650", "begin/final/max"),
            ("12/1.5/650", "begin/final/max"),
        ] {
            let error = edited("exp: 12/170/650", &format!("exp: {to}")).unwrap_err();
            assert!(error.contains(message), "{error}");
        }
        for exp in ["12/12/650", "12/650/650", "0/170/650"] {
            assert!(edited("exp: 12/170/650", &format!("exp: {exp}")).is_ok());
        }
    }

    #[test]
    fn results_stand_only_on_objects_that_can_carry_them() {
        assert!(edited("exp: 12/170/650", "life: 170").is_err());
        assert!(edited("retained: false", "grid_rows: {2: []}").is_err());
        assert!(
            edited(
                "grid_rows: {2: [], 3: [], 4: []}",
                "grid_rows: {2: []}, retained: false"
            )
            .unwrap_err()
            .contains("both grid_rows and retained")
        );
        assert!(
            edited("grid_rows: {2: [], 3: [], 4: []}", "grid_rows: {7: []}")
                .unwrap_err()
                .contains("point index 7")
        );
        assert!(edited("grid_rows: {2: [], 3: [], 4: []}", "retained: false").is_ok());
        assert!(
            edited(
                "name: sticky_oil_bomb, positions:",
                "name: shield_airdrop, positions:"
            )
            .unwrap_err()
            .contains("only a sticky_oil_bomb")
        );
        assert!(edited("name: shield_airdrop, standing: {position: {x: -150, y: -150}}, retained: false", "name: sticky_oil_bomb, standing: {control_points: [{x: -150, y: -150}]}, retained: false").is_err());
    }

    #[test]
    fn normalization_keeps_unchanged_nonzero_experience_and_drops_defaults() {
        let mut fight = example();
        fight.blue.units[0].exp.as_mut().unwrap().after = 12;
        fight.red.units[0].exp = Some(FightExperience {
            before: 0,
            after: 0,
            maximum: 750,
        });
        fight.blue.contraptions[0].retained = true;
        let written = canonical_yaml(fight.clone()).unwrap();
        assert!(written.contains("exp: 12/12/650"));
        assert!(!written.contains("exp: 0/0/750"));
        assert!(written.contains("core_damage: 0"));
        assert_eq!(parse_yaml(written.as_bytes()).unwrap(), fight.normalized());
    }

    #[test]
    fn seed_kind_projection_and_trajectory_are_validated() {
        assert!(edited("seed: 4242\n", "").unwrap_err().contains("seed"));
        assert!(edited("seed: 4242", "seed: 0").is_err());
        assert!(
            edited("kind: fight", "kind: layout")
                .unwrap_err()
                .contains("expected a fight")
        );
        assert!(edited("source: game", "source: replay").is_err());
        assert!(edited("source: game", "source: simulator").is_ok());
        assert!(edited("ticks: 870\n", "").unwrap_err().contains("together"));
        assert!(
            edited("ticks: 870", "ticks: 0")
                .unwrap_err()
                .contains("at least 1")
        );
        assert!(
            edited("23:380d", "23:380D")
                .unwrap_err()
                .contains("64 lowercase")
        );
        assert!(
            edited("hash: 23:", "hash: mcfr-23:")
                .unwrap_err()
                .contains("profile number")
        );
        assert!(
            edited(
                "exp: 12/170/650}\n",
                "exp: 12/170/650}\n  - {name: marksman, index: 1, position: {x: 0, y: -50}}\n"
            )
            .unwrap_err()
            .contains("collide")
        );
    }

    #[test]
    fn assertions_are_literal_sql_and_one_answer_row_per_line() {
        let mut fight = example();
        fight.asserts.push(Assert {
            sql: "SELECT tick\nFROM events\nWHERE type = 'damage'\nORDER BY tick;\n".into(),
            rows: Some(vec![vec![63.into()], vec![64.into()]]),
        });
        let written = canonical_yaml(fight.clone()).unwrap();
        assert!(written.contains("- sql: |\n    SELECT tick\n    FROM events\n"));
        assert!(written.contains("  rows:\n  - [63]\n  - [64]\n"));
        assert_eq!(parse_yaml(written.as_bytes()).unwrap(), fight);
        assert_eq!(
            canonical_yaml(parse_yaml(written.as_bytes()).unwrap()).unwrap(),
            written
        );
    }

    #[test]
    fn assertion_spelling_preserves_sql_and_empty_or_unrecorded_rows() {
        for sql in [
            "SELECT 1",
            "SELECT 1\n",
            "SELECT 1\n\n",
            "",
            "\n",
            "\n\n",
            "  SELECT 1\n",
            "SELECT 1\r\n",
        ] {
            for rows in [None, Some(vec![])] {
                let mut fight = example();
                fight.asserts.push(Assert {
                    sql: sql.into(),
                    rows,
                });
                let written = canonical_yaml(fight.clone()).unwrap();
                assert_eq!(parse_yaml(written.as_bytes()).unwrap(), fight, "{written}");
            }
        }
    }

    #[test]
    fn schema_requires_embedded_core_damage_and_three_term_experience() {
        let schema = serde_json::to_value(schemars::schema_for!(Fight)).unwrap();
        let required = schema["required"].as_array().unwrap();
        assert!(!required.contains(&serde_json::json!("outcome")));
        assert!(required.contains(&serde_json::json!("seed")));
        assert!(!required.contains(&serde_json::json!("ticks")));
        assert!(
            schema["$defs"]["FightSide"]["required"]
                .as_array()
                .unwrap()
                .contains(&serde_json::json!("core_damage"))
        );
        assert!(
            schema["$defs"]["FightExperience"]["pattern"]
                .as_str()
                .unwrap()
                .contains('/')
        );
    }
}
