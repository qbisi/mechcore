//! The fight document: one fight, the layout it starts from and the outcome
//! it leaves, kept apart.
//!
//! `docs/spec/document/fight.md` defines it. A fight is written as a layout,
//! a nested `outcome` that names units and contraptions by index, and an
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
/// outcome naming units and contraptions by index ([`WrittenFight`]).
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

/// How a fight is written: `kind` and `source`, the layout's root fields as
/// a layout writes them, the nested `outcome`, and the trajectory.
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
    /// Complete core damage for both sides, and changed objects by identity.
    outcome: Outcome,
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

/// Settlement is required even when both cores take zero damage.
#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Outcome {
    blue: SideOutcome,
    red: SideOutcome,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SideOutcome {
    #[schemars(range(min = 0))]
    core_damage: i32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    units: Vec<UnitOutcome>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    contraptions: Vec<ContraptionOutcome>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    battle_skills: Vec<BattleSkillOutcome>,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct UnitOutcome {
    #[schemars(range(min = 0))]
    index: i32,
    exp: Experience,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ContraptionOutcome {
    #[schemars(range(min = 0))]
    index: i32,
    retained: bool,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct BattleSkillOutcome {
    /// Position in the layout's normalized `battle_skills` list.
    at: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    grid_rows: Option<BTreeMap<u32, Vec<u32>>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    retained: Option<bool>,
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
            asserts: written.asserts,
            blue: unfought(written.blue),
            red: unfought(written.red),
        }
        // Battle skills without native indices use normalized list positions.
        .normalized();
        apply_results("blue", &mut fight.blue, written.outcome.blue)?;
        apply_results("red", &mut fight.red, written.outcome.red)?;
        Ok(fight)
    }
}

impl From<Fight> for WrittenFight {
    fn from(fight: Fight) -> Self {
        let outcome = Outcome {
            blue: write_results(&fight.blue),
            red: write_results(&fight.red),
        };
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
            asserts: fight.asserts,
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
        recovered: side.recovered.clone(),
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
                source: unit.source,
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

/// Apply sparse results by native index, or normalized battle-skill position.
fn apply_results(name: &str, side: &mut FightSide, outcome: SideOutcome) -> Result<(), String> {
    side.core_damage = outcome.core_damage;
    let mut seen = std::collections::BTreeSet::new();
    for result in outcome.units {
        let index = result.index;
        let path = format!("{name}.units[index={index}].exp");
        if !seen.insert(index) {
            return Err(format!("outcome {name}.units repeats index {index}"));
        }
        let unit = side
            .units
            .iter_mut()
            .find(|unit| unit.index == index)
            .ok_or_else(|| format!("outcome {path} names no units entry"))?;
        let before = unit.exp.map_or(0, |exp| exp.before);
        if result.exp.current == before {
            return Err(format!(
                "outcome {path} is the {before} the unit starts with, which is not written"
            ));
        }
        unit.exp = Some(FightExperience {
            before,
            after: result.exp.current,
            maximum: result.exp.maximum,
        });
    }
    seen.clear();
    for result in outcome.contraptions {
        let index = result.index;
        let path = format!("{name}.contraptions[index={index}].retained");
        if !seen.insert(index) {
            return Err(format!("outcome {name}.contraptions repeats index {index}"));
        }
        let contraption = side
            .contraptions
            .iter_mut()
            .find(|entry| entry.index == index)
            .ok_or_else(|| format!("outcome {path} names no contraptions entry"))?;
        contraption.retained = not_retained(&path, &result.retained.into())?;
    }
    let mut positions = std::collections::BTreeSet::new();
    for result in outcome.battle_skills {
        let at = result.at;
        let path = format!("{name}.battle_skills[at={at}]");
        if !positions.insert(at) {
            return Err(format!("outcome {name}.battle_skills repeats at {at}"));
        }
        if result.retained.is_none() && result.grid_rows.is_none() {
            return Err(format!("outcome {path} states no result"));
        }
        let entry = side
            .battle_skills
            .get_mut(at)
            .ok_or_else(|| format!("outcome {path} names no battle_skills entry"))?;
        if let Some(retained) = result.retained {
            apply_battle_skill_result(entry, "retained", &path, retained.into())?;
        }
        if let Some(grid_rows) = result.grid_rows {
            apply_battle_skill_result(
                entry,
                "grid_rows",
                &path,
                serde_yaml::to_value(grid_rows).map_err(|error| error.to_string())?,
            )?;
        }
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

/// Changed objects in layout order, with explicit core damage on each side.
fn write_results(side: &FightSide) -> SideOutcome {
    SideOutcome {
        core_damage: side.core_damage,
        units: side
            .units
            .iter()
            .filter_map(|unit| {
                unit.exp
                    .filter(|exp| exp.after != exp.before)
                    .map(|exp| UnitOutcome {
                        index: unit.index,
                        exp: Experience {
                            current: exp.after,
                            maximum: exp.maximum,
                        },
                    })
            })
            .collect(),
        contraptions: side
            .contraptions
            .iter()
            .filter(|entry| !entry.retained)
            .map(|entry| ContraptionOutcome {
                index: entry.index,
                retained: false,
            })
            .collect(),
        battle_skills: side
            .battle_skills
            .iter()
            .enumerate()
            .filter_map(|(at, entry)| {
                let (grid_rows, retained) = match entry {
                    FightBattleSkill::Release(release) => {
                        (Some(&release.grid_rows), release.retained)
                    }
                    FightBattleSkill::Standing(standing) => (None, standing.retained),
                };
                let grid_rows = grid_rows.filter(|rows| !rows.is_empty()).cloned();
                let retained = (!retained).then_some(false);
                (grid_rows.is_some() || retained.is_some()).then_some(BattleSkillOutcome {
                    at,
                    grid_rows,
                    retained,
                })
            })
            .collect(),
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
    pub units: Vec<FightUnit>,
    pub recovered: Vec<i32>,
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
    pub source: crate::layout::UnitSource,
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
  blue:
    core_damage: 0
    units:
    - {index: 0, exp: 170/650}
    contraptions:
    - {index: 0, retained: false}
    battle_skills:
    - {at: 0, retained: false}
  red:
    core_damage: 37
    units:
    - {index: 0, exp: 40/750}
    battle_skills:
    - {at: 0, grid_rows: {2: [], 3: [], 4: []}}
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
            )
            .replace("index: 0, exp: 170/650", "index: 7, exp: 170/650")
            .replace("index: 0, retained: false", "index: 9, retained: false");
        let fight = parse_yaml(text.as_bytes()).unwrap();
        assert_eq!(fight.blue.units[0].index, 7);
        assert_eq!(fight.blue.units[0].exp.unwrap().after, 170);
        assert_eq!(fight.blue.contraptions[0].index, 9);
        assert!(!fight.blue.contraptions[0].retained);
        assert_eq!(canonical_yaml(fight).unwrap(), text);
    }

    #[test]
    fn missing_and_repeated_identities_are_refused() {
        for (from, to, message) in [
            (
                "index: 0, exp: 170/650",
                "index: 1, exp: 170/650",
                "names no units entry",
            ),
            (
                "index: 0, retained: false",
                "index: 1, retained: false",
                "names no contraptions entry",
            ),
            (
                "at: 0, retained: false",
                "at: 1, retained: false",
                "names no battle_skills entry",
            ),
            (
                "    - {index: 0, exp: 170/650}",
                "    - {index: 0, exp: 170/650}\n    - {index: 0, exp: 171/650}",
                "repeats index",
            ),
            (
                "    - {index: 0, retained: false}",
                "    - {index: 0, retained: false}\n    - {index: 0, retained: false}",
                "repeats index",
            ),
            (
                "    - {at: 0, retained: false}",
                "    - {at: 0, retained: false}\n    - {at: 0, retained: false}",
                "repeats at",
            ),
        ] {
            let error = edited(from, to).unwrap_err();
            assert!(error.contains(message), "{error}");
        }
    }

    #[test]
    fn settlement_requires_both_core_damages_and_accepts_zero() {
        for text in [
            EXAMPLE.replace("    core_damage: 0\n", ""),
            EXAMPLE.replace("    core_damage: 37\n", ""),
            EXAMPLE[..EXAMPLE.find("outcome:").unwrap()].to_owned(),
        ] {
            assert!(parse_yaml(text.as_bytes()).is_err());
        }
        assert!(edited("core_damage: 37", "core_damage: 0").is_ok());
        assert!(
            edited("core_damage: 37", "core_damage: -1")
                .unwrap_err()
                .contains("non-negative")
        );
        let old = EXAMPLE[..EXAMPLE.find("outcome:").unwrap()].to_owned()
            + "outcome:\n  red.core_damage: 37\n";
        assert!(parse_yaml(old.as_bytes()).is_err());
    }

    #[test]
    fn experience_states_the_same_bar_as_the_layout() {
        for (to, message) in [
            ("651/650", "full bar"),
            ("170/651", "not its level"),
            ("11/650", "only adds"),
            ("12/650", "not written"),
            ("170", "current/maximum"),
            ("1.5/650", "current/maximum"),
        ] {
            let error = edited("exp: 170/650", &format!("exp: {to}")).unwrap_err();
            assert!(error.contains(message), "{error}");
        }
        assert!(edited("exp: 170/650", "exp: 650/650").is_ok());
    }

    #[test]
    fn results_stand_only_on_objects_that_can_carry_them() {
        assert!(edited("index: 0, exp: 170/650", "index: 0, life: 170").is_err());
        assert!(
            edited("index: 0, retained: false", "index: 0, retained: true")
                .unwrap_err()
                .contains("not written")
        );
        assert!(
            edited("at: 0, retained: false", "at: 0")
                .unwrap_err()
                .contains("no result")
        );
        assert!(
            edited("at: 0, retained: false", "at: 0, grid_rows: {2: []}")
                .unwrap_err()
                .contains("only a")
        );
        assert!(
            edited(
                "at: 0, grid_rows: {2: [], 3: [], 4: []}",
                "at: 0, grid_rows: {2: []}, retained: false"
            )
            .unwrap_err()
            .contains("both grid_rows and retained")
        );
        assert!(
            edited("grid_rows: {2: [], 3: [], 4: []}", "grid_rows: {7: []}")
                .unwrap_err()
                .contains("point index 7")
        );
        assert!(
            edited(
                "at: 0, grid_rows: {2: [], 3: [], 4: []}",
                "at: 0, retained: false"
            )
            .is_ok()
        );
    }

    #[test]
    fn a_fight_keeps_no_result_inside_the_layout() {
        for (from, to) in [
            ("exp: 12/650", "exp: 12/170/650"),
            ("red:\n", "red:\n  core_damage: 37\n"),
            (
                "position: {x: 5, y: -95}}",
                "position: {x: 5, y: -95}, retained: false}",
            ),
        ] {
            assert!(edited(from, to).is_err());
        }
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
                "exp: 12/650}\n",
                "exp: 12/650}\n  - {name: marksman, index: 1, position: {x: 0, y: -50}}\n"
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
    fn a_fight_has_a_typed_required_outcome_schema() {
        let schema = serde_json::to_value(schemars::schema_for!(Fight)).unwrap();
        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&serde_json::json!("outcome")));
        assert!(required.contains(&serde_json::json!("seed")));
        assert!(!required.contains(&serde_json::json!("ticks")));
        assert_eq!(
            schema["$defs"]["Outcome"]["required"],
            serde_json::json!(["blue", "red"])
        );
        assert!(
            schema["$defs"]["SideOutcome"]["required"]
                .as_array()
                .unwrap()
                .contains(&serde_json::json!("core_damage"))
        );
    }
}
