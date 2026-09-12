//! Reads the native deployment observations `record_replay_battle` writes.
//!
//! `docs/spec/adapter/adapter.md` defines the file under `record_replay_battle`. A record
//! carries a position before a decision and the position after it, in the
//! game's own spelling, so the
//! stream is an oracle for [`crate::transition`]: map both positions onto
//! documents, apply the decision to the first, and the second says whether the
//! application was right.
//!
//! The mapping is the same one [`crate::convert`] performs on a replay's round
//! snapshot, over a live reconstruction rather than over the serialized XML.
//! Two differences follow from that and are the whole of what this module adds.
//! A live `supply` already holds the round's income, which a serialized
//! snapshot precedes, and a live shop counter is what the round has left, which
//! a serialized one states a round late. Neither needs the surrounding rounds,
//! so a position here maps on its own.

use crate::battle::{
    Action, DECLINED_OFFER, NextIndex, PanelSkill, Release, ShopState, SideState, SkillTarget,
    State, StateFormation, StateSides,
};
use crate::catalog::{construction_type_from_id, terrain_type_from_skill, unit_type_from_id};
use crate::grbr::{decode_grbr_byte_mask, decode_grbr_grid_groups, rotate_oil_grid_rows};
use crate::layout::{
    ContraptionPlacement, Formation, Position, StaticPlacement, Techs, Terrain, TerrainType,
};
use serde::Deserialize;
use std::collections::BTreeMap;

/// What an observation's header calls the format it is in.
///
/// A file says what it is, the way every document here does, so nothing has to
/// be inferred from an extension or from the shape of a line.
pub const SCHEMA: &str = "mechcore.battle-observation.v1";

/// Officers a research centre blueprint grants; `blueprints` owns them instead.
const CHAIN_OFFICERS: [i32; 4] = [20300, 20301, 20310, 20311];
/// Shield Airdrop, whose range item is one shield still standing on the board.
const SHIELD_AIRDROP_SKILL: i32 = 800_001;
/// How many points a Sticky Oil Bomb terrain carries.
const OIL_TERRAIN_POINT_COUNT: usize = 7;

/// Which half of the map a side plays on, and so how its positions are read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Seat {
    Blue,
    Red,
}

impl Seat {
    /// The seat a record's `team` names.
    ///
    /// # Errors
    ///
    /// Returns an error for any value but `0` and `1`.
    pub fn from_team(team: i32) -> Result<Self, String> {
        match team {
            0 => Ok(Self::Blue),
            1 => Ok(Self::Red),
            other => Err(format!(
                "observation names team {other}, which is not a seat"
            )),
        }
    }

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Blue => "blue",
            Self::Red => "red",
        }
    }

    /// Red's native coordinates are the blue frame turned half a turn.
    const fn local(self, x: i32, y: i32) -> Position {
        match self {
            Self::Blue => Position { x, y },
            Self::Red => Position { x: -x, y: -y },
        }
    }
}

/// One record of an observation file.
///
/// The eleven kinds differ in which position fields they carry rather than in
/// shape, so one struct with optional fields reads them all and [`Record::kind`]
/// resolves the kind.
#[derive(Debug, Deserialize)]
pub struct Record {
    pub kind: String,
    pub sequence: i64,
    /// Present on the header record alone, where it names the format.
    #[serde(default)]
    pub schema: Option<String>,
    #[serde(default)]
    pub round: Option<i32>,
    #[serde(default)]
    pub team: Option<i32>,
    #[serde(default)]
    pub native_type: Option<String>,
    #[serde(default)]
    pub ordinal: Option<i32>,
    #[serde(default)]
    pub accepted: Option<bool>,
    #[serde(default)]
    pub action: Option<NativeAction>,
    /// The position a decision was taken from.
    #[serde(default)]
    pub before: Option<Observed>,
    /// The position it produced.
    #[serde(default)]
    pub after: Option<Observed>,
    /// A `round_start` states one position rather than a pair.
    #[serde(default)]
    pub state: Option<Observed>,
    /// A `round_end`, `battle_end` or `summary` states its closing position.
    #[serde(default)]
    pub terminal: Option<Observed>,
}

/// One decision in the game's own spelling.
///
/// The fields are the union of the sixteen `PAD_` shapes, the same way
/// [`crate::record::ActionRecord`] reads the serialized ones. They differ in
/// spelling because one is XML and the other JSON, and in nothing else.
#[derive(Debug, Default, Deserialize)]
pub struct NativeAction {
    #[serde(default, rename = "ID")]
    pub id: Option<i32>,
    #[serde(default, rename = "Index")]
    pub index: Option<i32>,
    #[serde(default, rename = "UID")]
    pub unit_id: Option<i32>,
    #[serde(default, rename = "UIDX")]
    pub unit_index_allocated: Option<i32>,
    #[serde(default, rename = "TechID")]
    pub tech_id: Option<i32>,
    #[serde(default, rename = "SkillID")]
    pub skill_id: Option<i32>,
    #[serde(default, rename = "SkillIndex")]
    pub skill_index: Option<i32>,
    #[serde(default, rename = "UnitIndex")]
    pub unit_index: Option<i32>,
    #[serde(default, rename = "ConstructionIndex")]
    pub construction_index: Option<i32>,
    #[serde(default, rename = "EquipmentID")]
    pub equipment_id: Option<i32>,
    #[serde(default, rename = "ContraptionID")]
    pub contraption_id: Option<i32>,
    #[serde(default, rename = "position")]
    pub buy_position: Option<Vector>,
    #[serde(default, rename = "Position")]
    pub release_position: Option<Vector>,
    #[serde(default, rename = "ExtraPosition")]
    pub extra_position: Option<Vector>,
    #[serde(default, rename = "Positions")]
    pub positions: Option<Vec<Vector>>,
    #[serde(default, rename = "moveUnitDatas")]
    pub moves: Option<Vec<NativeMove>>,
}

#[derive(Debug, Deserialize)]
pub struct NativeMove {
    #[serde(rename = "unitIndex")]
    pub unit_index: i32,
    pub position: Vector,
    #[serde(rename = "isRotate")]
    pub rotated: bool,
}

/// A native two-component deployment coordinate, boxed the way the game writes
/// it in a decision. A snapshot writes the same pair unboxed.
#[derive(Debug, Deserialize)]
pub struct Vector {
    pub vector: RawVector,
}

#[derive(Clone, Copy, Debug, Deserialize)]
pub struct RawVector {
    #[serde(rename = "m_X")]
    pub x: i32,
    #[serde(rename = "m_Y")]
    pub y: i32,
}

/// One position, as both sides hold it.
#[derive(Debug, Deserialize)]
pub struct Observed {
    pub board: Board,
    pub sides: ObservedSides,
}

#[derive(Debug, Deserialize)]
pub struct ObservedSides {
    pub blue: ObservedSide,
    pub red: ObservedSide,
}

/// The board as the adapter projects it, which is a layout.
///
/// Two things are read from here and nothing else. `travelling` is native set
/// membership that no `PlayerData` field carries, and a released contraption
/// stands on the board without joining `PlayerData.contraptions` until the next
/// snapshot is taken. Every other field the projection carries restates the
/// snapshot, and reading it twice would let a projection bug pass for a state.
#[derive(Debug, Deserialize)]
pub struct Board {
    pub sides: BoardSides,
}

#[derive(Debug, Deserialize)]
pub struct BoardSides {
    pub blue: BoardSide,
    pub red: BoardSide,
}

#[derive(Debug, Deserialize)]
pub struct BoardSide {
    #[serde(default)]
    pub formations: Vec<BoardFormation>,
    #[serde(default)]
    pub contraptions: Vec<ContraptionPlacement>,
}

#[derive(Debug, Deserialize)]
pub struct BoardFormation {
    pub index: i32,
    #[serde(default)]
    pub travelling: bool,
}

#[derive(Debug, Deserialize)]
pub struct ObservedSide {
    /// The energy tower skills this round has activated.
    #[serde(default)]
    pub active_energy_tower_skills: Vec<i32>,
    pub deploy_over: bool,
    pub snapshot: Snapshot,
}

/// A side's native `PlayerData`, in the spelling the adapter writes.
#[derive(Debug, Deserialize)]
pub struct Snapshot {
    #[serde(rename = "reactorCore")]
    pub reactor_core: i32,
    pub supply: i32,
    pub shop: Shop,
    #[serde(default, rename = "bluepints")]
    pub blueprints: Vec<i32>,
    #[serde(default, rename = "towerStrengthenLevels")]
    pub tower_strengthen_levels: Vec<i32>,
    #[serde(default, rename = "equipmentDatas")]
    pub equipment: Vec<EquipmentEntry>,
    #[serde(default, rename = "commanderSkills")]
    pub commander_skills: Vec<CommanderSkill>,
    #[serde(rename = "unitIndex")]
    pub unit_index: i32,
    #[serde(rename = "contraptionIndex")]
    pub contraption_index: i32,
    #[serde(default)]
    pub officers: Vec<i32>,
    #[serde(default, rename = "activeTechnologies")]
    pub active_technologies: Vec<TechnologyRow>,
    #[serde(default)]
    pub units: Vec<UnitEntry>,
    #[serde(default, rename = "constructionSnapshotDatas")]
    pub constructions: Vec<ConstructionEntry>,
    #[serde(default)]
    pub contraptions: Vec<ContraptionEntry>,
}

#[derive(Debug, Deserialize)]
pub struct Shop {
    /// What is left of this round's purchases, counting down.
    #[serde(rename = "buyCount")]
    pub buys_remaining: i32,
    #[serde(rename = "unlockCount")]
    pub unlocks_remaining: i32,
    #[serde(default, rename = "unlockedUnits")]
    pub unlocked_units: Vec<i32>,
}

#[derive(Debug, Deserialize)]
pub struct EquipmentEntry {
    pub id: i32,
    pub durability: i32,
}

#[derive(Debug, Deserialize)]
pub struct CommanderSkill {
    pub index: i32,
    pub id: i32,
    #[serde(rename = "coolingRound")]
    pub cooling_round: i32,
    /// Whether this round has released the slot.
    #[serde(default, rename = "isActive")]
    pub released: bool,
    #[serde(default, rename = "rangeItems")]
    pub range_items: Vec<RangeItem>,
}

/// One object a skill left standing, which the round it opens outlived.
#[derive(Debug, Deserialize)]
pub struct RangeItem {
    #[serde(rename = "activeState")]
    pub active_state: i32,
    #[serde(default, rename = "gridInfo")]
    pub grid_info: Vec<i32>,
    #[serde(default)]
    pub positions: Vec<RawVector>,
    /// An oil terrain's remaining lifetime; a shield always records zero.
    pub round: i32,
}

#[derive(Debug, Deserialize)]
pub struct TechnologyRow {
    #[serde(default)]
    pub technologies: Vec<TechnologyRef>,
}

#[derive(Debug, Deserialize)]
pub struct TechnologyRef {
    #[serde(rename = "Value")]
    pub value: i32,
}

#[derive(Debug, Deserialize)]
pub struct UnitEntry {
    pub id: i32,
    pub index: i32,
    pub level: i32,
    pub exp: i32,
    pub position: RawVector,
    #[serde(rename = "isRotate")]
    pub rotated: bool,
    #[serde(rename = "equipmentID")]
    pub equipment: i32,
    #[serde(rename = "sellSupply")]
    pub sell_supply: i32,
}

#[derive(Debug, Deserialize)]
pub struct ConstructionEntry {
    pub id: i32,
    pub index: i32,
    pub position: Vector,
}

#[derive(Debug, Deserialize)]
pub struct ContraptionEntry {
    pub id: i32,
    pub index: i32,
    pub position: RawVector,
}

impl Observed {
    /// Maps this position onto the state document that describes it.
    ///
    /// # Errors
    ///
    /// Returns an error when a native ID has no layout type in this build, or
    /// when a retained object belongs to a skill this reader has not been
    /// measured against.
    pub fn state(&self) -> Result<State, String> {
        Ok(State {
            // A live position has no dealt offer to read: the four openings a
            // side refused are not board state.
            reinforce_offers: None,
            sides: StateSides {
                blue: self.side_state(Seat::Blue)?,
                red: self.side_state(Seat::Red)?,
            },
        })
    }

    /// Maps one side of this position.
    ///
    /// # Errors
    ///
    /// Returns an error for the same reasons [`Observed::state`] does.
    pub fn side_state(&self, seat: Seat) -> Result<SideState, String> {
        let (side, board) = match seat {
            Seat::Blue => (&self.sides.blue, &self.board.sides.blue),
            Seat::Red => (&self.sides.red, &self.board.sides.red),
        };
        let snapshot = &side.snapshot;
        let formations = formations(snapshot, board, seat)?;
        let constructions = constructions(snapshot, seat)?;
        // A contraption released this round stands on the board and is not in
        // `PlayerData.contraptions` until the next snapshot is taken, so the
        // board is what says which ones the position holds.
        let mut contraptions = board.contraptions.clone();
        contraptions.sort_by_key(|contraption| contraption.index);
        let battle_skills = panel(snapshot);
        let (terrains, airdrop_shields) = retained(&snapshot.commander_skills, seat)?;

        let mut officers: Vec<i32> = snapshot
            .officers
            .iter()
            .copied()
            .filter(|officer| !CHAIN_OFFICERS.contains(officer))
            .collect();
        officers.sort_unstable();
        let mut units: Vec<i32> = snapshot
            .active_technologies
            .iter()
            .flat_map(|row| row.technologies.iter().map(|tech| tech.value))
            .collect();
        units.sort_unstable();
        let mut blueprints = snapshot.blueprints.clone();
        blueprints.sort_unstable();
        let mut unlocked_units = snapshot.shop.unlocked_units.clone();
        unlocked_units.sort_unstable();
        let mut energy_tower_skills = side.active_energy_tower_skills.clone();
        energy_tower_skills.sort_unstable();

        Ok(SideState {
            opening_offers: None,
            reactor_core: snapshot.reactor_core,
            // A live supply already holds this round's income.
            supply: snapshot.supply,
            shop: ShopState {
                unlocked_units,
                buys_remaining: snapshot.shop.buys_remaining,
                unlocks_remaining: snapshot.shop.unlocks_remaining,
            },
            blueprints,
            energy_tower_skills,
            tower_strengthen_levels: snapshot.tower_strengthen_levels.clone(),
            equipment: unfitted(snapshot),
            battle_skills,
            next_index: NextIndex {
                unit: snapshot.unit_index,
                contraption: snapshot.contraption_index,
            },
            techs: Techs { officers, units },
            formations,
            constructions,
            contraptions,
            airdrop_shields,
            terrains,
        })
    }
}

/// Whether this side has said its deployment is over.
impl ObservedSide {
    #[must_use]
    pub const fn finished(&self) -> bool {
        self.deploy_over
    }
}

/// The roster, as the layout formations a projection would keep.
fn formations(
    snapshot: &Snapshot,
    board: &BoardSide,
    seat: Seat,
) -> Result<Vec<StateFormation>, String> {
    let travelling: BTreeMap<i32, bool> = board
        .formations
        .iter()
        .map(|formation| (formation.index, formation.travelling))
        .collect();
    let mut formations = Vec::with_capacity(snapshot.units.len());
    for unit in &snapshot.units {
        let (type_name, _) = unit_type_from_id(unit.id)
            .ok_or_else(|| format!("unit ID {} has no layout type", unit.id))?;
        formations.push(StateFormation {
            value: Some(unit.sell_supply),
            formation: Formation {
                type_name: type_name.to_owned(),
                index: unit.index,
                position: seat.local(unit.position.x, unit.position.y),
                // The record counts paid upgrades from zero; a layout displays
                // the level from one.
                level: Some(unit.level + 1).filter(|level| *level != 1),
                exp: Some(unit.exp).filter(|exp| *exp != 0),
                rotated: Some(unit.rotated).filter(|rotated| *rotated),
                equipment: Some(unit.equipment).filter(|id| *id != 0),
                travelling: travelling.get(&unit.index).copied().filter(|set| *set),
            },
        });
    }
    formations.sort_by_key(|entry| entry.formation.index);
    Ok(formations)
}

fn constructions(snapshot: &Snapshot, seat: Seat) -> Result<Vec<StaticPlacement>, String> {
    let mut constructions = Vec::with_capacity(snapshot.constructions.len());
    for construction in &snapshot.constructions {
        let (type_name, _) = construction_type_from_id(construction.id)
            .ok_or_else(|| format!("construction ID {} has no layout type", construction.id))?;
        constructions.push(StaticPlacement {
            type_name: type_name.to_owned(),
            index: construction.index,
            position: seat.local(
                construction.position.vector.x,
                construction.position.vector.y,
            ),
        });
    }
    constructions.sort_by_key(|construction| construction.index);
    Ok(constructions)
}

/// The skill panel, and which of its slots this round released.
///
/// A snapshot says that a slot was released and not where in the round or at
/// what, so the release read back here carries neither. Comparing a produced
/// release against one of these compares presence, which is what a snapshot
/// decides.
fn panel(snapshot: &Snapshot) -> Vec<PanelSkill> {
    let mut battle_skills: Vec<PanelSkill> = snapshot
        .commander_skills
        .iter()
        .map(|skill| PanelSkill {
            index: skill.index,
            id: skill.id,
            cooldown: skill.cooling_round,
            release: skill.released.then(|| Release {
                order: 0,
                target: SkillTarget::Area(Vec::new()),
            }),
        })
        .collect();
    battle_skills.sort_by_key(|skill| skill.index);
    battle_skills
}

/// What the side owns and no formation wears.
///
/// The native inventory holds every item the side has, fitted or not, so each
/// formation's own item takes one copy out of it.
fn unfitted(snapshot: &Snapshot) -> Vec<crate::battle::EquipmentItem> {
    let mut fitted: Vec<i32> = snapshot
        .units
        .iter()
        .map(|unit| unit.equipment)
        .filter(|id| *id != 0)
        .collect();
    let mut unfitted = Vec::new();
    for item in &snapshot.equipment {
        if let Some(position) = fitted.iter().position(|id| *id == item.id) {
            fitted.swap_remove(position);
            continue;
        }
        unfitted.push(crate::battle::EquipmentItem {
            id: item.id,
            durability: Some(item.durability).filter(|durability| *durability != -1),
        });
    }
    unfitted.sort_unstable();
    unfitted
}

/// The objects a skill left standing when the position was taken.
///
/// This is the live counterpart of [`crate::retained_from_grbr_round`], reading
/// the same `rangeItems` out of the same panel. The two kinds differ in what
/// they do with `round`: an oil terrain counts a remaining lifetime down and is
/// gone at zero, while a shield is not time-limited and records zero always.
fn retained(
    skills: &[CommanderSkill],
    seat: Seat,
) -> Result<(Vec<Terrain>, Vec<Position>), String> {
    let mut terrains = Vec::new();
    let mut shields = Vec::new();
    for skill in skills {
        for item in &skill.range_items {
            if skill.id == SHIELD_AIRDROP_SKILL {
                shields.push(shield_center(item, seat)?);
                continue;
            }
            // Any skill that leaves a battlefield area behind leaves one of
            // these, and which substance it is the catalogue says.
            let Some(terrain_type) = terrain_type_from_skill(skill.id) else {
                return Err(format!(
                    "observation carries an unsupported retained commander-skill object {}",
                    skill.id
                ));
            };
            // An area counts its remaining lifetime down and is gone at zero,
            // unlike a shield, which carries none.
            if item.round <= 0 {
                continue;
            }
            if let Some(terrain) = range_item_terrain(item, terrain_type, seat)? {
                terrains.push(terrain);
            }
        }
    }
    shields.sort_unstable_by_key(|position| (position.x, position.y));
    Ok((terrains, shields))
}

/// One retained battlefield area, or nothing when no point of it is still
/// active.
fn range_item_terrain(
    item: &RangeItem,
    terrain_type: TerrainType,
    seat: Seat,
) -> Result<Option<Terrain>, String> {
    let [start, end] = <[RawVector; 2]>::try_from(item.positions.as_slice()).map_err(|_| {
        format!(
            "retained-terrain observation requires two line endpoints, got {}",
            item.positions.len()
        )
    })?;
    let active = decode_grbr_byte_mask(item.active_state)?;
    let live = active.iter().filter(|point| **point).count();
    if live == 0 {
        return Ok(None);
    }
    let grids = decode_grbr_grid_groups(&item.grid_info)?;
    if grids.len() != live {
        return Err(format!(
            "retained-terrain observation has {live} active points but {} grids",
            grids.len()
        ));
    }
    let mut grid_rows = BTreeMap::new();
    let mut grid = 0;
    for (point, is_active) in active.into_iter().enumerate() {
        if !is_active {
            continue;
        }
        let mut rows = grids[grid].clone();
        grid += 1;
        if seat == Seat::Red {
            rows = rotate_oil_grid_rows(&rows);
        }
        grid_rows.insert(
            u32::try_from(point)
                .map_err(|_| "retained-terrain point index exceeds u32".to_owned())?,
            rows,
        );
    }
    if grid_rows.len() == OIL_TERRAIN_POINT_COUNT && grid_rows.values().all(Vec::is_empty) {
        grid_rows.clear();
    }
    Ok(Some(Terrain {
        terrain_type,
        control_points: vec![seat.local(start.x, start.y), seat.local(end.x, end.y)],
        grid_rows,
    }))
}

/// The centre of one retained Shield Airdrop.
fn shield_center(item: &RangeItem, seat: Seat) -> Result<Position, String> {
    if item.round != 0 {
        return Err(format!(
            "shield-airdrop observation carries lifetime {}, and the object has none",
            item.round
        ));
    }
    let [center] = <[RawVector; 1]>::try_from(item.positions.as_slice()).map_err(|_| {
        format!(
            "shield-airdrop observation requires one centre, got {}",
            item.positions.len()
        )
    })?;
    if decode_grbr_byte_mask(item.active_state)? != [true] {
        return Err(format!(
            "shield-airdrop observation activeState {} does not decode to one active point",
            item.active_state
        ));
    }
    Ok(seat.local(center.x, center.y))
}

impl Record {
    /// The decision this record states, or nothing when it states none.
    ///
    /// A retraction is a record and not a decision: `docs/spec/document/turn.md` defines a
    /// turn's sequence as what stands after the collapse, so `PAD_Undo`,
    /// `PAD_Redo` and `PAD_CancelReleaseCommanderSkill` resolve to nothing here
    /// rather than to an action no document can hold. `PAD_FinishDeploy` and
    /// `PAD_GiveUp` resolve to nothing for the reasons those documents give.
    ///
    /// A move resolves to one action per unit it carries, in the recorded
    /// order, which is the form a turn stores.
    ///
    /// `specialist` answers the one operand the record does not carry: the
    /// officer half of a round 0 opening, which only the position the choice
    /// produced states. Every other kind ignores it.
    ///
    /// # Errors
    ///
    /// Returns an error when the record names a decision whose operands are
    /// missing, or a `PAD_` kind no document defines.
    pub fn actions(&self, seat: Seat, specialist: Option<i32>) -> Result<Vec<Action>, String> {
        let Some(native) = self.native_type.as_deref() else {
            return Ok(Vec::new());
        };
        let Some(action) = self.action.as_ref() else {
            return Ok(Vec::new());
        };
        let field = |name: &'static str, value: Option<i32>| {
            value.ok_or_else(|| format!("{native} has no {name}"))
        };
        let position = |value: &Option<Vector>| {
            value
                .as_ref()
                .map(|boxed| seat.local(boxed.vector.x, boxed.vector.y))
                .ok_or_else(|| format!("{native} has no position"))
        };
        Ok(vec![match native {
            "PAD_ChooseReinforceItem" => {
                let offer = field("Index", action.index)?;
                Action::ChooseReinforceItem {
                    offer,
                    id: if offer == DECLINED_OFFER {
                        None
                    } else {
                        Some(field("ID", action.id)?)
                    },
                }
            }
            // The record logs the team half and not the specialist half, so
            // the caller reads the specialist out of the position the choice
            // produced and hands it in.
            "PAD_ChooseAdvanceTeam" => Action::ChooseAdvanceTeam {
                offer: field("Index", action.index)?,
                id: field("ID", action.id)?,
                specialist,
            },
            "PAD_BuyUnit" => Action::BuyUnit {
                unit: field("UID", action.unit_id)?,
                position: position(&action.buy_position)?,
            },
            "PAD_UpgradeUnit" => Action::UpgradeUnit {
                index: field("UIDX", action.unit_index_allocated)?,
            },
            "PAD_UnlockUnit" => Action::UnlockUnit {
                unit: field("UID", action.unit_id)?,
            },
            "PAD_UpgradeTechnology" => Action::UpgradeTechnology {
                unit: field("UID", action.unit_id)?,
                tech: field("TechID", action.tech_id)?,
            },
            "PAD_ActiveBlueprint" => Action::ActiveBlueprint {
                id: field("ID", action.id)?,
            },
            "PAD_ActiveEnergyTowerSkill" => Action::ActiveEnergyTowerSkill {
                skill: field("SkillID", action.skill_id)?,
            },
            "PAD_StrengthenTower" => Action::StrengthenTower {
                tower: field("Index", action.index)?,
            },
            "PAD_UseEquipment" => Action::UseEquipment {
                equipment: field("EquipmentID", action.equipment_id)?,
                unit: field("UnitIndex", action.unit_index)?,
            },
            "PAD_ReleaseCommanderSkill" => Action::ReleaseCommanderSkill {
                skill: field("SkillIndex", action.skill_index)?,
                target: skill_target(action, seat)?,
            },
            "PAD_ReleaseContraption" => Action::ReleaseContraption {
                contraption: field("ContraptionID", action.contraption_id)?,
                position: position(&action.release_position)?,
                extra_position: action
                    .extra_position
                    .as_ref()
                    .filter(|extra| extra.vector.x != 0 || extra.vector.y != 0)
                    .map(|extra| seat.local(extra.vector.x, extra.vector.y)),
            },
            "PAD_MoveUnit" => {
                let moves = action
                    .moves
                    .as_ref()
                    .ok_or_else(|| "PAD_MoveUnit has no moveUnitDatas".to_owned())?;
                return Ok(moves
                    .iter()
                    .map(|moved| Action::MoveUnit {
                        index: moved.unit_index,
                        position: seat.local(moved.position.vector.x, moved.position.vector.y),
                        rotated: moved.rotated,
                    })
                    .collect());
            }
            "PAD_Undo"
            | "PAD_Redo"
            | "PAD_CancelReleaseCommanderSkill"
            | "PAD_FinishDeploy"
            | "PAD_GiveUp" => return Ok(Vec::new()),
            other => return Err(format!("action {other} has no turn representation")),
        }])
    }
}

/// Resolves the exclusive target of a release.
fn skill_target(action: &NativeAction, seat: Seat) -> Result<SkillTarget, String> {
    if let Some(unit) = action.unit_index.filter(|index| *index >= 0) {
        return Ok(SkillTarget::Unit(unit));
    }
    if let Some(construction) = action.construction_index.filter(|index| *index >= 0) {
        return Ok(SkillTarget::Construction(construction));
    }
    let positions: Vec<Position> = action
        .positions
        .as_ref()
        .map(|positions| {
            positions
                .iter()
                .map(|position| seat.local(position.vector.x, position.vector.y))
                .collect()
        })
        .unwrap_or_default();
    if positions.is_empty() {
        return Err("PAD_ReleaseCommanderSkill names neither an object nor an area".into());
    }
    Ok(SkillTarget::Area(positions))
}

/// Reads one observation file into its records.
///
/// # Errors
///
/// Returns an error naming the line that does not parse.
pub fn read(text: &str) -> Result<Vec<Record>, String> {
    read_records(text)
}

/// Whether a file calls itself an observation.
///
/// Only the first record is read, and the question has three answers rather
/// than two. A file whose first line is a JSON object is a record stream, and
/// then its header has to name a schema this build reads. A file whose first
/// line is anything else is not a stream at all, and the caller is free to try
/// it as one of the YAML documents.
///
/// The middle case is the one worth separating. YAML is a superset of JSON, so
/// a stream that was truncated before its header, or one carrying a schema from
/// another build, would otherwise reach the layout parser and be refused for
/// the wrong reason.
///
/// # Errors
///
/// Returns an error when the file is a record stream whose first record does
/// not name this format.
pub fn is_observation(text: &str) -> Result<bool, String> {
    let Some(first) = text.lines().find(|line| !line.trim().is_empty()) else {
        return Ok(false);
    };
    let Ok(serde_json::Value::Object(record)) = serde_json::from_str::<serde_json::Value>(first)
    else {
        return Ok(false);
    };
    let schema = record.get("schema").and_then(serde_json::Value::as_str);
    let kind = record.get("kind").and_then(serde_json::Value::as_str);
    if kind == Some("header") && schema == Some(SCHEMA) {
        return Ok(true);
    }
    Err(match schema {
        Some(named) => {
            format!("first record names schema {named:?}, and this build reads {SCHEMA:?}")
        }
        None => format!(
            "first record is a JSON object naming no schema, so it is not a \
             {SCHEMA:?} stream; a layout document is YAML with `kind: layout` at its root"
        ),
    })
}

fn read_records(text: &str) -> Result<Vec<Record>, String> {
    let mut records = Vec::new();
    for (line, text) in text.lines().enumerate() {
        if text.trim().is_empty() {
            continue;
        }
        records.push(
            serde_json::from_str::<Record>(text)
                .map_err(|error| format!("observation line {}: {error}", line + 1))?,
        );
    }
    Ok(records)
}

/// Collapses one side's records of a round onto the decisions that stand.
///
/// This is `docs/spec/document/turn.md`'s net-decision collapse over a live stream rather than
/// over a serialized list, and it is the same rule: every record is one entry
/// on the undo stack, `PAD_Undo` pops the newest entry whether or not it still
/// stands, `PAD_Redo` pushes it back, a cancel spends the newest standing
/// release with its panel index and is itself pushed as a spent entry, and
/// finishing a deployment clears what could be redone.
#[must_use]
pub fn net_records<'a>(records: &[&'a Record]) -> Vec<&'a Record> {
    /// An entry that no longer stands for a decision but still absorbs an undo.
    const SPENT: bool = false;
    let mut taken: Vec<(&Record, bool)> = Vec::with_capacity(records.len());
    let mut undone: Vec<(&Record, bool)> = Vec::new();
    for record in records {
        match record.native_type.as_deref().unwrap_or_default() {
            "PAD_Undo" => {
                if let Some(last) = taken.pop() {
                    undone.push(last);
                }
            }
            "PAD_Redo" => {
                if let Some(last) = undone.pop() {
                    taken.push(last);
                }
            }
            "PAD_CancelReleaseCommanderSkill" => {
                undone.clear();
                let cancelled = record.action.as_ref().and_then(|action| action.skill_index);
                if let Some(entry) = taken.iter_mut().rev().find(|(candidate, stands)| {
                    *stands
                        && candidate.native_type.as_deref() == Some("PAD_ReleaseCommanderSkill")
                        && candidate
                            .action
                            .as_ref()
                            .and_then(|action| action.skill_index)
                            == cancelled
                }) {
                    entry.1 = SPENT;
                }
                taken.push((record, SPENT));
            }
            "PAD_FinishDeploy" => undone.clear(),
            _ => {
                undone.clear();
                taken.push((record, true));
            }
        }
    }
    taken
        .into_iter()
        .filter_map(|(record, stands)| stands.then_some(record))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{Record, Seat, is_observation, net_records, read};
    use crate::battle::{Action, SkillTarget};
    use crate::layout::Position;

    /// One action record, in the spelling the adapter writes.
    fn record(native: &str, action: &str) -> Record {
        let line = format!(
            "{{\"kind\":\"action\",\"sequence\":1,\"round\":3,\"team\":0,\
              \"native_type\":\"{native}\",\"action\":{action}}}"
        );
        read(&line).expect("one record").pop().expect("one record")
    }

    /// A move carries several units and resolves to one decision per unit.
    #[test]
    fn a_move_resolves_to_one_decision_per_unit() {
        let moved = record(
            "PAD_MoveUnit",
            "{\"moveUnitDatas\":[\
               {\"unitIndex\":0,\"position\":{\"vector\":{\"m_X\":85,\"m_Y\":-80}},\"isRotate\":false},\
               {\"unitIndex\":3,\"position\":{\"vector\":{\"m_X\":45,\"m_Y\":-85}},\"isRotate\":true}]}",
        );
        assert_eq!(
            moved.actions(Seat::Blue, None).unwrap(),
            vec![
                Action::MoveUnit {
                    index: 0,
                    position: Position { x: 85, y: -80 },
                    rotated: false,
                },
                Action::MoveUnit {
                    index: 3,
                    position: Position { x: 45, y: -85 },
                    rotated: true,
                },
            ]
        );
    }

    /// Red's native coordinates are the blue frame turned half a turn.
    #[test]
    fn red_reads_its_own_frame() {
        let bought = record(
            "PAD_BuyUnit",
            "{\"UID\":30,\"position\":{\"vector\":{\"m_X\":0,\"m_Y\":160}}}",
        );
        assert_eq!(
            bought.actions(Seat::Red, None).unwrap(),
            vec![Action::BuyUnit {
                unit: 30,
                position: Position { x: 0, y: -160 },
            }]
        );
    }

    /// A release names one object or one area, never both.
    ///
    /// The recorded shape is not exclusive: a pointing release also carries the
    /// click point, which names no state.
    #[test]
    fn a_release_resolves_to_its_exclusive_target() {
        let pointed = record(
            "PAD_ReleaseCommanderSkill",
            "{\"SkillIndex\":0,\"UnitIndex\":4,\"ConstructionIndex\":-1,\
              \"Positions\":[{\"vector\":{\"m_X\":103,\"m_Y\":-63}}]}",
        );
        assert_eq!(
            pointed.actions(Seat::Blue, None).unwrap(),
            vec![Action::ReleaseCommanderSkill {
                skill: 0,
                target: SkillTarget::Unit(4),
            }]
        );
        let area = record(
            "PAD_ReleaseCommanderSkill",
            "{\"SkillIndex\":1,\"UnitIndex\":-1,\"ConstructionIndex\":-1,\
              \"Positions\":[{\"vector\":{\"m_X\":10,\"m_Y\":-20}}]}",
        );
        assert_eq!(
            area.actions(Seat::Blue, None).unwrap(),
            vec![Action::ReleaseCommanderSkill {
                skill: 1,
                target: SkillTarget::Area(vec![Position { x: 10, y: -20 }]),
            }]
        );
    }

    /// Declining names no item, and the record spells its offer as `-1`.
    #[test]
    fn declining_takes_no_item() {
        let declined = record("PAD_ChooseReinforceItem", "{\"Index\":-1,\"ID\":0}");
        assert_eq!(
            declined.actions(Seat::Blue, None).unwrap(),
            vec![Action::ChooseReinforceItem {
                offer: -1,
                id: None,
            }]
        );
    }

    /// A retraction is a record and not a decision.
    #[test]
    fn a_retraction_states_no_decision() {
        for native in ["PAD_Undo", "PAD_Redo", "PAD_FinishDeploy"] {
            assert!(
                record(native, "{}")
                    .actions(Seat::Blue, None)
                    .unwrap()
                    .is_empty()
            );
        }
    }

    /// An undo steps back over a recorded entry rather than a standing one.
    ///
    /// The cancel and the release it retracts are both entries, so the two of
    /// them absorb two undos between them and the purchase before them stands.
    #[test]
    fn the_collapse_counts_entries_rather_than_decisions() {
        let bought = record(
            "PAD_BuyUnit",
            "{\"UID\":9,\"position\":{\"vector\":{\"m_X\":0,\"m_Y\":-160}}}",
        );
        let released = record(
            "PAD_ReleaseCommanderSkill",
            "{\"SkillIndex\":0,\"UnitIndex\":1}",
        );
        let cancelled = record("PAD_CancelReleaseCommanderSkill", "{\"SkillIndex\":0}");
        let undo = record("PAD_Undo", "{}");
        let taken = vec![&bought, &released, &cancelled, &undo, &undo];
        let standing = net_records(&taken);
        assert_eq!(standing.len(), 1);
        assert_eq!(standing[0].native_type.as_deref(), Some("PAD_BuyUnit"));
    }

    /// A cancel spends the release with its own panel index.
    #[test]
    fn a_cancel_matches_its_release_on_the_panel_index() {
        let first = record(
            "PAD_ReleaseCommanderSkill",
            "{\"SkillIndex\":0,\"UnitIndex\":1}",
        );
        let second = record(
            "PAD_ReleaseCommanderSkill",
            "{\"SkillIndex\":1,\"UnitIndex\":2}",
        );
        let cancelled = record("PAD_CancelReleaseCommanderSkill", "{\"SkillIndex\":0}");
        let taken = vec![&first, &second, &cancelled];
        let standing = net_records(&taken);
        let indices: Vec<Option<i32>> = standing
            .iter()
            .map(|record| record.action.as_ref().and_then(|action| action.skill_index))
            .collect();
        assert_eq!(indices, vec![Some(1)]);
    }

    /// A blank line is not a record, and a malformed one names its line.
    #[test]
    fn a_malformed_line_names_itself() {
        assert!(read("\n\n").unwrap().is_empty());
        let error = read("{\"kind\":\"action\"}").unwrap_err();
        assert!(error.starts_with("observation line 1:"), "{error}");
    }

    /// A file says what it is, and a stream that says something else says so.
    ///
    /// YAML is a superset of JSON, so a stream whose header is missing or
    /// carries another build's schema would otherwise reach the layout parser
    /// and be refused for the wrong reason.
    #[test]
    fn a_file_is_routed_by_the_schema_it_names() {
        let header = format!(
            "{{\"kind\":\"header\",\"schema\":\"{}\",\"sequence\":0}}",
            super::SCHEMA
        );
        assert_eq!(is_observation(&header), Ok(true));
        assert_eq!(is_observation("kind: layout\nround: 1\n"), Ok(false));
        assert_eq!(is_observation(""), Ok(false));

        let older = "{\"kind\":\"header\",\"schema\":\"mechcore.battle-observation.v0\"}";
        let error = is_observation(older).unwrap_err();
        assert!(error.contains("mechcore.battle-observation.v0"), "{error}");

        // A stream whose header never arrived is a stream, not a document.
        let truncated = "{\"kind\":\"action\",\"sequence\":9}";
        assert!(is_observation(truncated).is_err());
    }
}
