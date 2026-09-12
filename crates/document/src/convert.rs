//! Fills a battle document from one recorded match.
//!
//! Most fields are copied. Four are rebuilt, because the snapshot the game
//! writes precedes the round's own reset: `supply` gains the round's income,
//! the two shop counters are restored to the round's allowance, the energy
//! tower list is the set activated during the round and so empty at its
//! start, and the equipment list drops what the formations carry.
//! `docs/spec/document/battle.md` says what the conversion refuses.

use crate::battle::{
    Action, Battle, BattleSide, BattleSides, Concession, DECLINED_OFFER, EquipmentItem, NextIndex,
    PanelSkill, ShopState, Side, SideState, SkillTarget, State, StateFormation, StateSides, Turn,
    TurnActions,
};
use crate::catalog::{construction_type_from_id, contraption_type_from_id, unit_type_from_id};
use crate::layout::{ContraptionPlacement, Formation, Position, StaticPlacement, Techs};
use crate::record::{self, ActionRecord, PlayerData, PlayerRoundRecord};
use crate::economy::{Economy, OpeningKind, RoundSupply};
use crate::ledger;
use crate::{DocumentKind, retained_from_grbr_round};
use std::collections::BTreeMap;

/// The build these catalogues and conventions are pinned to.
const BUILD: &str = "2259";
/// `Shop.BUY_COUNT_PER_ROUND`, before any officer or energy tower modifier.
const BUY_COUNT_PER_ROUND: i32 = 2;
/// `Shop.UNLOCK_COUNT_PER_ROUND`.
const UNLOCK_COUNT_PER_ROUND: i32 = 1;
/// Energy tower skill `1` 快速补给 pays 200 now against this at the next round.
const RAPID_SUPPLY_DEBT: i32 = 300;
/// Energy tower skill `1`, the only one carrying a next-round supply change.
const RAPID_SUPPLY_SKILL: i32 = 1;
/// Energy tower skill `3` 批量征召, which raises this round's buy allowance.
const MASS_RECRUIT_SKILL: i32 = 3;
/// Officers a research centre blueprint grants; `blueprints` owns them instead.
const CHAIN_OFFICERS: [i32; 4] = [20300, 20301, 20310, 20311];

/// Which half of the map a side plays on, and so how its positions are read.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Seat {
    Blue,
    Red,
}

impl Seat {
    const fn side(self) -> Side {
        match self {
            Self::Blue => Side::Blue,
            Self::Red => Side::Red,
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Blue => "blue",
            Self::Red => "red",
        }
    }

    /// Red's recorded coordinates are the blue frame turned half a turn.
    fn position(self, record: &record::PositionRecord) -> Position {
        match self {
            Self::Blue => Position {
                x: record.x,
                y: record.y,
            },
            Self::Red => Position {
                x: -record.x,
                y: -record.y,
            },
        }
    }
}

/// Converts one locally recorded GRBR replay into a battle document.
///
/// # Errors
///
/// Returns an error when the replay is not a build-2259 standard 1v1 recorded
/// by this machine, when its rounds are not the contiguous sequence both sides
/// and the match share, or when it contains an object this build's catalogues
/// or this format cannot name.
pub fn battle_from_grbr(grbr: &[u8]) -> Result<Battle, String> {
    let record = record::read(grbr)?;
    if record.version != BUILD {
        return Err(format!(
            "replay is build {}, and this converter reads build {BUILD}",
            record.version
        ));
    }
    if record.seat < 0 {
        return Err(
            "replay was downloaded from the server, whose snapshots are reconstructions; \
             see tests/grbr/README.md"
                .into(),
        );
    }
    if record.info.match_mode != "VS_1_1" {
        return Err(format!(
            "replay is match mode {}, and this format describes VS_1_1",
            record.info.match_mode
        ));
    }
    if let Some(match_type) = &record.info.match_type {
        return Err(format!(
            "replay is a {match_type} match, not a played one; \
             its snapshots were installed by Training Ground commands"
        ));
    }
    if !record.info.game_rules.values.is_empty() {
        return Err(format!(
            "replay carries game rules {:?}, which this format has not been measured against",
            record.info.game_rules.values
        ));
    }
    let [blue, red] = <[record::PlayerRecord; 2]>::try_from(record.players.entries)
        .map_err(|players| format!("replay has {} sides, and a battle has two", players.len()))?;

    let match_rounds: Vec<i32> = record
        .match_rounds
        .entries
        .iter()
        .map(|entry| entry.round)
        .collect();
    let expected: Vec<i32> = (0..i32::try_from(match_rounds.len()).unwrap_or(i32::MAX)).collect();
    if match_rounds != expected {
        return Err(format!(
            "match rounds {match_rounds:?} are not the contiguous sequence from zero"
        ));
    }
    for (seat, player) in [("blue", &blue), ("red", &red)] {
        let rounds: Vec<i32> = player.rounds.entries.iter().map(|entry| entry.round).collect();
        if rounds != match_rounds {
            return Err(format!(
                "{seat} records rounds {rounds:?} while the match records {match_rounds:?}"
            ));
        }
    }

    let economy = Economy::embedded()?;
    let mut turns = Vec::with_capacity(match_rounds.len());
    for (position, round) in match_rounds.iter().copied().enumerate() {
        let offers = record.match_rounds.entries[position]
            .reinforce_items
            .arrays
            .first()
            .map(|array| array.values.clone());
        turns.push(Turn {
            round,
            state: State {
                reinforce_offers: offers,
                sides: StateSides {
                    blue: side_state(grbr, &economy, &blue, position, Seat::Blue)?,
                    red: side_state(grbr, &economy, &red, position, Seat::Red)?,
                },
            },
            actions: TurnActions {
                blue: actions(
                    &blue.rounds.entries[position],
                    Seat::Blue,
                    opening_specialist(&economy, &blue, position),
                )?,
                red: actions(
                    &red.rounds.entries[position],
                    Seat::Red,
                    opening_specialist(&economy, &red, position),
                )?,
            },
        });
    }

    Ok(Battle {
        kind: DocumentKind::Battle,
        map_id: record.info.map_id,
        seed: record.info.system_seed,
        concession: concession(&blue, &red)?,
        sides: BattleSides {
            blue: battle_side(&blue),
            red: battle_side(&red),
        },
        turns,
    })
}

/// How the match ended, when a player ended it.
///
/// `PAD_GiveUp` is the one recorded action that overrides `IsExitMatchAction`,
/// and the override returns true unconditionally: it leaves the match rather
/// than moving the position its round started from. So it is read out of the
/// action list and written at the battle's root, and no turn carries it.
///
/// # Errors
///
/// Returns an error when both sides concede, which no match can produce: the
/// first concession ends it.
fn concession(
    blue: &record::PlayerRecord,
    red: &record::PlayerRecord,
) -> Result<Option<Concession>, String> {
    let mut conceded = Vec::new();
    for (seat, player) in [(Seat::Blue, blue), (Seat::Red, red)] {
        for entry in &player.rounds.entries {
            if net_actions(&entry.actions.entries)
                .iter()
                .any(|action| action.kind == "PAD_GiveUp")
            {
                conceded.push(Concession {
                    side: seat.side(),
                    round: entry.round,
                });
            }
        }
    }
    match conceded.as_slice() {
        [] => Ok(None),
        [only] => Ok(Some(*only)),
        many => Err(format!(
            "replay records {} concessions, and the first one ends the match",
            many.len()
        )),
    }
}

fn battle_side(player: &record::PlayerRecord) -> BattleSide {
    let mut loadout = BTreeMap::new();
    for row in &player.data.unit_datas.entries {
        let mut techs: Vec<i32> = row.techs.entries.iter().map(|tech| tech.data).collect();
        techs.sort_unstable();
        loadout.insert(row.id, techs);
    }
    BattleSide {
        tech_loadout: loadout,
    }
}

fn side_state(
    grbr: &[u8],
    economy: &Economy,
    player: &record::PlayerRecord,
    position: usize,
    seat: Seat,
) -> Result<SideState, String> {
    let entry = &player.rounds.entries[position];
    let data = &entry.data;
    let round = entry.round;

    let formations = formations(data, seat)?;
    let constructions = constructions(data, seat)?;
    let contraptions = contraptions(data, seat)?;

    let mut battle_skills = Vec::with_capacity(data.commander_skills.entries.len());
    for skill in &data.commander_skills.entries {
        battle_skills.push(PanelSkill {
            index: skill.index,
            id: skill.id,
            cooldown: skill.cooling_round,
            // A converted state opens a round, and a round opens with nothing
            // released. The round's releases are its actions.
            release: None,
        });
    }
    battle_skills.sort_by_key(|skill| skill.index);

    let retained = retained_from_grbr_round(grbr, u32::try_from(round).unwrap_or(0))?;
    let mut retained = match seat {
        Seat::Blue => retained.blue,
        Seat::Red => retained.red,
    };
    retained
        .airdrop_shields
        .sort_unstable_by_key(|position| (position.x, position.y));

    let mut blueprints = data.blueprints.values.clone();
    blueprints.sort_unstable();

    let mut officers: Vec<i32> = data
        .officers
        .values
        .iter()
        .copied()
        .filter(|officer| !CHAIN_OFFICERS.contains(officer))
        .collect();
    officers.sort_unstable();

    let mut units: Vec<i32> = data
        .active_technologies
        .entries
        .iter()
        .flat_map(|row| row.techs.entries.iter().map(|tech| tech.data))
        .collect();
    units.sort_unstable();

    let mut unlocked_units = data.shop.unlocked_units.values.clone();
    unlocked_units.sort_unstable();

    Ok(SideState {
        // A replay records the opening taken and not the three refused.
        opening_offers: None,
        reactor_core: data.reactor_core,
        supply: data.supply + round_income(economy, player, position, seat)?,
        shop: ShopState {
            unlocked_units,
            buys_remaining: allowance(player, position, Allowance::Buy),
            unlocks_remaining: allowance(player, position, Allowance::Unlock),
        },
        blueprints,
        // Every energy tower skill lasts one round, so a round starts with none.
        energy_tower_skills: Vec::new(),
        tower_strengthen_levels: data.tower_strengthen_levels.values.clone(),
        equipment: unfitted_equipment(data),
        battle_skills,
        next_index: NextIndex {
            unit: data.unit_index,
            contraption: data.contraption_index,
        },
        techs: Techs { officers, units },
        formations,
        constructions,
        contraptions,
        airdrop_shields: retained.airdrop_shields,
        terrains: retained.terrains,
    })
}

/// The unit roster, as the layout formations a projection would keep.
fn formations(data: &PlayerData, seat: Seat) -> Result<Vec<StateFormation>, String> {
    let mut formations = Vec::with_capacity(data.units.entries.len());
    for unit in &data.units.entries {
        let (type_name, _) = unit_type_from_id(unit.id)
            .ok_or_else(|| format!("unit ID {} has no layout type in build {BUILD}", unit.id))?;
        formations.push(StateFormation {
            value: Some(unit.sell_supply),
            formation: Formation {
            type_name: type_name.to_owned(),
            index: unit.index,
            position: seat.position(&unit.position),
            // The record counts paid upgrades from zero; a layout displays the
            // level from one.
            level: Some(unit.level + 1).filter(|level| *level != 1),
            exp: Some(unit.exp).filter(|exp| *exp != 0),
            rotated: Some(unit.rotated).filter(|rotated| *rotated),
            equipment: Some(unit.equipment_id).filter(|id| *id != 0),
            // No recorded field states it; see docs/spec/document/battle.md.
            travelling: None,
            },
        });
    }
    formations.sort_by_key(|entry| entry.formation.index);
    Ok(formations)
}

fn constructions(data: &PlayerData, seat: Seat) -> Result<Vec<StaticPlacement>, String> {
    let mut constructions = Vec::with_capacity(data.constructions.entries.len());
    for construction in &data.constructions.entries {
        let (type_name, _) = construction_type_from_id(construction.id).ok_or_else(|| {
            format!(
                "construction ID {} has no layout type in build {BUILD}",
                construction.id
            )
        })?;
        constructions.push(StaticPlacement {
            type_name: type_name.to_owned(),
            index: construction.index,
            position: seat.position(&construction.position),
        });
    }
    constructions.sort_by_key(|construction| construction.index);
    Ok(constructions)
}

fn contraptions(data: &PlayerData, seat: Seat) -> Result<Vec<ContraptionPlacement>, String> {
    let mut contraptions = Vec::with_capacity(data.contraptions.entries.len());
    for contraption in &data.contraptions.entries {
        let type_name = contraption_type_from_id(contraption.id).ok_or_else(|| {
            format!(
                "contraption ID {} has no layout type in build {BUILD}",
                contraption.id
            )
        })?;
        contraptions.push(ContraptionPlacement {
            type_name: type_name.to_owned(),
            index: contraption.index,
            position: seat.position(&contraption.position),
        });
    }
    contraptions.sort_by_key(|contraption| contraption.index);
    Ok(contraptions)
}

/// The recorded inventory holds fitted items too, and a state stores the rest.
fn unfitted_equipment(data: &PlayerData) -> Vec<EquipmentItem> {
    let mut fitted: Vec<i32> = data
        .units
        .entries
        .iter()
        .map(|unit| unit.equipment_id)
        .filter(|id| *id != 0)
        .collect();
    let mut unfitted = Vec::new();
    for item in &data.equipment.entries {
        if let Some(position) = fitted.iter().position(|id| *id == item.id) {
            fitted.swap_remove(position);
            continue;
        }
        unfitted.push(EquipmentItem {
            id: item.id,
            durability: Some(item.durability).filter(|durability| *durability != -1),
        });
    }
    unfitted.sort_unstable();
    unfitted
}

/// Whether last round's Rapid Supply is deducted from this round's income.
///
/// Two readings of one fact have to agree, and this is where they meet. The
/// debt follows from the previous round's own decisions, which is what the
/// ledger prices. The game also snapshots it: the activation flag survives into
/// the round after the one that set it, so a round's recorded
/// `energyTowerSkills` names the previous round's activation rather than its
/// own, and only skill `1` has a deferred half to be snapshotted at all.
///
/// # Errors
///
/// Returns an error when the two disagree, which means either the collapse of
/// the previous round's actions or the reading of the recorded list is wrong.
/// Guessing which would put a supply figure into the document that no reading
/// supports.
fn energy_tower_debt(
    player: &record::PlayerRecord,
    position: usize,
    seat: Seat,
) -> Result<bool, String> {
    let entry = &player.rounds.entries[position];
    let owed = position.checked_sub(1).is_some_and(|previous| {
        net_actions(&player.rounds.entries[previous].actions.entries)
            .iter()
            .any(|action| {
                action.kind == "PAD_ActiveEnergyTowerSkill"
                    && action.skill_id == Some(RAPID_SUPPLY_SKILL)
            })
    });
    let expected: &[i32] = if owed { &[RAPID_SUPPLY_SKILL] } else { &[] };
    let recorded = entry.data.energy_tower_skills.values.as_slice();
    if recorded == expected {
        return Ok(owed);
    }
    Err(format!(
        "{} round {} records energy tower skills {recorded:?}, and the round before it \
         decided {expected:?}; see docs/spec/document/battle.md",
        seat.name(),
        entry.round
    ))
}

/// The income this round adds, which the recorded supply precedes.
///
/// The map's own row is recorded per player, so no map catalogue is consulted.
/// An energy tower skill activated last round is paid for here.
fn round_income(
    economy: &Economy,
    player: &record::PlayerRecord,
    position: usize,
    seat: Seat,
) -> Result<i32, String> {
    let debt = energy_tower_debt(player, position, seat)?;
    let entry = &player.rounds.entries[position];
    let round = entry.round;
    if round < 1 {
        return Ok(0);
    }
    let setup = &player.data;
    let officers = &entry.data.officers.values;
    let base = ledger::round_income(
        economy,
        round,
        officers,
        // The record carries the map's own row per player, so the income comes
        // from the replay rather than from the shared rule.
        RoundSupply {
            first: setup.first_round_supply,
            increase: setup.round_supply_increase,
            max: setup.max_round_supply,
        },
    );
    Ok(base - if debt { RAPID_SUPPLY_DEBT } else { 0 })
}

#[derive(Clone, Copy)]
enum Allowance {
    Buy,
    Unlock,
}

/// What the round allows, which the recorded counter states one round late.
///
/// The recorded counter is what was left of the *previous* round's allowance,
/// so this round's is the next snapshot's counter plus what this round spent.
/// The last round has no next snapshot and falls back to the shipped constant.
fn allowance(player: &record::PlayerRecord, position: usize, allowance: Allowance) -> i32 {
    let Some(next) = player.rounds.entries.get(position + 1) else {
        return match allowance {
            Allowance::Buy => BUY_COUNT_PER_ROUND,
            Allowance::Unlock => UNLOCK_COUNT_PER_ROUND,
        };
    };
    let taken = net_actions(&player.rounds.entries[position].actions.entries);
    let count = |kind: &str| {
        i32::try_from(taken.iter().filter(|action| action.kind == kind).count()).unwrap_or(0)
    };
    match allowance {
        Allowance::Buy => {
            // Mass Recruitment raises the allowance during the round, so it is
            // not part of what the round started with.
            let granted = i32::try_from(
                taken
                    .iter()
                    .filter(|action| {
                        action.kind == "PAD_ActiveEnergyTowerSkill"
                            && action.skill_id == Some(MASS_RECRUIT_SKILL)
                    })
                    .count(),
            )
            .unwrap_or(0);
            next.data.shop.buy_count + count("PAD_BuyUnit") - granted
        }
        Allowance::Unlock => next.data.shop.unlock_count + count("PAD_UnlockUnit"),
    }
}

/// Collapses a recorded action list onto the decisions that took effect.
///
/// Every recorded action is one entry on the undo stack, and `Undo` pops the
/// newest entry whether or not it still stands for a decision. That is what
/// distinguishes this from popping the newest surviving decision: a cancelled
/// release stays on the stack as an entry, and so does the cancel itself, so
/// the two of them absorb two undos between them.
///
/// One player-round settles it. A side chose a card, bought two units, moved
/// one of them twice, released a skill, cancelled it, released it again,
/// unlocked a unit, and then pressed undo seven times. Its next snapshot keeps
/// the card and one of the two units, which is what stepping back over seven
/// recorded entries leaves and is two entries further than stepping back over
/// seven surviving decisions.
///
/// `Redo` pushes the newest undone entry back, and any other action clears what
/// could be redone. `docs/spec/document/turn.md` states the rule.
fn net_actions(recorded: &[ActionRecord]) -> Vec<&ActionRecord> {
    /// An entry that no longer stands for a decision but still absorbs an undo.
    const SPENT: bool = false;
    let mut taken: Vec<(&ActionRecord, bool)> = Vec::with_capacity(recorded.len());
    let mut undone: Vec<(&ActionRecord, bool)> = Vec::new();
    for action in recorded {
        match action.kind.as_str() {
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
                if let Some(entry) = taken.iter_mut().rev().find(|(candidate, stands)| {
                    *stands
                        && candidate.kind == "PAD_ReleaseCommanderSkill"
                        && candidate.skill_index == action.skill_index
                }) {
                    entry.1 = SPENT;
                }
                taken.push((action, SPENT));
            }
            "PAD_FinishDeploy" => undone.clear(),
            _ => {
                undone.clear();
                taken.push((action, true));
            }
        }
    }
    taken
        .into_iter()
        .filter_map(|(action, stands)| stands.then_some(action))
        .collect()
}

/// The opening specialist a side ends round 0 holding.
///
/// The record logs the team half of the opening and not the specialist half,
/// so the specialist is read back from the officer list of the round the
/// opening produced. Exactly one officer of a side is an opening specialist,
/// in every player-round of the local set.
fn opening_specialist(
    economy: &Economy,
    player: &record::PlayerRecord,
    position: usize,
) -> Option<i32> {
    let next = player.rounds.entries.get(position + 1)?;
    next.data
        .officers
        .values
        .iter()
        .copied()
        .find(|officer| {
            economy
                .advance_team(*officer)
                .is_some_and(|team| team.kind == OpeningKind::Officer)
        })
}

fn actions(
    round: &PlayerRoundRecord,
    seat: Seat,
    specialist: Option<i32>,
) -> Result<Vec<Action>, String> {
    let mut converted = Vec::new();
    for action in net_actions(&round.actions.entries) {
        let field = |name: &'static str, value: Option<i32>| {
            value.ok_or_else(|| format!("{} has no {name}", action.kind))
        };
        let position = |value: &Option<record::PositionRecord>| {
            value
                .as_ref()
                .map(|position| seat.position(position))
                .ok_or_else(|| format!("{} has no position", action.kind))
        };
        converted.push(match action.kind.as_str() {
            "PAD_ChooseReinforceItem" => {
                // Declining is the same decision at the declined offer, and
                // the game records its `ID` as zero rather than omitting it.
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
                    .filter(|extra| extra.x != 0 || extra.y != 0)
                    .map(|extra| seat.position(extra)),
            },
            "PAD_MoveUnit" => {
                // One recorded move can carry several units. Order is all a turn
                // keeps, and the collapse has already run, so the batch flattens.
                let moves = action
                    .moves
                    .as_ref()
                    .ok_or_else(|| "PAD_MoveUnit has no moveUnitDatas".to_owned())?;
                for moved in &moves.entries {
                    converted.push(Action::MoveUnit {
                        index: moved.unit_index,
                        position: seat.position(&moved.position),
                        rotated: moved.rotated,
                    });
                }
                continue;
            }
            // Conceding ends the match rather than moving the position, so
            // the battle's root carries it and the sequence does not.
            "PAD_GiveUp" => continue,
            other => return Err(format!("action {other} has no turn representation")),
        });
    }
    Ok(converted)
}

/// Resolves the exclusive target of a release.
///
/// The recorded shape is not exclusive: a pointing release also carries the
/// player's click point, which names no state. `docs/spec/document/state.md` states why the
/// resolved target is stored and the coordinate is dropped.
fn skill_target(action: &ActionRecord, seat: Seat) -> Result<SkillTarget, String> {
    if let Some(unit) = action.unit_index.filter(|index| *index >= 0) {
        return Ok(SkillTarget::Unit(unit));
    }
    if let Some(construction) = action.construction_index.filter(|index| *index >= 0) {
        return Ok(SkillTarget::Construction(construction));
    }
    let positions = action
        .positions
        .as_ref()
        .map(|positions| {
            positions
                .entries
                .iter()
                .map(|position| seat.position(position))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if positions.is_empty() {
        return Err("PAD_ReleaseCommanderSkill names neither an object nor an area".into());
    }
    Ok(SkillTarget::Area(positions))
}

/// Unit IDs a state may hold, for the catalogue coverage test.
#[cfg(test)]
fn recorded_unit_ids(battle: &Battle) -> std::collections::BTreeSet<String> {
    battle
        .turns
        .iter()
        .flat_map(|turn| [&turn.state.sides.blue, &turn.state.sides.red])
        .flat_map(|side| side.formations.iter().map(|unit| unit.formation.type_name.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{battle_from_grbr, recorded_unit_ids};
    use crate::battle::{Action, SideState, SkillTarget, Turn, canonical_yaml};
    use crate::grbr::SHIELD_AIRDROP_SKILL;
    use crate::{DocumentKind, Position};

    const TUFF: &str = "../../tests/grbr/2259_20260901--201562374_[crower]VS[[TUFF]MARLFAUX].grbr";
    const CAINE: &str = "../../tests/grbr/2259_20260901--201562557_[crower]VS[[BORK]  Caine].grbr";
    const CRBN: &str = "../../tests/grbr/2259_20260911--67398165_[Dr. crbN]VS[trevorism].grbr";
    const THORRRIN: &str = "../../tests/grbr/2259_20260911--134508150_[Thorrrin]VS[占星].grbr";

    fn tuff() -> super::Battle {
        battle_from_grbr(&std::fs::read(TUFF).expect("tracked GRBR fixture")).unwrap()
    }

    /// The one player-round that names the two fixed towers apart.
    ///
    /// `tests/layouts/tuff-replay-round-7.yaml` was captured from the game while
    /// this replay played, and the record keys the same two levels by
    /// building-manager position. Red bought both of its strengthenings in round
    /// 5, so the round's start and its deployment end hold the same levels and
    /// the two documents are comparing one fact.
    ///
    /// Agreement here is what shows the record's list is keyed by position and
    /// not by a fixed tower order: the live capture read red's kinds as
    /// `[ResearchCenter, EnergyTower]` and blue's as the mirror image, so a list
    /// written in tower order would not match the record on both sides at once.
    #[test]
    fn the_captured_layout_keys_its_towers_the_way_the_record_does() {
        let battle = tuff();
        let round = battle
            .turns
            .iter()
            .find(|turn| turn.round == 7)
            .expect("the replay reaches round 7");
        let bytes = std::fs::read("../../tests/layouts/tuff-replay-round-7.yaml")
            .expect("tracked layout fixture");
        let layout = crate::layout::parse_yaml(&bytes).expect("a valid layout");
        // Normal form omits an all-zero list, which the state still writes.
        let stated = |levels: &[i32]| {
            if levels.iter().all(|level| *level == 0) {
                Vec::new()
            } else {
                levels.to_vec()
            }
        };
        assert_eq!(
            stated(&round.state.sides.red.tower_strengthen_levels),
            layout.sides.red.tower_strengthen_levels
        );
        assert_eq!(
            stated(&round.state.sides.blue.tower_strengthen_levels),
            layout.sides.blue.tower_strengthen_levels
        );
        assert_eq!(
            layout.sides.red.tower_strengthen_levels,
            vec![0, 2],
            "the live capture put the level 2 at red's second position"
        );
    }

    #[test]
    fn hoists_what_every_round_shares() {
        let battle = tuff();
        assert_eq!(battle.kind, DocumentKind::Battle);
        assert_eq!(battle.map_id, 1021);
        assert_eq!(battle.seed, 31_103_914);
        assert_eq!(battle.turns.len(), 9);
        let rounds: Vec<i32> = battle.turns.iter().map(|turn| turn.round).collect();
        assert_eq!(rounds, (0..9).collect::<Vec<_>>());
    }

    #[test]
    fn reads_the_tech_loadout_of_both_sides() {
        let battle = tuff();
        assert_eq!(battle.sides.blue.tech_loadout.len(), 34);
        assert_eq!(battle.sides.red.tech_loadout.len(), 34);
        assert_eq!(
            battle.sides.blue.tech_loadout[&1],
            vec![1105, 10301, 10401, 10801]
        );
        assert_ne!(battle.sides.blue.tech_loadout, battle.sides.red.tech_loadout);
    }

    #[test]
    fn every_researched_technology_comes_from_that_sides_loadout() {
        let battle = tuff();
        for turn in &battle.turns {
            for (state, side) in [
                (&turn.state.sides.blue, &battle.sides.blue),
                (&turn.state.sides.red, &battle.sides.red),
            ] {
                for tech in &state.techs.units {
                    assert!(
                        side.tech_loadout.values().any(|row| row.contains(tech)),
                        "round {} researched {tech}, which its loadout does not offer",
                        turn.round
                    );
                }
            }
        }
    }

    #[test]
    fn formations_match_the_layout_projection_of_the_same_round() {
        // tests/layouts/tuff-replay-round-7.yaml is the position round 7 ends
        // in, so its roster is the round 8 snapshot.
        let battle = tuff();
        let blue = &battle.turns[8].state.sides.blue;
        let vortex = blue
            .formations
            .iter()
            .find(|entry| entry.formation.index == 2)
            .unwrap();
        assert_eq!(vortex.formation.type_name, "vortex");
        assert_eq!(vortex.formation.level, Some(3));
        assert_eq!(vortex.formation.position, Position { x: -250, y: -120 });
        // What recovering it pays back is what the side paid for it.
        assert_eq!(vortex.value, Some(100));
        let red = &battle.turns[8].state.sides.red;
        let marksman = red
            .formations
            .iter()
            .find(|entry| entry.formation.index == 20)
            .unwrap();
        assert_eq!(marksman.formation.type_name, "marksman");
        assert_eq!(marksman.formation.level, Some(4));
        // Red's recorded (-190, 170) is (190, -170) in its own frame.
        assert_eq!(marksman.formation.position, Position { x: 190, y: -170 });
    }

    #[test]
    fn a_round_starts_with_its_income_and_a_full_allowance() {
        let battle = tuff();
        for state in [
            &battle.turns[0].state.sides.blue,
            &battle.turns[0].state.sides.red,
        ] {
            assert_eq!(state.supply, 0);
        }
        // The map pays 200 in round 1, and red holds a supply officer that
        // adds fifty to every round's income.
        let opening = &battle.turns[1].state.sides;
        assert_eq!(opening.blue.supply, 200);
        assert_eq!(opening.red.supply, 250);
        for state in [&opening.blue, &opening.red] {
            assert_eq!(state.shop.buys_remaining, 2);
            assert_eq!(state.shop.unlocks_remaining, 1);
        }
        for turn in &battle.turns {
            for state in [&turn.state.sides.blue, &turn.state.sides.red] {
                assert!(state.energy_tower_skills.is_empty());
                assert_eq!(state.tower_strengthen_levels.len(), 2);
            }
        }
    }

    #[test]
    fn collapses_undo_and_flattens_a_move_batch() {
        let battle = tuff();
        let blue = &battle.turns[7].actions.blue;
        // Round 7 records three undos, and every retraction is gone.
        assert!(!blue.iter().any(|action| matches!(
            action,
            Action::ChooseReinforceItem { id: Some(0), .. }
        )));
        let bought: Vec<i32> = blue
            .iter()
            .filter_map(|action| match action {
                Action::BuyUnit { unit, .. } => Some(*unit),
                _ => None,
            })
            .collect();
        assert_eq!(bought, vec![10, 31, 31]);
        let moved: Vec<i32> = blue
            .iter()
            .filter_map(|action| match action {
                Action::MoveUnit { index, .. } => Some(*index),
                _ => None,
            })
            .collect();
        assert_eq!(moved, vec![21, 22, 23]);
    }

    #[test]
    fn a_release_names_one_target() {
        let battle = tuff();
        let mut units = 0;
        let mut areas = 0;
        for turn in &battle.turns {
            for action in turn.actions.blue.iter().chain(&turn.actions.red) {
                if let Action::ReleaseCommanderSkill { target, .. } = action {
                    match target {
                        SkillTarget::Unit(_) => units += 1,
                        SkillTarget::Area(positions) => {
                            assert!(!positions.is_empty());
                            areas += 1;
                        }
                        SkillTarget::Construction(_) => {}
                    }
                }
            }
        }
        assert!(units > 0 && areas > 0);
    }

    #[test]
    fn every_unit_in_the_corpus_has_a_layout_type() {
        let battle = tuff();
        assert!(recorded_unit_ids(&battle).contains("vortex"));
        let caine = battle_from_grbr(&std::fs::read(CAINE).expect("tracked GRBR fixture")).unwrap();
        assert!(!recorded_unit_ids(&caine).is_empty());
    }

    #[test]
    fn converts_every_tracked_standard_replay() {
        let mut converted = 0;
        for entry in std::fs::read_dir("../../tests/grbr").unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|extension| extension != "grbr") {
                continue;
            }
            let grbr = std::fs::read(&path).unwrap();
            match battle_from_grbr(&grbr) {
                Ok(battle) => {
                    converted += 1;
                    assert!(!battle.turns.is_empty());
                    for turn in &battle.turns {
                        for state in [&turn.state.sides.blue, &turn.state.sides.red] {
                            assert!(state.supply >= 0);
                            assert!(state.next_index.unit >= 0);
                            for formation in &state.formations {
                                assert!(formation.formation.index < state.next_index.unit);
                            }
                        }
                    }
                }
                Err(error) => panic!("{}: {error}", path.display()),
            }
        }
        assert_eq!(converted, 41);
    }

    /// A standing Shield Airdrop is one the round before released or held.
    ///
    /// The snapshot records it in the same `rangeItems` the retained oil
    /// terrain comes from, so the round before is a second witness and the two
    /// are compared. The tracked set stands five shields, and the one in
    /// `[elRAKAMAKAFON]` stands for two rounds before a fight destroys it,
    /// which is what makes the entry a retained object rather than a
    /// restatement of one round's release.
    #[test]
    fn a_standing_shield_is_the_previous_rounds_release_or_its_own_survival() {
        fn pick<'a>(turn: &'a Turn, side: &str) -> (&'a SideState, &'a [Action]) {
            match side {
                "blue" => (&turn.state.sides.blue, &turn.actions.blue),
                _ => (&turn.state.sides.red, &turn.actions.red),
            }
        }
        let mut standing = Vec::new();
        for entry in std::fs::read_dir("../../tests/grbr").unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|extension| extension != "grbr") {
                continue;
            }
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let battle = battle_from_grbr(&std::fs::read(&path).unwrap()).unwrap();
            for (position, turn) in battle.turns.iter().enumerate() {
                let before = position.checked_sub(1).map(|index| &battle.turns[index]);
                for side_name in ["blue", "red"] {
                    let (state, _) = pick(turn, side_name);
                    if state.airdrop_shields.is_empty() {
                        continue;
                    }
                    let centers = state
                        .airdrop_shields
                        .iter()
                        .map(|center| (center.x, center.y))
                        .collect::<Vec<_>>();
                    standing.push(format!(
                        "{name} round {} {side_name}: {centers:?}",
                        turn.round
                    ));
                    let before = before.unwrap_or_else(|| {
                        panic!(
                            "{name} round {} {side_name} opens holding a shield",
                            turn.round
                        )
                    });
                    let (earlier, actions) = pick(before, side_name);
                    // The panel slot is read from this round and not the one
                    // before, because a skill taken as a reinforcement is
                    // released in the round that takes it and only reaches the
                    // next round's snapshot. The slot itself does not move.
                    let slot = state
                        .battle_skills
                        .iter()
                        .find(|skill| skill.id == SHIELD_AIRDROP_SKILL)
                        .map(|skill| skill.index);
                    for center in &state.airdrop_shields {
                        let released = actions.iter().any(|action| {
                            matches!(
                                action,
                                Action::ReleaseCommanderSkill {
                                    skill,
                                    target: SkillTarget::Area(points),
                                } if Some(*skill) == slot && points.as_slice() == [*center]
                            )
                        });
                        assert!(
                            released || earlier.airdrop_shields.contains(center),
                            "{name} round {} {side_name} stands a shield at ({}, {}) \
                             that the round before neither released nor held",
                            turn.round,
                            center.x,
                            center.y
                        );
                    }
                }
            }
        }
        standing.sort();
        assert_eq!(
            standing,
            [
                "2259_20260910--134504097_[Dre420]VS[[TUFF] Wumple Doodle].grbr round 7 blue: [(-169, -110)]",
                "2259_20260910--134504097_[Dre420]VS[[TUFF] Wumple Doodle].grbr round 7 red: [(-210, -130)]",
                "2259_20260910--67396921_[elRAKAMAKAFON]VS[p站智慧官叫馆].grbr round 3 red: [(72, -29)]",
                "2259_20260910--67396921_[elRAKAMAKAFON]VS[p站智慧官叫馆].grbr round 4 red: [(72, -29)]",
                "2259_20260911--134508150_[Thorrrin]VS[占星].grbr round 3 red: [(-235, -74)]",
            ]
        );
    }

    /// A retained object from a skill this reader has not been measured
    /// against is refused rather than dropped.
    ///
    /// The tracked `[Thorrrin]` replay stands one shield, and its skill ID is
    /// edited in memory to an ID no catalogue holds. The replacement is the
    /// same number of bytes, so the `BinaryFormatter` framing stays intact.
    #[test]
    fn a_retained_object_from_an_unmeasured_skill_is_refused() {
        let tracked = std::fs::read(THORRRIN).expect("tracked GRBR fixture");
        let recorded = b"<id>800001</id>";
        assert!(
            tracked
                .windows(recorded.len())
                .any(|window| window == recorded),
            "the tracked replay holds the Shield Airdrop"
        );
        let mut edited = Vec::with_capacity(tracked.len());
        let mut rest = tracked.as_slice();
        while let Some(at) = rest
            .windows(recorded.len())
            .position(|window| window == recorded)
        {
            edited.extend_from_slice(&rest[..at]);
            edited.extend_from_slice(b"<id>800009</id>");
            rest = &rest[at + recorded.len()..];
        }
        edited.extend_from_slice(rest);
        let error = battle_from_grbr(&edited).expect_err("the skill is not measured");
        assert!(
            error.contains("unsupported retained commander-skill object 800009"),
            "{error}"
        );
    }

    /// The recorded energy tower list is the previous round's debt.
    ///
    /// Two readings of one fact, and the conversion now refuses a replay where
    /// they disagree. The tracked set exercises the claim in both directions:
    /// skill `1` is activated 82 times, so a debt the snapshot never
    /// carried would fail, and the other four skills are activated 469
    /// times between them, so a snapshot carrying any of those would fail too.
    #[test]
    fn the_recorded_energy_tower_list_is_the_previous_rounds_debt() {
        let mut deferred = 0;
        let mut immediate = 0;
        for entry in std::fs::read_dir("../../tests/grbr").unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|extension| extension != "grbr") {
                continue;
            }
            let Ok(battle) = battle_from_grbr(&std::fs::read(&path).unwrap()) else {
                continue;
            };
            for turn in &battle.turns {
                for action in turn.actions.blue.iter().chain(&turn.actions.red) {
                    if let Action::ActiveEnergyTowerSkill { skill } = action {
                        if *skill == super::RAPID_SUPPLY_SKILL {
                            deferred += 1;
                        } else {
                            immediate += 1;
                        }
                    }
                }
            }
        }
        assert_eq!((deferred, immediate), (109, 551));
    }

    /// A snapshot that names a skill the round before did not activate is
    /// refused rather than converted.
    ///
    /// The one recorded `energyTowerSkills` entry in the tracked set is edited
    /// in memory from skill `1` to skill `2`, which is the same number of bytes
    /// and so leaves the `BinaryFormatter` framing intact. Skill `2` has no
    /// deferred half, so no round can owe it.
    #[test]
    fn a_snapshot_that_disagrees_with_the_round_before_is_refused() {
        let mut grbr = std::fs::read(TUFF).expect("tracked GRBR fixture");
        let recorded = b"<energyTowerSkills>\n              <int>1</int>";
        let at = grbr
            .windows(recorded.len())
            .position(|window| window == recorded)
            .expect("the tracked replay records one deferred skill");
        let digit = at + recorded.len() - "</int>".len() - 1;
        assert_eq!(grbr[digit], b'1');
        grbr[digit] = b'2';
        let error = battle_from_grbr(&grbr).expect_err("the two readings disagree");
        assert!(error.contains("energy tower skills [2]"), "{error}");
        assert!(error.contains("decided [1]"), "{error}");
    }

    /// Conceding ends a match, so it is the battle's and not a turn's.
    ///
    /// `[Dr. crbN]VS[trevorism]` is the one tracked replay holding a
    /// `PAD_GiveUp`. Red concedes in round 9, which is the last round the
    /// battle holds, and the action leaves no trace in either side's sequence.
    #[test]
    fn a_conceded_match_says_who_conceded_and_when() {
        let battle = battle_from_grbr(&std::fs::read(CRBN).expect("tracked GRBR fixture")).unwrap();
        assert_eq!(
            battle.concession,
            Some(crate::battle::Concession {
                side: crate::battle::Side::Red,
                round: 9,
            })
        );
        assert_eq!(battle.turns.last().map(|turn| turn.round), Some(9));
        let yaml = canonical_yaml(&battle).unwrap();
        assert!(yaml.contains("concession:\n  side: red\n  round: 9\n"), "{yaml}");
    }

    /// Every other tracked match ended in a way no replay records.
    #[test]
    fn a_match_nobody_conceded_carries_no_concession() {
        assert_eq!(tuff().concession, None);
        let yaml = canonical_yaml(&tuff()).unwrap();
        assert!(!yaml.contains("concession"));
    }

    #[test]
    fn serializes_to_normal_form_yaml() {
        let yaml = canonical_yaml(&tuff()).unwrap();
        assert!(yaml.starts_with("kind: battle\nmap_id: 1021\nseed: 31103914\n"));
        assert!(yaml.contains("      position: {x: -250, y: -120}\n"));
        assert!(yaml.contains("  - type: buy_unit\n"));
        assert!(yaml.ends_with('\n'));
    }
}
