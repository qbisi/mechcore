//! The fight document: one fight and its result, written onto the layout it
//! starts from.
//!
//! `docs/spec/document/fight.md` defines it. A fight is a layout with the
//! result written in, so [`project`] drops the result and gives back the
//! layout, and every layout rule reaches a fight through that projection:
//! [`parse_yaml`] refuses a fight whose projection does not compile. What
//! this module adds is the result itself, where it may stand, and the
//! bounds it keeps.

use crate::layout::{
    BattleSkillEntry, BattleSkillFields, BattleSkillRelease, Experience, Layout, Position,
    SHIELD_AIRDROP_SKILL, STICKY_OIL_BOMB_SKILL, Side, Standing, StaticPlacement, UnitPlacement,
};
use crate::{DocumentKind, compile::compile_layout};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const FIGHT_KIND: &str = "fight";

/// A fight document.
///
/// The fields up to `round` and the sides' layout fields are the layout's,
/// with `seed` required; `source`, `ticks` and `hash` at the root, and the
/// result fields on the sides and their objects, are the fight's.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Fight {
    pub kind: FightKind,
    /// The build whose tables this document is written against, which a
    /// document stating nothing inherits from the binary that reads it.
    #[serde(default = "crate::economy::this_build")]
    pub game_build: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1))]
    pub map_id: Option<i32>,
    /// The match seed the fight was fought with: a result is one seed's.
    pub seed: i32,
    #[schemars(range(min = 1))]
    pub round: i32,
    /// Where the result was read, and so what it may be checked against.
    pub source: Source,
    /// The fight's logical ticks; stated exactly when `source` is not
    /// `replay`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1))]
    pub ticks: Option<u32>,
    /// The trajectory hash; stated exactly when `source` is not `replay`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hash: Option<FightHash>,
    pub blue: FightSide,
    pub red: FightSide,
}

/// A fight document names itself `fight`, and nothing else.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FightKind {
    Fight,
}

/// Where a fight's result was read.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// A recording the game made of this layout and seed.
    Recording,
    /// The next round of a native replay, which states what the fight left
    /// but not how it went.
    Replay,
    /// The simulator's own run: a valid document, never evidence.
    Simulator,
}

impl Source {
    /// The name the document writes.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Recording => "recording",
            Self::Replay => "replay",
            Self::Simulator => "simulator",
        }
    }

    /// Whether a result from this source carries a trajectory: `ticks` and
    /// `hash`.
    #[must_use]
    pub const fn has_trajectory(self) -> bool {
        !matches!(self, Self::Replay)
    }
}

/// A trajectory hash and the profile that defines it,
/// `docs/spec/mcfr/mcfr.md`'s `hash_profile` and `result_hash`.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FightHash {
    pub profile: String,
    /// 64 lowercase hex digits.
    #[schemars(regex(pattern = r"^[0-9a-f]{64}$"))]
    pub result: String,
}

/// One side of a fight: the layout's side, what the fight took off its
/// reactor core, and the result on each of its objects.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FightSide {
    /// What the fight took off the side's reactor core; absent when `0`.
    #[serde(default, skip_serializing_if = "is_zero")]
    #[schemars(range(min = 0))]
    pub core_damage: i32,
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        with = "crate::names::officer::many"
    )]
    #[schemars(with = "Vec<String>")]
    pub officers: Vec<i32>,
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        with = "crate::names::technologies"
    )]
    #[schemars(with = "BTreeMap<String, Vec<String>>")]
    pub techs: Vec<i32>,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub constructions: Vec<StaticPlacement>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contraptions: Vec<FightContraption>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub battle_skills: Vec<FightBattleSkill>,
}

/// A layout unit whose `exp` also says what the fight ended it on.
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
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        with = "crate::names::equipment::many"
    )]
    #[schemars(with = "Vec<String>")]
    pub equipment: Vec<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub travelling: Option<bool>,
}

/// A unit's experience gauge across a fight, written
/// `before/after/maximum`, such as `12/170/650`.
///
/// `before/maximum` is the layout's `current/maximum`; `after` is what the
/// unit ends the fight holding. A full bar takes no further share, so
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
        let malformed = || format!("experience {text:?} is not before/after/maximum");
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
            "description": "Experience within the formation's level across the fight, written before/after/maximum, where maximum is the level's full bar.",
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

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_zero(value: &i32) -> bool {
    *value == 0
}

const fn yes() -> bool {
    true
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_true(value: &bool) -> bool {
    *value
}

/// The layout a fight starts from: the fight with its result dropped.
///
/// Every layout field is copied unchanged, `seed` included, and a unit's
/// `exp` keeps its first term: `before/after/maximum` becomes
/// `before/maximum`, and nothing when `before` is `0`. The projection of a
/// fight in normal form is a layout in normal form.
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
    /// is dropped as an absent one says the same. `core_damage: 0` and
    /// `retained: true` are defaults the writer never writes.
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
    crate::spelling::document(&value)
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

/// `ticks` and `hash` are stated exactly when the source has a trajectory.
///
/// The profile is checked for form only: the profile the MCFR crate
/// computes lives in a crate that reads this one, not one this crate reads.
fn validate_trajectory(fight: &Fight) -> Result<(), String> {
    let source = fight.source.as_str();
    match (fight.source.has_trajectory(), fight.ticks, &fight.hash) {
        (false, None, None) => Ok(()),
        (false, _, _) => Err(format!(
            "a fight read from a {source} states neither ticks nor hash: a native replay \
             records what the fight left, not how it went"
        )),
        (true, Some(ticks), Some(hash)) => {
            if ticks == 0 {
                return Err("fight ticks must be at least 1".to_owned());
            }
            if hash.profile.is_empty()
                || !hash.profile.bytes().all(|byte| {
                    byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"-.".contains(&byte)
                })
            {
                return Err(format!(
                    "fight hash profile {:?} is not a profile name",
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
        (true, _, _) => Err(format!(
            "a fight from a {source} states both ticks and hash"
        )),
    }
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

    /// The example `docs/spec/document/fight.md` opens with, as the
    /// canonical writer spells it.
    const EXAMPLE: &str = "\
kind: fight
seed: 4242
round: 3
source: recording
ticks: 870
hash: {profile: mcfr-content-0.7.0, result: 380d721bf2aa581622f521e4386160a0b5eedfb16ffed7b477b7e288c31534ef}
blue:
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
";

    /// The layout the example starts from.
    const EXAMPLE_LAYOUT: &str = "\
kind: layout
seed: 4242
round: 3
blue:
  officers: [extended_range_marksman]
  units:
  - {name: marksman, index: 0, position: {x: 0, y: -50}, exp: 12/650}
  contraptions:
  - {name: interceptor, index: 0, position: {x: 5, y: -95}}
  battle_skills:
  - {name: shield_airdrop, standing: {position: {x: -150, y: -150}}}
red:
  units:
  - {name: arclight, index: 0, position: {x: 0, y: -100}}
  battle_skills:
  - {name: sticky_oil_bomb, positions: [{x: -30, y: 150}, {x: 60, y: 150}]}
";

    /// The example with its build stated, which the writer always writes.
    fn with_build(text: &str, kind: &str) -> String {
        text.replacen(
            &format!("kind: {kind}\n"),
            &format!(
                "kind: {kind}\ngame_build: {}\n",
                crate::economy::game_build()
            ),
            1,
        )
    }

    fn example() -> Fight {
        parse_yaml(EXAMPLE.as_bytes()).unwrap()
    }

    /// The example with one line replaced.
    fn edited(from: &str, to: &str) -> Result<Fight, String> {
        assert!(EXAMPLE.contains(from), "{from}");
        parse_yaml(EXAMPLE.replacen(from, to, 1).as_bytes())
    }

    #[test]
    fn the_worked_example_is_in_normal_form() {
        let fight = example();
        assert_eq!(fight.source, Source::Recording);
        assert_eq!(fight.red.core_damage, 37);
        assert_eq!(
            fight.blue.units[0].exp,
            Some(FightExperience {
                before: 12,
                after: 170,
                maximum: 650
            })
        );
        assert!(!fight.blue.contraptions[0].retained);
        let FightBattleSkill::Release(oil) = &fight.red.battle_skills[0] else {
            panic!("a release")
        };
        assert!(oil.retained);
        assert_eq!(oil.grid_rows.keys().copied().collect::<Vec<_>>(), [2, 3, 4]);
        assert_eq!(canonical_yaml(fight).unwrap(), with_build(EXAMPLE, "fight"));
    }

    #[test]
    fn a_fight_round_trips_through_its_normal_form() {
        // Standing entries out of order, and every default stated.
        let loose = "\
kind: fight
seed: 4242
round: 3
source: replay
blue:
  core_damage: 0
  units:
  - {name: marksman, index: 0, position: {x: 0, y: -50}, exp: 12/170/650}
  - {name: arclight, index: 4, position: {x: 60, y: -50}, level: 1, exp: 0/0/750, rotated: false}
  contraptions:
  - {name: interceptor, index: 2, position: {x: 5, y: -95}, retained: true}
  battle_skills:
  - {name: shield_airdrop, positions: [{x: 0, y: -150}], retained: true}
  - {name: shield_airdrop, standing: {position: {x: 150, y: -150}}}
  - {name: sticky_oil_bomb, positions: [{x: -30, y: 150}, {x: 60, y: 150}], grid_rows: {0: [], 1: [], 2: [], 3: [], 4: [], 5: [], 6: []}}
  - {name: shield_airdrop, standing: {position: {x: -150, y: -150}}, retained: false}
red:
  units:
  - {name: arclight, index: 0, position: {x: 0, y: -100}}
";
        let fight = parse_yaml(loose.as_bytes()).unwrap();
        let written = canonical_yaml(fight.clone()).unwrap();
        assert_eq!(
            written,
            with_build(
                "\
kind: fight
seed: 4242
round: 3
source: replay
blue:
  units:
  - {name: marksman, index: 0, position: {x: 0, y: -50}, exp: 12/170/650}
  - {name: arclight, index: 4, position: {x: 60, y: -50}}
  contraptions:
  - {name: interceptor, index: 2, position: {x: 5, y: -95}}
  battle_skills:
  - {name: shield_airdrop, standing: {position: {x: -150, y: -150}}, retained: false}
  - {name: shield_airdrop, standing: {position: {x: 150, y: -150}}}
  - {name: shield_airdrop, positions: [{x: 0, y: -150}]}
  - {name: sticky_oil_bomb, positions: [{x: -30, y: 150}, {x: 60, y: 150}]}
red:
  units:
  - {name: arclight, index: 0, position: {x: 0, y: -100}}
",
                "fight"
            )
        );
        let again = parse_yaml(written.as_bytes()).unwrap();
        assert_eq!(again, fight.clone().normalized());
        assert_eq!(canonical_yaml(again).unwrap(), written);
    }

    #[test]
    fn the_projection_is_the_layout_the_fight_starts_from() {
        let layout = crate::layout::parse_yaml(EXAMPLE_LAYOUT.as_bytes()).unwrap();
        assert_eq!(project(&example()), layout);
        assert_eq!(
            crate::layout::canonical_yaml(project(&example())).unwrap(),
            with_build(EXAMPLE_LAYOUT, "layout")
        );
    }

    #[test]
    fn a_layout_carrying_a_result_is_refused() {
        for (from, to) in [
            ("round: 3\n", "round: 3\nsource: recording\n"),
            ("red:\n", "red:\n  core_damage: 37\n"),
            ("exp: 12/650", "exp: 12/170/650"),
            (
                "position: {x: 5, y: -95}}",
                "position: {x: 5, y: -95}, retained: false}",
            ),
            ("{x: 60, y: 150}]}", "{x: 60, y: 150}], grid_rows: {2: []}}"),
        ] {
            assert!(EXAMPLE_LAYOUT.contains(from), "{from}");
            let text = EXAMPLE_LAYOUT.replacen(from, to, 1);
            assert!(
                crate::layout::parse_yaml(text.as_bytes()).is_err(),
                "a layout accepted {to:?}"
            );
        }
    }

    #[test]
    fn a_document_of_another_kind_is_refused_as_that_kind() {
        assert_eq!(
            parse_yaml(EXAMPLE_LAYOUT.as_bytes()).unwrap_err(),
            "expected a fight document, found kind \"layout\""
        );
        assert_eq!(
            edited("kind: fight\n", "").unwrap_err(),
            "document does not name its kind: a fight starts with `kind: fight`"
        );
        assert_eq!(
            crate::layout::parse_yaml(EXAMPLE.as_bytes()).unwrap_err(),
            "expected a layout document, found kind \"fight\""
        );
    }

    #[test]
    fn a_fight_names_its_seed() {
        let error = edited("seed: 4242\n", "").unwrap_err();
        assert!(error.contains("missing field `seed`"), "{error}");
        let error = edited("seed: 4242\n", "seed: 0\n").unwrap_err();
        assert!(error.contains("seed 0"), "{error}");
    }

    #[test]
    fn a_fight_whose_projection_does_not_compile_is_refused() {
        // Two blue units on one square.
        let error = edited(
            "exp: 12/170/650}\n",
            "exp: 12/170/650}\n  - {name: marksman, index: 1, position: {x: 0, y: -50}}\n",
        )
        .unwrap_err();
        assert!(error.contains("collide"), "{error}");
    }

    #[test]
    fn the_trajectory_follows_the_source() {
        let error = edited("source: recording", "source: replay").unwrap_err();
        assert!(error.contains("neither ticks nor hash"), "{error}");
        let hash_line = EXAMPLE
            .lines()
            .find(|line| line.starts_with("hash:"))
            .unwrap();
        let replay = EXAMPLE
            .replacen("source: recording", "source: replay", 1)
            .replacen("ticks: 870\n", "", 1)
            .replacen(&format!("{hash_line}\n"), "", 1);
        assert!(parse_yaml(replay.as_bytes()).is_ok());
        let error = edited("ticks: 870\n", "").unwrap_err();
        assert!(error.contains("both ticks and hash"), "{error}");
        assert!(edited("source: recording", "source: simulator").is_ok());
        let error = edited("result: 380d", "result: 380D").unwrap_err();
        assert!(error.contains("64 lowercase hex"), "{error}");
        let error = edited("ticks: 870", "ticks: 0").unwrap_err();
        assert!(error.contains("at least 1"), "{error}");
    }

    #[test]
    fn experience_keeps_to_its_bar() {
        let error = edited("12/170/650", "12/651/650").unwrap_err();
        assert!(
            error.contains("a full bar takes no further share"),
            "{error}"
        );
        assert!(edited("12/170/650", "12/650/650").is_ok());
        let error = edited("12/170/650", "170/12/650").unwrap_err();
        assert!(error.contains("only adds experience"), "{error}");
        // The bar is the table's even where the layout keeps no gauge.
        let error = edited("0/40/750", "0/40/450").unwrap_err();
        assert!(error.contains("not its level 1 bar"), "{error}");
        let error = edited("12/170/650", "12/650").unwrap_err();
        assert!(error.contains("before/after/maximum"), "{error}");
    }

    #[test]
    fn a_result_stands_only_where_something_can_be_left() {
        // A standing oil area is in its last round.
        let error = edited(
            "  - {name: shield_airdrop, standing: {position: {x: -150, y: -150}}, retained: false}\n",
            "  - {name: sticky_oil_bomb, standing: {control_points: [{x: -24, y: 11}, {x: 80, y: 1}]}, retained: false}\n",
        )
        .unwrap_err();
        assert!(error.contains("standing area carries no result"), "{error}");
        // A missile strike leaves nothing.
        let error = edited(
            "  - {name: shield_airdrop, standing: {position: {x: -150, y: -150}}, retained: false}\n",
            "  - {name: missile_strike, positions: [{x: 0, y: 150}], retained: false}\n",
        )
        .unwrap_err();
        assert!(error.contains("leaves nothing"), "{error}");
        // A shield leaves no area.
        let error = edited(
            "  - {name: shield_airdrop, standing: {position: {x: -150, y: -150}}, retained: false}\n",
            "  - {name: shield_airdrop, positions: [{x: 0, y: -150}], grid_rows: {2: []}}\n",
        )
        .unwrap_err();
        assert!(error.contains("only a sticky_oil_bomb"), "{error}");
        // An area that is gone has no points left.
        let error = edited(
            "grid_rows: {2: [], 3: [], 4: []}}",
            "grid_rows: {2: []}, retained: false}",
        )
        .unwrap_err();
        assert!(error.contains("both grid_rows and retained"), "{error}");
        assert!(edited("grid_rows: {2: [], 3: [], 4: []}}", "retained: false}").is_ok());
        let error = edited("grid_rows: {2: [], 3: [], 4: []}", "grid_rows: {7: []}").unwrap_err();
        assert!(error.contains("point index 7"), "{error}");
        // Only a sticky oil bomb's release carries its area.
        let error = edited(
            "{name: shield_airdrop, standing: {position: {x: -150, y: -150}}, retained: false}",
            "{name: shield_airdrop, standing: {position: {x: -150, y: -150}}, grid_rows: {2: []}}",
        )
        .unwrap_err();
        assert!(
            error.contains("standing entry carries grid_rows"),
            "{error}"
        );
    }

    #[test]
    fn a_fight_has_a_schema() {
        let schema = serde_json::to_value(schemars::schema_for!(Fight)).unwrap();
        let properties = &schema["properties"];
        for field in ["seed", "source", "ticks", "hash", "blue", "red"] {
            assert!(properties.get(field).is_some(), "{field}");
        }
        assert_eq!(
            schema["$defs"]["FightKind"]["enum"],
            serde_json::json!(["fight"])
        );
        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&serde_json::json!("seed")));
    }
}
