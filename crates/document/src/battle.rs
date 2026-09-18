//! The battle document and the state and action segments it is made of.
//!
//! `docs/spec/document/battle.md` defines a battle as a stream of YAML
//! documents: a header, the opening's decisions, and then each round's opening
//! state followed by the decisions taken from it. `docs/spec/document/state.md`
//! and `docs/spec/document/action.md` define the two segment shapes. Filling
//! one from a replay is [`crate::convert`].

use crate::layout::{ContraptionPlacement, Formation, Position, StaticPlacement, Techs, Terrain};
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
    /// Which technologies each unit may research, keyed by unit ID.
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
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct OpeningOffer {
    pub team: i32,
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
            specialist: Some(taken.specialist),
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
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct State {
    /// Absent in rounds 0 and 1, which are dealt no reinforcement offer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reinforce_offers: Option<Vec<i32>>,
    pub sides: StateSides,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct StateSides {
    pub blue: SideState,
    pub red: SideState,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq, Eq)]
pub struct SideState {
    pub reactor_core: i32,
    pub supply: i32,
    pub shop: ShopState,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub blueprints: Vec<i32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub energy_tower_skills: Vec<i32>,
    pub tower_strengthen_levels: Vec<i32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub equipment: Vec<EquipmentItem>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub battle_skills: Vec<PanelSkill>,
    pub next_index: NextIndex,
    pub techs: Techs,
    pub formations: Vec<StateFormation>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub constructions: Vec<StaticPlacement>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub contraptions: Vec<ContraptionPlacement>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub airdrop_shields: Vec<Position>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub terrains: Vec<Terrain>,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq, Eq)]
pub struct ShopState {
    pub unlocked_units: Vec<i32>,
    pub buys_remaining: i32,
    pub unlocks_remaining: i32,
}

/// A formation, and what recovering it pays back.
///
/// `value` is not a function of the unit's type and level. It is what the side
/// actually paid, at the prices its officers made at the time, so two identical
/// looking formations bought a round apart can be worth different amounts.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct StateFormation {
    #[serde(flatten)]
    pub formation: Formation,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<i32>,
}

/// An owned item no formation carries; a fitted one is named by its formation.
#[derive(Clone, Debug, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct EquipmentItem {
    pub id: i32,
    /// Absent means `-1`, which is every item a standard 1v1 hands out.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub durability: Option<i32>,
}

/// One commander skill panel slot.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct PanelSkill {
    pub index: i32,
    pub id: i32,
    pub cooldown: i32,
    /// True on a deployment skill this round used.
    ///
    /// A deployment skill does its work before the fight, as a change to the
    /// position, so the slot records that it was spent and nothing else: no
    /// target, no place in the release order, and no entry in a layout. A
    /// round's opening position carries none.
    #[serde(skip_serializing_if = "is_false")]
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
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Release {
    pub order: i32,
    pub target: SkillTarget,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq, Eq)]
pub struct NextIndex {
    pub unit: i32,
    pub contraption: i32,
}

/// Each side's decisions in one round, which a battle writes as an action
/// segment.
#[derive(Debug, Serialize, PartialEq, Eq)]
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
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<i32>,
    },
    /// The opening, which is one decision with two halves: the team of
    /// formations and the specialist officer bound to it.
    ///
    /// It is the only decision of round zero, and no other round holds one.
    ChooseAdvanceTeam {
        offer: i32,
        id: i32,
        #[serde(skip_serializing_if = "Option::is_none")]
        specialist: Option<i32>,
    },
    BuyUnit {
        unit: i32,
        position: Position,
    },
    UpgradeUnit {
        index: i32,
    },
    UnlockUnit {
        unit: i32,
    },
    UpgradeTechnology {
        unit: i32,
        tech: i32,
    },
    ActiveBlueprint {
        id: i32,
    },
    ActiveEnergyTowerSkill {
        skill: i32,
    },
    StrengthenTower {
        tower: i32,
    },
    UseEquipment {
        equipment: i32,
        unit: i32,
    },
    MoveUnit {
        index: i32,
        position: Position,
        #[serde(skip_serializing_if = "is_false")]
        rotated: bool,
    },
    ReleaseCommanderSkill {
        skill: i32,
        target: SkillTarget,
    },
    ReleaseContraption {
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

/// A release covers an area or points at one object, never both.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkillTarget {
    Area(Vec<Position>),
    Unit(i32),
    Construction(i32),
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
    #[serde(skip_serializing_if = "Option::is_none")]
    reinforce_offers: Option<&'a Vec<i32>>,
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
            reinforce_offers: turn.state.reinforce_offers.as_ref(),
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
            "{HEADER}{OPENING}---\nkind: state\nround: 1\nsides:\n  blue:\n    battle_skills:\n    - {{index: 0, id: 900001, cooldown: 0, release: {{order: 1, target: !unit 4}}}}\n"
        );
        assert!(
            read(&released)
                .unwrap_err()
                .contains("round 1 state carries a release")
        );
    }

    /// The spelling follows the three rules and says what the value says, a
    /// tagged release target included.
    #[test]
    fn a_segment_is_spelled_by_shape() {
        let block = "kind: state\nround: 3\nreinforce_offers:\n- 1033115\n- 1031122\nsides:\n  blue:\n    shop:\n      unlocked_units:\n      - 2\n      - 10\n      buys_remaining: 2\n    next_index:\n      unit: 7\n      contraption: 0\n    formations:\n    - type: vortex\n      index: 0\n      position:\n        x: -120\n        y: -100\n    constructions: []\n  red:\n    formations:\n    - type: release_commander_skill\n      target: !area\n      - x: 197\n        y: -40\n    - type: concede\n";
        let value: serde_yaml::Value = serde_yaml::from_str(block).unwrap();
        let spelled = crate::spelling::document(&value).unwrap();
        assert_eq!(
            spelled,
            "kind: state\nround: 3\nreinforce_offers: [1033115, 1031122]\nsides:\n  blue:\n\
             \x20   shop:\n      unlocked_units: [2, 10]\n      buys_remaining: 2\n\
             \x20   next_index: {unit: 7, contraption: 0}\n    formations:\n\
             \x20   - {type: vortex, index: 0, position: {x: -120, y: -100}}\n\
             \x20   constructions: []\n  red:\n    formations:\n\
             \x20   - {type: release_commander_skill, target: !area [{x: 197, y: -40}]}\n\
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
