//! Moves a side's position from one decision, and one round, to the next.
//!
//! [`step`] applies one decision to the position it was taken from and returns
//! the whole next position, board included. [`open_round`] is what a round's
//! opening does before any decision: it resets what lasts one round, pays the
//! income and makes the officers' deliveries. [`predict`] composes them: a
//! round's decisions stepped in order, the travelling set the fight empties,
//! and the next round opened on the result, which is the position the next
//! round opens with in everything the fight does not decide.
//!
//! `docs/spec/document/battle.md` states what a transition reproduces, and
//! [`crate::coverage`] measures it against a battle's recorded positions.

use crate::battle::{
    Action, EquipmentItem, PanelSkill, Release, SideState, SkillTarget, StateFormation,
};
use crate::catalog::{contraption_type_from_id, unit_id_from_type, unit_type_from_id};
use crate::economy::{CardKind, Economy, OpeningKind};
use crate::layout::{ContraptionPlacement, Experience, Position, Region, StaticPlacement};
use crate::ledger::Purse;

/// What one decision's application could not settle.
///
/// A position that reaches one of these is not a position the transition got
/// wrong; it is one this build's tables cannot decide, or a decision the game
/// does not allow from it. Reporting the reason rather than a guess is what
/// keeps an unsupported sample out of the closed count.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Unsettled {
    /// The build's tables carry no price or catalogue row for the decision.
    Unpriced(&'static str),
    /// The decision names a formation, panel slot or construction the position
    /// does not hold.
    Missing(&'static str),
    /// Where the formations a card or an opening hands out arrive is decided by
    /// the board they arrive on, not by the decision that summoned them, and
    /// the caller did not say which side's board that is, or the board has no
    /// room. [`crate::landing`] is the board's rule.
    GrantedPosition,
    /// The game does not allow the decision from this position.
    ///
    /// The fault is the decision's, not the tables': a recording never holds
    /// one, and a decision written by anything else is refused rather than
    /// applied.
    Refused(&'static str),
}

/// Commander skills that fill a formation's experience bar: 强化训练,
/// Intensive Training.
///
/// A deployment skill: it does its work on the position before the fight, so
/// its slot is marked used rather than released.
pub(crate) const TRAINING_SKILLS: [i32; 1] = [1_100_001];

/// Whether a commander skill is a deployment skill, whose slot a round marks
/// used rather than released: Intensive Training and Redeploy.
pub(crate) fn is_deployment_skill(id: i32) -> bool {
    TRAINING_SKILLS.contains(&id) || crate::mobility::REDEPLOY_SKILLS.contains(&id)
}
/// Energy tower skill `3` 批量征召, which adds a purchase to this round.
const MASS_RECRUIT_SKILL: i32 = 3;
/// Reinforcement card `10004` 额外部署位, which adds one too. It is an officer
/// the side keeps, so every later round opens with the extra purchase as well.
pub(crate) const EXTRA_DEPLOYMENT_CARD: i32 = 10_004;
/// How many purchases each of those two adds.
///
/// Measured rather than read. No extracted table carries the count: the energy
/// tower row has no allowance field at all, and `OfficerData.IsAddExtraUnit` is
/// a shipped boolean rather than a number. Every activation of skill `3` and
/// every take of card `10004` in the local observation set raises the round's
/// remaining purchases by exactly this, and nothing else in that set raises it.
const EXTRA_BUYS: i32 = 1;

/// Applies one decision to the position it was taken from.
///
/// This is the deployment's own transition: it answers what the position looks
/// like after one decision, while the round is still running, board included.
/// What the next round opens with is [`predict`]'s.
///
/// The result is a position, not an edit: a decision that cannot be settled
/// leaves the caller the position it started from rather than half of the one
/// it was reaching for.
///
/// # Errors
///
/// Returns the reason the decision could not be settled. Every arm of
/// [`Unsettled`] is a boundary of this build's tables rather than a failure of
/// the caller's position.
pub fn step(economy: &Economy, state: &SideState, action: &Action) -> Result<SideState, Unsettled> {
    step_placing(economy, state, action, &mut |_, _| None)
}

/// [`step`], told where the formations a decision hands out arrive.
///
/// A card and an opening summon formations that no decision gives a position
/// to, and the board places each one clear of what already stands, so the same
/// card lands differently in two matches. [`crate::landing::placement`] is the
/// board's rule, and it needs to know the side. `placement` is asked, for each
/// formation handed out and in order, with the position just before it arrives
/// and its unit type; answering `None` is what makes [`step`] report
/// [`Unsettled::GrantedPosition`] rather than invent one.
///
/// # Errors
///
/// The same as [`step`].
#[allow(clippy::too_many_lines)] // Thirteen decisions, each one short.
pub fn step_placing(
    economy: &Economy,
    state: &SideState,
    action: &Action,
    placement: &mut dyn FnMut(&SideState, &str) -> Option<Position>,
) -> Result<SideState, Unsettled> {
    let mut next = state.clone();
    let mut purse = Purse::new(economy, &state.techs.officers);
    // Elite Recruitment raises the shop for the rest of the round, and the
    // round's activations are in the position rather than in the decision.
    purse.raised = state
        .energy_tower_skills
        .iter()
        .filter_map(|skill| economy.energy_tower_skill(*skill))
        .map(|skill| skill.shop_unit_level)
        .sum();
    match action {
        // Declining is one of this decision's two answers, and the one that
        // pays: the item it takes is a supply grant of its own.
        Action::ChooseReinforceItem { id: None, .. } => {
            next.supply += economy.reinforce_decline();
        }
        Action::ChooseReinforceItem { id: Some(card), .. } => {
            let price = economy.card(*card).ok_or(Unsettled::Unpriced("card"))?;
            next.supply -= price;
            // An officer card joins the list whether or not this build gives
            // it an effect: the list is what the side holds, and only the
            // supply it hands over comes from the effects table.
            match economy.card_kind(*card) {
                Some(CardKind::Officer) => {
                    next.supply += economy
                        .officer(*card)
                        .map_or(0, |officer| officer.granted_supply);
                    if *card == EXTRA_DEPLOYMENT_CARD {
                        next.shop.buys_remaining += EXTRA_BUYS;
                    }
                    next.techs.officers.push(*card);
                    next.techs.officers.sort_unstable();
                }
                Some(CardKind::CommanderSkill) => panel_add(&mut next, *card),
                Some(CardKind::Equipment) => next.equipment.push(EquipmentItem {
                    id: *card,
                    durability: None,
                }),
                _ => {}
            }
            if let Some(reinforcement) = economy.unit_reinforcement(*card) {
                // A card that hands out squads puts their unit in the shop.
                unlock(&mut next, reinforcement.unit);
                hand_out(
                    economy,
                    &mut next,
                    reinforcement.unit,
                    reinforcement.squads,
                    reinforcement.level,
                    placement,
                )?;
            }
            next.equipment.sort();
        }
        Action::ChooseAdvanceTeam { id, specialist, .. } => {
            next.supply -= economy.card(*id).ok_or(Unsettled::Unpriced("opening"))?;
            let team = economy
                .advance_team(*id)
                .ok_or(Unsettled::Unpriced("opening"))?;
            // The opening is one choice with two halves and each half prices
            // the reactor core, so a team and its specialist both move it.
            next.reactor_core += team.reactor_core
                + specialist
                    .filter(|chosen| chosen != id)
                    .and_then(|chosen| economy.advance_team(chosen))
                    .map_or(0, |officer| officer.reactor_core);
            // An opening of formations delivers nothing now. Its squads and
            // the shop rows they unlock arrive when the first deployment round
            // opens, which is not a decision and so not a step.
            if team.kind != OpeningKind::Units {
                next.techs.officers.push(*id);
            }
            if let Some(specialist) = specialist.filter(|chosen| chosen != id) {
                next.techs.officers.push(specialist);
            }
            next.techs.officers.sort_unstable();
        }
        Action::BuyUnit { unit, position } => {
            let price = purse.buy(*unit).ok_or(Unsettled::Unpriced("unit"))?;
            let level = purse.shop_level(*unit);
            let upgrade = purse.upgrade(*unit).ok_or(Unsettled::Unpriced("upgrade"))?;
            // A unit that arrives above level 1 is paid for as a purchase plus
            // one upgrade per level above the first.
            next.supply -= price + (level - 1) * upgrade;
            next.shop.buys_remaining -= 1;
            place(&mut next, *unit, level, price, &mut |_, _| Some(*position))?;
        }
        Action::UpgradeUnit { index } => {
            let formation = formation_mut(&mut next, *index)?;
            let unit = unit_id_from_type(&formation.formation.type_name)
                .ok_or(Unsettled::Unpriced("unit"))?;
            let worn = formation.formation.equipment;
            formation.formation.level = Some(formation.formation.level.unwrap_or(1) + 1);
            // A formation starts the next rank with nothing carried over.
            formation.formation.exp = None;
            // A discount can exceed the price, and an upgrade is never paid
            // backwards.
            let upgrade = purse.upgrade(unit).ok_or(Unsettled::Unpriced("upgrade"))?;
            next.supply -=
                (upgrade + worn.map_or(0, |id| economy.equipment_upgrade_supply(id))).max(0);
        }
        Action::UnlockUnit { unit } => {
            next.supply -= purse.unlock(*unit).ok_or(Unsettled::Unpriced("unlock"))?;
            next.shop.unlocks_remaining -= 1;
            unlock(&mut next, *unit);
        }
        Action::UpgradeTechnology { tech, .. } => {
            let unit = economy
                .technology_owner(*tech)
                .ok_or(Unsettled::Unpriced("technology"))?;
            // Each technology already on the unit makes the next one dearer.
            let researched = i32::try_from(
                state
                    .techs
                    .units
                    .iter()
                    .filter(|held| economy.technology_owner(**held) == Some(unit))
                    .count(),
            )
            .unwrap_or(0);
            next.supply -= purse
                .technology(*tech, unit, researched)
                .ok_or(Unsettled::Unpriced("technology"))?;
            next.techs.units.push(*tech);
            next.techs.units.sort_unstable();
            // A Jump Drive frees every formation of its unit to move.
            free_to_move(&mut next);
        }
        Action::ActiveBlueprint { id } => {
            next.supply -= economy
                .blueprint(*id)
                .ok_or(Unsettled::Unpriced("blueprint"))?;
            // A chain's second level replaces its first rather than joining it.
            next.blueprints
                .retain(|held| economy.blueprint_successor(*held) != Some(*id));
            next.blueprints.push(*id);
            next.blueprints.sort_unstable();
            if let Some(skill) = economy.blueprint_skill(*id) {
                panel_add(&mut next, skill);
            }
        }
        Action::ActiveEnergyTowerSkill { skill } => {
            let row = economy
                .energy_tower_skill(*skill)
                .ok_or(Unsettled::Unpriced("energy tower skill"))?;
            next.supply -= row.supply - row.granted;
            if *skill == MASS_RECRUIT_SKILL {
                next.shop.buys_remaining += EXTRA_BUYS;
            }
            next.energy_tower_skills.push(*skill);
            next.energy_tower_skills.sort_unstable();
        }
        Action::StrengthenTower { tower } => {
            let level = usize::try_from(*tower)
                .ok()
                .and_then(|index| next.tower_strengthen_levels.get_mut(index))
                .ok_or(Unsettled::Missing("tower"))?;
            *level += 1;
            let price = economy
                .tower_strengthen(*level)
                .ok_or(Unsettled::Unpriced("tower level"))?;
            next.supply -= price;
        }
        // Fitting is free; the card was paid for when it was taken.
        Action::UseEquipment { equipment, unit } => {
            let position = next
                .equipment
                .iter()
                .position(|item| item.id == *equipment)
                .ok_or(Unsettled::Missing("equipment"))?;
            next.equipment.remove(position);
            formation_mut(&mut next, *unit)?.formation.equipment = Some(*equipment);
            // A Deployment Module frees the formation that wears it to move.
            free_to_move(&mut next);
        }
        Action::MoveUnit {
            index,
            position,
            rotated,
        } => {
            let formation = formation_mut(&mut next, *index)?;
            if !formation.movable {
                return Err(Unsettled::Refused("moving a formation fixed in place"));
            }
            let left = Region::of(formation.formation.position);
            let arrived = Region::of(*position);
            formation.formation.position = *position;
            formation.formation.rotated = Some(*rotated).filter(|rotated| *rotated);
            if left != arrived {
                formation.formation.travelling = arrived.is_flank().then_some(true);
            }
        }
        Action::ReleaseCommanderSkill { skill, target } => {
            release(economy, &mut next, *skill, target)?;
        }
        // Giving up ends the match without moving the position.
        Action::Concede => {}
        // A contraption is bought from the shop as it is placed.
        Action::ReleaseContraption {
            contraption,
            position,
            ..
        } => {
            next.supply -= economy
                .contraption(*contraption)
                .ok_or(Unsettled::Unpriced("contraption"))?;
            let type_name =
                contraption_type_from_id(*contraption).ok_or(Unsettled::Unpriced("contraption"))?;
            next.contraptions.push(ContraptionPlacement {
                type_name: type_name.to_owned(),
                index: next.next_index.contraption,
                position: *position,
            });
            next.next_index.contraption += 1;
        }
    }
    Ok(next)
}

/// Records a release on the panel, and applies what it does to the board.
///
/// Most releases write nothing but the slot: what they do belongs to the fight.
/// Two write the position as well. Field Recovery takes one of the side's own
/// objects away and pays back what it cost, and the experience skills raise a
/// formation's experience by an amount no shipped table carries.
fn release(
    economy: &Economy,
    next: &mut SideState,
    slot: i32,
    target: &SkillTarget,
) -> Result<(), Unsettled> {
    let order = i32::try_from(
        next.battle_skills
            .iter()
            .filter(|skill| skill.release.is_some())
            .count(),
    )
    .unwrap_or(0);
    let id = next
        .battle_skills
        .iter()
        .find(|skill| skill.index == slot)
        .map(|skill| skill.id)
        .ok_or(Unsettled::Missing("panel slot"))?;
    if crate::mobility::REDEPLOY_SKILLS.contains(&id) {
        let SkillTarget::Unit(index) = target else {
            return Err(Unsettled::Missing("redeploy target"));
        };
        formation_mut(next, *index)?.movable = true;
        spend(next, slot)?;
        return Ok(());
    }
    if TRAINING_SKILLS.contains(&id) {
        let SkillTarget::Unit(index) = target else {
            return Err(Unsettled::Missing("training target"));
        };
        let formation = &mut formation_mut(next, *index)?.formation;
        let level = formation.level.unwrap_or(1);
        // Training takes neither a formation at the last level nor one whose
        // bar is already full.
        if level >= crate::experience::MAX_LEVEL {
            return Err(Unsettled::Refused("training a formation at the last level"));
        }
        // A unit the experience table does not name has no bar to fill.
        let maximum = crate::experience::full(&formation.type_name, level)
            .ok_or(Unsettled::Unpriced("experience"))?;
        if formation.exp.is_some_and(Experience::is_full) {
            return Err(Unsettled::Refused("training a full formation"));
        }
        formation.exp = Some(Experience {
            current: maximum,
            maximum,
        });
        spend(next, slot)?;
        return Ok(());
    }
    if crate::ledger::RECOVERY_SKILLS.contains(&id) {
        match target {
            SkillTarget::Unit(index) => recover_formation(economy, next, *index)?,
            SkillTarget::Construction(index) => {
                let position = next
                    .constructions
                    .iter()
                    .position(|placement| placement.index == *index)
                    .ok_or(Unsettled::Missing("construction"))?;
                let placement = next.constructions.remove(position);
                next.supply += economy
                    .construction_recovery(&placement.type_name)
                    .ok_or(Unsettled::Unpriced("construction"))?;
            }
            SkillTarget::Area(_) => return Err(Unsettled::Missing("recovery target")),
        }
    }
    let slot = next
        .battle_skills
        .iter_mut()
        .find(|skill| skill.index == slot)
        .ok_or(Unsettled::Missing("panel slot"))?;
    slot.release = Some(Release {
        order,
        target: clone_target(target),
    });
    Ok(())
}

/// Marks a deployment skill's slot spent.
fn spend(next: &mut SideState, slot: i32) -> Result<(), Unsettled> {
    next.battle_skills
        .iter_mut()
        .find(|skill| skill.index == slot)
        .ok_or(Unsettled::Missing("panel slot"))?
        .used = true;
    Ok(())
}

/// Takes one of the side's own formations away and pays back what it cost.
///
/// What it cost is the price paid for it, at the prices its officers made at
/// the time, plus one upgrade for every level above the first. What it wore
/// goes back into the stock, in time to be fitted again in the same round.
fn recover_formation(economy: &Economy, next: &mut SideState, index: i32) -> Result<(), Unsettled> {
    let position = next
        .formations
        .iter()
        .position(|entry| entry.formation.index == index)
        .ok_or(Unsettled::Missing("formation"))?;
    let entry = next.formations.remove(position);
    let unit = unit_id_from_type(&entry.formation.type_name).ok_or(Unsettled::Unpriced("unit"))?;
    let purse = Purse::new(economy, &next.techs.officers);
    let upgrade = purse.upgrade(unit).ok_or(Unsettled::Unpriced("upgrade"))?;
    next.supply += entry.value.unwrap_or(0) + (entry.formation.level.unwrap_or(1) - 1) * upgrade;
    if let Some(worn) = entry.formation.equipment {
        next.equipment.push(EquipmentItem {
            id: worn,
            durability: None,
        });
        next.equipment.sort();
    }
    Ok(())
}

/// Files the formations a card or an opening hands out.
///
/// Each takes the next index, which is what everything bought afterwards is
/// counted from, so they are filed even though where they land is not settled.
fn hand_out(
    economy: &Economy,
    next: &mut SideState,
    unit: i32,
    squads: i32,
    level: i32,
    placement: &mut dyn FnMut(&SideState, &str) -> Option<Position>,
) -> Result<(), Unsettled> {
    // Recovering one pays back the unit's own price: the side never bought it,
    // so no officer discount ever applied to it.
    let price = economy
        .unit(unit)
        .ok_or(Unsettled::Unpriced("unit"))?
        .supply;
    for _ in 0..squads {
        place(next, unit, level, price, placement)?;
    }
    Ok(())
}

/// Frees every formation that [`crate::mobility::free`] says moves in every
/// round. A formation already free stays free.
fn free_to_move(next: &mut SideState) {
    let techs = next.techs.units.clone();
    for entry in &mut next.formations {
        entry.movable |= crate::mobility::free(&entry.formation, &techs);
    }
}

/// Puts one formation on the board under the next index.
fn place(
    next: &mut SideState,
    unit: i32,
    level: i32,
    value: i32,
    placement: &mut dyn FnMut(&SideState, &str) -> Option<Position>,
) -> Result<(), Unsettled> {
    let (type_name, _) = unit_type_from_id(unit).ok_or(Unsettled::Unpriced("unit"))?;
    let Some(position) = placement(next, type_name) else {
        return Err(Unsettled::GrantedPosition);
    };
    let index = next.next_index.unit;
    next.next_index.unit += 1;
    next.formations.push(StateFormation {
        formation: crate::layout::Formation {
            type_name: type_name.to_owned(),
            index,
            position,
            level: Some(level).filter(|level| *level != 1),
            exp: None,
            rotated: None,
            equipment: None,
            travelling: None,
        },
        value: Some(value),
        // A formation moves in the round it arrives.
        movable: true,
    });
    next.formations.sort_by_key(|entry| entry.formation.index);
    Ok(())
}

/// Adds a skill to the panel, under the first slot index nothing holds.
fn panel_add(next: &mut SideState, id: i32) {
    // A panel is short and its slots are dense, so the first index nothing
    // holds is at most one past the end.
    let index = (0..=i32::try_from(next.battle_skills.len()).unwrap_or(i32::MAX))
        .find(|candidate| {
            !next
                .battle_skills
                .iter()
                .any(|skill| skill.index == *candidate)
        })
        .unwrap_or(0);
    next.battle_skills.push(PanelSkill {
        index,
        id,
        cooldown: 0,
        used: false,
        release: None,
    });
    next.battle_skills.sort_by_key(|skill| skill.index);
}

fn unlock(next: &mut SideState, unit: i32) {
    if !next.shop.unlocked_units.contains(&unit) {
        next.shop.unlocked_units.push(unit);
        next.shop.unlocked_units.sort_unstable();
    }
}

fn formation_mut(next: &mut SideState, index: i32) -> Result<&mut StateFormation, Unsettled> {
    next.formations
        .iter_mut()
        .find(|entry| entry.formation.index == index)
        .ok_or(Unsettled::Missing("formation"))
}

fn clone_target(target: &SkillTarget) -> SkillTarget {
    match target {
        SkillTarget::Area(positions) => SkillTarget::Area(positions.clone()),
        SkillTarget::Unit(index) => SkillTarget::Unit(*index),
        SkillTarget::Construction(index) => SkillTarget::Construction(*index),
    }
}

/// The position an opening is taken from.
///
/// A battle states the opening under `sides` rather than as a round, so the
/// position it moves is not a document field. It is the same for every side of
/// every match: nothing bought, nothing researched, and two towers at level
/// zero. [`before_opening`] adds what the header deals to it.
pub(crate) fn opening_position() -> SideState {
    SideState {
        tower_strengthen_levels: vec![0, 0],
        ..SideState::default()
    }
}

/// `Shop.BUY_COUNT_PER_ROUND`, before any officer or energy tower modifier.
pub(crate) const BUY_COUNT_PER_ROUND: i32 = 2;
/// `Shop.UNLOCK_COUNT_PER_ROUND`.
pub(crate) const UNLOCK_COUNT_PER_ROUND: i32 = 1;

/// Opens `round` on the position the previous round left.
///
/// Two things happen, in this order, before either side takes a decision, so
/// both belong to the position a round opens with. First the round resets what
/// lasts one round and pays its income: the skill panel counts down, the
/// shop's allowances refill, the income arrives less what an energy tower skill
/// still owes, the energy tower skills lapse, and the board's formations are
/// fixed again unless something frees them. Then the officers deliver: an officer's squad,
/// its commander skills and its equipment arrive in its `active_round`, and its
/// unit joins the shop in its `unlock_round`. The squad lands where the board
/// puts it, which `placement` supplies; it arrived this round, so it may move,
/// and a delivered skill starts after the count-down rather than inside it.
///
/// # Errors
///
/// Returns [`Unsettled`] when a spent skill has no cooldown, an activated
/// energy tower skill has no price, or a delivered unit has no price or no
/// landing.
pub fn open_round(
    economy: &Economy,
    state: &SideState,
    round: i32,
    placement: &mut dyn FnMut(&SideState, &str) -> Option<Position>,
) -> Result<SideState, Unsettled> {
    let mut next = state.clone();
    reset(economy, &mut next, round)?;
    for officer in state.techs.officers.clone() {
        let Some(row) = economy.officer(officer) else {
            continue;
        };
        if let Some(opening) = row.opening_unit
            && opening.unlock_round == round
        {
            unlock(&mut next, opening.unit);
        }
        if row.active_round != round {
            continue;
        }
        for skill in &row.commander_skills {
            panel_add(&mut next, *skill);
        }
        next.equipment
            .extend(row.equipment.iter().map(|id| EquipmentItem {
                id: *id,
                durability: None,
            }));
        next.equipment.sort();
        if let Some(opening) = row.opening_unit {
            hand_out(
                economy,
                &mut next,
                opening.unit,
                1,
                opening.level,
                placement,
            )?;
        }
    }
    Ok(next)
}

/// Resets what lasts one round, as `round` opens, and pays its income.
///
/// A panel slot spent in the previous round, by a release or as a deployment
/// skill, restarts at its skill's cooldown; every other counts down by one to
/// no lower than zero. The shop allows two purchases, one more for every
/// Extra Deployment card the side holds, and one unlock. Every energy tower
/// skill lasts one round, and one that defers part of its price is paid for
/// out of this round's income. The income is the map's schedule, the officers
/// the side holds, and the equipment its formations wear; standard 1v1 pays
/// nothing during a fight, so nothing else reaches the supply between rounds. A formation on the board was there last round, so
/// it is fixed unless [`crate::mobility::free`] frees it, except in round 1,
/// whose whole board arrived with the opening. `docs/rules/commander_skills.md`
/// and `docs/rules/mobility.md` state the rules.
fn reset(economy: &Economy, next: &mut SideState, round: i32) -> Result<(), Unsettled> {
    for slot in &mut next.battle_skills {
        slot.cooldown = if slot.used || slot.release.is_some() {
            economy
                .cooldown(slot.id)
                .ok_or(Unsettled::Unpriced("cooldown"))?
                .spent
        } else {
            (slot.cooldown - 1).max(0)
        };
        slot.used = false;
        slot.release = None;
    }
    let extra = next
        .techs
        .officers
        .iter()
        .filter(|officer| **officer == EXTRA_DEPLOYMENT_CARD)
        .count();
    next.shop.buys_remaining = BUY_COUNT_PER_ROUND + i32::try_from(extra).unwrap_or(0);
    next.shop.unlocks_remaining = UNLOCK_COUNT_PER_ROUND;
    // The round's income: the map's schedule and what the side's officers add
    // to it, what the equipment on the board pays, and less what an energy
    // tower skill the previous round activated still owes.
    let mut owed = 0;
    for skill in &next.energy_tower_skills {
        owed += economy
            .energy_tower_skill(*skill)
            .ok_or(Unsettled::Unpriced("energy tower skill"))?
            .owed;
    }
    let worn: i32 = next
        .formations
        .iter()
        .filter_map(|entry| entry.formation.equipment)
        .map(|equipment| economy.equipment_round_supply(equipment))
        .sum();
    let income =
        crate::ledger::round_income(economy, round, &next.techs.officers, economy.round_supply());
    next.supply += income + worn - owed;
    next.energy_tower_skills.clear();
    let techs = next.techs.units.clone();
    for entry in &mut next.formations {
        entry.movable = round <= 1 || crate::mobility::free(&entry.formation, &techs);
    }
    Ok(())
}

/// The position a side takes its opening decision from.
///
/// A standard 1v1 map unlocks no unit and hands out no commander skill before
/// the opening, so the position holds only what the header deals it: the map's
/// reactor core, which [`crate::opening::reactor_core`] reads, the dealt
/// constructions, and two towers at level zero.
#[must_use]
pub fn before_opening(reactor_core: i32, constructions: Vec<StaticPlacement>) -> SideState {
    SideState {
        reactor_core,
        constructions,
        ..opening_position()
    }
}

/// Hands out a team of formations, as round 1 opens: each unit type the team
/// is made of joins the shop, and each of its formations lands at level 1 where
/// the board puts it, in the team's order. A specialist opening hands out
/// nothing here; its officer delivers on its own schedule.
fn deliver_team(
    economy: &Economy,
    next: &mut SideState,
    team: i32,
    placement: &mut dyn FnMut(&SideState, &str) -> Option<Position>,
) -> Result<(), Unsettled> {
    let team = economy
        .advance_team(team)
        .ok_or(Unsettled::Unpriced("opening"))?;
    if team.kind != OpeningKind::Units {
        return Ok(());
    }
    for unit in &team.units {
        unlock(next, *unit);
        hand_out(economy, next, *unit, 1, 1, placement)?;
    }
    Ok(())
}

/// Predicts the position a side opens `round + 1` with, from the position it
/// opened `round` with and the decisions it took there.
///
/// The decisions are stepped in order, and then the next round opens on the
/// result. What a fight changes in between is not applied, and neither is any
/// opening rule [`open_round`] does not hold yet; [`crate::coverage`] names
/// those fields rather than reading them from the recorded next position.
/// `red` names the side, because where the board lands a formation depends on
/// it.
///
/// # Errors
///
/// Returns [`Unsettled`] for the first decision, or the first delivery, this
/// build's tables cannot settle from the position it meets.
pub fn predict(
    economy: &Economy,
    round: i32,
    state: &SideState,
    actions: &[Action],
    red: bool,
) -> Result<SideState, Unsettled> {
    let mut placement = crate::landing::placement(red);
    let mut position = state.clone();
    for action in actions {
        position = step_placing(economy, &position, action, &mut placement)?;
    }
    // Whatever else the fight does, it empties the travelling set: a
    // formation's crossing is over once the fight has run.
    for entry in &mut position.formations {
        entry.formation.travelling = None;
    }
    // Round zero has no fight. The team it chose arrives as round 1 opens,
    // and a position that picked a team of formations keeps no trace of which
    // one, so it is delivered from the decision here.
    if round == 0 {
        for action in actions {
            if let Action::ChooseAdvanceTeam { id, .. } = action {
                deliver_team(economy, &mut position, *id, &mut placement)?;
            }
        }
    }
    open_round(economy, &position, round + 1, &mut placement)
}

#[cfg(all(test, feature = "convert"))]
mod tests {
    use super::{EXTRA_DEPLOYMENT_CARD, Unsettled, step, step_placing};
    use crate::battle::{
        Action, EquipmentItem, PanelSkill, Release, SideState, SkillTarget, StateFormation,
    };
    use crate::convert::battle_from_grbr;
    use crate::economy::{CardKind, Economy};
    use crate::layout::{Experience, Formation, Position};

    /// Steps `actions` in order from `state`, landing whatever a decision hands
    /// out at the main region's centre.
    fn fold(
        economy: &Economy,
        state: &SideState,
        actions: &[Action],
    ) -> Result<SideState, Unsettled> {
        actions.iter().try_fold(state.clone(), |position, action| {
            step_placing(economy, &position, action, &mut |_, _| {
                Some(Position { x: 0, y: -160 })
            })
        })
    }

    /// The indices of the formations `state` holds travelling.
    fn travelling(state: &SideState) -> Vec<i32> {
        state
            .formations
            .iter()
            .filter(|entry| entry.formation.travelling == Some(true))
            .map(|entry| entry.formation.index)
            .collect()
    }

    /// A side holding the given formations and nothing else.
    fn side_holding(placed: &[(i32, Position)]) -> SideState {
        SideState {
            formations: placed
                .iter()
                .map(|(index, position)| StateFormation {
                    formation: Formation {
                        type_name: "marksman".into(),
                        index: *index,
                        position: *position,
                        level: None,
                        exp: None,
                        rotated: None,
                        equipment: None,
                        travelling: None,
                    },
                    value: None,
                    // Placed this round, so free to move.
                    movable: true,
                })
                .collect(),
            ..SideState::default()
        }
    }

    /// A repeatable officer card stacks rather than replacing itself.
    ///
    /// `20022` may be taken again, and one side in the local set holds three
    /// copies of it. Deduplicating the officer list would hide that, so the
    /// list is a multiset and this pins it without needing a replay.
    #[test]
    fn a_repeatable_officer_card_stacks() {
        let economy = Economy::embedded().unwrap();
        let state = SideState {
            techs: crate::layout::Techs {
                officers: vec![20022],
                units: Vec::new(),
            },
            ..SideState::default()
        };
        let taken = [
            Action::ChooseReinforceItem {
                offer: 0,
                id: Some(20022),
            },
            Action::ChooseReinforceItem {
                offer: 1,
                id: Some(20022),
            },
        ];
        let next = fold(&economy, &state, &taken).unwrap();
        assert_eq!(next.techs.officers, vec![20022, 20022, 20022]);
    }

    /// Declining is the same decision, and it hands out nothing.
    ///
    /// The two answers share one action, so the only thing separating them is
    /// the absent `id`. A decline that fell through to the taken branch would
    /// look up a card that does not exist.
    #[test]
    fn a_declined_offer_hands_out_nothing() {
        let economy = Economy::embedded().unwrap();
        let state = SideState::default();
        let declined = [Action::ChooseReinforceItem {
            offer: crate::battle::DECLINED_OFFER,
            id: None,
        }];
        let next = fold(&economy, &state, &declined).unwrap();
        // A decline pays supply back, which is all it hands out.
        assert_eq!(
            SideState {
                supply: state.supply,
                ..next
            },
            state
        );
    }

    /// A card taken and not fitted stays in the side's stock.
    #[test]
    fn an_unfitted_card_stays_in_stock() {
        let economy = Economy::embedded().unwrap();
        let taken = [Action::ChooseReinforceItem {
            offer: 0,
            id: Some(13_030_001),
        }];
        let next = fold(&economy, &SideState::default(), &taken).unwrap();
        assert_eq!(
            next.equipment,
            vec![crate::battle::EquipmentItem {
                id: 13_030_001,
                durability: None,
            }]
        );
    }

    /// Fitting the card taken in the same round leaves the stock where it was.
    ///
    /// This is the only shape the tracked replays exercise, and it is the one
    /// that hides a missing term: a grant and a fit that cancel look the same
    /// as neither being applied.
    #[test]
    fn taking_and_fitting_in_one_round_leaves_the_stock_alone() {
        let economy = Economy::embedded().unwrap();
        let taken = [
            Action::ChooseReinforceItem {
                offer: 0,
                id: Some(13_030_001),
            },
            Action::UseEquipment {
                equipment: 13_030_001,
                unit: 4,
            },
        ];
        let state = side_holding(&[(4, Position { x: 0, y: -160 })]);
        let next = fold(&economy, &state, &taken).unwrap();
        assert!(next.equipment.is_empty());
        assert_eq!(next.formations[0].formation.equipment, Some(13_030_001));
    }

    fn slot(index: i32, id: i32, cooldown: i32) -> PanelSkill {
        PanelSkill {
            index,
            id,
            cooldown,
            used: false,
            release: None,
        }
    }

    /// A slot the round spent restarts at its skill's cooldown, however it was
    /// spent; every other counts down, and none goes below zero.
    #[test]
    fn an_opening_restarts_spent_slots_and_counts_the_rest_down() {
        let economy = Economy::embedded().unwrap();
        let released = PanelSkill {
            release: Some(Release {
                order: 0,
                target: SkillTarget::Area(Vec::new()),
            }),
            ..slot(0, 300_001, 0)
        };
        let trained = PanelSkill {
            used: true,
            ..slot(1, 1_100_001, 0)
        };
        let state = SideState {
            battle_skills: vec![released, trained, slot(2, 300_004, 3), slot(3, 200_001, 0)],
            ..SideState::default()
        };
        let opened = super::open_round(&economy, &state, 4, &mut |_, _| None).unwrap();
        let restart = |id| economy.cooldown(id).unwrap().spent;
        assert_eq!(
            opened.battle_skills,
            vec![
                slot(0, 300_001, restart(300_001)),
                slot(1, 1_100_001, restart(1_100_001)),
                slot(2, 300_004, 2),
                slot(3, 200_001, 0),
            ]
        );
    }

    /// The shop refills to two purchases and one unlock, plus one purchase for
    /// every Extra Deployment card held, and energy tower skills lapse.
    #[test]
    fn an_opening_refills_the_shop_and_lapses_tower_skills() {
        let economy = Economy::embedded().unwrap();
        let state = SideState {
            shop: crate::battle::ShopState {
                unlocked_units: Vec::new(),
                buys_remaining: 0,
                unlocks_remaining: 0,
            },
            energy_tower_skills: vec![1],
            techs: crate::layout::Techs {
                officers: vec![EXTRA_DEPLOYMENT_CARD, EXTRA_DEPLOYMENT_CARD],
                units: Vec::new(),
            },
            ..SideState::default()
        };
        let opened = super::open_round(&economy, &state, 4, &mut |_, _| None).unwrap();
        assert_eq!(
            (opened.shop.buys_remaining, opened.shop.unlocks_remaining),
            (4, 1)
        );
        assert!(opened.energy_tower_skills.is_empty());
    }

    /// What was on the board last round is fixed, unless something frees it;
    /// round 1's whole board arrived with the opening.
    #[test]
    fn an_opening_fixes_the_board_unless_something_frees_it() {
        let economy = Economy::embedded().unwrap();
        let formation = |index, equipment| StateFormation {
            formation: crate::layout::Formation {
                type_name: "crawler".into(),
                index,
                position: Position { x: 0, y: -160 },
                level: None,
                exp: None,
                rotated: None,
                equipment,
                travelling: None,
            },
            value: Some(100),
            movable: true,
        };
        let state = SideState {
            formations: vec![
                formation(0, None),
                formation(1, Some(crate::mobility::DEPLOYMENT_MODULE)),
            ],
            ..SideState::default()
        };
        let movable = |round| {
            super::open_round(&economy, &state, round, &mut |_, _| None)
                .unwrap()
                .formations
                .iter()
                .map(|entry| entry.movable)
                .collect::<Vec<_>>()
        };
        assert_eq!(movable(1), [true, true]);
        assert_eq!(movable(2), [false, true]);
    }

    /// The opening pays the map's schedule, the officers' income and what
    /// worn equipment pays, less what a Rapid Supply activated in the previous
    /// round still owes.
    #[test]
    fn an_opening_pays_the_income_less_what_rapid_supply_owes() {
        let economy = Economy::embedded().unwrap();
        let worn = |index| StateFormation {
            formation: crate::layout::Formation {
                type_name: "crawler".into(),
                index,
                position: Position { x: 0, y: -160 },
                level: None,
                exp: None,
                rotated: None,
                // Command Core pays 50 a round to the side wearing it.
                equipment: Some(13_030_010),
                travelling: None,
            },
            value: Some(100),
            movable: false,
        };
        let state = SideState {
            supply: 10,
            // Supply Specialist adds 50 a round.
            techs: crate::layout::Techs {
                officers: vec![10002],
                units: Vec::new(),
            },
            formations: vec![worn(0), worn(1)],
            // Rapid Supply and one skill that owes nothing.
            energy_tower_skills: vec![1, 3],
            ..SideState::default()
        };
        let opened = super::open_round(&economy, &state, 3, &mut |_, _| None).unwrap();
        // Round 3 of the shared schedule pays 200 + 2 × 200.
        assert_eq!(opened.supply, 10 + 600 + 50 + 2 * 50 - 300);
        assert!(opened.energy_tower_skills.is_empty());
    }

    /// Round zero opens round 1 on the chosen team: its unit types join the
    /// shop, its formations land at level 1 in the team's order, and both
    /// halves of the opening move the map's reactor core.
    #[test]
    fn round_zero_opens_round_one_on_the_chosen_team() {
        let economy = Economy::embedded().unwrap();
        let before = super::before_opening(4500, Vec::new());
        let choice = Action::ChooseAdvanceTeam {
            offer: 2,
            id: 9891,
            specialist: Some(10002),
        };
        let opened = super::predict(&economy, 0, &before, &[choice], true).unwrap();
        assert_eq!(opened.reactor_core, 4500 - 300 - 600);
        assert_eq!(opened.shop.unlocked_units, [10, 24]);
        assert_eq!(opened.next_index.unit, 5);
        assert_eq!(opened.techs.officers, [10002]);
        let team: Vec<_> = opened
            .formations
            .iter()
            .map(|entry| {
                (
                    entry.formation.type_name.as_str(),
                    entry.formation.level,
                    entry.movable,
                )
            })
            .collect();
        assert_eq!(
            team,
            [
                ("crawler", None, true),
                ("crawler", None, true),
                ("crawler", None, true),
                ("tarantula", None, true),
                ("tarantula", None, true),
            ]
        );
    }

    /// The fight empties the travelling set whatever else it does, so a
    /// prediction carries no crossing into the next round.
    #[test]
    fn a_prediction_carries_no_crossing_into_the_next_round() {
        let economy = Economy::embedded().unwrap();
        let mut state = SideState::default();
        state.formations.push(StateFormation {
            formation: crate::layout::Formation {
                type_name: "crawler".into(),
                index: 0,
                position: Position { x: 0, y: -160 },
                level: None,
                exp: None,
                rotated: None,
                equipment: None,
                travelling: Some(true),
            },
            value: Some(100),
            movable: false,
        });
        let predicted = super::predict(&economy, 3, &state, &[], false).unwrap();
        assert_eq!(predicted.formations[0].formation.travelling, None);
    }

    /// An officer delivers its equipment in its own round, not when it arrives.
    ///
    /// 增幅专家 `10013` is the one officer in this build that hands out
    /// equipment, and it hands out three copies of `13030009` in round 1.
    #[test]
    fn an_officer_delivers_its_equipment_on_its_own_schedule() {
        let economy = Economy::embedded().unwrap();
        let state = SideState {
            techs: crate::layout::Techs {
                officers: vec![10013],
                units: Vec::new(),
            },
            ..SideState::default()
        };
        // Round 1 opens with the three items, and applying round 0 is what
        // reaches that position from the one before it.
        let opened = super::open_round(&economy, &state, 1, &mut |_, _| None).unwrap();
        assert_eq!(
            opened
                .equipment
                .iter()
                .map(|item| item.id)
                .collect::<Vec<_>>(),
            vec![13_030_009; 3]
        );
        assert!(
            super::open_round(&economy, &state, 2, &mut |_, _| None)
                .unwrap()
                .equipment
                .is_empty()
        );
    }

    /// Fitting one of several copies takes exactly one out.
    #[test]
    fn fitting_takes_one_copy_out_of_a_stack() {
        let economy = Economy::embedded().unwrap();
        let state = SideState {
            techs: crate::layout::Techs {
                officers: vec![10013],
                units: Vec::new(),
            },
            ..side_holding(&[(0, Position { x: 0, y: -160 })])
        };
        let opened = super::open_round(&economy, &state, 1, &mut |_, _| None).unwrap();
        let fitted = [Action::UseEquipment {
            equipment: 13_030_009,
            unit: 0,
        }];
        assert_eq!(fold(&economy, &opened, &fitted).unwrap().equipment.len(), 2);
    }

    /// Recovering a formation hands back what it wore, in time to re-fit it.
    ///
    /// Round 5 of `[你是蓬莱花仙]VS[crower]` is this shape: blue takes an
    /// Upgrade Kit, fits it to formation 5, upgrades that formation, recovers
    /// it with Field Recovery, and fits the same item to formation 7. Without
    /// the return the second fit has nothing to take.
    #[test]
    fn recovering_a_formation_returns_what_it_wore() {
        let economy = Economy::embedded().unwrap();
        let state = SideState {
            battle_skills: vec![crate::battle::PanelSkill {
                index: 0,
                id: 900_001,
                cooldown: 0,
                used: false,
                release: None,
            }],
            formations: vec![crate::battle::StateFormation {
                formation: crate::layout::Formation {
                    type_name: "marksman".into(),
                    index: 5,
                    position: crate::layout::Position { x: 0, y: 0 },
                    level: None,
                    exp: None,
                    rotated: None,
                    equipment: Some(13_030_004),
                    travelling: None,
                },
                value: Some(100),
                movable: false,
            }],
            ..SideState::default()
        };
        let mut state = state;
        state
            .formations
            .extend(side_holding(&[(7, Position { x: 0, y: -160 })]).formations);
        let recovered = [Action::ReleaseCommanderSkill {
            skill: 0,
            target: crate::battle::SkillTarget::Unit(5),
        }];
        assert_eq!(
            fold(&economy, &state, &recovered)
                .unwrap()
                .equipment
                .iter()
                .map(|item| item.id)
                .collect::<Vec<_>>(),
            vec![13_030_004]
        );
        let refitted = [
            Action::ReleaseCommanderSkill {
                skill: 0,
                target: crate::battle::SkillTarget::Unit(5),
            },
            Action::UseEquipment {
                equipment: 13_030_004,
                unit: 7,
            },
        ];
        let next = fold(&economy, &state, &refitted).unwrap();
        assert!(next.equipment.is_empty());
        assert_eq!(next.formations[0].formation.index, 7);
        assert_eq!(next.formations[0].formation.equipment, Some(13_030_004));
    }

    /// A fit with nothing to take is refused rather than clamped away.
    ///
    /// An empty stock is where a balanced round and an impossible one both end,
    /// so a fit that found nothing has to stop the round rather than leave a
    /// position that compares equal to a recorded one.
    #[test]
    fn a_fit_with_nothing_in_stock_is_refused() {
        let economy = Economy::embedded().unwrap();
        let fitted = Action::UseEquipment {
            equipment: 13_030_004,
            unit: 0,
        };
        let state = side_holding(&[(0, Position { x: 0, y: -160 })]);
        assert_eq!(
            step(&economy, &state, &fitted),
            Err(Unsettled::Missing("equipment"))
        );
    }

    /// What the tracked replays say about the inventory.
    ///
    /// The tracked set pins fits, equipment cards and stock carried across a
    /// round boundary, and every fit it takes finds its item in the stock.
    #[test]
    fn tracked_equipment_is_exercised_and_every_fit_finds_its_item() {
        let economy = Economy::embedded().unwrap();
        let (mut fits, mut cards) = (0, 0);
        let mut held = 0;
        let mut refused = Vec::new();
        for entry in std::fs::read_dir("../../tests/grbr").expect("tracked replay directory") {
            let path = entry.expect("directory entry").path();
            if path.extension().is_none_or(|extension| extension != "grbr") {
                continue;
            }
            let Ok(battle) = battle_from_grbr(&std::fs::read(&path).unwrap()) else {
                continue;
            };
            for turn in &battle.turns {
                for (side, state, actions) in [
                    ("blue", &turn.state.sides.blue, &turn.actions.blue),
                    ("red", &turn.state.sides.red, &turn.actions.red),
                ] {
                    if !state.equipment.is_empty() {
                        held += 1;
                    }
                    if let Err(reason) = fold(&economy, state, actions) {
                        refused.push(format!(
                            "{} round {} {side}: {reason:?}",
                            path.file_name().unwrap().to_string_lossy(),
                            turn.round
                        ));
                    }
                    for action in actions {
                        match action {
                            Action::UseEquipment { .. } => fits += 1,
                            Action::ChooseReinforceItem { id: Some(id), .. }
                                if economy.card_kind(*id) == Some(CardKind::Equipment) =>
                            {
                                cards += 1;
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        assert_eq!((fits, cards), (118, 97));
        assert_eq!(held, 24);
        assert!(refused.is_empty(), "{refused:#?}");
    }

    /// A release is the only decision that moves the contraption allocator.
    ///
    /// The fight consumes a contraption, but the index it took is never handed
    /// out again, which is what makes the allocator a decision's to settle
    /// rather than a maximum over whatever survived.
    #[test]
    fn releasing_a_contraption_advances_its_allocator() {
        let economy = Economy::embedded().unwrap();
        let state = SideState {
            next_index: crate::battle::NextIndex {
                unit: 7,
                contraption: 3,
            },
            ..SideState::default()
        };
        let released = [
            Action::ReleaseContraption {
                contraption: 20001,
                position: crate::layout::Position { x: 0, y: 0 },
                extra_position: None,
            },
            Action::ReleaseContraption {
                contraption: 10001,
                position: crate::layout::Position { x: 10, y: 10 },
                extra_position: None,
            },
        ];
        assert_eq!(
            fold(&economy, &state, &released)
                .unwrap()
                .next_index
                .contraption,
            5
        );
    }

    /// A purchase prices the unit, takes a slot and files it under the next
    /// index.
    #[test]
    fn a_purchase_files_the_formation_it_creates() {
        let economy = Economy::embedded().unwrap();
        let state = SideState {
            supply: 1000,
            shop: crate::battle::ShopState {
                unlocked_units: vec![9],
                buys_remaining: 2,
                unlocks_remaining: 1,
            },
            next_index: crate::battle::NextIndex {
                unit: 7,
                contraption: 0,
            },
            ..SideState::default()
        };
        let bought = Action::BuyUnit {
            unit: 9,
            position: Position { x: 0, y: -160 },
        };
        let next = step(&economy, &state, &bought).unwrap();
        assert_eq!(next.next_index.unit, 8);
        assert_eq!(next.shop.buys_remaining, 1);
        assert_eq!(next.formations.len(), 1);
        let placed = &next.formations[0];
        assert_eq!(placed.formation.index, 7);
        assert_eq!(placed.formation.position, Position { x: 0, y: -160 });
        // What it is worth to recover is what this side paid for it.
        assert_eq!(placed.value, Some(state.supply - next.supply));
    }

    /// Upgrading a formation starts its new rank with no experience.
    ///
    /// The recorded decision carries an `expRecord` for its undo, which is the
    /// clue: the experience it saves is the experience the upgrade discards.
    #[test]
    fn an_upgrade_clears_the_experience_it_replaces() {
        let economy = Economy::embedded().unwrap();
        let mut state = side_holding(&[(0, Position { x: 0, y: -160 })]);
        state.supply = 1000;
        state.formations[0].formation.exp = Some(Experience {
            current: 650,
            maximum: 650,
        });
        let next = step(&economy, &state, &Action::UpgradeUnit { index: 0 }).unwrap();
        assert_eq!(next.formations[0].formation.level, Some(2));
        assert_eq!(next.formations[0].formation.exp, None);
        assert!(next.supply < state.supply);
    }

    /// A card that hands out squads also puts their unit in the shop.
    ///
    /// The squads take the next indices as the card is taken, which is what
    /// every later purchase of the round is counted from.
    #[test]
    fn a_squad_card_unlocks_its_unit_and_takes_its_indices() {
        let economy = Economy::embedded().unwrap();
        let state = SideState {
            supply: 1000,
            next_index: crate::battle::NextIndex {
                unit: 4,
                contraption: 0,
            },
            ..SideState::default()
        };
        let card = 102_212;
        let reinforcement = economy.unit_reinforcement(card).expect("a squad card");
        let taken = Action::ChooseReinforceItem {
            offer: 0,
            id: Some(card),
        };
        // Where a squad lands is the board's to decide, so the plain step
        // stops at the position and names why.
        assert_eq!(
            step(&economy, &state, &taken),
            Err(crate::transition::Unsettled::GrantedPosition)
        );
        let mut placed = vec![Position { x: 0, y: -160 }, Position { x: -20, y: -160 }];
        placed.reverse();
        let next =
            crate::transition::step_placing(&economy, &state, &taken, &mut |_, _| placed.pop())
                .unwrap();
        assert_eq!(next.next_index.unit, 4 + reinforcement.squads);
        assert_eq!(next.shop.unlocked_units, vec![reinforcement.unit]);
        assert_eq!(next.formations.len(), 2);
        assert_eq!(next.formations[0].formation.index, 4);
        assert_eq!(next.formations[1].formation.index, 5);
    }

    /// An opening of formations delivers nothing at the moment it is chosen.
    ///
    /// Its squads arrive when the first deployment round opens, which is not a
    /// decision. What the choice does settle is the reactor core, and both
    /// halves of it price that: round 0 of `[你是蓬莱花仙]VS[crower]` takes team
    /// `9890` with specialist `20005` and moves the core by `100`, which is the
    /// team's own `100` and the specialist's `0`.
    #[test]
    fn an_opening_settles_the_core_and_delivers_nothing() {
        let economy = Economy::embedded().unwrap();
        let state = SideState::default();
        let chosen = Action::ChooseAdvanceTeam {
            offer: 2,
            id: 9890,
            specialist: Some(20005),
        };
        let next = step(&economy, &state, &chosen).unwrap();
        assert_eq!(next.reactor_core, 100);
        assert!(next.formations.is_empty());
        assert_eq!(next.next_index.unit, 0);
        assert_eq!(next.techs.officers, vec![20005]);
    }

    /// The specialist half prices the core as well as the team half.
    #[test]
    fn both_halves_of_an_opening_price_the_core() {
        let economy = Economy::embedded().unwrap();
        let chosen = Action::ChooseAdvanceTeam {
            offer: 0,
            id: 9890,
            specialist: Some(20032),
        };
        let next = step(&economy, &SideState::default(), &chosen).unwrap();
        assert_eq!(next.reactor_core, 600);
    }

    /// Field Recovery pays back what a formation cost and returns what it wore.
    #[test]
    fn recovering_a_formation_pays_and_returns() {
        let economy = Economy::embedded().unwrap();
        let mut state = side_holding(&[(5, Position { x: 0, y: -160 })]);
        state.battle_skills = vec![crate::battle::PanelSkill {
            index: 0,
            id: 900_001,
            cooldown: 0,
            used: false,
            release: None,
        }];
        state.formations[0].value = Some(400);
        state.formations[0].formation.equipment = Some(13_030_004);
        let released = Action::ReleaseCommanderSkill {
            skill: 0,
            target: crate::battle::SkillTarget::Unit(5),
        };
        let next = step(&economy, &state, &released).unwrap();
        assert!(next.formations.is_empty());
        assert_eq!(next.supply, 400);
        assert_eq!(
            next.equipment,
            vec![EquipmentItem {
                id: 13_030_004,
                durability: None,
            }]
        );
        assert!(next.battle_skills[0].release.is_some());
    }

    /// Intensive Training fills its formation's bar and spends its slot, and
    /// leaves no release behind: its work is done before the fight.
    #[test]
    fn training_fills_the_bar_and_spends_the_slot() {
        let economy = Economy::embedded().unwrap();
        let mut state = side_holding(&[(0, Position { x: 0, y: -160 })]);
        state.formations[0].formation.exp = Some(Experience {
            current: 54,
            maximum: 650,
        });
        state.battle_skills = vec![crate::battle::PanelSkill {
            index: 0,
            id: 1_100_001,
            cooldown: 0,
            used: false,
            release: None,
        }];
        let train = Action::ReleaseCommanderSkill {
            skill: 0,
            target: crate::battle::SkillTarget::Unit(0),
        };
        let next = step(&economy, &state, &train).unwrap();
        assert_eq!(
            next.formations[0].formation.exp,
            Some(Experience {
                current: 650,
                maximum: 650,
            })
        );
        assert!(next.battle_skills[0].used);
        assert!(next.battle_skills[0].release.is_none());
        assert_eq!(next.supply, state.supply);

        // Training refuses a full formation.
        state.formations[0].formation.exp = next.formations[0].formation.exp;
        assert_eq!(
            step(&economy, &state, &train),
            Err(crate::transition::Unsettled::Refused(
                "training a full formation"
            ))
        );

        // It refuses the last level too, full or not.
        state.formations[0].formation.level = Some(9);
        state.formations[0].formation.exp = Some(Experience {
            current: 12,
            maximum: 4373,
        });
        assert_eq!(
            step(&economy, &state, &train),
            Err(crate::transition::Unsettled::Refused(
                "training a formation at the last level"
            ))
        );
    }

    /// A formation on the board since an earlier round stays where it is,
    /// until something frees it: a Deployment Module, its unit's Jump Drive,
    /// or a Redeploy release. Each frees it for good or for the round, and a
    /// move after any of them is allowed.
    #[test]
    fn only_a_formation_free_to_move_moves() {
        let economy = Economy::embedded().unwrap();
        let mut fixed = side_holding(&[(0, Position { x: 0, y: -160 })]);
        fixed.formations[0].movable = false;
        fixed.supply = 1000;
        let shift = Action::MoveUnit {
            index: 0,
            position: Position { x: 20, y: -160 },
            rotated: false,
        };
        assert_eq!(
            step(&economy, &fixed, &shift),
            Err(crate::transition::Unsettled::Refused(
                "moving a formation fixed in place"
            ))
        );

        // A Deployment Module frees the formation that wears it.
        let mut worn = fixed.clone();
        worn.equipment = vec![EquipmentItem {
            id: crate::mobility::DEPLOYMENT_MODULE,
            durability: None,
        }];
        let fitted = step(
            &economy,
            &worn,
            &Action::UseEquipment {
                equipment: crate::mobility::DEPLOYMENT_MODULE,
                unit: 0,
            },
        )
        .unwrap();
        assert!(fitted.formations[0].movable);
        assert!(step(&economy, &fitted, &shift).is_ok());

        // Redeploy frees its target and spends its slot without a release.
        let mut panel = fixed.clone();
        panel.battle_skills = vec![crate::battle::PanelSkill {
            index: 0,
            id: crate::mobility::REDEPLOY_SKILLS[0],
            cooldown: 0,
            used: false,
            release: None,
        }];
        let redeployed = step(
            &economy,
            &panel,
            &Action::ReleaseCommanderSkill {
                skill: 0,
                target: crate::battle::SkillTarget::Unit(0),
            },
        )
        .unwrap();
        assert!(redeployed.formations[0].movable);
        assert!(redeployed.battle_skills[0].used);
        assert!(redeployed.battle_skills[0].release.is_none());
        assert!(step(&economy, &redeployed, &shift).is_ok());

        // A Jump Drive frees every formation of its unit, and no other.
        let mut wasps = side_holding(&[
            (0, Position { x: 0, y: -160 }),
            (1, Position { x: 40, y: -160 }),
        ]);
        wasps.supply = 1000;
        wasps.formations[0].formation.type_name = "wasp".into();
        for entry in &mut wasps.formations {
            entry.movable = false;
        }
        let researched = step(
            &economy,
            &wasps,
            &Action::UpgradeTechnology {
                unit: 6,
                tech: 1606,
            },
        )
        .unwrap();
        assert!(researched.formations[0].movable);
        assert!(!researched.formations[1].movable);
    }

    /// A move writes the board and nothing else, travelling included.
    #[test]
    fn a_move_writes_the_board_alone() {
        let economy = Economy::embedded().unwrap();
        let mut state = side_holding(&[(0, Position { x: 0, y: -160 })]);
        state.supply = 700;
        let next = step(
            &economy,
            &state,
            &Action::MoveUnit {
                index: 0,
                position: Position { x: 310, y: 20 },
                rotated: true,
            },
        )
        .unwrap();
        assert_eq!(next.supply, 700);
        assert_eq!(next.formations[0].formation.travelling, Some(true));
        assert_eq!(next.formations[0].formation.rotated, Some(true));
    }

    /// A move into a flank is what puts a formation in the travelling set.
    ///
    /// The formation starts in the main half, so the move changes region and
    /// the region it arrives in decides.
    #[test]
    fn arriving_on_a_flank_starts_travelling() {
        let economy = Economy::embedded().unwrap();
        let state = side_holding(&[(0, Position { x: 0, y: -160 })]);
        let moved = [Action::MoveUnit {
            index: 0,
            position: Position { x: 310, y: 20 },
            rotated: false,
        }];
        assert_eq!(
            travelling(&fold(&economy, &state, &moved).unwrap()),
            vec![0]
        );
        assert!(travelling(&state).is_empty());
    }

    /// Shuffling a formation about inside one region leaves the set alone.
    ///
    /// Both directions matter. A settled formation moved about the main half
    /// does not join the set, and a travelling one moved about its own flank
    /// does not leave it.
    #[test]
    fn a_move_inside_one_region_settles_nothing() {
        let economy = Economy::embedded().unwrap();
        let state = side_holding(&[(0, Position { x: 0, y: -160 })]);
        let about_the_main_half = [Action::MoveUnit {
            index: 0,
            position: Position { x: 200, y: -40 },
            rotated: false,
        }];
        assert!(travelling(&fold(&economy, &state, &about_the_main_half).unwrap()).is_empty());

        let arrived = [
            Action::MoveUnit {
                index: 0,
                position: Position { x: 310, y: 20 },
                rotated: false,
            },
            Action::MoveUnit {
                index: 0,
                position: Position { x: 330, y: 290 },
                rotated: false,
            },
        ];
        assert_eq!(
            travelling(&fold(&economy, &state, &arrived).unwrap()),
            vec![0]
        );
    }

    /// Crossing from one flank to the other is a change of region.
    ///
    /// Round 6 of `[oolly]VS[二阶唐Cirno]` is the shape: blue's formation 14
    /// sits settled on the left flank at `(-330, 85)`, crosses to `(350, 285)`
    /// on the right one, and travels again. Treating the two flanks as one
    /// ambush zone would leave it settled, and this is the only crossing in
    /// the tracked set that starts from a settled formation.
    #[test]
    fn crossing_between_flanks_travels_again() {
        let economy = Economy::embedded().unwrap();
        let state = side_holding(&[(0, Position { x: -330, y: 100 })]);
        let crossed = [Action::MoveUnit {
            index: 0,
            position: Position { x: 330, y: 100 },
            rotated: false,
        }];
        assert_eq!(
            travelling(&fold(&economy, &state, &crossed).unwrap()),
            vec![0]
        );
    }

    /// Coming back to the main half takes a formation out of the set.
    #[test]
    fn returning_to_the_main_half_settles() {
        let economy = Economy::embedded().unwrap();
        let mut state = side_holding(&[(0, Position { x: -330, y: 100 })]);
        state.formations[0].formation.travelling = Some(true);
        assert_eq!(travelling(&state), vec![0]);
        let returned = [Action::MoveUnit {
            index: 0,
            position: Position { x: 0, y: -160 },
            rotated: false,
        }];
        assert!(travelling(&fold(&economy, &state, &returned).unwrap()).is_empty());
    }

    /// A formation this round created starts in the main half.
    ///
    /// A purchase and a card both put their formation there, and neither names
    /// a position the state already holds, so the index is unknown until the
    /// round hands it out. Starting it anywhere else would make its first move
    /// look like a change of region.
    #[test]
    fn a_formation_created_this_round_starts_settled() {
        let economy = Economy::embedded().unwrap();
        let bought = [
            Action::BuyUnit {
                unit: 1,
                position: Position { x: 0, y: -160 },
            },
            Action::MoveUnit {
                index: 0,
                position: Position { x: 100, y: -60 },
                rotated: false,
            },
        ];
        assert!(travelling(&fold(&economy, &SideState::default(), &bought).unwrap()).is_empty());
    }

    /// What the tracked replays say about the travelling set.
    ///
    /// The set is the deployment's own state, so no converted document holds
    /// it to compare against and these are coverage counts rather than a
    /// transition test. They say the tracked set exercises every arm: moves
    /// that start a flank deployment, moves that end one, and rounds that
    /// leave several formations travelling at once.
    #[test]
    fn the_tracked_set_pins_travelling_coverage() {
        let economy = Economy::embedded().unwrap();
        let (mut side_rounds, mut travelled, mut formations) = (0, 0, 0);
        let mut widest = 0;
        let mut first_round = i32::MAX;
        for entry in std::fs::read_dir("../../tests/grbr").expect("tracked replay directory") {
            let path = entry.expect("directory entry").path();
            if path.extension().is_none_or(|extension| extension != "grbr") {
                continue;
            }
            let Ok(battle) = battle_from_grbr(&std::fs::read(&path).unwrap()) else {
                continue;
            };
            for turn in &battle.turns {
                for (state, actions) in [
                    (&turn.state.sides.blue, &turn.actions.blue),
                    (&turn.state.sides.red, &turn.actions.red),
                ] {
                    // No converted state carries one, which is the fight
                    // clearing the set between rounds.
                    assert!(
                        state
                            .formations
                            .iter()
                            .all(|entry| entry.formation.travelling != Some(true)),
                        "{} round {} opens with a travelling formation",
                        path.file_name().unwrap().to_string_lossy(),
                        turn.round
                    );
                    let indices = travelling(&fold(&economy, state, actions).unwrap());
                    side_rounds += 1;
                    if !indices.is_empty() {
                        travelled += 1;
                        first_round = first_round.min(turn.round);
                    }
                    widest = widest.max(indices.len());
                    formations += indices.len();
                }
            }
        }
        assert_eq!(
            (side_rounds, travelled, formations, widest),
            (668, 100, 146, 5)
        );
        // The flank regions open at round 2, so nothing can travel before it.
        assert_eq!(first_round, 2);
    }
}
