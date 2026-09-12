//! The battle, turn and state documents.
//!
//! `docs/spec/document/battle.md`, `docs/spec/document/turn.md` and `docs/spec/document/state.md` define them. A battle
//! holds what every round of a match shares and the turns in order; a turn
//! holds the state a round starts from and the decisions taken from it.
//! Filling one from a replay is [`crate::convert`].

use crate::DocumentKind;
use crate::layout::{ContraptionPlacement, Formation, Position, StaticPlacement, Techs, Terrain};
use serde::Serialize;
use std::collections::BTreeMap;

/// One recorded match, as `docs/spec/document/battle.md` defines it.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Battle {
    pub kind: DocumentKind,
    pub map_id: i32,
    pub seed: i32,
    /// Present when a player conceded, which is the one ending a decision
    /// produces. Absent means the match ended some other way, and which way is
    /// not recorded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub concession: Option<Concession>,
    pub sides: BattleSides,
    pub turns: Vec<Turn>,
}

/// Which side a match-level fact belongs to.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Blue,
    Red,
}

/// How a match ended, when it ended because a player said so.
///
/// Conceding is a decision, but it is not an action: it ends the match rather
/// than moving the position the round started from, so it sits here and not in
/// a turn's sequence. `round` is the round the player was in when they
/// conceded, which is the last round the battle holds.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
pub struct Concession {
    pub side: Side,
    pub round: i32,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct BattleSides {
    pub blue: BattleSide,
    pub red: BattleSide,
}

/// What a side holds for the whole match rather than for one round.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct BattleSide {
    /// Which technologies each unit may research, keyed by unit ID.
    pub tech_loadout: BTreeMap<i32, Vec<i32>>,
}

/// One deployment round: the state it starts from and the decisions taken.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Turn {
    pub round: i32,
    pub state: State,
    pub actions: TurnActions,
}

/// A match position, carrying the state document's shape without its `kind`.
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
    /// The four openings this side was dealt, in round 0 and no other round.
    ///
    /// A decision needs what it chose between, and this offer is private to
    /// the side, so it sits here rather than beside the shared
    /// `reinforce_offers`. A replay records only the one taken, so a converted
    /// battle leaves it absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opening_offers: Option<Vec<Opening>>,
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

/// One of the openings a side was dealt: a team of formations and the
/// specialist officer bound to it.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Opening {
    pub team: i32,
    pub specialist: i32,
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
    /// The round 0 opening, which is one decision with two halves: the team
    /// of formations and the specialist officer bound to it.
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

/// Serializes a battle in the normal form the three documents define.
///
/// # Errors
///
/// Returns an error when the battle cannot be serialized.
pub fn canonical_yaml(battle: &Battle) -> Result<String, String> {
    let yaml = serde_yaml::to_string(battle)
        .map_err(|error| format!("cannot serialize battle YAML: {error}"))?;
    Ok(crate::layout::collapse_placement_positions(&yaml))
}
