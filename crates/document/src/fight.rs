//! The fight document: one fight, the layout it starts from and the outcome
//! it leaves, kept apart.
//!
//! `docs/spec/document/fight.md` defines it. A fight is written as a layout,
//! an `outcome` that names each result by its path in the layout, and an
//! optional trajectory; the code holds each result on the object it is
//! about ([`Fight`]). [`project`] gives back the layout, and every layout
//! rule reaches a fight through it: [`parse_yaml`] refuses a fight whose
//! projection does not compile. What this module adds is the outcome, where
//! each result may stand, and the bounds it keeps.

use crate::layout::{
    BattleSkillEntry, BattleSkillRelease, Experience, Layout, Position, SHIELD_AIRDROP_SKILL,
    STICKY_OIL_BOMB_SKILL, Side, Standing, StaticPlacement, UnitPlacement,
};
use crate::{DocumentKind, compile::compile_layout};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const FIGHT_KIND: &str = "fight";

/// A fight document, as the code holds it: the layout's fields, with each
/// result on the object it is about.
///
/// It is written, and read, as the layout and an `outcome` kept apart, the
/// outcome naming each object by its path in the layout ([`WrittenFight`]).
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
    pub blue: FightSide,
    pub red: FightSide,
}

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

/// How a fight is written: `kind` and `source`, the layout's root fields as
/// a layout writes them, the `outcome` by path, and the trajectory.
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
    blue: Side,
    red: Side,
    /// What the fight left, each result under its path in the layout, such
    /// as `blue.units[0].exp`; absent when it left nothing.
    #[serde(default, skip_serializing_if = "serde_yaml::Mapping::is_empty")]
    #[schemars(with = "BTreeMap<String, serde_json::Value>")]
    outcome: serde_yaml::Mapping,
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
        let mut fight = Self {
            kind: written.kind,
            game_build: written.game_build,
            map_id: written.map_id,
            seed: written.seed,
            round: written.round,
            source: written.source,
            trajectory,
            blue: unfought(written.blue),
            red: unfought(written.red),
        }
        // A path counts positions in the layout's normal form.
        .normalized();
        for (path, value) in written.outcome {
            let path = path
                .as_str()
                .ok_or_else(|| format!("outcome key {path:?} is not a path"))?;
            apply_result(&mut fight, path, value)?;
        }
        Ok(fight)
    }
}

impl From<Fight> for WrittenFight {
    fn from(fight: Fight) -> Self {
        let mut outcome = serde_yaml::Mapping::new();
        for (name, side) in [("blue", &fight.blue), ("red", &fight.red)] {
            write_results(name, side, &mut outcome);
        }
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
            blue: project_side(&fight.blue),
            red: project_side(&fight.red),
            outcome,
            ticks,
            hash,
        }
    }
}

/// A layout side as a fight holds it before any result: every unit at the
/// experience it starts with, everything standing.
fn unfought(side: Side) -> FightSide {
    FightSide {
        core_damage: 0,
        officers: side.officers,
        techs: side.techs,
        blueprints: side.blueprints,
        energy_tower_skills: side.energy_tower_skills,
        tower_strengthen_levels: side.tower_strengthen_levels,
        legacy_index: side.legacy_index,
        units: side
            .units
            .into_iter()
            .map(|unit| FightUnit {
                exp: unit.exp.map(|exp| FightExperience {
                    before: exp.current,
                    after: exp.current,
                    maximum: exp.maximum,
                }),
                type_name: unit.type_name,
                index: unit.index,
                position: unit.position,
                level: unit.level,
                rotated: unit.rotated,
                equipment: unit.equipment,
                travelling: unit.travelling,
            })
            .collect(),
        constructions: side.constructions,
        contraptions: side
            .contraptions
            .into_iter()
            .map(|contraption| FightContraption {
                type_name: contraption.type_name,
                index: contraption.index,
                position: contraption.position,
                retained: true,
            })
            .collect(),
        battle_skills: side
            .battle_skills
            .into_iter()
            .map(|entry| match entry {
                BattleSkillEntry::Release(release) => FightBattleSkill::Release(FightRelease {
                    release,
                    retained: true,
                    grid_rows: BTreeMap::new(),
                }),
                BattleSkillEntry::Standing(standing) => FightBattleSkill::Standing(FightStanding {
                    standing,
                    retained: true,
                }),
            })
            .collect(),
    }
}

/// The commander skill a battle-skill name stands for, when it names one.
fn skill_of(name: &str) -> Option<i32> {
    crate::catalog::resolve_battle_skill_type(name).map(|skill| skill.commander_skill_id)
}

/// One `outcome` entry written onto the object its path names.
fn apply_result(fight: &mut Fight, path: &str, value: serde_yaml::Value) -> Result<(), String> {
    let unknown = || format!("outcome path {path} names no result a fight writes");
    let (side_name, rest) = path.split_once('.').ok_or_else(unknown)?;
    let side = match side_name {
        "blue" => &mut fight.blue,
        "red" => &mut fight.red,
        _ => return Err(unknown()),
    };
    if rest == "core_damage" {
        let damage = integer(path, &value)?;
        if damage <= 0 {
            return Err(format!(
                "outcome {path} is {damage}: a side's reactor core loses a positive amount, \
                 and none is not written"
            ));
        }
        side.core_damage = damage;
        return Ok(());
    }
    let (entry, field) = rest.rsplit_once('.').ok_or_else(unknown)?;
    let (list, position) = entry
        .strip_suffix(']')
        .and_then(|entry| entry.split_once('['))
        .ok_or_else(unknown)?;
    let position: usize = position.parse().map_err(|_| unknown())?;
    let absent = || format!("outcome path {path} names no {list} entry of side {side_name}");
    match (list, field) {
        ("units", "exp") => {
            let unit = side.units.get_mut(position).ok_or_else(absent)?;
            let after = integer(path, &value)?;
            let before = unit.exp.map_or(0, |exp| exp.before);
            if after == before {
                return Err(format!(
                    "outcome {path} is the {before} the unit starts with, which is not written"
                ));
            }
            let level = unit.level.unwrap_or(1);
            let maximum = unit
                .exp
                .map(|exp| exp.maximum)
                .or_else(|| crate::experience::full(&unit.type_name, level))
                .ok_or_else(|| {
                    format!(
                        "outcome {path}: unit {} level {level} has no bar",
                        unit.type_name
                    )
                })?;
            unit.exp = Some(FightExperience {
                before,
                after,
                maximum,
            });
        }
        ("contraptions", "retained") => {
            side.contraptions
                .get_mut(position)
                .ok_or_else(absent)?
                .retained = not_retained(path, &value)?;
        }
        ("battle_skills", field @ ("retained" | "grid_rows")) => {
            let entry = side.battle_skills.get_mut(position).ok_or_else(absent)?;
            apply_battle_skill_result(entry, field, path, value)?;
        }
        _ => return Err(unknown()),
    }
    Ok(())
}

/// A `retained` or `grid_rows` result on a `battle_skills` entry, which only
/// what can outlive the round carries.
fn apply_battle_skill_result(
    entry: &mut FightBattleSkill,
    field: &str,
    path: &str,
    value: serde_yaml::Value,
) -> Result<(), String> {
    if field == "retained" {
        let retained = not_retained(path, &value)?;
        match entry {
            FightBattleSkill::Standing(standing) => {
                if !matches!(standing.standing, Standing::Shield { .. }) {
                    return Err(format!(
                        "outcome {path}: a standing area carries no result, as oil lasts \
                         two rounds and a standing area is in its second"
                    ));
                }
                standing.retained = retained;
            }
            FightBattleSkill::Release(release) => {
                let name = &release.release.type_name;
                if !matches!(
                    skill_of(name),
                    Some(SHIELD_AIRDROP_SKILL | STICKY_OIL_BOMB_SKILL)
                ) {
                    return Err(format!(
                        "outcome {path}: battle skill {name}'s release leaves nothing that \
                         outlives the round, so it carries no retained"
                    ));
                }
                if !release.grid_rows.is_empty() {
                    return Err(format!(
                        "outcome {path}: battle skill {name}'s release states both grid_rows \
                         and retained: false"
                    ));
                }
                release.retained = retained;
            }
        }
        return Ok(());
    }
    let FightBattleSkill::Release(release) = entry else {
        return Err(format!(
            "outcome {path}: a standing entry carries no grid_rows as a result; only a \
             sticky_oil_bomb release does"
        ));
    };
    let name = &release.release.type_name;
    if skill_of(name) != Some(STICKY_OIL_BOMB_SKILL) {
        return Err(format!(
            "outcome {path}: battle skill {name}'s release carries grid_rows, and only a \
             sticky_oil_bomb release leaves an area"
        ));
    }
    if !release.retained {
        return Err(format!(
            "outcome {path}: battle skill {name}'s release states both grid_rows and \
             retained: false"
        ));
    }
    release.grid_rows = serde_yaml::from_value(value)
        .map_err(|error| format!("outcome {path} is not a grid_rows mapping: {error}"))?;
    Ok(())
}

fn integer(path: &str, value: &serde_yaml::Value) -> Result<i32, String> {
    value
        .as_i64()
        .and_then(|value| i32::try_from(value).ok())
        .ok_or_else(|| format!("outcome {path} is not a whole number"))
}

/// A `retained` result, which is written only as `false`.
fn not_retained(path: &str, value: &serde_yaml::Value) -> Result<bool, String> {
    match value.as_bool() {
        Some(false) => Ok(false),
        Some(true) => Err(format!(
            "outcome {path} is true, the default, which is not written"
        )),
        None => Err(format!("outcome {path} is not false")),
    }
}

/// A side's results as `outcome` entries, in the order normal form lists
/// them: the side's own, then each list by position, `grid_rows` before
/// `retained`.
fn write_results(name: &str, side: &FightSide, outcome: &mut serde_yaml::Mapping) {
    let mut write = |path: String, value: serde_yaml::Value| {
        outcome.insert(serde_yaml::Value::String(path), value);
    };
    if side.core_damage != 0 {
        write(format!("{name}.core_damage"), side.core_damage.into());
    }
    for (position, unit) in side.units.iter().enumerate() {
        if let Some(exp) = unit.exp.filter(|exp| exp.after != exp.before) {
            write(format!("{name}.units[{position}].exp"), exp.after.into());
        }
    }
    for (position, contraption) in side.contraptions.iter().enumerate() {
        if !contraption.retained {
            write(
                format!("{name}.contraptions[{position}].retained"),
                false.into(),
            );
        }
    }
    for (position, entry) in side.battle_skills.iter().enumerate() {
        let at = |field: &str| format!("{name}.battle_skills[{position}].{field}");
        let (grid_rows, retained) = match entry {
            FightBattleSkill::Release(release) => (Some(&release.grid_rows), release.retained),
            FightBattleSkill::Standing(standing) => (None, standing.retained),
        };
        if let Some(grid_rows) = grid_rows.filter(|grid_rows| !grid_rows.is_empty()) {
            write(
                at("grid_rows"),
                serde_yaml::to_value(grid_rows).expect("grid rows serialize"),
            );
        }
        if !retained {
            write(at("retained"), false.into());
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
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FightSide {
    /// What the fight took off the side's reactor core; absent when `0`.
    pub core_damage: i32,
    pub officers: Vec<i32>,
    pub techs: Vec<i32>,
    pub blueprints: Vec<i32>,
    pub energy_tower_skills: Vec<i32>,
    pub tower_strengthen_levels: Vec<i32>,
    pub legacy_index: i32,
    pub units: Vec<FightUnit>,
    pub constructions: Vec<StaticPlacement>,
    pub contraptions: Vec<FightContraption>,
    pub battle_skills: Vec<FightBattleSkill>,
}

/// A layout unit whose `exp` also says what the fight ended it on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FightUnit {
    pub type_name: String,
    pub index: i32,
    pub position: Position,
    pub level: Option<i32>,
    pub exp: Option<FightExperience>,
    pub rotated: Option<bool>,
    pub equipment: Vec<i32>,
    pub travelling: Option<bool>,
}

/// A unit's experience across a fight.
///
/// `before` and `maximum` are the layout's `current` and the level's bar;
/// `after` is what the unit ends the fight holding, the outcome's `exp`. A full bar takes no further share, so
/// `after` never passes `maximum`, and a fight only adds experience, so it
/// never falls below `before`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FightExperience {
    pub before: i32,
    pub after: i32,
    pub maximum: i32,
}

/// A layout contraption, and whether it still stands when the fight ends.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FightContraption {
    pub type_name: String,
    pub index: i32,
    pub position: Position,
    /// Written only as `false`.
    pub retained: bool,
}

/// One `battle_skills` entry of a fight: the layout's entry and what the
/// fight left of it.
#[derive(Clone, Debug, PartialEq, Eq)]
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

/// The layout a fight starts from: the fight with its outcome and
/// trajectory dropped.
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
        legacy_index: side.legacy_index,
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
    crate::spelling::document_listing(&value, "outcome")
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
source: game
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
outcome:
  blue.units[0].exp: 170
  blue.contraptions[0].retained: false
  blue.battle_skills[0].retained: false
  red.core_damage: 37
  red.units[0].exp: 40
  red.battle_skills[0].grid_rows: {2: [], 3: [], 4: []}
ticks: 870
hash: 23:380d721bf2aa581622f521e4386160a0b5eedfb16ffed7b477b7e288c31534ef
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
        assert_eq!(fight.source, Source::Game);
        assert_eq!(fight.red.core_damage, 37);
        assert_eq!(
            fight.blue.units[0].exp,
            Some(FightExperience {
                before: 12,
                after: 170,
                maximum: 650
            })
        );
        // The bar is the table's where the layout keeps no gauge.
        assert_eq!(
            fight.red.units[0].exp,
            Some(FightExperience {
                before: 0,
                after: 40,
                maximum: 750
            })
        );
        assert!(!fight.blue.contraptions[0].retained);
        let FightBattleSkill::Release(oil) = &fight.red.battle_skills[0] else {
            panic!("a release")
        };
        assert!(oil.retained);
        assert_eq!(oil.grid_rows.keys().copied().collect::<Vec<_>>(), [2, 3, 4]);
        assert_eq!(canonical_yaml(fight).unwrap(), EXAMPLE);
    }

    #[test]
    fn a_fight_round_trips_through_its_normal_form() {
        // Fields out of order, standing entries out of order, and the
        // outcome's paths counted in the layout's normal form.
        let loose = "\
kind: fight
seed: 4242
round: 3
blue:
  units:
  - {name: arclight, index: 4, position: {x: 60, y: -50}, level: 1, rotated: false}
  - {name: marksman, index: 0, position: {x: 0, y: -50}, exp: 12/650}
  contraptions:
  - {name: interceptor, index: 2, position: {x: 5, y: -95}}
  battle_skills:
  - {name: shield_airdrop, positions: [{x: 0, y: -150}]}
  - {name: shield_airdrop, standing: {position: {x: 150, y: -150}}}
  - {name: sticky_oil_bomb, positions: [{x: -30, y: 150}, {x: 60, y: 150}]}
  - {name: shield_airdrop, standing: {position: {x: -150, y: -150}}}
red:
  units:
  - {name: arclight, index: 0, position: {x: 0, y: -100}}
hash: 23:380d721bf2aa581622f521e4386160a0b5eedfb16ffed7b477b7e288c31534ef
outcome:
  blue.battle_skills[0].retained: false
  blue.units[0].exp: 170
source: game
ticks: 870
";
        let fight = parse_yaml(loose.as_bytes()).unwrap();
        let written = canonical_yaml(fight.clone()).unwrap();
        assert_eq!(
            written,
            "\
kind: fight
source: game
seed: 4242
round: 3
blue:
  units:
  - {name: marksman, index: 0, position: {x: 0, y: -50}, exp: 12/650}
  - {name: arclight, index: 4, position: {x: 60, y: -50}}
  contraptions:
  - {name: interceptor, index: 2, position: {x: 5, y: -95}}
  battle_skills:
  - {name: shield_airdrop, standing: {position: {x: -150, y: -150}}}
  - {name: shield_airdrop, standing: {position: {x: 150, y: -150}}}
  - {name: shield_airdrop, positions: [{x: 0, y: -150}]}
  - {name: sticky_oil_bomb, positions: [{x: -30, y: 150}, {x: 60, y: 150}]}
red:
  units:
  - {name: arclight, index: 0, position: {x: 0, y: -100}}
outcome:
  blue.units[0].exp: 170
  blue.battle_skills[0].retained: false
ticks: 870
hash: 23:380d721bf2aa581622f521e4386160a0b5eedfb16ffed7b477b7e288c31534ef
",
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
            EXAMPLE_LAYOUT
        );
    }

    #[test]
    fn a_layout_carrying_a_result_is_refused() {
        for (from, to) in [
            ("round: 3\n", "round: 3\nsource: game\n"),
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
    fn a_fight_keeps_no_result_inside_its_layout() {
        for (from, to) in [
            ("exp: 12/650", "exp: 12/170/650"),
            ("red:\n", "red:\n  core_damage: 37\n"),
            (
                "position: {x: 5, y: -95}}",
                "position: {x: 5, y: -95}, retained: false}",
            ),
        ] {
            assert!(edited(from, to).is_err(), "a fight accepted {to:?}");
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
            "exp: 12/650}\n",
            "exp: 12/650}\n  - {name: marksman, index: 1, position: {x: 0, y: -50}}\n",
        )
        .unwrap_err();
        assert!(error.contains("collide"), "{error}");
    }

    #[test]
    fn a_trajectory_is_ticks_and_hash_together() {
        let error = edited("source: game", "source: replay").unwrap_err();
        assert!(error.contains("replay"), "{error}");
        assert!(edited("source: game", "source: simulator").is_ok());
        let error = edited("ticks: 870\n", "").unwrap_err();
        assert!(error.contains("together"), "{error}");
        let outcome_alone = edited(
            "ticks: 870\nhash: 23:380d721bf2aa581622f521e4386160a0b5eedfb16ffed7b477b7e288c31534ef\n",
            "",
        )
        .unwrap();
        assert_eq!(outcome_alone.trajectory, None);
        assert!(!canonical_yaml(outcome_alone).unwrap().contains("ticks"));
        let error = edited("23:380d", "23:380D").unwrap_err();
        assert!(error.contains("64 lowercase hex"), "{error}");
        let error = edited("hash: 23:", "hash: mcfr-23:").unwrap_err();
        assert!(error.contains("not a profile number"), "{error}");
        let error = edited("hash: 23:380d", "hash: 380d").unwrap_err();
        assert!(error.contains("<profile>:<hex>"), "{error}");
        let error = edited("ticks: 870", "ticks: 0").unwrap_err();
        assert!(error.contains("at least 1"), "{error}");
    }

    #[test]
    fn a_path_names_an_object_of_the_layout() {
        let error = edited("blue.units[0].exp", "blue.units[1].exp").unwrap_err();
        assert!(error.contains("names no units entry"), "{error}");
        let error = edited("blue.units[0].exp", "green.units[0].exp").unwrap_err();
        assert!(error.contains("names no result"), "{error}");
        let error = edited("blue.units[0].exp", "blue.units[0].life").unwrap_err();
        assert!(error.contains("names no result"), "{error}");
        let error = edited("red.core_damage: 37", "red.core_damage: 0").unwrap_err();
        assert!(error.contains("not written"), "{error}");
    }

    #[test]
    fn experience_keeps_to_its_bar() {
        let error = edited("blue.units[0].exp: 170", "blue.units[0].exp: 651").unwrap_err();
        assert!(
            error.contains("a full bar takes no further share"),
            "{error}"
        );
        assert!(edited("blue.units[0].exp: 170", "blue.units[0].exp: 650").is_ok());
        let error = edited("blue.units[0].exp: 170", "blue.units[0].exp: 11").unwrap_err();
        assert!(error.contains("only adds experience"), "{error}");
        let error = edited("blue.units[0].exp: 170", "blue.units[0].exp: 12").unwrap_err();
        assert!(error.contains("not written"), "{error}");
        let error = edited("blue.units[0].exp: 170", "blue.units[0].exp: 1.5").unwrap_err();
        assert!(error.contains("whole number"), "{error}");
    }

    #[test]
    fn a_result_stands_only_where_something_can_be_left() {
        // A standing oil area is in its last round.
        let error = edited(
            "  - {name: shield_airdrop, standing: {position: {x: -150, y: -150}}}\n",
            "  - {name: sticky_oil_bomb, standing: {control_points: [{x: -24, y: 11}, {x: 80, y: 1}]}}\n",
        )
        .unwrap_err();
        assert!(error.contains("standing area carries no result"), "{error}");
        // A missile strike leaves nothing.
        let error = edited(
            "  - {name: shield_airdrop, standing: {position: {x: -150, y: -150}}}\n",
            "  - {name: missile_strike, positions: [{x: 0, y: 150}]}\n",
        )
        .unwrap_err();
        assert!(error.contains("leaves nothing"), "{error}");
        // A shield leaves no area.
        let error = edited(
            "red.battle_skills[0].grid_rows",
            "blue.battle_skills[0].grid_rows",
        )
        .unwrap_err();
        assert!(error.contains("only a"), "{error}");
        // An area that is gone has no points left.
        let error = edited(
            "red.battle_skills[0].grid_rows: {2: [], 3: [], 4: []}\n",
            "red.battle_skills[0].grid_rows: {2: []}\n  red.battle_skills[0].retained: false\n",
        )
        .unwrap_err();
        assert!(error.contains("both grid_rows and retained"), "{error}");
        assert!(
            edited(
                "red.battle_skills[0].grid_rows: {2: [], 3: [], 4: []}",
                "red.battle_skills[0].retained: false"
            )
            .is_ok()
        );
        let error = edited("grid_rows: {2: [], 3: [], 4: []}", "grid_rows: {7: []}").unwrap_err();
        assert!(error.contains("point index 7"), "{error}");
        // Retained is written only as false.
        let error = edited(
            "blue.contraptions[0].retained: false",
            "blue.contraptions[0].retained: true",
        )
        .unwrap_err();
        assert!(error.contains("not written"), "{error}");
    }

    #[test]
    fn a_fight_has_a_schema() {
        let schema = serde_json::to_value(schemars::schema_for!(Fight)).unwrap();
        let properties = &schema["properties"];
        for field in ["seed", "source", "outcome", "ticks", "hash", "blue", "red"] {
            assert!(properties.get(field).is_some(), "{field}");
        }
        assert_eq!(
            schema["$defs"]["FightKind"]["enum"],
            serde_json::json!(["fight"])
        );
        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&serde_json::json!("seed")));
        assert!(!required.contains(&serde_json::json!("ticks")));
    }
}
