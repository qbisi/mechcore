//! The match document and the state and action segments it is made of.
//!
//! `docs/spec/document/match.md` defines a match as a stream of YAML
//! documents: a header, the opening's decisions, and then each round's opening
//! state followed by the decisions taken from it. `docs/spec/document/state.md`
//! and `docs/spec/document/action.md` define the deployment segment shapes.
//! An optional final result closes the score without another round's state.
//! Filling one from a replay is [`crate::convert`].

use crate::layout::{
    ContraptionPlacement, Experience, Position, Standing, StaticPlacement, UnitPlacement,
    UnitSource,
};
pub use crate::names::OfferEntry;
use serde::{Deserialize, Serialize};
use serde_yaml::Value;
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

/// One recorded match, as `docs/spec/document/match.md` defines it.
///
/// This is the match held whole. Its document is a stream, and
/// [`canonical_yaml`] writes it as one: the header from `map_id`, `seed` and
/// `sides`, the opening's decisions from each side's [`Opening`], and a state
/// segment and an action segment per turn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Match {
    /// Whether the stream includes round zero; false for a captured suffix.
    pub has_opening: bool,
    /// The build whose tables this match is written against.
    pub game_build: String,
    pub source: Source,
    pub reinforce: bool,
    pub max_round: i32,
    pub map_id: i32,
    pub seed: i32,
    pub blue: MatchSide,
    pub red: MatchSide,
    /// The contiguous deployment rounds, starting at the first captured one.
    /// The opening is round zero and has its own opening state segment holding
    /// each side's offers and initial constructions.
    pub turns: Vec<Turn>,
    pub result: Option<MatchResult>,
}

/// How compactable fields (`tech_loadout` and `reinforce_pool`) are written in a match.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MatchFormat {
    #[default]
    Compact,
    Expanded,
}

/// Battle backend that produced a match; transcoding preserves it.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Game,
    Simulator,
}

impl Source {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Game => "game",
            Self::Simulator => "simulator",
        }
    }
}

/// Maximum researched technologies for one unit type on one side.
#[must_use]
pub const fn research_limit(unit: i32) -> usize {
    if matches!(unit, 17 | 29 | 2002) { 6 } else { 4 }
}

/// The final score, independent of any next-round opening.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MatchResult {
    pub round: i32,
    pub winner: Winner,
    pub blue: FinalCore,
    pub red: FinalCore,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Winner {
    Blue,
    Red,
    Draw,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FinalCore {
    pub reactor_core: i32,
}

impl Winner {
    #[must_use]
    pub const fn from_cores(blue: i32, red: i32) -> Option<Self> {
        match (blue <= 0, red <= 0) {
            (false, true) => Some(Self::Blue),
            (true, false) => Some(Self::Red),
            (true, true) => Some(Self::Draw),
            (false, false) => None,
        }
    }
}

/// A match from another game version is readable under the current catalogues,
/// but operations do not recover that version's historical rules.
#[must_use]
pub fn build_warning(build: &str) -> Option<String> {
    (build != crate::game_build()).then(|| format!(
        "match build {build} differs from this binary's game build {}. Operations use the current catalogues and rules; projected layouts and battle results may differ from the original game.",
        crate::game_build()
    ))
}

/// What a side holds for the whole match rather than for one round.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatchSide {
    /// The player or bot display name; the seat remains blue or red.
    pub name: String,
    /// Historical 1v1 matchmaking rating captured in the source replay.
    pub elo_score: Option<i32>,
    /// The opening this side was dealt and the one it took.
    pub opening: Opening,
    /// The construction layout the map dealt this side before the first round.
    ///
    /// A state's own `constructions` is the live list, shortened when a
    /// building is recovered or destroyed. This one is what the side started
    /// with, so a match says where the buildings came from rather than having
    /// them appear in the first round it happens to hold.
    pub constructions: Vec<StaticPlacement>,
    /// Which technologies each unit may research, keyed by unit ID and
    /// written keyed by type name, in ID order. Only units a standard 1v1
    /// match can field have a row.
    pub tech_loadout: BTreeMap<i32, Vec<i32>>,
    /// Round zero's starting reactor core, recorded in `state(0)` before the
    /// opening choice's `core_modify`. Absent in a partial match without opening.
    pub reactor_core: Option<i32>,
    /// The seed of the side's own stream, `PlayerRecord.seed`, recorded in
    /// `state(0)`, which an officer that draws its hand-out draws from. Absent,
    /// such a hand-out cannot be predicted.
    pub seed: Option<i32>,
}

/// The opening a side was dealt, and which of it the side took.
///
/// The round-zero state segment holds the four combinations (`offers`), and the
/// round-zero action segment holds the choice (`choose`). They are kept
/// together here because every reader of one needs the other.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Opening {
    /// Zero-based index of the combination taken from `offers`, absent while
    /// the side has not taken one.
    ///
    /// A played match always names it. A match in progress may not: a side
    /// deals before either player opens, and one player may open before the
    /// other. The round-zero action segment writes an empty list for a side
    /// that has not.
    pub choose: Option<i32>,
    /// The four combinations this side was dealt, in the order shown.
    ///
    /// Conversion reconstructs it from the replay's random state.
    pub offers: Vec<OpeningOffer>,
}

/// One of the openings a side was dealt: a team of formations, the
/// specialist officer bound to it, and the net `core_modify` applied on pick.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OpeningOffer {
    pub team: i32,
    pub specialist: i32,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct OpeningOfferWire {
    /// Written as the team's name, its two unit types.
    #[serde(with = "crate::names::advance_team::one")]
    team: i32,
    #[serde(with = "crate::names::officer::one")]
    specialist: i32,
    core_modify: i32,
}

impl OpeningOffer {
    /// Reactor core delta from choosing this team and specialist together.
    ///
    /// # Errors
    ///
    /// Returns an error if the team or specialist ID is absent from the catalogue.
    pub fn core_modify(self) -> Result<i32, String> {
        let economy = crate::economy::Economy::embedded()?;
        let team = economy
            .advance_team(self.team)
            .ok_or_else(|| format!("unknown advance team {}", self.team))?;
        let specialist = economy
            .advance_team(self.specialist)
            .ok_or_else(|| format!("unknown specialist {}", self.specialist))?;
        Ok(team.reactor_core + specialist.reactor_core)
    }
}

impl Serialize for OpeningOffer {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::Error;
        let core_modify = self.core_modify().map_err(S::Error::custom)?;
        OpeningOfferWire {
            team: self.team,
            specialist: self.specialist,
            core_modify,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for OpeningOffer {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let wire = OpeningOfferWire::deserialize(deserializer)?;
        let offer = Self {
            team: wire.team,
            specialist: wire.specialist,
        };
        let expected = offer.core_modify().map_err(D::Error::custom)?;
        if wire.core_modify != expected {
            return Err(D::Error::custom(format!(
                "opening offer core_modify is {}, expected {expected}",
                wire.core_modify
            )));
        }
        Ok(offer)
    }
}

impl Opening {
    /// The opening as the decision that took it, which is what the round-zero
    /// action segment holds, or nothing while the side has not taken one.
    ///
    /// # Panics
    ///
    /// Panics if `choose` is not an index into `offers`. Conversion checks this
    /// before constructing the opening, and [`read`] checks it when reading
    /// one back.
    #[must_use]
    pub fn action(&self) -> Option<Action> {
        let choose = self.choose?;
        let taken = usize::try_from(choose)
            .ok()
            .and_then(|at| self.offers.get(at))
            .expect("opening choice must name a dealt combination");
        Some(Action::ChooseAdvanceTeam {
            index: choose,
            id: taken.team,
            specialist: taken.specialist,
        })
    }
}

/// One deployment round: the state it opens with and the decisions taken from
/// it, which the stream writes as a state segment and an action segment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Turn {
    pub round: i32,
    pub state: State,
    pub actions: TurnActions,
}

/// A match position, which a match writes as a state segment.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct State {
    /// The shared reinforcement stream after this round's deal.
    pub random_state: [u64; 4],
    /// Effective changes to the ordinary pool initialized by the header seed.
    #[serde(default, skip_serializing_if = "PoolDelta::is_empty")]
    pub reinforce_pool: PoolDelta,
    /// Absent in rounds 0 and 1, which are dealt no reinforcement offer.
    /// Written as structured offer entries, ending with the supply decline.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::names::card::offers"
    )]
    pub reinforce_offers: Option<Offers>,
    pub blue: SideState,
    pub red: SideState,
}

impl State {
    /// Installs the round's deal before either side has answered it.
    pub fn set_reinforce_offers(&mut self, offers: Option<Offers>) {
        self.reinforce_offers = offers;
        self.open_reinforcement();
    }

    /// Populates each side's opening reinforcement status from the round's offers.
    pub(crate) fn open_reinforcement(&mut self) {
        let choice = self.reinforce_offers.as_ref().map(|_| Offers::UNANSWERED);
        self.blue.reinforce_choice = choice;
        self.red.reinforce_choice = choice;
    }

    pub(crate) fn check_reinforcement(&self, opening: bool) -> Result<(), String> {
        let offers = self.reinforce_offers.as_ref();
        for (name, side) in [("blue", &self.blue), ("red", &self.red)] {
            let valid = match (offers, side.reinforce_choice) {
                (_, Some(_)) if opening => false,
                (_, None) if opening => true,
                (None, None) | (Some(_), None | Some(Offers::UNANSWERED)) => true,
                (Some(offers), Some(index)) => offers.choice(index).is_some(),
                _ => false,
            };
            if !valid {
                return Err(format!(
                    "{name} reinforce_choice disagrees with the round's offers or phase"
                ));
            }
        }
        Ok(())
    }

    pub(crate) fn check_towers(&self, require_present: bool) -> Result<(), String> {
        for (name, side) in [("blue", &self.blue), ("red", &self.red)] {
            if !require_present && side.tower_strengthen_levels.is_empty() {
                continue;
            }
            if side.tower_strengthen_levels.len() != crate::layout::TOWER_COUNT {
                return Err(format!(
                    "{name} tower_strengthen_levels must hold {} levels, not {}",
                    crate::layout::TOWER_COUNT,
                    side.tower_strengthen_levels.len()
                ));
            }
            for (index, &level) in side.tower_strengthen_levels.iter().enumerate() {
                if !(0..=crate::layout::MAX_TOWER_STRENGTHEN_LEVEL).contains(&level) {
                    return Err(format!(
                        "{name} tower_strengthen_levels[{index}] must be 0..={}",
                        crate::layout::MAX_TOWER_STRENGTHEN_LEVEL
                    ));
                }
            }
        }
        Ok(())
    }
}

/// Current membership relative to the seed's initial ordinary reinforcement pool.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PoolDelta {
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        with = "crate::names::card::many"
    )]
    pub removed: Vec<i32>,
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        with = "crate::names::card::many"
    )]
    pub added: Vec<i32>,
}

impl PoolDelta {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.removed.is_empty() && self.added.is_empty()
    }
}

/// What a round offers both sides: the cards it dealt, in order, and the
/// decline, which it always offers after them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Offers {
    pub dealt: Vec<i32>,
    /// What declining pays: the map's figure, or in a unit round the
    /// schedule's for that round.
    pub refund: i32,
}

impl Serialize for Offers {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        crate::names::card::offers::write(self, serializer)
    }
}

impl Offers {
    /// A side has been dealt an offer and has yet to answer it.
    pub const UNANSWERED: i32 = -1;

    /// The decline's position, after every card dealt.
    #[must_use]
    pub fn decline_index(&self) -> i32 {
        i32::try_from(self.dealt.len()).unwrap_or(i32::MAX)
    }

    #[must_use]
    pub fn choice(&self, index: i32) -> Option<ReinforceChoice> {
        if index == self.decline_index() {
            Some(ReinforceChoice::Refund(self.refund))
        } else {
            let index = usize::try_from(index).ok()?;
            self.dealt.get(index).copied().map(ReinforceChoice::Card)
        }
    }
}

/// A reinforcement answer names a catalogue card or the current refund amount.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReinforceChoice {
    Card(i32),
    Refund(i32),
}

impl ReinforceChoice {
    #[must_use]
    pub const fn card(self) -> Option<i32> {
        match self {
            Self::Card(id) => Some(id),
            Self::Refund(_) => None,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SideState {
    /// The player stream after this round's officer deliveries.
    pub random_state: [u64; 4],
    pub reactor_core: i32,
    pub supply: i32,
    /// The round's selected offer, or -1 while the offer is unanswered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reinforce_choice: Option<i32>,
    /// Unit IDs, written as their type names in ID order.
    #[serde(with = "unit_names::units")]
    pub unlocked_units: Vec<i32>,
    /// What the round still allows, which a match does not write: each
    /// opens at a value the round's opening fixes, and only the round's own
    /// decisions spend it.
    #[serde(skip)]
    pub shop: Allowances,
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
    #[serde(default)]
    pub tower_strengthen_levels: Vec<i32>,
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        with = "crate::names::equipment::many"
    )]
    pub equipments: Vec<i32>,
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
}

/// A round's allowances, as [`crate::transition::open_round`] sets them and
/// its decisions spend them.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Allowances {
    pub buys_remaining: i32,
    pub unlocks_remaining: i32,
    /// What the round still lets the side release of contraptions and
    /// constructions, which spend one allowance between them.
    pub contraptions_remaining: i32,
}

/// A formation, and what recovering it pays back.
///
/// `value` is not a function of the unit's type and level. It is what the side
/// actually paid, at the prices its officers made at the time, so two identical
/// looking formations bought a round apart can be worth different amounts.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(remote = "Self", deny_unknown_fields)]
pub struct StateUnit {
    #[serde(rename = "name")]
    pub type_name: String,
    pub index: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<i32>,
    #[serde(default, skip_serializing_if = "UnitSource::is_legacy")]
    pub source: UnitSource,
    #[serde(default, skip_serializing_if = "is_false")]
    pub recovered: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<Position>,
    /// Every active formation has a bar. A zero bar is omitted on the wire.
    #[serde(default, skip_serializing_if = "Experience::omitted")]
    pub exp: Option<Experience>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rotated: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        with = "crate::names::equipment::many"
    )]
    pub equipments: Vec<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub travelling: Option<bool>,
    #[serde(rename = "recordPosition", skip_serializing_if = "Option::is_none")]
    pub record_position: Option<Position>,
    #[serde(rename = "recordRotated", skip_serializing_if = "Option::is_none")]
    pub record_rotated: Option<bool>,
}

impl Serialize for StateUnit {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        StateUnit::serialize(self, serializer)
    }
}

impl<'de> Deserialize<'de> for StateUnit {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let mut unit = StateUnit::deserialize(deserializer)?;
        if !unit.recovered && Experience::omitted(&unit.exp) {
            unit.exp = Experience::at_level(0, &unit.type_name, unit.level.unwrap_or(1));
        }
        Ok(unit)
    }
}

impl StateUnit {
    /// The layout pose and experience of a standing formation, with its fitted items.
    pub(crate) fn placement(&self) -> Result<UnitPlacement, String> {
        Ok(UnitPlacement {
            type_name: self.type_name.clone(),
            index: self.index,
            position: self
                .position
                .ok_or_else(|| format!("formation {} has no position", self.index))?,
            record_position: self.record_position,
            record_rotated: self.record_rotated,
            level: self.level,
            exp: self.exp.filter(|bar| bar.current != 0),
            rotated: self.rotated,
            equipments: self.equipments.clone(),
            travelling: self.travelling,
            source: self.source,
        })
    }

    /// Deployment movement follows the source, persistent mobility modifiers,
    /// and any Redeploy action already taken in this round.
    #[must_use]
    pub fn movable(&self, side: &SideState) -> bool {
        !self.recovered
            && (self.source != UnitSource::Legacy
                || crate::mobility::free(self, side)
                || side.battle_skills.iter().any(|slot| {
                    crate::mobility::REDEPLOY_SKILLS.contains(&slot.id)
                        && slot
                            .release
                            .as_ref()
                            .is_some_and(|release| release.target == SkillTarget::Unit(self.index))
                }))
    }

    pub(crate) fn mark_recovered(&mut self) {
        self.recovered = true;
        self.position = None;
        self.exp = None;
        self.rotated = None;
        self.equipments.clear();
        self.travelling = None;
        self.record_position = None;
        self.record_rotated = None;
    }
}

impl SideState {
    pub fn active_units(&self) -> impl Iterator<Item = &StateUnit> {
        self.units.iter().filter(|unit| !unit.recovered)
    }

    /// What a formation wears, in fitting order, including duplicate items.
    #[must_use]
    pub fn worn_equipment(&self, index: i32) -> Vec<i32> {
        self.active_units()
            .find(|unit| unit.index == index)
            .map(|unit| unit.equipments.clone())
            .unwrap_or_default()
    }

    pub(crate) fn check_board(&self, opening: bool) -> Result<(), String> {
        let mut indices = BTreeSet::new();
        let spent: BTreeSet<_> = self
            .battle_skills
            .iter()
            .filter_map(|slot| {
                if crate::ledger::RECOVERY_SKILLS.contains(&slot.id)
                    && let SkillTarget::Unit(index) = slot.release.as_ref()?.target
                {
                    return Some(index);
                }
                None
            })
            .collect();
        for unit in &self.units {
            if !indices.insert(unit.index) {
                return Err(format!("formation {} appears twice", unit.index));
            }
            if unit.recovered {
                if opening
                    || !spent.contains(&unit.index)
                    || unit.position.is_some()
                    || unit.rotated.is_some()
                    || unit.exp.is_some()
                    || !unit.equipments.is_empty()
                    || unit.travelling.is_some()
                    || unit.record_position.is_some()
                    || unit.record_rotated.is_some()
                {
                    return Err(format!(
                        "formation {} is an invalid recovery tombstone",
                        unit.index
                    ));
                }
            } else if unit.position.is_none() || spent.contains(&unit.index) {
                return Err(format!(
                    "formation {} has an invalid on-board state",
                    unit.index
                ));
            }
        }
        if let Some(index) = spent.iter().find(|index| !indices.contains(index)) {
            return Err(format!("recovery of formation {index} has no tombstone"));
        }
        Ok(())
    }
}

/// One commander skill panel slot.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(try_from = "PanelSkillFields")]
pub struct PanelSkill {
    pub index: i32,
    #[serde(rename = "name", with = "crate::names::commander_skill::one")]
    pub id: i32,
    pub cooldown: i32,
    /// What the slot's earlier releases left standing, in the payload the
    /// slot's skill leaves. Only a Shield Airdrop's and a Sticky Oil Bomb's
    /// outlive their round.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub standing: Vec<Standing>,
    /// Present on a skill this round released. A state is defined after each
    /// action, so a round's opening position carries none and a deployment's
    /// closing position carries one per release.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release: Option<Release>,
}

/// How a panel slot is read, before its standing objects are checked to be
/// its skill's.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PanelSkillFields {
    index: i32,
    #[serde(rename = "name", with = "crate::names::commander_skill::one")]
    id: i32,
    cooldown: i32,
    #[serde(default)]
    standing: Vec<Standing>,
    #[serde(default)]
    release: Option<Release>,
}

impl TryFrom<PanelSkillFields> for PanelSkill {
    type Error = String;

    fn try_from(fields: PanelSkillFields) -> Result<Self, String> {
        for standing in &fields.standing {
            standing.require_skill(fields.id)?;
        }
        if let Some(release) = &fields.release {
            release.require_skill(fields.id)?;
        }
        Ok(Self {
            index: fields.index,
            id: fields.id,
            cooldown: fields.cooldown,
            standing: fields.standing,
            release: fields.release,
        })
    }
}

/// Where in a round's release sequence a skill went off, and at what.
///
/// `order` is explicit because the panel is sorted by `index`, so array
/// position cannot carry it. A layout is the other way round: it lists only
/// releases, and there the array position is the order.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Release {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub order: Option<i32>,
    #[serde(with = "serde_yaml::with::singleton_map")]
    pub target: SkillTarget,
}

impl Release {
    pub(crate) fn is_deployment(skill: i32) -> bool {
        crate::ledger::RECOVERY_SKILLS.contains(&skill)
            || crate::mobility::REDEPLOY_SKILLS.contains(&skill)
            || crate::transition::TRAINING_SKILLS.contains(&skill)
    }

    fn require_skill(&self, skill: i32) -> Result<(), String> {
        let valid = if crate::ledger::RECOVERY_SKILLS.contains(&skill) {
            self.order.is_none() && !matches!(self.target, SkillTarget::Area(_))
        } else if Self::is_deployment(skill) {
            self.order.is_none() && matches!(self.target, SkillTarget::Unit(_))
        } else {
            self.order.is_some_and(|order| order >= 0)
        };
        if valid {
            Ok(())
        } else {
            Err(format!(
                "release has an invalid order or target for skill {skill}"
            ))
        }
    }
}

impl PanelSkill {
    pub(crate) fn battle_order(&self) -> Option<i32> {
        self.release.as_ref()?.order
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct NextIndex {
    pub unit: i32,
    pub contraption: i32,
}

/// Each side's decisions in one round, which a match writes as an action
/// segment.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TurnActions {
    pub blue: Vec<Action>,
    pub red: Vec<Action>,
}

/// The default runtime deployment limit and standard native replay clock.
///
/// It is the game's own `DeployTime`, which every tracked replay records as
/// 100 seconds. What the game does when it runs out is not established, and
/// what this platform does is `docs/spec/mechcore/cli.md`'s own rule.
pub const DEFAULT_DEPLOY_TIME: i32 = 100;

/// One decision that took effect, in the order the side took it.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    /// The round's reinforcement answer, which declining is one of.
    ///
    /// `index` is the position of what is taken in `reinforce_offers`: a
    /// card dealt, or the decline after them, which the round always offers.
    /// `id` names the card or `refund_<amount>`. A refund must equal the
    /// amount this round offers; it cannot choose how much supply to receive.
    ChooseReinforceItem {
        index: i32,
        #[serde(rename = "name", with = "crate::names::card::choice")]
        id: ReinforceChoice,
    },
    /// The opening, which is one decision with two halves: the team of
    /// formations and the specialist officer bound to it.
    ///
    /// It is the only decision of round zero, and no other round holds one.
    ChooseAdvanceTeam {
        index: i32,
        #[serde(rename = "name", with = "crate::names::advance_team::one")]
        id: i32,
        #[serde(with = "crate::names::officer::one")]
        specialist: i32,
    },
    /// A purchase at the default landing or coupled with its immediate move.
    /// Its level is determined by the current officers and tower skills.
    BuyUnit {
        #[serde(rename = "name", with = "unit_names::unit")]
        unit: i32,
        #[serde(skip_serializing_if = "Option::is_none")]
        position: Option<Position>,
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

impl Action {
    /// Reads a decision with the round that gives unit reinforcement cards
    /// their identity, just as an action segment does.
    ///
    /// # Errors
    ///
    /// Refuses malformed decisions and names absent from that round.
    pub fn read_in_round(value: impl Serialize, round: i32) -> Result<Self, serde_yaml::Error> {
        let mut value = serde_yaml::to_value(value)?;
        qualify_action(&mut value, i64::from(round));
        serde_yaml::from_value(value)
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
        index: i32,
        #[serde(rename = "name", with = "crate::names::card::choice")]
        id: ReinforceChoice,
    },
    ChooseAdvanceTeam {
        index: i32,
        #[serde(rename = "name", with = "crate::names::advance_team::one")]
        id: i32,
        #[serde(with = "crate::names::officer::one")]
        specialist: i32,
    },
    BuyUnit {
        #[serde(rename = "name", with = "unit_names::unit")]
        unit: i32,
        #[serde(default)]
        position: Option<Position>,
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
    if let Some(Value::Sequence(offers)) = fields.get_mut("reinforce_offers") {
        for offer in offers {
            if let Some(name) = offer.get_mut("name") {
                qualify_card(name, round);
            }
        }
    }
    for side in ["blue", "red"] {
        let Some(Value::Sequence(actions)) = fields.get_mut(side) else {
            continue;
        };
        for action in actions {
            qualify_action(action, round);
        }
    }
}

fn qualify_action(action: &mut Value, round: i64) {
    if action.get("type").and_then(Value::as_str) == Some("choose_reinforce_item")
        && let Some(name) = action.get_mut("name")
    {
        qualify_card(name, round);
    }
}

fn qualify_card(name: &mut Value, round: i64) {
    if let Value::String(text) = name
        && crate::names::is_unit_card(text)
    {
        *text = format!("{text}@{round}");
    }
}

/// A match names a unit type by the name a layout gives it, never by its ID.
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

/// One segment of a match stream, tagged by the `kind` it opens with.
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Segment<'a> {
    Match(Header<'a>),
    State(StateKindSegment<'a>),
    Action(ActionSegment<'a>),
    Result(&'a MatchResult),
}

#[derive(Serialize)]
#[serde(untagged)]
enum StateKindSegment<'a> {
    Opening(OpeningStateSegment<'a>),
    Round(StateSegment<'a>),
}

#[derive(Serialize)]
struct Header<'a> {
    build: &'a str,
    source: Source,
    reinforce: bool,
    max_round: i32,
    map_id: i32,
    seed: i32,
    blue: HeaderSide<'a>,
    red: HeaderSide<'a>,
}

#[derive(Serialize)]
struct HeaderSide<'a> {
    name: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    elo_score: Option<i32>,
    tech_loadout: HeaderLoadout<'a>,
}

enum HeaderLoadout<'a> {
    Compact(&'a BTreeMap<i32, Vec<i32>>),
    Expanded(&'a BTreeMap<i32, Vec<i32>>),
}

impl Serialize for HeaderLoadout<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Compact(map) => crate::names::loadout::serialize_compact(map, serializer),
            Self::Expanded(map) => crate::names::loadout::serialize(map, serializer),
        }
    }
}

impl<'a> HeaderSide<'a> {
    fn of(side: &'a MatchSide, format: MatchFormat) -> Self {
        Self {
            name: &side.name,
            elo_score: side.elo_score,
            tech_loadout: match format {
                MatchFormat::Compact => HeaderLoadout::Compact(&side.tech_loadout),
                MatchFormat::Expanded => HeaderLoadout::Expanded(&side.tech_loadout),
            },
        }
    }
}

#[derive(Serialize)]
struct OpeningStateSegment<'a> {
    round: i32,
    #[serde(
        skip_serializing_if = "Option::is_none",
        with = "crate::names::card::many_opt"
    )]
    reinforce_pool: Option<Vec<i32>>,
    blue: OpeningStateSide<'a>,
    red: OpeningStateSide<'a>,
}

#[derive(Serialize)]
struct OpeningStateSide<'a> {
    reactor_core: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    seed: Option<i32>,
    offers: &'a [OpeningOffer],
    #[serde(serialize_with = "serialize_opening_constructions")]
    constructions: OpeningConstructions<'a>,
}

struct OpeningConstructions<'a> {
    items: &'a [StaticPlacement],
    format: MatchFormat,
}

fn serialize_opening_constructions<S: serde::Serializer>(
    value: &OpeningConstructions<'_>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match value.format {
        MatchFormat::Compact => value.items.serialize(serializer),
        MatchFormat::Expanded => {
            crate::board::serialize_expanded_constructions(value.items, serializer)
        }
    }
}

enum StatePool<'a> {
    Compact(&'a PoolDelta),
    Expanded(Option<Vec<i32>>),
}

impl StatePool<'_> {
    fn is_omitted(&self) -> bool {
        match self {
            Self::Compact(delta) => delta.is_empty(),
            Self::Expanded(members) => members.is_none(),
        }
    }
}

impl Serialize for StatePool<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Compact(delta) => delta.serialize(serializer),
            Self::Expanded(Some(members)) => {
                crate::names::card::many::serialize(members, serializer)
            }
            Self::Expanded(None) => serializer.serialize_none(),
        }
    }
}

struct MatchRoundSide<'a> {
    side: &'a SideState,
    format: MatchFormat,
}

#[derive(Serialize)]
struct StateSegment<'a> {
    round: i32,
    random_state: [u64; 4],
    #[serde(skip_serializing_if = "StatePool::is_omitted")]
    reinforce_pool: StatePool<'a>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reinforce_offers: Option<&'a Offers>,
    #[serde(serialize_with = "serialize_opening_side")]
    blue: MatchRoundSide<'a>,
    #[serde(serialize_with = "serialize_opening_side")]
    red: MatchRoundSide<'a>,
}

fn serialize_opening_side<S: serde::Serializer>(
    wrapped: &MatchRoundSide<'_>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let mut opening = wrapped.side.clone();
    opening.reinforce_choice = None;
    match wrapped.format {
        MatchFormat::Compact => opening.serialize(serializer),
        MatchFormat::Expanded => crate::board::serialize_expanded_round_side(&opening, serializer),
    }
}

#[derive(Serialize)]
struct ActionSegment<'a> {
    round: i32,
    blue: Cow<'a, [Action]>,
    red: Cow<'a, [Action]>,
}

/// The line that separates two segments of a stream.
const SEPARATOR: &str = "---\n";

/// Serializes a match as the stream `docs/spec/document/match.md` defines,
/// in the normal form the match, state and action documents define.
///
/// # Errors
///
/// Returns an error when a segment cannot be serialized.
pub fn canonical_yaml(r#match: &Match) -> Result<String, String> {
    canonical_yaml_with(r#match, MatchFormat::Compact)
}

/// Serializes a match with the requested compact or expanded representation.
///
/// # Errors
///
/// Returns an error when a segment cannot be serialized.
pub fn canonical_yaml_with(r#match: &Match, format: MatchFormat) -> Result<String, String> {
    write_values(&segment_values_with(r#match, format)?)
}

fn write_values(values: &[Value]) -> Result<String, String> {
    let mut yaml = String::new();
    for (at, value) in values.iter().enumerate() {
        if at > 0 {
            yaml.push_str(SEPARATOR);
        }
        yaml.push_str(&crate::spelling::document(value)?);
    }
    Ok(yaml)
}

/// The match's YAML documents as values, in their written order and shape.
///
/// # Errors
/// Returns an error when a segment cannot be serialized.
pub fn segment_values(game: &Match) -> Result<Vec<Value>, String> {
    segment_values_with(game, MatchFormat::Compact)
}

/// The match's YAML documents as values, with the requested representation.
///
/// # Errors
/// Returns an error when a segment cannot be serialized.
pub fn segment_values_with(game: &Match, format: MatchFormat) -> Result<Vec<Value>, String> {
    values_of(&segments_of(game, format)?)
}

fn values_of(segments: &[Segment<'_>]) -> Result<Vec<Value>, String> {
    segments
        .iter()
        .map(|segment| {
            serde_yaml::to_value(segment)
                .map_err(|error| format!("cannot serialize match YAML: {error}"))
        })
        .collect()
}

/// The match header in its written shape, without serializing round history.
///
/// # Errors
/// Returns an error when the header cannot be serialized.
pub fn header_value(game: &Match) -> Result<Value, String> {
    serde_yaml::to_value(Segment::Match(header_of(game, MatchFormat::Compact)))
        .map_err(|error| format!("cannot serialize match header: {error}"))
}

/// Round zero's opening state segment (`offers` and initial `constructions`).
///
/// # Errors
/// Returns an error when the opening state cannot be serialized.
pub fn opening_state_value(game: &Match) -> Result<Value, String> {
    serde_yaml::to_value(Segment::State(StateKindSegment::Opening(opening_state_of(
        game,
        MatchFormat::Compact,
    )?)))
    .map_err(|error| format!("cannot serialize opening state: {error}"))
}

/// Reads a match document whole, or nothing when the file is not a match.
///
/// This is [`canonical_yaml`]'s other direction: what it writes, this reads
/// back. A match still being played reads too, which is what lets a
/// match keep its ledger on disk rather than in memory — a side that has not
/// taken its opening, and a round whose decisions only one side has written,
/// are both positions a document may be in between two operations.
///
/// # Errors
///
/// Returns an error when the stream's grammar is broken, a segment is not
/// readable, or an opening names an offer its side was not dealt.
pub fn read(bytes: &[u8]) -> Result<Option<Match>, String> {
    let Some(stated) = crate::opening::stated(bytes)? else {
        return Ok(None);
    };
    let side = |name: &str, side: crate::opening::StatedSide| {
        let choose = match &side.opening {
            None => None,
            Some(opening) => {
                let taken = usize::try_from(opening.choose)
                    .ok()
                    .and_then(|at| side.offers.get(at))
                    .ok_or_else(|| {
                        format!(
                            "{name} opening offer {} is not one of the {} dealt",
                            opening.choose,
                            side.offers.len()
                        )
                    })?;
                if *taken != opening.taken {
                    return Err(format!(
                        "{name} opening offer {} holds {taken:?}, and the decision names {:?}",
                        opening.choose, opening.taken
                    ));
                }
                Some(opening.choose)
            }
        };
        Ok(MatchSide {
            elo_score: side.elo_score,
            name: side.name.unwrap_or_else(|| name.to_owned()),
            opening: Opening {
                choose,
                offers: side.offers,
            },
            constructions: side.constructions,
            tech_loadout: side.tech_loadout,
            reactor_core: side.reactor_core,
            seed: side.seed,
        })
    };
    Ok(Some(Match {
        has_opening: stated.has_opening,
        game_build: stated.build,
        source: stated.source,
        reinforce: stated.reinforce,
        max_round: stated.max_round,
        map_id: stated.map_id,
        seed: stated.seed,
        blue: side("blue", stated.blue)?,
        red: side("red", stated.red)?,
        turns: stated.turns,
        result: stated.result,
    }))
}

/// The header shared by the document and decision-view writers.
fn header_of(r#match: &Match, format: MatchFormat) -> Header<'_> {
    Header {
        build: &r#match.game_build,
        source: r#match.source,
        reinforce: r#match.reinforce,
        max_round: r#match.max_round,
        map_id: r#match.map_id,
        seed: r#match.seed,
        blue: HeaderSide::of(&r#match.blue, format),
        red: HeaderSide::of(&r#match.red, format),
    }
}

fn opening_side_of<'a>(
    side: &'a MatchSide,
    name: &str,
    format: MatchFormat,
) -> Result<OpeningStateSide<'a>, String> {
    let reactor_core = side
        .reactor_core
        .filter(|&core| core > 0)
        .ok_or_else(|| format!("{name} round 0 state has no positive reactor_core"))?;
    Ok(OpeningStateSide {
        reactor_core,
        seed: side.seed,
        offers: &side.opening.offers,
        constructions: OpeningConstructions {
            items: &side.constructions,
            format,
        },
    })
}

fn opening_state_of(
    r#match: &Match,
    format: MatchFormat,
) -> Result<OpeningStateSegment<'_>, String> {
    let reinforce_pool = match format {
        MatchFormat::Expanded if r#match.reinforce => {
            Some(crate::reinforcement::initial_members(r#match.seed)?)
        }
        _ => None,
    };
    Ok(OpeningStateSegment {
        round: 0,
        reinforce_pool,
        blue: opening_side_of(&r#match.blue, "blue", format)?,
        red: opening_side_of(&r#match.red, "red", format)?,
    })
}

/// A match's segments, in stream order.
fn segments_of(r#match: &Match, format: MatchFormat) -> Result<Vec<Segment<'_>>, String> {
    let mut segments = vec![Segment::Match(header_of(r#match, format))];
    if r#match.has_opening {
        segments.push(Segment::State(StateKindSegment::Opening(opening_state_of(
            r#match, format,
        )?)));
        segments.push(Segment::Action(ActionSegment {
            round: 0,
            blue: Cow::Owned(r#match.blue.opening.action().into_iter().collect()),
            red: Cow::Owned(r#match.red.opening.action().into_iter().collect()),
        }));
    }
    for turn in &r#match.turns {
        let reinforce_pool = match format {
            MatchFormat::Compact => StatePool::Compact(&turn.state.reinforce_pool),
            MatchFormat::Expanded if r#match.reinforce => StatePool::Expanded(Some(
                crate::reinforcement::expand_pool(r#match.seed, &turn.state.reinforce_pool)?,
            )),
            MatchFormat::Expanded => StatePool::Expanded(None),
        };
        segments.push(Segment::State(StateKindSegment::Round(StateSegment {
            round: turn.round,
            random_state: turn.state.random_state,
            reinforce_pool,
            reinforce_offers: turn.state.reinforce_offers.as_ref(),
            blue: MatchRoundSide {
                side: &turn.state.blue,
                format,
            },
            red: MatchRoundSide {
                side: &turn.state.red,
                format,
            },
        })));
        // A destroyed reactor core ends the match on the state that states
        // it, so the round it opens has no decisions to write.
        if turn.state.blue.reactor_core <= 0 || turn.state.red.reactor_core <= 0 {
            break;
        }
        segments.push(Segment::Action(ActionSegment {
            round: turn.round,
            blue: Cow::Borrowed(&turn.actions.blue),
            red: Cow::Borrowed(&turn.actions.red),
        }));
    }
    if let Some(result) = &r#match.result {
        segments.push(Segment::Result(result));
    }
    Ok(segments)
}

/// Deserializes a round `state` segment, accepting both compact delta mappings
/// and expanded card-list `reinforce_pool` encodings.
pub(crate) fn read_round_state(
    mut value: Value,
    seed: i32,
    reinforce: bool,
) -> Result<State, String> {
    if let Some(fields) = value.as_mapping_mut() {
        for side_name in ["blue", "red"] {
            if let Some(side_map) = fields
                .get_mut(Value::String(side_name.into()))
                .and_then(Value::as_mapping_mut)
            {
                crate::board::normalize_expanded_round_side(side_map, side_name)?;
            }
        }
    }
    let expanded = value.as_mapping_mut().and_then(|fields| {
        let key = Value::String("reinforce_pool".into());
        fields
            .get(&key)?
            .is_sequence()
            .then(|| fields.remove(&key))?
    });
    let delta = match expanded {
        Some(seq) => {
            if !reinforce {
                return Err("states reinforce_pool while reinforcements are disabled".into());
            }
            let members = crate::names::card::many::deserialize(seq)
                .map_err(|error| format!("reinforce_pool: {error}"))?;
            Some(crate::reinforcement::compact_pool(seed, &members)?)
        }
        None => None,
    };
    let mut state: State = payload(value).map_err(|error| error.to_string())?;
    if let Some(delta) = delta {
        state.reinforce_pool = delta;
    }
    Ok(state)
}

/// A match stream read as the segments it is made of, each checked for its
/// place in the sequence and otherwise left as YAML.
///
/// A reader that needs only part of a match deserializes only that part, so
/// what this checks is the grammar: which segment may follow which, and where
/// the stream has to stop.
#[derive(Debug)]
pub struct Segments {
    pub header: Value,
    /// Round zero's opening state segment (`offers` and `constructions`), absent
    /// from a stream that holds only its header or a partial match starting at
    /// a later round.
    pub opening_state: Option<Value>,
    /// The round-zero action segment, absent from a stream that holds only its
    /// header, stops on `state(0)`, or is a partial match starting at a later
    /// round.
    pub opening: Option<Value>,
    pub rounds: Vec<RoundSegments>,
    pub result: Option<MatchResult>,
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

/// Reads a match stream into its segments, or nothing when the file is not a
/// match.
///
/// A file is a match when its first document says `kind: match`. After that
/// a complete prefix starts with round zero's state and actions. A partial match
/// starts with any positive round's state. Both then alternate consecutive
/// states and same-round actions. It may end after any segment. Only a final
/// result may follow a concession or a state in which a reactor core has fallen
/// to zero; nothing follows a result.
///
/// # Errors
///
/// Returns an error when a match's segments are out of order, misnumbered,
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
    if kind_of(&header) != Some("match") {
        return Ok(None);
    }
    let mut stream = Segments {
        header,
        opening_state: None,
        opening: None,
        rounds: Vec::new(),
        result: None,
    };
    let mut ended: Option<&str> = None;
    for (at, document) in documents.enumerate() {
        let position = at + 2;
        let segment = Value::deserialize(document)
            .map_err(|error| format!("match segment {position} is not YAML: {error}"))?;
        if stream.result.is_some() {
            return Err(format!("match segment {position} follows a result"));
        }
        if kind_of(&segment) == Some("result") {
            stream.result = Some(read_result(segment, &stream.rounds, &stream.header)?);
            continue;
        }
        if let Some(end) = ended {
            return Err(format!("match segment {position} follows {end}"));
        }
        let kind = kind_of(&segment)
            .ok_or_else(|| format!("match segment {position} names no kind"))?
            .to_owned();
        let round = segment
            .get("round")
            .and_then(Value::as_i64)
            .and_then(|round| i32::try_from(round).ok())
            .ok_or_else(|| format!("match segment {position} ({kind}) names no round"))?;
        let (expected_kind, expected_round) =
            match (&stream.opening_state, &stream.opening, stream.rounds.last()) {
                (None, None, None) if kind == "state" && round >= 1 => ("state", round),
                (None, _, None) => ("state", 0),
                (Some(_), None, None) => ("action", 0),
                (Some(_), Some(_), None) => ("state", 1),
                (_, _, Some(last)) if last.actions.is_none() => ("action", last.round),
                (_, _, Some(last)) => (
                    "state",
                    last.round.checked_add(1).ok_or("match round overflow")?,
                ),
            };
        if kind != expected_kind || round != expected_round {
            return Err(format!(
                "match segment {position} is {kind} round {round}; \
                 the stream expects {expected_kind} round {expected_round}"
            ));
        }
        if kind == "state" {
            if round == 0 {
                stream.opening_state = Some(segment);
                continue;
            }
            if released(&segment) {
                return Err(format!(
                    "round {round} state carries a release; a match \
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

fn read_result(
    segment: Value,
    rounds: &[RoundSegments],
    header: &Value,
) -> Result<MatchResult, String> {
    let last = rounds
        .last()
        .ok_or("a result requires a deployment round")?;
    let mut value = segment;
    if let Value::Mapping(fields) = &mut value {
        fields.remove("kind");
    }
    let result: MatchResult = serde_yaml::from_value(value)
        .map_err(|error| format!("match result is not readable: {error}"))?;
    let expected_round = if core_destroyed(&last.state) {
        last.round - 1
    } else if last.actions.is_some() {
        last.round
    } else {
        return Err("a result requires the last round's decisions or a terminal state".into());
    };
    if result.round != expected_round {
        return Err(format!(
            "result round {} must be {expected_round}",
            result.round
        ));
    }
    let concession = last.actions.as_ref().and_then(|actions| {
        each_side(actions).find_map(|(side, list)| {
            list.as_sequence()?
                .iter()
                .any(|action| action.get("type").and_then(Value::as_str) == Some("concede"))
                .then_some(if side == "blue" {
                    Winner::Red
                } else {
                    Winner::Blue
                })
        })
    });
    let map_id = header
        .get("map_id")
        .and_then(Value::as_i64)
        .and_then(|id| i32::try_from(id).ok())
        .ok_or("a result requires a map_id")?;
    let max_round = match header.get("max_round") {
        Some(value) => value
            .as_i64()
            .and_then(|round| i32::try_from(round).ok())
            .filter(|round| *round >= 1)
            .ok_or("match max_round must be >= 1")?,
        None => crate::opening::max_round(map_id)?,
    };
    let at_limit = result.round == max_round;
    let winner = concession
        .or_else(|| Winner::from_cores(result.blue.reactor_core, result.red.reactor_core))
        .or_else(|| {
            if !at_limit {
                return None;
            }
            match result.blue.reactor_core.cmp(&result.red.reactor_core) {
                std::cmp::Ordering::Greater => Some(Winner::Blue),
                std::cmp::Ordering::Less => Some(Winner::Red),
                // Native ranking breaks equal cores by team bonus, which
                // this result does not carry. Retain the recorded winner.
                std::cmp::Ordering::Equal => None,
            }
        });
    if winner.is_some_and(|winner| winner != result.winner) || (winner.is_none() && !at_limit) {
        return Err("result winner disagrees with the final cores or concession".into());
    }
    if core_destroyed(&last.state)
        && each_side(&last.state).any(|(side, state)| {
            let core = if side == "blue" {
                result.blue.reactor_core
            } else {
                result.red.reactor_core
            };
            state.get("reactor_core").and_then(Value::as_i64) != Some(i64::from(core))
        })
    {
        return Err("result cores disagree with the terminal state".into());
    }
    Ok(result)
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
        .filter_map(move |side| segment.get(side))
}

fn released(state: &Value) -> bool {
    state_sides(state).any(|side| {
        side.get("battle_skills")
            .and_then(Value::as_sequence)
            .is_some_and(|panel| panel.iter().any(|slot| slot.get("release").is_some()))
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
    fn hidden_and_explicit_zero_restore_the_same_level_bar() {
        use super::{Experience, StateUnit};

        for (level, maximum) in [(1, 450), (2, 1014)] {
            for exp in [String::new(), format!(", exp: 0/{maximum}")] {
                let text = format!("{{name: crawler, index: 8, level: {level}{exp}}}");
                let unit: StateUnit = serde_yaml::from_str(&text).unwrap();
                assert_eq!(
                    unit.exp,
                    Some(Experience {
                        current: 0,
                        maximum
                    })
                );
                assert!(serde_json::to_value(&unit).unwrap().get("exp").is_none());
                assert_eq!(
                    serde_yaml::from_str::<StateUnit>(&serde_yaml::to_string(&unit).unwrap())
                        .unwrap(),
                    unit
                );
            }
        }
    }

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
            "{type: buy_unit}",
            "{type: buy_unit, name: crawler, level: 1}",
            "{type: choose_advance_team, index: 1, name: vortex-fire_badger}",
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
                index: 4,
                id: crate::r#match::ReinforceChoice::Refund(50),
            },
            Action::ChooseReinforceItem {
                index: 2,
                id: crate::r#match::ReinforceChoice::Card(1_305_003),
            },
            Action::ChooseAdvanceTeam {
                index: 1,
                id: 9910,
                specialist: 20005,
            },
            Action::BuyUnit {
                unit: 2,
                position: Some(at),
                rotated: true,
            },
            Action::BuyUnit {
                unit: 2,
                position: Some(at),
                rotated: false,
            },
            Action::BuyUnit {
                unit: 2,
                position: None,
                rotated: false,
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

    /// A round's offers are its structured cards and then the supply decline,
    /// and a list that does not end in the supply entry is not a round's offers.
    #[test]
    fn offers_end_in_the_decline_and_its_refund() {
        use super::{Offers, State, payload};
        let state = |offers: &str| {
            format!(
                "random_state: [1, 2, 3, 4]\nreinforce_offers: {offers}\nblue: {{random_state: [1, 2, 3, 4], reactor_core: 0, supply: 0, unlocked_units: [], \
                 tower_strengthen_levels: [], next_index: {{unit: 0, contraption: 0}}, units: []}}\n\
                 red: {{random_state: [1, 2, 3, 4], reactor_core: 0, supply: 0, unlocked_units: [], tower_strengthen_levels: [], \
                 next_index: {{unit: 0, contraption: 0}}, units: []}}\n"
            )
        };
        let read: State = serde_yaml::from_str(&state(
            "[{type: equipment, name: photon_coating, cost: 0}, \
              {type: commander_skill, name: missile_strike, cost: 50}, \
              {type: supply, name: refund_150, cost: -150}]",
        ))
        .unwrap();
        let offers = read.reinforce_offers.clone().unwrap();
        assert_eq!(
            offers,
            Offers {
                dealt: vec![1_305_003, 300_001],
                refund: 150
            }
        );
        assert_eq!(offers.decline_index(), 2);
        let written = serde_yaml::to_string(&read).unwrap();
        assert_eq!(serde_yaml::from_str::<State>(&written).unwrap(), read);
        assert!(written.contains("name: refund_150"));
        assert!(written.contains("cost: -150"));
        assert!(written.contains("cost: 0"));

        let unit_yaml = format!(
            "kind: state\nround: 2\n{}",
            state(
                "[{type: officer, name: super_supply_enhancement, cost: 250}, \
                  {type: unit, name: rhino_1x_lv1, unit: rhino, level: 1, squads: 1, cost: 50}, \
                  {type: supply, name: refund_200, cost: -200}]"
            )
        );
        let unit_state: State = payload(serde_yaml::from_str(&unit_yaml).unwrap()).unwrap();
        assert_eq!(
            unit_state.reinforce_offers,
            Some(Offers {
                dealt: vec![10_003, 102_115],
                refund: 200
            })
        );

        for amount in [0, 50, 400, i32::MAX] {
            let neg = -amount;
            let value: State = serde_yaml::from_str(&state(&format!(
                "[{{type: supply, name: refund_{amount}, cost: {neg}}}]"
            )))
            .unwrap();
            assert_eq!(value.reinforce_offers.unwrap().refund, amount);
        }
        for refused in [
            "[]",
            "[photon_coating, refund_50]",
            "[{type: supply, name: refund_-1, cost: 1}]",
            "[{type: supply, name: refund_2147483648, cost: -2147483648}]",
            "[{type: supply, name: refund_, cost: 0}]",
            "[{type: supply, name: refund_+50, cost: -50}]",
            "[{type: supply, name: refund_50, cost: 50}]",
            "[{type: supply, name: refund_50, cost: -100}]",
            "[{type: supply, name: refund_50, cost: -50}, {type: supply, name: refund_100, cost: -100}]",
            "[{type: equipment, name: photon_coating, cost: 0}, {type: commander_skill, name: missile_strike, cost: 50}]",
            "[{type: supply, name: refund_50, cost: -50}, {type: equipment, name: photon_coating, cost: 0}]",
            "[{type: commander_skill, name: photon_coating, cost: 0}, {type: supply, name: refund_50, cost: -50}]",
            "[{type: equipment, name: photon_coating, cost: 50}, {type: supply, name: refund_50, cost: -50}]",
            "[{type: equipment, name: photon_coating, cost: 0, extra: 1}, {type: supply, name: refund_50, cost: -50}]",
            "[{name: decline, refund: 50}]",
        ] {
            assert!(
                serde_yaml::from_str::<State>(&state(refused)).is_err(),
                "{refused}"
            );
        }
        for refused_unit in [
            "[{type: unit, name: rhino_1x_lv1, unit: wasp, level: 1, squads: 1, cost: 50}, {type: supply, name: refund_200, cost: -200}]",
            "[{type: unit, name: rhino_1x_lv1, unit: rhino, level: 2, squads: 1, cost: 50}, {type: supply, name: refund_200, cost: -200}]",
            "[{type: unit, name: rhino_1x_lv1, unit: rhino, level: 1, squads: 2, cost: 50}, {type: supply, name: refund_200, cost: -200}]",
            "[{type: unit, name: rhino_1x_lv1, unit: rhino, level: 1, squads: 1, cost: 0}, {type: supply, name: refund_200, cost: -200}]",
        ] {
            let yaml = format!("kind: state\nround: 2\n{}", state(refused_unit));
            assert!(
                payload::<State>(serde_yaml::from_str(&yaml).unwrap()).is_err(),
                "{refused_unit}"
            );
        }
    }

    #[test]
    fn pool_changes_use_card_names_and_require_complete_random_states() {
        let state = super::State {
            random_state: [1, 2, 3, 4],
            reinforce_pool: super::PoolDelta {
                removed: vec![300_001],
                added: vec![1_305_003],
            },
            blue: super::SideState {
                random_state: [5, 6, 7, 8],
                ..Default::default()
            },
            red: super::SideState {
                random_state: [9, 10, 11, 12],
                ..Default::default()
            },
            ..Default::default()
        };
        let yaml = serde_yaml::to_string(&state).unwrap();
        assert!(yaml.contains("missile_strike"));
        assert!(yaml.contains("photon_coating"));
        assert_eq!(serde_yaml::from_str::<super::State>(&yaml).unwrap(), state);
        for invalid in [
            yaml.replacen("random_state:", "missing_state:", 1),
            yaml.replacen("- 4\n", "", 1),
            yaml.replace("missile_strike", "unknown_card"),
        ] {
            assert!(serde_yaml::from_str::<super::State>(&invalid).is_err());
        }
        let empty = serde_yaml::to_string(&super::State::default()).unwrap();
        assert!(!empty.contains("reinforce_pool"));
    }

    #[test]
    fn a_state_refuses_a_field_it_does_not_define() {
        let formation = "{name: crawler, index: 0, position: {x: 0, y: -160}, value: 100}";
        let read =
            |formation: &str| serde_yaml::from_str::<super::StateUnit>(formation).map(|_| ());
        read(formation).unwrap();
        for broken in [
            formation.replace("value: 100", "value: 100, typo: 1"),
            formation.replace("value: 100", "value: 100, movable: true"),
            formation.replace("value: 100", "value: 100, unpinned: true"),
            formation.replace("value: 100", "value: 100, redeployed: true"),
            formation.replace(", index: 0", ""),
        ] {
            assert!(read(&broken).is_err(), "{broken}");
        }
    }

    #[test]
    fn equipments_spell_inline_and_tombstones_refuse_worn_items() {
        use super::{PanelSkill, Release, SideState, SkillTarget, StateUnit};

        let unit: StateUnit = serde_yaml::from_str(
            "{name: crawler, index: 0, position: {x: 0, y: -160}, equipments: [heavy_armor, laser_sights]}",
        )
        .unwrap();
        assert_eq!(unit.equipments, [13_030_002, 13_030_001]);

        let side = SideState {
            equipments: vec![13_030_001, 13_030_001],
            units: vec![unit],
            ..Default::default()
        };
        side.check_board(false).unwrap();
        let written = crate::spelling::document(&serde_yaml::to_value(&side).unwrap()).unwrap();
        assert!(
            written.contains("equipments: [laser_sights, laser_sights]\n"),
            "{written}"
        );
        assert!(
            written.contains(
                "- {name: crawler, index: 0, position: {x: 0, y: -160}, equipments: [heavy_armor, laser_sights]}\n"
            ),
            "{written}"
        );
        let restored: SideState = serde_yaml::from_str(&written).unwrap();
        assert_eq!(restored, side);

        let tombstone = StateUnit {
            recovered: true,
            position: None,
            equipments: vec![13_030_001],
            ..restored.units.into_iter().next().unwrap()
        };
        let invalid = SideState {
            battle_skills: vec![PanelSkill {
                index: 0,
                id: crate::ledger::RECOVERY_SKILLS[0],
                cooldown: 0,
                standing: Vec::new(),
                release: Some(Release {
                    order: None,
                    target: SkillTarget::Unit(0),
                }),
            }],
            units: vec![tombstone],
            ..Default::default()
        };
        assert!(
            invalid
                .check_board(false)
                .unwrap_err()
                .contains("recovery tombstone")
        );
    }

    use super::segments;

    #[test]
    fn the_header_always_preserves_its_game_build() {
        let input = b"kind: match\nsource: simulator\nreinforce: true\nmax_round: 40\nmap_id: 1021\nseed: 1\nblue: {tech_loadout: {}}\nred: {tech_loadout: {}}\n---\nkind: state\nround: 0\nblue: {reactor_core: 4500, offers: [], constructions: []}\nred: {reactor_core: 4500, offers: [], constructions: []}\n---\nkind: action\nround: 0\nblue: []\nred: []\n";
        let game = super::read(input).unwrap().unwrap();
        let written = super::canonical_yaml(&game).unwrap();
        let header = super::segment_values(&game).unwrap().remove(0);
        assert_eq!(header["build"].as_str(), Some(crate::game_build()));
        assert!(header.get("game_build").is_none());
        let read = super::read(written.as_bytes()).unwrap().unwrap();
        assert_eq!(super::canonical_yaml(&read).unwrap(), written);
        let foreign = written.replace(
            &format!("build: {}", crate::game_build()),
            "build: another-version",
        );
        let foreign = super::read(foreign.as_bytes()).unwrap().unwrap();
        assert_eq!(foreign.game_build, "another-version");
        assert!(
            super::canonical_yaml(&foreign)
                .unwrap()
                .contains("build: another-version")
        );
        assert!(
            super::build_warning(&foreign.game_build)
                .unwrap()
                .contains("another-version")
        );
        assert!(super::build_warning(crate::game_build()).is_none());
    }

    #[test]
    fn historical_ratings_are_optional_header_metadata() {
        let input = b"kind: match\nsource: simulator\nreinforce: true\nmap_id: 1021\nseed: 1\nblue: {elo_score: 1448, tech_loadout: {}}\nred: {tech_loadout: {}}\n---\nkind: state\nround: 0\nblue: {reactor_core: 4500, offers: [], constructions: []}\nred: {reactor_core: 4500, offers: [], constructions: []}\n---\nkind: action\nround: 0\nblue: []\nred: []\n";
        let game = super::read(input).unwrap().unwrap();
        assert_eq!(game.blue.elo_score, Some(1448));
        assert_eq!(game.red.elo_score, None);
        let formatted = super::canonical_yaml(&game).unwrap();
        let restored = super::read(formatted.as_bytes()).unwrap().unwrap();
        assert_eq!(restored, game);
        let header = super::header_value(&restored).unwrap();
        assert_eq!(header["blue"]["elo_score"].as_i64(), Some(1448));
        assert!(header["red"].get("elo_score").is_none());
        let zero = String::from_utf8(input.to_vec())
            .unwrap()
            .replace("elo_score: 1448", "elo_score: 0");
        assert_eq!(
            super::read(zero.as_bytes())
                .unwrap()
                .unwrap()
                .blue
                .elo_score,
            Some(0)
        );
        let invalid = zero.replace("elo_score: 0", "elo_score: unknown");
        assert!(super::read(invalid.as_bytes()).is_err());
    }

    const HEADER: &str =
        "kind: match\nsource: simulator\nreinforce: true\nmax_round: 40\nmap_id: 1021\nseed: 1\n";
    const OPENING_STATE: &str = "---\nkind: state\nround: 0\nblue: {reactor_core: 4500, offers: [], constructions: []}\nred: {reactor_core: 4500, offers: [], constructions: []}\n";
    const OPENING: &str = "---\nkind: state\nround: 0\nblue: {reactor_core: 4500, offers: [], constructions: []}\nred: {reactor_core: 4500, offers: [], constructions: []}\n---\nkind: action\nround: 0\nblue: []\nred: []\n";

    fn state(round: i32, blue_core: i32) -> String {
        format!(
            "---\nkind: state\nround: {round}\nblue: {{reactor_core: {blue_core}}}\nred: {{reactor_core: 4800}}\n"
        )
    }

    fn actions(round: i32, red: &str) -> String {
        format!("---\nkind: action\nround: {round}\nblue: []\nred: {red}\n")
    }

    fn read(stream: &str) -> Result<super::Segments, String> {
        segments(stream.as_bytes()).map(|read| read.expect("a match stream"))
    }

    fn read_ok(stream: &str) -> bool {
        read(stream).is_ok()
    }

    #[test]
    fn final_results_are_terminal_and_require_consistent_scores() {
        let prefix = format!("{HEADER}{}{}", state(6, 4800), actions(6, "[]"));
        let result = "---\nkind: result\nround: 6\nwinner: blue\nblue: {reactor_core: 4700}\nred: {reactor_core: -240}\n";
        let complete = format!("{prefix}{result}");
        assert_eq!(
            read(&complete).unwrap().result.unwrap().red.reactor_core,
            -240
        );
        for invalid in [
            format!("{complete}{}", state(7, 4700)),
            format!("{complete}{result}"),
            complete.replace("round: 6\nwinner", "round: 7\nwinner"),
            complete.replace("winner: blue", "winner: red"),
            complete.replace("red: {reactor_core: -240}", "red: {reactor_core: 240}"),
            complete.replace("blue: {reactor_core: 4700}\n", ""),
            format!("{HEADER}{}{result}", state(6, 4800)),
        ] {
            assert!(read(&invalid).is_err(), "{invalid}");
        }
        let conceded = format!(
            "{HEADER}{}{}{result}",
            state(6, 4800),
            actions(6, "[{type: concede}]")
        );
        assert!(read_ok(&conceded));
        let terminal = format!("{prefix}{}{result}", state(7, 0));
        assert!(read(&terminal).is_err());
    }

    #[test]
    fn positive_final_cores_require_the_maps_round_limit() {
        let stream = format!(
            "kind: match\nsource: simulator\nreinforce: true\nmap_id: 1012\nseed: 1\n{}{}---\nkind: result\nround: 13\nwinner: blue\nblue: {{reactor_core: 402}}\nred: {{reactor_core: 12}}\n",
            state(13, 402),
            actions(13, "[]"),
        );
        assert!(read_ok(&stream));
        assert!(read(&stream.replace("map_id: 1012", "map_id: 1011")).is_err());
        assert!(read(&stream.replace("map_id: 1012\n", "map_id: 1011\nmax_round: 13\n")).is_ok());
        assert!(read(&stream.replace("round: 13", "round: 12")).is_err());
        assert!(read(&stream.replace("winner: blue", "winner: red")).is_err());
        // Equal cores leave the authoritative winner intact; team bonus is absent.
        assert!(read_ok(
            &stream.replace("reactor_core: 12", "reactor_core: 402")
        ));
        assert_eq!(crate::opening::max_round(1011).unwrap(), 40);
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
        assert!(read.opening_state.is_some());
        assert!(read.opening.is_some());
        assert_eq!(read.rounds.len(), 2);
        assert!(read.rounds[0].actions.is_some());
        assert!(read.rounds[1].actions.is_none());
        assert!(super::segments(HEADER.as_bytes()).unwrap().is_some());
        let at_opening_state = format!("{HEADER}{OPENING_STATE}");
        let parsed = self::read(&at_opening_state).unwrap();
        assert!(parsed.opening_state.is_some());
        assert!(parsed.opening.is_none());
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
        for first in [1, 7] {
            let suffix = format!(
                "{HEADER}{}{}{}",
                state(first, 4800),
                actions(first, "[]"),
                state(first + 1, 4800)
            );
            let captured = read(&suffix).unwrap();
            assert!(captured.opening_state.is_none());
            assert!(captured.opening.is_none());
            assert_eq!(captured.rounds[0].round, first);
            assert!(!read_ok(&format!("{suffix}{}", actions(first + 2, "[]"))));
            assert!(!read_ok(&format!(
                "{HEADER}{}{}{}",
                state(first, 4800),
                actions(first, "[]"),
                state(first + 2, 4800)
            )));
        }
        let action_before_opening_state = format!("{HEADER}{}", actions(0, "[]"));
        assert!(!read_ok(&action_before_opening_state));
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

    /// A match a fight has just ended is written as one a reader takes: its
    /// stream stops on the state that destroyed a reactor core, with no
    /// decisions after it.
    #[test]
    fn a_match_ended_by_a_fight_is_written_without_a_round_after_it() {
        let side = || super::MatchSide {
            elo_score: None,
            name: "player".to_owned(),
            opening: super::Opening {
                choose: None,
                offers: Vec::new(),
            },
            constructions: Vec::new(),
            tech_loadout: std::collections::BTreeMap::new(),
            reactor_core: Some(4500),
            seed: None,
        };
        let state = |round, blue| super::Turn {
            round,
            state: super::State {
                random_state: [0; 4],
                reinforce_pool: crate::r#match::PoolDelta::default(),
                reinforce_offers: None,
                blue: super::SideState {
                    reactor_core: blue,
                    ..Default::default()
                },
                red: super::SideState {
                    reactor_core: 4800,
                    ..Default::default()
                },
            },
            actions: super::TurnActions::default(),
        };
        let ended = super::Match {
            source: super::Source::Simulator,
            reinforce: true,
            max_round: 40,
            result: None,
            has_opening: true,
            game_build: crate::game_build().to_owned(),
            map_id: 1021,
            seed: 1,
            blue: side(),
            red: side(),
            turns: vec![state(1, 4800), state(2, 0)],
        };
        let written = super::canonical_yaml(&ended).unwrap();
        let segments = read(&written).unwrap();
        assert_eq!(segments.rounds.len(), 2);
        assert!(segments.rounds[0].actions.is_some());
        assert!(segments.rounds[1].actions.is_none(), "{written}");
    }

    /// A state segment is the position a round opens with, so no decision of
    /// that round can have left a release on its panel.
    #[test]
    fn a_state_segment_carries_no_release() {
        let released = format!(
            "{HEADER}{OPENING}---\nkind: state\nround: 1\nblue:\n  battle_skills:\n  - {{index: 0, id: 900001, cooldown: 0, release: {{order: 1, target: {{unit: 4}}}}}}\n"
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
        let block = "kind: state\nround: 3\nreinforce_offers:\n- 1033115\n- 1031122\nblue:\n  shop:\n    unlocked_units:\n    - 2\n    - 10\n    buys_remaining: 2\n  next_index:\n    unit: 7\n    contraption: 0\n  units:\n  - type: vortex\n    index: 0\n    position:\n      x: -120\n      y: -100\n  constructions: []\nred:\n  units:\n  - type: release_commander_skill\n    target:\n      area:\n      - x: 197\n        y: -40\n  - type: concede\n";
        let value: serde_yaml::Value = serde_yaml::from_str(block).unwrap();
        let spelled = crate::spelling::document(&value).unwrap();
        assert_eq!(
            spelled,
            "kind: state\nround: 3\nreinforce_offers: [1033115, 1031122]\nblue:\n\
             \x20 shop:\n    unlocked_units: [2, 10]\n    buys_remaining: 2\n\
             \x20 next_index: {unit: 7, contraption: 0}\n  units:\n\
             \x20 - {type: vortex, index: 0, position: {x: -120, y: -100}}\n\
             \x20 constructions: []\nred:\n  units:\n\
             \x20 - {type: release_commander_skill, target: {area: [{x: 197, y: -40}]}}\n\
             \x20 - {type: concede}\n"
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
            "blue:\n  tech_loadout:\n    1: [1001, 1105]\n    2001: [32001]\n",
        )
        .unwrap();
        assert_eq!(
            crate::spelling::document(&value).unwrap(),
            "blue:\n  tech_loadout:\n    1: [1001, 1105]\n    2001: [32001]\n"
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

    #[test]
    fn only_a_match_header_opens_a_match() {
        assert!(segments(b"kind: layout\nsides: {}\n").unwrap().is_none());
        assert!(segments(b"not: [a document").unwrap().is_none());
        assert!(segments(b"").unwrap().is_none());
    }

    #[test]
    fn opening_state_segment_omits_and_refuses_reinforce_choice() {
        let side = || super::MatchSide {
            name: "side".into(),
            elo_score: None,
            opening: super::Opening {
                choose: None,
                offers: Vec::new(),
            },
            constructions: Vec::new(),
            tech_loadout: std::collections::BTreeMap::new(),
            reactor_core: None,
            seed: None,
        };
        let mut r2 = super::State {
            random_state: [1, 2, 3, 4],
            blue: super::SideState {
                random_state: [5, 6, 7, 8],
                reactor_core: 4800,
                tower_strengthen_levels: vec![0, 0],
                ..Default::default()
            },
            red: super::SideState {
                random_state: [9, 10, 11, 12],
                reactor_core: 4800,
                tower_strengthen_levels: vec![0, 0],
                ..Default::default()
            },
            ..Default::default()
        };
        r2.set_reinforce_offers(Some(super::Offers {
            dealt: vec![1_100_001],
            refund: 50,
        }));
        let game = super::Match {
            source: super::Source::Simulator,
            reinforce: true,
            max_round: 40,
            result: None,
            has_opening: false,
            game_build: crate::game_build().to_owned(),
            map_id: 1021,
            seed: 1,
            blue: side(),
            red: side(),
            turns: vec![super::Turn {
                round: 2,
                state: r2,
                actions: super::TurnActions::default(),
            }],
        };
        let written = super::canonical_yaml(&game).unwrap();
        assert!(!written.contains("reinforce_choice"), "{written}");
        let restored = super::read(written.as_bytes()).unwrap().unwrap();
        assert_eq!(
            restored.turns[0].state.blue.reinforce_choice,
            Some(super::Offers::UNANSWERED)
        );
        assert_eq!(
            restored.turns[0].state.red.reinforce_choice,
            Some(super::Offers::UNANSWERED)
        );
        let explicit =
            written.replacen("  supply: 0\n", "  supply: 0\n  reinforce_choice: -1\n", 1);
        assert!(explicit.contains("reinforce_choice: -1"));
        assert!(
            super::read(explicit.as_bytes())
                .unwrap_err()
                .contains("reinforce_choice disagrees")
        );
    }

    #[test]
    fn invalid_tower_strengthen_levels_are_refused() {
        let valid = || super::State {
            blue: super::SideState {
                tower_strengthen_levels: vec![0, 0],
                ..Default::default()
            },
            red: super::SideState {
                tower_strengthen_levels: vec![0, 0],
                ..Default::default()
            },
            ..Default::default()
        };
        valid().check_towers(true).unwrap();

        let mut wrong_count = valid();
        wrong_count.blue.tower_strengthen_levels.pop();
        assert!(
            wrong_count
                .check_towers(true)
                .unwrap_err()
                .contains("2 levels")
        );

        let mut bad_level = valid();
        bad_level.red.tower_strengthen_levels[1] = 5;
        assert!(bad_level.check_towers(true).unwrap_err().contains("0..=4"));
    }

    #[test]
    fn opening_offer_serializes_core_modify_and_refuses_mismatch() {
        let predicted =
            crate::opening::predict(crate::economy::Economy::embedded().unwrap(), 12345, 1012)
                .unwrap();
        let offer = predicted.deal.blue[0];
        let expected_modify = offer.core_modify().unwrap();
        let value = serde_yaml::to_value(offer).unwrap();
        let text = serde_yaml::to_string(&value).unwrap();
        assert_eq!(
            value.get("core_modify").and_then(serde_yaml::Value::as_i64),
            Some(i64::from(expected_modify))
        );
        let parsed: super::OpeningOffer = serde_yaml::from_str(&text).unwrap();
        assert_eq!(parsed, offer);

        let wrong = text.replace(
            &format!("core_modify: {expected_modify}"),
            &format!("core_modify: {}", expected_modify + 100),
        );
        assert!(
            serde_yaml::from_str::<super::OpeningOffer>(&wrong)
                .unwrap_err()
                .to_string()
                .contains("core_modify")
        );
    }

    #[test]
    fn canonical_match_writes_compact_and_expanded_loadouts() {
        let economy = crate::economy::Economy::embedded().unwrap();
        let mut custom = economy.unit_technologies();
        for (unit, list) in &mut custom {
            list.truncate(super::research_limit(*unit));
        }
        let side = |loadout| super::MatchSide {
            name: "side".into(),
            elo_score: None,
            opening: super::Opening {
                choose: None,
                offers: Vec::new(),
            },
            constructions: Vec::new(),
            tech_loadout: loadout,
            reactor_core: Some(4700),
            seed: None,
        };
        let game = super::Match {
            has_opening: true,
            game_build: crate::game_build().to_owned(),
            source: super::Source::Simulator,
            reinforce: true,
            max_round: 13,
            map_id: 1012,
            seed: 1,
            blue: side(economy.unit_technologies()),
            red: side(custom),
            turns: Vec::new(),
            result: None,
        };
        let compact = super::canonical_yaml(&game).unwrap();
        assert!(compact.contains("reinforce: true\nmax_round: 13\n"));
        assert!(compact.contains("tech_loadout: full"));
        assert!(compact.contains("---\nkind: state\nround: 0\nblue:\n  reactor_core: 4700\n"));
        assert_eq!(super::read(compact.as_bytes()).unwrap().unwrap(), game);

        let expanded = super::canonical_yaml_with(&game, super::MatchFormat::Expanded).unwrap();
        assert!(expanded.contains("  tech_loadout:\n    fortress: ["));
        assert!(expanded.contains("round: 0\nreinforce_pool: ["));
        assert_eq!(super::read(expanded.as_bytes()).unwrap().unwrap(), game);
    }

    #[test]
    fn canonical_match_expanded_towers_and_entity_sizes_round_trip_and_refuse_mismatch() {
        let economy = crate::economy::Economy::embedded().unwrap();
        let pred = crate::opening::predict(economy, 7, 1001).unwrap();
        let side = || super::MatchSide {
            name: "side".into(),
            elo_score: None,
            opening: super::Opening {
                choose: Some(0),
                offers: pred.deal.blue.clone(),
            },
            constructions: pred.constructions.blue.clone(),
            tech_loadout: economy.unit_technologies(),
            reactor_core: Some(4500),
            seed: None,
        };
        let mut deployed_side: super::SideState = serde_json::from_value(serde_json::json!({
            "random_state": [1, 2, 3, 4],
            "reactor_core": 4500,
            "supply": 200,
            "unlocked_units": [],
            "tower_strengthen_levels": [1, 2],
            "next_index": {"unit": 2, "contraption": 2},
            "units": [
                {"name": "steel_ball", "index": 0, "value": 200, "position": {"x": -330, "y": 50}, "rotated": true},
                {"name": "steel_ball", "index": 1, "value": 200, "position": {"x": -330, "y": 150}},
            ],
            "constructions": pred.constructions.blue.clone(),
            "contraptions": [
                {"name": "shield", "index": 0, "position": {"x": 0, "y": -80}},
                {"name": "interceptor", "index": 1, "position": {"x": 60, "y": -80}},
            ],
        }))
        .unwrap();
        deployed_side.shop = super::Allowances {
            buys_remaining: 2,
            unlocks_remaining: 1,
            contraptions_remaining: 8,
        };
        let game = super::Match {
            has_opening: true,
            game_build: crate::game_build().to_owned(),
            source: super::Source::Simulator,
            reinforce: true,
            max_round: 40,
            map_id: 1001,
            seed: 7,
            blue: side(),
            red: side(),
            turns: vec![super::Turn {
                round: 1,
                state: super::State {
                    random_state: [5, 6, 7, 8],
                    reinforce_pool: super::PoolDelta::default(),
                    reinforce_offers: None,
                    blue: deployed_side.clone(),
                    red: deployed_side,
                },
                actions: super::TurnActions::default(),
            }],
            result: None,
        };
        let expanded = super::canonical_yaml_with(&game, super::MatchFormat::Expanded).unwrap();
        assert!(!expanded.contains("tower_strengthen_levels"));
        assert!(expanded.contains("  towers:\n  - {name: energy_tower, index: 0, position: {x: -140, y: -170}, size: {width: 20, depth: 20}, level: 1}"));
        assert!(expanded.contains("size: {width: 50, depth: 20}"));
        assert!(expanded.contains("size: {width: 20, depth: 50}"));
        assert!(expanded.contains(
            "{name: interceptor, index: 1, position: {x: 60, y: -80}, size: {width: 30, depth: 30}}"
        ));
        assert!(!expanded.contains("{name: shield, index: 0, position: {x: 0, y: -80}, size:"));
        assert_eq!(super::read(expanded.as_bytes()).unwrap().unwrap(), game);

        for broken in [
            expanded.replace(
                "size: {width: 20, depth: 50}",
                "size: {width: 50, depth: 20}",
            ),
            expanded.replace("name: research_center", "name: energy_tower"),
            expanded.replace(
                "shield, index: 0, position: {x: 0, y: -80}",
                "shield, index: 0, position: {x: 0, y: -80}, size: {width: 20, depth: 20}",
            ),
            expanded.replace(
                "  towers:\n",
                "  tower_strengthen_levels: [1, 2]\n  towers:\n",
            ),
        ] {
            assert!(super::read(broken.as_bytes()).is_err(), "{broken}");
        }
    }

    fn pool_fixture_match(reinforce: bool) -> super::Match {
        let economy = crate::economy::Economy::embedded().unwrap();
        let initial = crate::reinforcement::initial_members(7).unwrap();
        let group1 = [30101, 30102, 30104, 30105];
        let removed = *group1
            .iter()
            .find(|id| initial.contains(id))
            .expect("group-1 card in seed 7 initial pool");
        let added = *group1
            .iter()
            .find(|id| !initial.contains(id))
            .expect("group-1 card outside seed 7 initial pool");
        let side = || super::MatchSide {
            name: "side".into(),
            elo_score: None,
            opening: super::Opening {
                choose: None,
                offers: Vec::new(),
            },
            constructions: Vec::new(),
            tech_loadout: economy.unit_technologies(),
            reactor_core: Some(4500),
            seed: None,
        };
        let side_state = || super::SideState {
            random_state: [1, 2, 3, 4],
            reactor_core: 4500,
            supply: 200,
            shop: super::Allowances {
                buys_remaining: 2,
                unlocks_remaining: 1,
                contraptions_remaining: 8,
            },
            next_index: super::NextIndex {
                unit: 0,
                contraption: 0,
            },
            tower_strengthen_levels: vec![0, 0],
            ..Default::default()
        };
        let delta = if reinforce {
            super::PoolDelta {
                removed: vec![removed],
                added: vec![added],
            }
        } else {
            super::PoolDelta::default()
        };
        super::Match {
            has_opening: true,
            game_build: crate::game_build().to_owned(),
            source: super::Source::Simulator,
            reinforce,
            max_round: 40,
            map_id: 1001,
            seed: 7,
            blue: side(),
            red: side(),
            turns: vec![
                super::Turn {
                    round: 1,
                    state: super::State {
                        random_state: [1, 2, 3, 4],
                        reinforce_pool: super::PoolDelta::default(),
                        reinforce_offers: None,
                        blue: side_state(),
                        red: side_state(),
                    },
                    actions: super::TurnActions::default(),
                },
                super::Turn {
                    round: 2,
                    state: super::State {
                        random_state: [5, 6, 7, 8],
                        reinforce_pool: delta,
                        reinforce_offers: None,
                        blue: side_state(),
                        red: side_state(),
                    },
                    actions: super::TurnActions::default(),
                },
            ],
            result: None,
        }
    }

    #[test]
    fn expanded_reinforce_pool_round_trips() {
        let game = pool_fixture_match(true);
        let expanded = super::canonical_yaml_with(&game, super::MatchFormat::Expanded).unwrap();
        assert_eq!(expanded.matches("reinforce_pool: [").count(), 3);
        assert_eq!(super::read(expanded.as_bytes()).unwrap().unwrap(), game);

        let no_reinforce = pool_fixture_match(false);
        let no_reinforce_expanded =
            super::canonical_yaml_with(&no_reinforce, super::MatchFormat::Expanded).unwrap();
        assert!(!no_reinforce_expanded.contains("reinforce_pool"));
        assert_eq!(
            super::read(no_reinforce_expanded.as_bytes())
                .unwrap()
                .unwrap(),
            no_reinforce
        );
    }

    #[test]
    fn expanded_reinforce_pool_refuses_invalid_lists() {
        let game = pool_fixture_match(true);
        let expanded = super::canonical_yaml_with(&game, super::MatchFormat::Expanded).unwrap();
        let no_reinforce = pool_fixture_match(false);
        let no_reinforce_expanded =
            super::canonical_yaml_with(&no_reinforce, super::MatchFormat::Expanded).unwrap();

        let initial = crate::reinforcement::initial_members(7).unwrap();
        let first_card = crate::names::card::name::<serde_yaml::Error>(initial[0]).unwrap();
        let second_card = crate::names::card::name::<serde_yaml::Error>(initial[1]).unwrap();
        let group1 = [30101, 30102, 30104, 30105];
        let mut with_group_dup = initial;
        let extra = *group1
            .iter()
            .find(|id| !with_group_dup.contains(id))
            .expect("spare group-1 card");
        with_group_dup.push(extra);
        with_group_dup.sort_unstable();
        let dup_names: Vec<_> = with_group_dup
            .iter()
            .map(|id| crate::names::card::name::<serde_yaml::Error>(*id).unwrap())
            .collect();
        let dup_line = format!("reinforce_pool: [{}]", dup_names.join(", "));
        let initial_line = expanded
            .lines()
            .find(|line| line.starts_with("reinforce_pool: ["))
            .unwrap();

        for invalid in [
            no_reinforce_expanded.replace(
                "round: 0\n",
                &format!("round: 0\nreinforce_pool: [{first_card}]\n"),
            ),
            expanded.replace(
                &format!("[{first_card}, {second_card}"),
                &format!("[{second_card}, {first_card}"),
            ),
            expanded.replace(&format!("[{first_card}, "), "[no_such_card, "),
            expanded.replacen(initial_line, &dup_line, 1),
            expanded.replacen(&format!("[{first_card}, "), "[", 1),
            super::canonical_yaml(&game).unwrap().replace(
                "round: 0\n",
                &format!("round: 0\nreinforce_pool:\n  removed: [{first_card}]\n"),
            ),
        ] {
            assert!(super::read(invalid.as_bytes()).is_err(), "{invalid}");
        }
    }
}
