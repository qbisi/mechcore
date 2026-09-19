//! The battle document and the state and action segments it is made of.
//!
//! `docs/spec/document/battle.md` defines a battle as a stream of YAML
//! documents: a header, the opening's decisions, and then each round's opening
//! state followed by the decisions taken from it. `docs/spec/document/state.md`
//! and `docs/spec/document/action.md` define the two segment shapes. Filling
//! one from a replay is [`crate::convert`].

use crate::layout::{ContraptionPlacement, Position, StaticPlacement, Terrain, UnitPlacement};
use serde::{Deserialize, Serialize};
use serde_yaml::Value;
use std::borrow::Cow;
use std::collections::BTreeMap;

/// One recorded match, as `docs/spec/document/battle.md` defines it.
///
/// This is the match held whole. Its document is a stream, and
/// [`canonical_yaml`] writes it as one: the header from `map_id`, `seed` and
/// `sides`, the opening's decisions from each side's [`Opening`], and a state
/// segment and an action segment per turn.
#[derive(Debug, PartialEq, Eq)]
pub struct Battle {
    pub map_id: i32,
    pub seed: i32,
    pub sides: BattleSides,
    /// The deployment rounds, from the first one. The opening is round zero
    /// and has no state to open it, so each side's [`Opening`] holds it.
    pub turns: Vec<Turn>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct BattleSides {
    pub blue: BattleSide,
    pub red: BattleSide,
}

/// What a side holds for the whole match rather than for one round.
#[derive(Debug, PartialEq, Eq)]
pub struct BattleSide {
    /// The opening this side was dealt and the one it took.
    pub opening: Opening,
    /// The construction layout the map dealt this side before the first round.
    ///
    /// A state's own `constructions` is the live list, shortened when a
    /// building is recovered or destroyed. This one is what the side started
    /// with, so a battle says where the buildings came from rather than having
    /// them appear in the first round it happens to hold.
    pub constructions: Vec<StaticPlacement>,
    /// Which technologies each unit may research, keyed by unit ID and
    /// written keyed by type name, in ID order. Only units a standard 1v1
    /// match can field have a row.
    pub tech_loadout: BTreeMap<i32, Vec<i32>>,
}

/// The opening a side was dealt, and which of it the side took.
///
/// The header states the four combinations and the round-zero action segment
/// states the choice, so the two halves are written in different places. They
/// are held together here because every reader of one needs the other.
#[derive(Debug, PartialEq, Eq)]
pub struct Opening {
    /// Zero-based index of the combination taken from `offers`.
    pub choose: i32,
    /// The four combinations this side was dealt, in the order shown.
    ///
    /// The deal is private to the side, which is why it sits under the side in
    /// the header and not beside a state's shared `reinforce_offers`.
    /// Conversion reconstructs it from the replay's random state.
    pub offers: Vec<OpeningOffer>,
}

/// One of the openings a side was dealt: a team of formations and the
/// specialist officer bound to it.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct OpeningOffer {
    /// Written as the team's name, its two unit types.
    #[serde(with = "crate::names::advance_team::one")]
    pub team: i32,
    #[serde(with = "crate::names::officer::one")]
    pub specialist: i32,
}

impl Opening {
    /// The opening as the decision that took it, which is what the round-zero
    /// action segment holds.
    ///
    /// # Panics
    ///
    /// Panics if `choose` is not an index into `offers`. Conversion checks this
    /// before constructing the opening.
    #[must_use]
    pub fn action(&self) -> Action {
        let taken = usize::try_from(self.choose)
            .ok()
            .and_then(|at| self.offers.get(at))
            .expect("opening choice must name a dealt combination");
        Action::ChooseAdvanceTeam {
            offer: self.choose,
            id: taken.team,
            specialist: taken.specialist,
        }
    }
}

/// One deployment round: the state it opens with and the decisions taken from
/// it, which the stream writes as a state segment and an action segment.
#[derive(Debug, PartialEq, Eq)]
pub struct Turn {
    pub round: i32,
    pub state: State,
    pub actions: TurnActions,
}

/// A match position, which a battle writes as a state segment.
#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct State {
    /// Absent in rounds 0 and 1, which are dealt no reinforcement offer.
    /// Written as card names, a unit card's read within this round.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::names::card::offers"
    )]
    pub reinforce_offers: Option<Vec<i32>>,
    pub sides: StateSides,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StateSides {
    pub blue: SideState,
    pub red: SideState,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SideState {
    pub reactor_core: i32,
    pub supply: i32,
    pub shop: ShopState,
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        with = "crate::names::blueprint::many"
    )]
    pub blueprints: Vec<i32>,
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        with = "crate::names::energy_tower_skill::many"
    )]
    pub energy_tower_skills: Vec<i32>,
    pub tower_strengthen_levels: Vec<i32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub equipment: Vec<EquipmentItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub battle_skills: Vec<PanelSkill>,
    pub next_index: NextIndex,
    /// The officers the side holds, a multiset in ascending ID, by name.
    #[serde(default, with = "crate::names::officer::many")]
    pub officers: Vec<i32>,
    /// The unit technologies the side has researched, in ascending ID,
    /// written grouped by the unit they belong to.
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        with = "crate::names::technologies"
    )]
    pub techs: Vec<i32>,
    pub units: Vec<StateUnit>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub constructions: Vec<StaticPlacement>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contraptions: Vec<ContraptionPlacement>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub airdrop_shields: Vec<Position>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub terrains: Vec<Terrain>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ShopState {
    /// Unit IDs, written as their type names in ID order.
    #[serde(with = "unit_names::units")]
    pub unlocked_units: Vec<i32>,
    pub buys_remaining: i32,
    pub unlocks_remaining: i32,
}

/// A formation, and what recovering it pays back.
///
/// `value` is not a function of the unit's type and level. It is what the side
/// actually paid, at the prices its officers made at the time, so two identical
/// looking formations bought a round apart can be worth different amounts.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StateUnit {
    #[serde(flatten)]
    pub unit: UnitPlacement,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<i32>,
    /// Whether the side may move the formation this round.
    ///
    /// A formation moves in the round it arrives and is fixed after that,
    /// unless [`crate::mobility`] frees it. The round it arrived in is not
    /// otherwise part of a position, so the answer is a field.
    #[serde(default, skip_serializing_if = "is_false")]
    pub movable: bool,
}

/// An owned item no formation carries; a fitted one is named by its formation.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct EquipmentItem {
    #[serde(rename = "name", with = "crate::names::equipment::one")]
    pub id: i32,
    /// Absent means `-1`, which is every item a standard 1v1 hands out.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub durability: Option<i32>,
}

/// One commander skill panel slot.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PanelSkill {
    pub index: i32,
    #[serde(rename = "name", with = "crate::names::commander_skill::one")]
    pub id: i32,
    pub cooldown: i32,
    /// True on a deployment skill this round used.
    ///
    /// A deployment skill does its work before the fight, as a change to the
    /// position, so the slot records that it was spent and nothing else: no
    /// target, no place in the release order, and no entry in a layout. A
    /// round's opening position carries none.
    #[serde(default, skip_serializing_if = "is_false")]
    pub used: bool,
    /// Present on a skill this round released. A state is defined after each
    /// action, so a round's opening position carries none and a deployment's
    /// closing position carries one per release.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release: Option<Release>,
}

/// Where in a round's release sequence a skill went off, and at what.
///
/// `order` is explicit because the panel is sorted by `index`, so array
/// position cannot carry it. A layout is the other way round: it lists only
/// releases, and there the array position is the order.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Release {
    pub order: i32,
    #[serde(with = "serde_yaml::with::singleton_map")]
    pub target: SkillTarget,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct NextIndex {
    pub unit: i32,
    pub contraption: i32,
}

/// Each side's decisions in one round, which a battle writes as an action
/// segment.
#[derive(Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TurnActions {
    pub blue: Vec<Action>,
    pub red: Vec<Action>,
}

/// The `offer` of a [`Action::ChooseReinforceItem`] that declined the round.
///
/// The game records declining as the same action at this offer, which is not a
/// position in `reinforce_offers`, with an `ID` of zero.
pub const DECLINED_OFFER: i32 = -1;

/// One decision that took effect, in the order the side took it.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    /// The round's reinforcement answer, which declining is one of.
    ///
    /// `offer` is the offer's position in `reinforce_offers`, or
    /// [`DECLINED_OFFER`] for the decline, which the round always makes
    /// available and never deals. `id` names the item taken and is present
    /// exactly when the offer was not declined: what a decline hands back is
    /// built from the match's progress rather than drawn from a catalogue, so
    /// it has no ID to carry.
    ChooseReinforceItem {
        offer: i32,
        #[serde(
            rename = "name",
            skip_serializing_if = "Option::is_none",
            with = "crate::names::card::option"
        )]
        id: Option<i32>,
    },
    /// The opening, which is one decision with two halves: the team of
    /// formations and the specialist officer bound to it.
    ///
    /// It is the only decision of round zero, and no other round holds one.
    ChooseAdvanceTeam {
        offer: i32,
        #[serde(rename = "name", with = "crate::names::advance_team::one")]
        id: i32,
        #[serde(with = "crate::names::officer::one")]
        specialist: i32,
    },
    /// A purchase and where the new formation is deployed.
    ///
    /// The game lands a purchase where [`crate::landing`] says the board has
    /// room, and the player moves it from there. A battle writes the position
    /// the formation reaches within the main half, which is where the
    /// purchase's own moves end, so those moves are not written again.
    BuyUnit {
        #[serde(rename = "name", with = "unit_names::unit")]
        unit: i32,
        position: Position,
        #[serde(skip_serializing_if = "is_false")]
        rotated: bool,
    },
    UpgradeUnit {
        index: i32,
    },
    UnlockUnit {
        #[serde(rename = "name", with = "unit_names::unit")]
        unit: i32,
    },
    /// `tech` is written by its name, which is only unique within `unit`.
    UpgradeTechnology {
        #[serde(with = "unit_names::unit")]
        unit: i32,
        #[serde(serialize_with = "crate::names::technology::serialize")]
        tech: i32,
    },
    ActiveBlueprint {
        #[serde(rename = "name", with = "crate::names::blueprint::one")]
        id: i32,
    },
    ActiveEnergyTowerSkill {
        #[serde(rename = "name", with = "crate::names::energy_tower_skill::one")]
        skill: i32,
    },
    StrengthenTower {
        tower: i32,
    },
    UseEquipment {
        #[serde(rename = "name", with = "crate::names::equipment::one")]
        equipment: i32,
        index: i32,
    },
    MoveUnit {
        index: i32,
        position: Position,
        #[serde(skip_serializing_if = "is_false")]
        rotated: bool,
    },
    /// `index` is the panel slot released and `id` the skill it holds. The
    /// slot is what the game names; the skill is stated beside it so a
    /// release reads without the panel, and a release whose slot holds
    /// another skill is refused.
    ReleaseCommanderSkill {
        index: i32,
        #[serde(rename = "name", with = "crate::names::commander_skill::one")]
        id: i32,
        #[serde(with = "serde_yaml::with::singleton_map")]
        target: SkillTarget,
    },
    ReleaseContraption {
        #[serde(rename = "name", with = "crate::names::contraption::one")]
        contraption: i32,
        position: Position,
        #[serde(skip_serializing_if = "Option::is_none")]
        extra_position: Option<Position>,
    },
    /// Giving up, which ends the match.
    ///
    /// It is the last decision its side takes: the round is not fought, so no
    /// state follows the segment that holds it.
    Concede,
}

/// A release covers an area or points at one object, never both. A document
/// writes it as a one-key mapping, `{unit: 4}` or `{area: [...]}`, which every
/// YAML and JSON reader takes, rather than `serde_yaml`'s `!unit 4` tag.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkillTarget {
    Area(Vec<Position>),
    Unit(i32),
    Construction(i32),
}

impl<'de> Deserialize<'de> for Action {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // A technology's name is resolved against the unit beside it, so the
        // value is read first and the name replaced by its ID.
        let value =
            name_technology(Value::deserialize(deserializer)?).map_err(serde::de::Error::custom)?;
        ActionReader::deserialize(value).map_err(serde::de::Error::custom)
    }
}

/// [`Action`]'s reader, which serde derives against the real type: a field
/// missing here or typed differently from there does not compile.
#[derive(Deserialize)]
#[serde(
    remote = "Action",
    tag = "type",
    rename_all = "snake_case",
    deny_unknown_fields
)]
enum ActionReader {
    ChooseReinforceItem {
        offer: i32,
        #[serde(rename = "name", default, with = "crate::names::card::option")]
        id: Option<i32>,
    },
    ChooseAdvanceTeam {
        offer: i32,
        #[serde(rename = "name", with = "crate::names::advance_team::one")]
        id: i32,
        #[serde(with = "crate::names::officer::one")]
        specialist: i32,
    },
    BuyUnit {
        #[serde(rename = "name", with = "unit_names::unit")]
        unit: i32,
        position: Position,
        #[serde(default)]
        rotated: bool,
    },
    UpgradeUnit {
        index: i32,
    },
    UnlockUnit {
        #[serde(rename = "name", with = "unit_names::unit")]
        unit: i32,
    },
    UpgradeTechnology {
        #[serde(with = "unit_names::unit")]
        unit: i32,
        /// Already an ID: [`name_technology`] resolves it within its unit.
        tech: i32,
    },
    ActiveBlueprint {
        #[serde(rename = "name", with = "crate::names::blueprint::one")]
        id: i32,
    },
    ActiveEnergyTowerSkill {
        #[serde(rename = "name", with = "crate::names::energy_tower_skill::one")]
        skill: i32,
    },
    StrengthenTower {
        tower: i32,
    },
    UseEquipment {
        #[serde(rename = "name", with = "crate::names::equipment::one")]
        equipment: i32,
        index: i32,
    },
    MoveUnit {
        index: i32,
        position: Position,
        #[serde(default)]
        rotated: bool,
    },
    ReleaseCommanderSkill {
        index: i32,
        #[serde(rename = "name", with = "crate::names::commander_skill::one")]
        id: i32,
        #[serde(with = "serde_yaml::with::singleton_map")]
        target: SkillTarget,
    },
    ReleaseContraption {
        #[serde(rename = "name", with = "crate::names::contraption::one")]
        contraption: i32,
        position: Position,
        extra_position: Option<Position>,
    },
    Concede,
}

/// Resolves an `upgrade_technology`'s `tech` name to its ID, within the unit
/// the same decision names, which a field's own reader cannot see. A number
/// there is refused: a technology is written by name.
fn name_technology(mut value: Value) -> Result<Value, String> {
    let Value::Mapping(fields) = &mut value else {
        return Ok(value);
    };
    if fields.get("type").and_then(Value::as_str) != Some("upgrade_technology") {
        return Ok(value);
    }
    let unit = fields
        .get("unit")
        .and_then(Value::as_str)
        .ok_or("upgrade_technology names no unit type")?;
    let unit = crate::catalog::unit_id_from_type(unit)
        .ok_or_else(|| format!("{unit} is not a unit type of this build"))?;
    let tech = fields
        .get("tech")
        .and_then(Value::as_str)
        .ok_or("upgrade_technology names no technology")?;
    let id = crate::names::technology_id::<serde_yaml::Error>(unit, tech)
        .map_err(|error| error.to_string())?;
    fields.insert("tech".into(), Value::Number(id.into()));
    Ok(value)
}

/// Segment framing has already been checked by `segments`; payload readers
/// reject unknown fields after removing those two framing keys.
///
/// A unit reinforcement card is named without its round, which the segment
/// states, so each such name in a state's offers or a reinforcement choice is
/// read as `name@round` before the fields are.
pub(crate) fn payload<T: serde::de::DeserializeOwned>(
    mut value: Value,
) -> Result<T, serde_yaml::Error> {
    if let Value::Mapping(fields) = &mut value {
        fields.remove(Value::String("kind".into()));
        let round = fields
            .remove(Value::String("round".into()))
            .and_then(|round| round.as_i64());
        if let Some(round) = round {
            within_round(fields, round);
        }
    }
    serde_yaml::from_value(value)
}

/// Appends `@round` to every unit card name a segment holds.
fn within_round(fields: &mut serde_yaml::Mapping, round: i64) {
    let qualify = |name: &mut Value| {
        if let Value::String(text) = name
            && crate::names::is_unit_card(text)
        {
            *text = format!("{text}@{round}");
        }
    };
    if let Some(Value::Sequence(offers)) = fields.get_mut("reinforce_offers") {
        offers.iter_mut().for_each(qualify);
    }
    for side in ["blue", "red"] {
        let Some(Value::Sequence(actions)) = fields.get_mut(side) else {
            continue;
        };
        for action in actions {
            if action.get("type").and_then(Value::as_str) == Some("choose_reinforce_item")
                && let Some(name) = action.get_mut("name")
            {
                qualify(name);
            }
        }
    }
}

/// A battle names a unit type by the name a layout gives it, never by its ID.
///
/// The document still orders unit types by ID, which is the order the game
/// lists them in, so the fields keep the ID and only their spelling is a name.
/// A name no unit type of this build carries is refused.
pub(crate) mod unit_names {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    fn name<E: serde::ser::Error>(id: i32) -> Result<&'static str, E> {
        crate::catalog::unit_type_from_id(id)
            .map(|(name, _)| name)
            .ok_or_else(|| E::custom(format!("unit ID {id} has no type name in this build")))
    }

    fn id<E: serde::de::Error>(name: &str) -> Result<i32, E> {
        crate::catalog::unit_id_from_type(name)
            .ok_or_else(|| E::custom(format!("{name} is not a unit type of this build")))
    }

    /// One unit type.
    pub(crate) mod unit {
        use super::{Deserialize, Deserializer, Serialize, Serializer};

        #[allow(clippy::trivially_copy_pass_by_ref)] // Required by serde's `with` shape.
        pub(crate) fn serialize<S: Serializer>(
            unit: &i32,
            serializer: S,
        ) -> Result<S::Ok, S::Error> {
            super::name(*unit)?.serialize(serializer)
        }

        pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
            deserializer: D,
        ) -> Result<i32, D::Error> {
            super::id(&String::deserialize(deserializer)?)
        }
    }

    /// A list of unit types, kept in the order it holds.
    pub(crate) mod units {
        use super::{Deserialize, Deserializer, Serializer};

        pub(crate) fn serialize<S: Serializer>(
            units: &[i32],
            serializer: S,
        ) -> Result<S::Ok, S::Error> {
            serializer.collect_seq(
                units
                    .iter()
                    .map(|unit| super::name(*unit))
                    .collect::<Result<Vec<_>, S::Error>>()?,
            )
        }

        pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
            deserializer: D,
        ) -> Result<Vec<i32>, D::Error> {
            Vec::<String>::deserialize(deserializer)?
                .iter()
                .map(|name| super::id(name))
                .collect()
        }
    }
}

#[allow(clippy::trivially_copy_pass_by_ref)] // Required by serde's predicate shape.
fn is_false(value: &bool) -> bool {
    !*value
}

/// One segment of a battle stream, tagged by the `kind` it opens with.
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Segment<'a> {
    Battle(Header<'a>),
    State(StateSegment<'a>),
    Action(ActionSegment<'a>),
}

#[derive(Serialize)]
struct Header<'a> {
    map_id: i32,
    seed: i32,
    sides: HeaderSides<'a>,
}

#[derive(Serialize)]
struct HeaderSides<'a> {
    blue: HeaderSide<'a>,
    red: HeaderSide<'a>,
}

#[derive(Serialize)]
struct HeaderSide<'a> {
    offers: &'a [OpeningOffer],
    constructions: &'a [StaticPlacement],
    #[serde(with = "crate::names::loadout")]
    tech_loadout: &'a BTreeMap<i32, Vec<i32>>,
}

impl<'a> HeaderSide<'a> {
    fn of(side: &'a BattleSide) -> Self {
        Self {
            offers: &side.opening.offers,
            constructions: &side.constructions,
            tech_loadout: &side.tech_loadout,
        }
    }
}

#[derive(Serialize)]
struct StateSegment<'a> {
    round: i32,
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::names::card::offers::serialize"
    )]
    reinforce_offers: Option<Cow<'a, [i32]>>,
    sides: &'a StateSides,
}

#[derive(Serialize)]
struct ActionSegment<'a> {
    round: i32,
    blue: Cow<'a, [Action]>,
    red: Cow<'a, [Action]>,
}

/// The line that separates two segments of a stream.
const SEPARATOR: &str = "---\n";

/// Serializes a battle as the stream `docs/spec/document/battle.md` defines,
/// in the normal form the battle, state and action documents define.
///
/// # Errors
///
/// Returns an error when a segment cannot be serialized.
pub fn canonical_yaml(battle: &Battle) -> Result<String, String> {
    let mut yaml = String::new();
    for (at, segment) in segments_of(battle).iter().enumerate() {
        if at > 0 {
            yaml.push_str(SEPARATOR);
        }
        let value = serde_yaml::to_value(segment)
            .map_err(|error| format!("cannot serialize battle YAML: {error}"))?;
        yaml.push_str(&crate::spelling::document(&value)?);
    }
    Ok(yaml)
}

/// A battle's segments, in stream order.
fn segments_of(battle: &Battle) -> Vec<Segment<'_>> {
    let mut segments = vec![
        Segment::Battle(Header {
            map_id: battle.map_id,
            seed: battle.seed,
            sides: HeaderSides {
                blue: HeaderSide::of(&battle.sides.blue),
                red: HeaderSide::of(&battle.sides.red),
            },
        }),
        Segment::Action(ActionSegment {
            round: 0,
            blue: Cow::Owned(vec![battle.sides.blue.opening.action()]),
            red: Cow::Owned(vec![battle.sides.red.opening.action()]),
        }),
    ];
    for turn in &battle.turns {
        segments.push(Segment::State(StateSegment {
            round: turn.round,
            reinforce_offers: turn.state.reinforce_offers.as_deref().map(Cow::Borrowed),
            sides: &turn.state.sides,
        }));
        segments.push(Segment::Action(ActionSegment {
            round: turn.round,
            blue: Cow::Borrowed(&turn.actions.blue),
            red: Cow::Borrowed(&turn.actions.red),
        }));
    }
    segments
}

/// A battle stream read as the segments it is made of, each checked for its
/// place in the sequence and otherwise left as YAML.
///
/// A reader that needs only part of a battle deserializes only that part, so
/// what this checks is the grammar: which segment may follow which, and where
/// the stream has to stop.
#[derive(Debug)]
pub struct Segments {
    pub header: Value,
    /// The round-zero action segment, absent from a stream that holds only its
    /// header.
    pub opening: Option<Value>,
    pub rounds: Vec<RoundSegments>,
}

/// One deployment round: its state, and its decisions once they are known.
#[derive(Debug)]
pub struct RoundSegments {
    pub round: i32,
    pub state: Value,
    /// Absent only on the stream's last round, whose decisions are not yet
    /// stated.
    pub actions: Option<Value>,
}

/// Reads a battle stream into its segments, or nothing when the file is not a
/// battle.
///
/// A file is a battle when its first document says `kind: battle`. After that
/// the stream alternates strictly: round zero's action segment, then each
/// round's state followed by that round's actions. It may end after any
/// segment, except that nothing follows a concession or a state in which a
/// reactor core has fallen to zero.
///
/// # Errors
///
/// Returns an error when a battle's segments are out of order, misnumbered,
/// continue past the end of the match, or state a release in a round's
/// opening position.
pub fn segments(bytes: &[u8]) -> Result<Option<Segments>, String> {
    let mut documents = serde_yaml::Deserializer::from_slice(bytes);
    let Some(header) = documents
        .next()
        .and_then(|first| Value::deserialize(first).ok())
    else {
        return Ok(None);
    };
    if kind_of(&header) != Some("battle") {
        return Ok(None);
    }
    let mut stream = Segments {
        header,
        opening: None,
        rounds: Vec::new(),
    };
    let mut ended: Option<&str> = None;
    for (at, document) in documents.enumerate() {
        let position = at + 2;
        let segment = Value::deserialize(document)
            .map_err(|error| format!("battle segment {position} is not YAML: {error}"))?;
        if let Some(end) = ended {
            return Err(format!("battle segment {position} follows {end}"));
        }
        let kind = kind_of(&segment)
            .ok_or_else(|| format!("battle segment {position} names no kind"))?
            .to_owned();
        let round = segment
            .get("round")
            .and_then(Value::as_i64)
            .and_then(|round| i32::try_from(round).ok())
            .ok_or_else(|| format!("battle segment {position} ({kind}) names no round"))?;
        let (expected_kind, expected_round) = match (&stream.opening, stream.rounds.last()) {
            (None, _) => ("action", 0),
            (Some(_), None) => ("state", 1),
            (Some(_), Some(last)) if last.actions.is_none() => ("action", last.round),
            (Some(_), Some(last)) => ("state", last.round + 1),
        };
        if kind != expected_kind || round != expected_round {
            return Err(format!(
                "battle segment {position} is {kind} round {round}; \
                 the stream expects {expected_kind} round {expected_round}"
            ));
        }
        if kind == "state" {
            if released(&segment) {
                return Err(format!(
                    "round {round} state carries a release or a used skill; a battle \
                     states the position a round opens with, before any decision"
                ));
            }
            if core_destroyed(&segment) {
                ended = Some("a destroyed reactor core");
            }
            stream.rounds.push(RoundSegments {
                round,
                state: segment,
                actions: None,
            });
        } else {
            if conceded(&segment, round)? {
                ended = Some("a concession");
            }
            match stream.rounds.last_mut() {
                None => stream.opening = Some(segment),
                Some(last) => last.actions = Some(segment),
            }
        }
    }
    Ok(Some(stream))
}

fn kind_of(segment: &Value) -> Option<&str> {
    segment.get("kind").and_then(Value::as_str)
}

fn each_side(segment: &Value) -> impl Iterator<Item = (&'static str, &Value)> {
    ["blue", "red"]
        .into_iter()
        .filter_map(move |side| Some((side, segment.get(side)?)))
}

fn state_sides(segment: &Value) -> impl Iterator<Item = &Value> {
    ["blue", "red"]
        .into_iter()
        .filter_map(move |side| segment.get("sides")?.get(side))
}

fn released(state: &Value) -> bool {
    state_sides(state).any(|side| {
        side.get("battle_skills")
            .and_then(Value::as_sequence)
            .is_some_and(|panel| {
                panel
                    .iter()
                    .any(|slot| slot.get("release").is_some() || slot.get("used").is_some())
            })
    })
}

fn core_destroyed(state: &Value) -> bool {
    state_sides(state).any(|side| {
        side.get("reactor_core")
            .and_then(Value::as_i64)
            .is_some_and(|core| core <= 0)
    })
}

/// Whether a side conceded in this action segment, refusing a concession that
/// is not its side's last decision.
fn conceded(actions: &Value, round: i32) -> Result<bool, String> {
    let mut any = false;
    for (side, list) in each_side(actions) {
        let Some(list) = list.as_sequence() else {
            continue;
        };
        let concede =
            |action: &Value| action.get("type").and_then(Value::as_str) == Some("concede");
        if let Some(at) = list.iter().position(concede) {
            if at + 1 != list.len() {
                return Err(format!(
                    "round {round} {side} decides after conceding; a concession is \
                     its side's last decision"
                ));
            }
            any = true;
        }
    }
    Ok(any)
}

#[cfg(test)]
mod tests {
    #[test]
    fn actions_read_targets_and_refuse_lost_operands() {
        for target in [
            "{area: [{x: 10, y: -20}]}",
            "{unit: 4}",
            "{construction: 2}",
        ] {
            let yaml = format!(
                "{{type: release_commander_skill, index: 3, name: missile_strike, target: {target}}}"
            );
            let action: super::Action = serde_yaml::from_str(&yaml).unwrap();
            let spelled = serde_yaml::to_string(&action).unwrap();
            assert!(!spelled.contains('!'), "{spelled}");
            assert_eq!(
                serde_yaml::from_str::<super::Action>(&spelled).unwrap(),
                action
            );
        }
        for yaml in [
            "{type: buy_unit, name: marksman}",
            "{type: choose_advance_team, offer: 1, name: vortex-fire_badger}",
            "{type: release_commander_skill, index: 0, target: {unit: 4}}",
            "{type: release_commander_skill, index: 0, name: missile_strike, target: !unit 4}",
            "{type: move_unit, index: 0, position: {x: 0, y: 0}, rotated: yes}",
            "{type: upgrade_unit, index: 0, typo: 1}",
            "{type: release_commander_skill, index: 0, name: missile_strike, target: {unknown: 1}}",
            "{type: unknown_action}",
        ] {
            assert!(
                serde_yaml::from_str::<super::Action>(yaml).is_err(),
                "{yaml}"
            );
        }
    }

    /// A unit type is written by name and ordered by ID: Vortex is unit 31
    /// and Mountain unit 2002, the reverse of their names' order. A number,
    /// or a name that is not a unit type, is refused.
    #[test]
    fn unit_types_are_names_in_id_order() {
        #[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq)]
        struct Loadout {
            #[serde(with = "crate::names::loadout")]
            rows: std::collections::BTreeMap<i32, Vec<i32>>,
        }
        let loadout = Loadout {
            rows: [(2002, vec![72_002]), (31, vec![631])].into(),
        };
        let yaml = serde_yaml::to_string(&loadout).unwrap();
        assert_eq!(
            yaml,
            "rows:\n  vortex:\n  - grid_integration\n  mountain:\n  - saturation_bombardment\n"
        );
        assert_eq!(serde_yaml::from_str::<Loadout>(&yaml).unwrap(), loadout);
        for yaml in [
            "{type: unlock_unit, name: 9}",
            "{type: unlock_unit, name: defensive_wall}",
            "{type: buy_unit, name: death_knell, position: {x: 0, y: -160}}",
        ] {
            assert!(
                serde_yaml::from_str::<super::Action>(yaml).is_err(),
                "{yaml}"
            );
        }
    }

    /// Every decision reads back what it writes. The match has no wildcard,
    /// so a decision added to [`super::Action`] stops this compiling until it
    /// is sampled here, and [`super::ActionReader`] is the other place it goes.
    #[test]
    fn every_action_reads_back_what_it_writes() {
        use super::{Action, SkillTarget};
        use crate::layout::Position;
        let at = Position { x: 10, y: -20 };
        let samples = [
            Action::ChooseReinforceItem {
                offer: -1,
                id: None,
            },
            Action::ChooseReinforceItem {
                offer: 2,
                id: Some(1_305_003),
            },
            Action::ChooseAdvanceTeam {
                offer: 1,
                id: 9910,
                specialist: 20005,
            },
            Action::BuyUnit {
                unit: 2,
                position: at,
                rotated: true,
            },
            Action::UpgradeUnit { index: 0 },
            Action::UnlockUnit { unit: 9 },
            Action::UpgradeTechnology { unit: 2, tech: 702 },
            Action::ActiveBlueprint { id: 5 },
            Action::ActiveEnergyTowerSkill { skill: 1 },
            Action::StrengthenTower { tower: 1 },
            Action::UseEquipment {
                equipment: 13_030_001,
                index: 0,
            },
            Action::MoveUnit {
                index: 0,
                position: at,
                rotated: true,
            },
            Action::ReleaseCommanderSkill {
                index: 0,
                id: 300_001,
                target: SkillTarget::Area(vec![at]),
            },
            Action::ReleaseCommanderSkill {
                index: 0,
                id: 300_001,
                target: SkillTarget::Unit(4),
            },
            Action::ReleaseCommanderSkill {
                index: 0,
                id: 300_001,
                target: SkillTarget::Construction(2),
            },
            Action::ReleaseContraption {
                contraption: 10_001,
                position: at,
                extra_position: Some(at),
            },
            Action::Concede,
        ];
        for action in samples {
            match action {
                Action::ChooseReinforceItem { .. }
                | Action::ChooseAdvanceTeam { .. }
                | Action::BuyUnit { .. }
                | Action::UpgradeUnit { .. }
                | Action::UnlockUnit { .. }
                | Action::UpgradeTechnology { .. }
                | Action::ActiveBlueprint { .. }
                | Action::ActiveEnergyTowerSkill { .. }
                | Action::StrengthenTower { .. }
                | Action::UseEquipment { .. }
                | Action::MoveUnit { .. }
                | Action::ReleaseCommanderSkill { .. }
                | Action::ReleaseContraption { .. }
                | Action::Concede => {}
            }
            let value = serde_yaml::to_value(&action).unwrap();
            let spelled = crate::spelling::document(&value).unwrap();
            assert_eq!(
                serde_yaml::from_str::<Action>(&spelled).unwrap(),
                action,
                "{spelled}"
            );
        }
    }

    #[test]
    fn a_state_refuses_a_field_it_does_not_define() {
        let formation = "{name: crawler, index: 0, position: {x: 0, y: -160}, value: 100}";
        let read =
            |formation: &str| serde_yaml::from_str::<super::StateUnit>(formation).map(|_| ());
        read(formation).unwrap();
        for broken in [
            formation.replace("value: 100", "value: 100, typo: 1"),
            formation.replace("value: 100", "value: 100, movable: yes"),
            formation.replace(", index: 0", ""),
        ] {
            assert!(read(&broken).is_err(), "{broken}");
        }
    }

    use super::segments;

    const HEADER: &str = "kind: battle\nmap_id: 1021\nseed: 1\n";
    const OPENING: &str = "---\nkind: action\nround: 0\nblue: []\nred: []\n";

    fn state(round: i32, blue_core: i32) -> String {
        format!(
            "---\nkind: state\nround: {round}\nsides:\n  blue: {{reactor_core: {blue_core}}}\n  red: {{reactor_core: 4800}}\n"
        )
    }

    fn actions(round: i32, red: &str) -> String {
        format!("---\nkind: action\nround: {round}\nblue: []\nred: {red}\n")
    }

    fn read(stream: &str) -> Result<super::Segments, String> {
        segments(stream.as_bytes()).map(|read| read.expect("a battle stream"))
    }

    fn read_ok(stream: &str) -> bool {
        read(stream).is_ok()
    }

    /// A stream may stop after any segment, which is what lets a later reader
    /// append the next one.
    #[test]
    fn a_stream_may_end_after_any_segment() {
        let full = format!(
            "{HEADER}{OPENING}{}{}{}",
            state(1, 4800),
            actions(1, "[]"),
            state(2, 4800)
        );
        let read = read(&full).unwrap();
        assert!(read.opening.is_some());
        assert_eq!(read.rounds.len(), 2);
        assert!(read.rounds[0].actions.is_some());
        assert!(read.rounds[1].actions.is_none());
        assert!(super::segments(HEADER.as_bytes()).unwrap().is_some());
        let ends_on_actions = format!("{HEADER}{OPENING}{}{}", state(1, 4800), actions(1, "[]"));
        assert!(read_ok(&ends_on_actions));
    }

    #[test]
    fn segments_alternate_and_count_up() {
        let skipped = format!("{HEADER}{OPENING}{}", state(2, 4800));
        assert!(
            read(&skipped)
                .unwrap_err()
                .contains("the stream expects state round 1")
        );
        let doubled = format!("{HEADER}{OPENING}{}{}", state(1, 4800), state(2, 4800));
        assert!(
            read(&doubled)
                .unwrap_err()
                .contains("the stream expects action round 1")
        );
        let unopened = format!("{HEADER}{}", state(1, 4800));
        assert!(
            read(&unopened)
                .unwrap_err()
                .contains("the stream expects action round 0")
        );
    }

    #[test]
    fn nothing_follows_a_concession() {
        let conceded = format!(
            "{HEADER}{OPENING}{}{}",
            state(1, 4800),
            actions(1, "[{type: concede}]")
        );
        assert!(read_ok(&conceded));
        let continued = format!("{conceded}{}", state(2, 4800));
        assert!(
            read(&continued)
                .unwrap_err()
                .contains("follows a concession")
        );
        let decided_after = format!(
            "{HEADER}{OPENING}{}{}",
            state(1, 4800),
            actions(1, "[{type: concede}, {type: unlock_unit, unit: 9}]")
        );
        assert!(
            read(&decided_after)
                .unwrap_err()
                .contains("round 1 red decides after conceding")
        );
    }

    #[test]
    fn nothing_follows_a_destroyed_reactor_core() {
        let ended = format!(
            "{HEADER}{OPENING}{}{}{}",
            state(1, 4800),
            actions(1, "[]"),
            state(2, 0)
        );
        assert!(read_ok(&ended));
        let continued = format!("{ended}{}", actions(2, "[]"));
        assert!(
            read(&continued)
                .unwrap_err()
                .contains("follows a destroyed reactor core")
        );
    }

    /// A state segment is the position a round opens with, so no decision of
    /// that round can have left a release on its panel.
    #[test]
    fn a_state_segment_carries_no_release() {
        let released = format!(
            "{HEADER}{OPENING}---\nkind: state\nround: 1\nsides:\n  blue:\n    battle_skills:\n    - {{index: 0, id: 900001, cooldown: 0, release: {{order: 1, target: {{unit: 4}}}}}}\n"
        );
        assert!(
            read(&released)
                .unwrap_err()
                .contains("round 1 state carries a release")
        );
    }

    /// The spelling follows the three rules and says what the value says.
    #[test]
    fn a_segment_is_spelled_by_shape() {
        let block = "kind: state\nround: 3\nreinforce_offers:\n- 1033115\n- 1031122\nsides:\n  blue:\n    shop:\n      unlocked_units:\n      - 2\n      - 10\n      buys_remaining: 2\n    next_index:\n      unit: 7\n      contraption: 0\n    units:\n    - type: vortex\n      index: 0\n      position:\n        x: -120\n        y: -100\n    constructions: []\n  red:\n    units:\n    - type: release_commander_skill\n      target:\n        area:\n        - x: 197\n          y: -40\n    - type: concede\n";
        let value: serde_yaml::Value = serde_yaml::from_str(block).unwrap();
        let spelled = crate::spelling::document(&value).unwrap();
        assert_eq!(
            spelled,
            "kind: state\nround: 3\nreinforce_offers: [1033115, 1031122]\nsides:\n  blue:\n\
             \x20   shop:\n      unlocked_units: [2, 10]\n      buys_remaining: 2\n\
             \x20   next_index: {unit: 7, contraption: 0}\n    units:\n\
             \x20   - {type: vortex, index: 0, position: {x: -120, y: -100}}\n\
             \x20   constructions: []\n  red:\n    units:\n\
             \x20   - {type: release_commander_skill, target: {area: [{x: 197, y: -40}]}}\n\
             \x20   - {type: concede}\n"
        );
        assert_eq!(
            serde_yaml::from_str::<serde_yaml::Value>(&spelled).unwrap(),
            value
        );
    }

    /// Integer keys, as a tech loadout has, are written bare.
    #[test]
    fn a_tech_loadout_row_is_one_line() {
        let value: serde_yaml::Value = serde_yaml::from_str(
            "sides:\n  blue:\n    tech_loadout:\n      1: [1001, 1105]\n      2001: [32001]\n",
        )
        .unwrap();
        assert_eq!(
            crate::spelling::document(&value).unwrap(),
            "sides:\n  blue:\n    tech_loadout:\n      1: [1001, 1105]\n      2001: [32001]\n"
        );
    }

    /// A string the reader would take for another type keeps its quotes.
    #[test]
    fn a_string_that_reads_as_another_type_is_quoted() {
        for (value, written) in [
            ("marksman", "marksman"),
            ("y", "y"),
            ("true", "\"true\""),
            ("null", "\"null\""),
            ("two words", "\"two words\""),
            ("124/450", "124/450"),
            ("12", "\"12\""),
        ] {
            let mut out = String::new();
            crate::spelling::flow(&serde_yaml::Value::from(value), &mut out).unwrap();
            assert_eq!(out, written);
        }
    }

    /// Every segment of every tracked battle reads back as the value it was
    /// spelled from.
    #[cfg(feature = "convert")]
    #[test]
    fn every_tracked_segment_folds_without_changing_its_content() {
        let mut folded = 0;
        for entry in std::fs::read_dir("../../tests/grbr").unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|extension| extension != "grbr") {
                continue;
            }
            let Ok(battle) = crate::convert::battle_from_grbr(&std::fs::read(&path).unwrap())
            else {
                continue;
            };
            for segment in super::segments_of(&battle) {
                let value = serde_yaml::to_value(&segment).unwrap();
                let spelled = crate::spelling::document(&value).unwrap();
                let read: serde_yaml::Value = serde_yaml::from_str(&spelled).unwrap();
                assert_eq!(read, value, "{}", path.display());
                folded += 1;
            }
        }
        assert_eq!(folded, 41 * 2 + 334 * 2);
    }

    #[test]
    fn only_a_battle_header_opens_a_battle() {
        assert!(segments(b"kind: layout\nsides: {}\n").unwrap().is_none());
        assert!(segments(b"not: [a document").unwrap().is_none());
        assert!(segments(b"").unwrap().is_none());
    }
}
