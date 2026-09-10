//! Fills a battle document from one recorded match.
//!
//! Most fields are copied. Four are rebuilt, because the snapshot the game
//! writes precedes the round's own reset: `supply` gains the round's income,
//! the two shop counters are restored to the round's allowance, the energy
//! tower list is the set activated during the round and so empty at its
//! start, and the equipment list drops what the formations carry.
//! `docs/battle.md` says what the conversion refuses.

use crate::battle::{
    Action, Battle, BattleSide, BattleSides, EquipmentItem, NextIndex, PanelSkill, ShopState,
    SideState, SkillTarget, State, StateFormation, StateSides, Turn, TurnActions,
};
use crate::catalog::{construction_type_from_id, contraption_type_from_id, unit_type_from_id};
use crate::layout::{ContraptionPlacement, Formation, Position, StaticPlacement, Techs};
use crate::record::{self, ActionRecord, PlayerData, PlayerRoundRecord};
use crate::economy::{Economy, OpeningKind, RoundSupply};
use crate::ledger;
use crate::{DocumentKind, terrains_from_grbr_round};
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
/// The Shield Airdrop commander skill, whose retained objects a replay omits.
const SHIELD_AIRDROP_SKILL: i32 = 800_001;

/// Which half of the map a side plays on, and so how its positions are read.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Seat {
    Blue,
    Red,
}

impl Seat {
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
        sides: BattleSides {
            blue: battle_side(&blue),
            red: battle_side(&red),
        },
        turns,
    })
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
        if skill.id == SHIELD_AIRDROP_SKILL {
            return Err(format!(
                "round {round} holds commander skill {SHIELD_AIRDROP_SKILL}, whose retained \
                 shields a replay does not record; see docs/battle.md"
            ));
        }
        battle_skills.push(PanelSkill {
            index: skill.index,
            id: skill.id,
            cooldown: skill.cooling_round,
        });
    }
    battle_skills.sort_by_key(|skill| skill.index);

    let terrains = terrains_from_grbr_round(grbr, u32::try_from(round).unwrap_or(0))?;
    let terrains = match seat {
        Seat::Blue => terrains.blue,
        Seat::Red => terrains.red,
    };

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
        supply: data.supply + round_income(economy, player, position),
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
        // A replay records no retained shield; refused above if one could exist.
        airdrop_shields: Vec::new(),
        terrains,
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
            // No recorded field states it; see docs/battle.md.
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

/// The income this round adds, which the recorded supply precedes.
///
/// The map's own row is recorded per player, so no map catalogue is consulted.
/// An energy tower skill activated last round is paid for here.
fn round_income(economy: &Economy, player: &record::PlayerRecord, position: usize) -> i32 {
    let entry = &player.rounds.entries[position];
    let round = entry.round;
    if round < 1 {
        return 0;
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
    let debt = position
        .checked_sub(1)
        .map(|previous| &player.rounds.entries[previous])
        .is_some_and(|previous| {
            net_actions(&previous.actions.entries)
                .iter()
                .any(|action| {
                    action.kind == "PAD_ActiveEnergyTowerSkill"
                        && action.skill_id == Some(RAPID_SUPPLY_SKILL)
                })
        });
    base - if debt { RAPID_SUPPLY_DEBT } else { 0 }
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
/// Each action is pushed. `Undo` pops the newest survivor, `Redo` pushes it
/// back, and a cancel removes the newest surviving release of the same skill
/// slot. `docs/turn.md` states the rule and the evidence for it.
fn net_actions(recorded: &[ActionRecord]) -> Vec<&ActionRecord> {
    let mut taken: Vec<&ActionRecord> = Vec::with_capacity(recorded.len());
    let mut undone: Vec<&ActionRecord> = Vec::new();
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
                if let Some(position) = taken.iter().rposition(|candidate| {
                    candidate.kind == "PAD_ReleaseCommanderSkill"
                        && candidate.skill_index == action.skill_index
                }) {
                    taken.remove(position);
                }
            }
            "PAD_FinishDeploy" => undone.clear(),
            _ => {
                undone.clear();
                taken.push(action);
            }
        }
    }
    taken
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
                let offer = field("Index", action.index)?;
                if offer < 0 {
                    Action::DeclineReinforceItem
                } else {
                    Action::ChooseReinforceItem {
                        offer,
                        id: field("ID", action.id)?,
                    }
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
            other => return Err(format!("action {other} has no turn representation")),
        });
    }
    Ok(converted)
}

/// Resolves the exclusive target of a release.
///
/// The recorded shape is not exclusive: a pointing release also carries the
/// player's click point, which names no state. `docs/state.md` states why the
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
    use crate::battle::{Action, SkillTarget, canonical_yaml};
    use crate::{DocumentKind, Position};

    const TUFF: &str = "../../tests/grbr/2259_20260901--201562374_[crower]VS[[TUFF]MARLFAUX].grbr";
    const CAINE: &str = "../../tests/grbr/2259_20260901--201562557_[crower]VS[[BORK]  Caine].grbr";

    fn tuff() -> super::Battle {
        battle_from_grbr(&std::fs::read(TUFF).expect("tracked GRBR fixture")).unwrap()
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
            Action::ChooseReinforceItem { id: 0, .. }
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
    fn converts_every_tracked_ranked_replay_and_refuses_the_rest() {
        let mut converted = 0;
        let mut refused = 0;
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
                Err(error) => {
                    refused += 1;
                    // The two computer matches were set up with Training Ground
                    // commands rather than played.
                    assert!(error.contains("Test match"), "{error}");
                }
            }
        }
        assert_eq!((converted, refused), (4, 2));
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
