//! Reconstructs deployment decisions from their two endpoint positions.
//!
//! The standard sequence uses the opening and closing states and the round's
//! offers. Conversion and formatting can retain an effective action history
//! when initialization depends on order or the standard sequence disagrees
//! with the closing state.

use std::collections::BTreeSet;

use crate::catalog::{NativeFormation, resolve_contraption_type, unit_id_from_type};
use crate::economy::Economy;
use crate::layout::{Experience, Region};
use crate::ledger::{Purse, RECOVERY_SKILLS};
use crate::r#match::{Action, Offers, SideState, SkillTarget, StateUnit};

const ENHANCEMENT_MODULE: i32 = 13_030_004;

/// The effective actions and the initialization effects of their order.
#[derive(Default)]
pub(crate) struct DeploymentHistory {
    actions: Vec<Action>,
    initialization: crate::project::InitializationHistory,
}

impl DeploymentHistory {
    /// Observe an action after it has been stepped in its historical order.
    pub(crate) fn record(&mut self, before: &SideState, position: &SideState, action: &Action) {
        self.initialization.record(before, position, action);
        if !matches!(action, Action::Concede) {
            self.actions.push(action.clone());
        }
    }
}

/// Normalizes a recorded deployment when its endpoint and initialization allow it.
pub(crate) fn deployment_with_history(
    economy: &Economy,
    opened: &SideState,
    ended: &SideState,
    offers: Option<&Offers>,
    red: bool,
    history: &DeploymentHistory,
) -> Result<Vec<Action>, String> {
    if !history.initialization.warnings(opened, ended).is_empty() {
        return historical_deployment(economy, opened, ended, red, offers, &history.actions);
    }
    match deployment(economy, opened, ended, offers, red) {
        Ok(actions) => Ok(actions),
        Err(error) if history.actions.is_empty() => Err(error),
        Err(error) => historical_deployment(economy, opened, ended, red, offers, &history.actions)
            .map_err(|historical| {
                format!("{error}; historical deployment also failed: {historical}")
            }),
    }
}

/// Formats a match, rebuilding ordinary complete action segments from their
/// endpoints and retaining histories when required to preserve the deployment.
///
/// # Errors
/// Returns a malformed match or a deployment neither normalization nor its
/// effective action history can represent.
pub fn match_yaml(bytes: &[u8]) -> Result<String, String> {
    match_yaml_with(bytes, crate::r#match::TechLoadoutFormat::Compact)
}

/// Formats a match with the requested `tech_loadout` representation.
///
/// # Errors
/// Returns a malformed match or a deployment neither normalization nor its
/// effective action history can represent.
pub fn match_yaml_with(
    bytes: &[u8],
    format: crate::r#match::TechLoadoutFormat,
) -> Result<String, String> {
    let stated = crate::opening::stated(bytes)?.ok_or("not a match document")?;
    let mut game = crate::r#match::read(bytes)?.ok_or("not a match document")?;
    let economy = Economy::embedded()?;
    let last = game.turns.len().saturating_sub(1);
    for (at, turn) in game.turns.iter_mut().enumerate() {
        if at == last && !stated.ends_on_actions {
            continue;
        }
        for (red, opened, actions) in [
            (false, &turn.state.blue, &mut turn.actions.blue),
            (true, &turn.state.red, &mut turn.actions.red),
        ] {
            if actions.is_empty() || actions.as_slice() == [Action::Concede] {
                continue;
            }
            let offers = turn.state.reinforce_offers.as_ref();
            let mut closed = opened.clone();
            let mut placement = crate::landing::placement(red);
            let mut history = DeploymentHistory::default();
            for action in actions.iter() {
                let next = crate::transition::step_placing(
                    economy,
                    &closed,
                    action,
                    offers,
                    &mut placement,
                )
                .map_err(|error| {
                    format!(
                        "round {} {}: {action:?}: {error}",
                        turn.round,
                        if red { "red" } else { "blue" }
                    )
                })?;
                history.record(&closed, &next, action);
                closed = next;
            }
            let conceded = actions.last() == Some(&Action::Concede);
            *actions = deployment_with_history(
                economy,
                opened,
                &closed,
                turn.state.reinforce_offers.as_ref(),
                red,
                &history,
            )
            .map_err(|error| {
                format!(
                    "round {} {}: {error}",
                    turn.round,
                    if red { "red" } else { "blue" }
                )
            })?;
            if conceded {
                actions.push(Action::Concede);
            }
        }
    }
    let mut values = crate::r#match::segment_values_with(&game, format)?;
    if !game.turns.is_empty()
        && !stated.ends_on_actions
        && values
            .last()
            .and_then(|value| value.get("kind"))
            .and_then(serde_yaml::Value::as_str)
            == Some("action")
    {
        values.pop();
    }
    values
        .iter()
        .map(crate::spelling::document)
        .collect::<Result<Vec<_>, _>>()
        .map(|segments| segments.join("---\n"))
}

/// Reconstructs a standard deployment solely from its endpoints and round's deal.
///
/// # Errors
/// Refuses an endpoint the standard sequence cannot reproduce.
pub(crate) fn deployment(
    economy: &Economy,
    opened: &SideState,
    ended: &SideState,
    offers: Option<&Offers>,
    red: bool,
) -> Result<Vec<Action>, String> {
    let mut builder = Builder {
        economy,
        target: ended,
        red,
        offers,
        position: opened.clone(),
        actions: Vec::new(),
        elite_recruitment_at: None,
        research: ended
            .techs
            .iter()
            .filter(|id| !opened.techs.contains(id))
            .copied()
            .collect(),
    };
    if let Some(offers) = offers {
        match ended.reinforce_choice {
            Some(Offers::UNANSWERED) if opened == ended => return Ok(Vec::new()),
            Some(index) => {
                let id = offers
                    .choice(index)
                    .ok_or("closing reinforcement choice is not among the offers")?;
                builder.push(Action::ChooseReinforceItem { index, id })?;
            }
            None => return Err("closing position omits its reinforcement choice".into()),
        }
    }
    if builder.can_activate(1) {
        builder.push(Action::ActiveEnergyTowerSkill { skill: 1 })?;
    }
    let first = builder.position.next_index.unit;
    let after_purchases = ended.next_index.unit;
    if after_purchases < first {
        return Err("deployment's closing unit allocator went backwards".into());
    }
    if !(first..after_purchases).eq(ended
        .units
        .iter()
        .filter(|unit| unit.index >= first)
        .map(|unit| unit.index))
    {
        return Err("closing unit allocator has a gap without a formation or tombstone".into());
    }
    let elite = builder.can_activate(4);
    let cuts = if elite {
        first..=after_purchases
    } else {
        first..=first
    };
    let mut best: Option<Vec<Action>> = None;
    let mut errors = Vec::new();
    for cut in cuts {
        let mut candidate = builder.clone();
        candidate.elite_recruitment_at = elite.then_some(cut);
        match candidate.finish() {
            Ok(actions) => {
                if best.as_ref().is_none_or(|held| actions.len() < held.len()) {
                    best = Some(actions);
                }
            }
            Err(error) => errors.push(error),
        }
    }
    if let Some(actions) = best {
        return Ok(actions);
    }
    errors.sort();
    errors.dedup();
    Err(format!(
        "deployment has no standard action sequence: {}",
        errors.join("; ")
    ))
}

fn historical_deployment(
    economy: &Economy,
    opened: &SideState,
    ended: &SideState,
    red: bool,
    offers: Option<&Offers>,
    history: &[Action],
) -> Result<Vec<Action>, String> {
    let check = |actions: &[Action]| -> Result<(), String> {
        let mut position = opened.clone();
        for action in actions {
            position = checked_step(economy, &position, action, offers, red)?;
        }
        if position == *ended {
            Ok(())
        } else {
            Err("historical actions do not reproduce the closing position".into())
        }
    };
    let compacted = collapse_same_region_moves(economy, opened, history, red, offers)?;
    if compacted != history && check(&compacted).is_ok() {
        return Ok(compacted);
    }
    check(history)?;
    Ok(history.to_vec())
}

fn checked_step(
    economy: &Economy,
    position: &SideState,
    action: &Action,
    offers: Option<&Offers>,
    red: bool,
) -> Result<SideState, String> {
    crate::transition::check_operation(position, action)
        .and_then(|()| crate::transition::check_place(position, action))
        .and_then(|()| {
            crate::transition::step_placing(
                economy,
                position,
                action,
                offers,
                &mut crate::landing::placement(red),
            )
        })
        .map_err(|error| format!("{action:?}: {error}"))
}

/// Keep crossing moves and every non-movement decision in place. A move can
/// replace its predecessor only when both stay inside the same region, refer
/// to the same unit, and have no intervening decision.
fn collapse_same_region_moves(
    economy: &Economy,
    opened: &SideState,
    history: &[Action],
    red: bool,
    offers: Option<&Offers>,
) -> Result<Vec<Action>, String> {
    let mut position = opened.clone();
    let mut actions = Vec::new();
    let mut previous_move = None;
    let mut placement = crate::landing::placement(red);
    for action in history {
        let current_move = match action {
            Action::MoveUnit {
                index,
                position: destination,
                ..
            } => position
                .units
                .iter()
                .find(|entry| entry.index == *index)
                .and_then(|entry| {
                    let region = Region::of(entry.position?);
                    (region == Region::of(*destination)).then_some((*index, region))
                }),
            _ => None,
        };
        if current_move.is_some() && current_move == previous_move {
            *actions.last_mut().expect("a merge follows a move") = action.clone();
        } else {
            actions.push(action.clone());
        }
        position =
            crate::transition::step_placing(economy, &position, action, offers, &mut placement)
                .map_err(|error| error.to_string())?;
        previous_move = current_move;
    }
    Ok(actions)
}

#[derive(Clone)]
struct Builder<'a> {
    economy: &'a Economy,
    target: &'a SideState,
    red: bool,
    offers: Option<&'a Offers>,
    position: SideState,
    actions: Vec<Action>,
    research: BTreeSet<i32>,
    elite_recruitment_at: Option<i32>,
}

impl Builder<'_> {
    fn push(&mut self, action: Action) -> Result<(), String> {
        self.dependencies(&action)?;
        self.position = checked_step(self.economy, &self.position, &action, self.offers, self.red)?;
        self.actions.push(action);
        Ok(())
    }

    fn can_activate(&self, skill: i32) -> bool {
        self.target.energy_tower_skills.contains(&skill)
            && !self.position.energy_tower_skills.contains(&skill)
    }

    fn dependencies(&mut self, action: &Action) -> Result<(), String> {
        match action {
            Action::BuyUnit { unit, .. } => {
                if !self.position.unlocked_units.contains(unit) {
                    self.push(Action::UnlockUnit { unit: *unit })?;
                }
                if self.position.shop.buys_remaining == 0 && self.can_activate(3) {
                    self.push(Action::ActiveEnergyTowerSkill { skill: 3 })?;
                }
                if self.elite_recruitment_at == Some(self.position.next_index.unit) {
                    self.push(Action::ActiveEnergyTowerSkill { skill: 4 })?;
                }
            }
            Action::ReleaseCommanderSkill { index, id, target } => {
                self.panel(*index, *id)?;
                if RECOVERY_SKILLS.contains(id)
                    && let SkillTarget::Unit(index) = target
                {
                    let name = self.unit(*index)?.type_name.clone();
                    if self
                        .position
                        .active_units()
                        .filter(|entry| entry.type_name == name)
                        .count()
                        == 1
                    {
                        let owner = unit_id_from_type(&name).ok_or("unknown recovered unit")?;
                        let research: Vec<_> = self
                            .research
                            .iter()
                            .copied()
                            .filter(|tech| self.economy.technology_owner(*tech) == Some(owner))
                            .collect();
                        for tech in research {
                            self.research(tech)?;
                        }
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn panel(&mut self, index: i32, id: i32) -> Result<(), String> {
        if self
            .position
            .battle_skills
            .iter()
            .any(|slot| slot.index == index && slot.id == id)
        {
            return Ok(());
        }
        let blueprint = self
            .target
            .blueprints
            .iter()
            .copied()
            .find(|blueprint| self.economy.blueprint_skill(*blueprint) == Some(id))
            .ok_or_else(|| format!("no blueprint creates panel slot {index} for skill {id}"))?;
        self.blueprint(blueprint)
    }

    fn blueprint(&mut self, id: i32) -> Result<(), String> {
        if self.position.blueprints.contains(&id) {
            return Ok(());
        }
        if let Some(first) = self.economy.blueprint_predecessor(id) {
            self.blueprint(first)?;
        }
        if let Some(skill) = self.economy.blueprint_skill(id) {
            let slot = self
                .target
                .battle_skills
                .iter()
                .find(|slot| {
                    slot.id == skill
                        && !self
                            .position
                            .battle_skills
                            .iter()
                            .any(|held| held.index == slot.index)
                })
                .ok_or("blueprint has no unfilled target panel slot")?
                .index;
            let before: Vec<_> = self
                .target
                .battle_skills
                .iter()
                .filter(|wanted| wanted.index < slot)
                .filter(|wanted| {
                    !self
                        .position
                        .battle_skills
                        .iter()
                        .any(|held| held.index == wanted.index)
                })
                .map(|wanted| (wanted.index, wanted.id))
                .collect();
            for (index, skill) in before {
                self.panel(index, skill)?;
            }
        }
        self.push(Action::ActiveBlueprint { id })
    }

    fn purchase_type(&self, index: i32) -> Result<i32, String> {
        unit_id_from_type(&self.wanted(index)?.type_name).ok_or("unknown purchased unit".into())
    }

    fn finish(mut self) -> Result<Vec<Action>, String> {
        for recovery in self.recoveries() {
            self.recover(recovery)?;
        }
        while self.position.next_index.unit < self.target.next_index.unit {
            self.buy_next()?;
        }
        for tech in self.research.clone() {
            self.research(tech)?;
        }
        self.remaining()
    }

    fn infrastructure(&mut self) -> Result<(), String> {
        for skill in self.target.energy_tower_skills.clone() {
            if !self.position.energy_tower_skills.contains(&skill) {
                self.push(Action::ActiveEnergyTowerSkill { skill })?;
            }
        }
        let mut blueprints = self.target.blueprints.clone();
        blueprints.sort_by_key(|id| {
            (
                self.economy
                    .blueprint_skill(*id)
                    .and_then(|skill| {
                        self.target
                            .battle_skills
                            .iter()
                            .find(|slot| slot.id == skill)
                            .map(|slot| slot.index)
                    })
                    .unwrap_or(i32::MAX),
                *id,
            )
        });
        for id in blueprints {
            if !self.position.blueprints.contains(&id) {
                self.blueprint(id)?;
            }
        }
        for (tower, level) in self.target.tower_strengthen_levels.iter().enumerate() {
            while self
                .position
                .tower_strengthen_levels
                .get(tower)
                .copied()
                .unwrap_or(0)
                < *level
            {
                self.push(Action::StrengthenTower {
                    tower: i32::try_from(tower).map_err(|error| error.to_string())?,
                })?;
            }
        }
        Ok(())
    }

    fn recoveries(&self) -> Vec<Action> {
        let mut slots: Vec<_> = self
            .target
            .battle_skills
            .iter()
            .filter(|slot| RECOVERY_SKILLS.contains(&slot.id) && slot.release.is_some())
            .collect();
        slots.sort_by_key(
            |slot| match slot.release.as_ref().map(|release| &release.target) {
                Some(SkillTarget::Unit(index)) => (0, *index, slot.index),
                Some(SkillTarget::Construction(index)) => (1, *index, slot.index),
                _ => (2, 0, slot.index),
            },
        );
        slots.into_iter().filter_map(|slot| {
            let target = slot.release.as_ref()?.target.clone();
            // A purchase recovered in the same round is filled at its allocator
            // position, after the earlier purchases have handed out their IDs.
            if matches!(target, SkillTarget::Unit(index) if index >= self.position.next_index.unit) {
                return None;
            }
            Some(Action::ReleaseCommanderSkill { index: slot.index, id: slot.id, target })
        }).collect()
    }

    fn recovery_upgrades(&self, index: i32) -> Result<i32, String> {
        let target = self.wanted(index)?;
        let upgrades = target.level.unwrap_or(1) - self.unit(index)?.level.unwrap_or(1);
        if !target.recovered || upgrades < 0 {
            return Err(format!(
                "formation {index} has an invalid recovery endpoint"
            ));
        }
        Ok(upgrades)
    }

    fn recover(&mut self, recovery: Action) -> Result<(), String> {
        if let Action::ReleaseCommanderSkill {
            target: SkillTarget::Unit(index),
            ..
        } = recovery
        {
            let upgrades = self.recovery_upgrades(index)?;
            self.redeploy(index)?;
            if upgrades > 0 {
                self.fit_recovery_modules(index)?;
            }
            for _ in 0..upgrades {
                self.upgrade(index)?;
            }
            self.train(index)?;
        }
        self.push(recovery)
    }

    // Only spend a slot and a decision on a module that lowers the price.
    // Recovery returns each fitted copy before any later formation needs one.
    fn fit_recovery_modules(&mut self, index: i32) -> Result<(), String> {
        let unit =
            unit_id_from_type(&self.unit(index)?.type_name).ok_or("unknown recovered unit")?;
        if !self
            .economy
            .unit(unit)
            .is_some_and(|row| row.wears_equipment)
        {
            return Ok(());
        }
        let worn = self.position.worn_equipment(index);
        let slots = self.economy.equipment_slots(&self.position.officers);
        let free = slots.saturating_sub(worn.len());
        let in_stock = self
            .position
            .equipments
            .iter()
            .filter(|id| **id == ENHANCEMENT_MODULE)
            .count();
        let mut discount = worn
            .into_iter()
            .map(|id| self.economy.equipment_upgrade_supply(id))
            .sum();
        let module = self.economy.equipment_upgrade_supply(ENHANCEMENT_MODULE);
        let purse = Purse::new(self.economy, &self.position.officers);
        for _ in 0..free.min(in_stock) {
            let price = purse
                .upgrade_wearing(unit, discount)
                .ok_or("unknown upgrade price")?;
            let reduced = purse
                .upgrade_wearing(unit, discount + module)
                .ok_or("unknown upgrade price")?;
            if reduced >= price {
                break;
            }
            self.push(Action::UseEquipment {
                equipment: ENHANCEMENT_MODULE,
                index,
            })?;
            discount += module;
        }
        Ok(())
    }

    fn research(&mut self, tech: i32) -> Result<(), String> {
        if !self.research.contains(&tech) {
            return Ok(());
        }
        let unit = self
            .economy
            .technology_owner(tech)
            .ok_or("unknown technology owner")?;
        let name = crate::catalog::unit_type_from_id(unit)
            .ok_or("unknown research unit")?
            .0;
        if !self
            .position
            .active_units()
            .any(|entry| entry.type_name == name)
        {
            return Err(format!("technology {tech} has no formation"));
        }
        if self.research.contains(&tech) {
            self.push(Action::UpgradeTechnology { unit, tech })?;
            self.research.remove(&tech);
        }
        Ok(())
    }

    fn unit(&self, index: i32) -> Result<&StateUnit, String> {
        self.position
            .active_units()
            .find(|entry| entry.index == index)
            .ok_or_else(|| format!("formation {index} is absent"))
    }

    fn wanted(&self, index: i32) -> Result<&StateUnit, String> {
        self.target
            .units
            .iter()
            .find(|entry| entry.index == index)
            .ok_or_else(|| format!("target formation {index} is absent"))
    }

    fn redeploy(&mut self, index: i32) -> Result<(), String> {
        for action in self.pending_skill(index, &crate::mobility::REDEPLOY_SKILLS) {
            self.push(action)?;
        }
        Ok(())
    }

    fn moves(&mut self) -> Result<(), String> {
        let mut pending = Vec::new();
        let standing: Vec<_> = self.position.active_units().cloned().collect();
        for held in standing {
            let wanted = self.wanted(held.index)?.clone();
            let from = held.position.ok_or("standing formation has no position")?;
            let to = wanted.position.ok_or("target formation has no position")?;
            let moved = from != to
                || held.rotated.unwrap_or(false) != wanted.rotated.unwrap_or(false)
                || held.travelling.unwrap_or(false) != wanted.travelling.unwrap_or(false);
            self.redeploy(held.index)?;
            if !moved {
                continue;
            }
            if !self.unit(held.index)?.movable(&self.position) {
                return Err(format!(
                    "formation {} cannot move to its target position",
                    held.index
                ));
            }
            if Region::of(from) == Region::of(to)
                && Region::of(to).is_flank()
                && wanted.travelling == Some(true)
                && held.travelling != Some(true)
            {
                let aside = crate::landing::free_spot(
                    &self.position,
                    &held.type_name,
                    held.rotated.unwrap_or(false),
                    Region::Main,
                    self.red,
                    Some(held.index),
                    &[],
                )
                .ok_or("no main-region waypoint to reproduce travelling")?;
                pending.push(Action::MoveUnit {
                    index: held.index,
                    position: aside,
                    rotated: held.rotated.unwrap_or(false),
                });
            }
            pending.push(Action::MoveUnit {
                index: held.index,
                position: to,
                rotated: wanted.rotated.unwrap_or(false),
            });
        }
        let mut aside = BTreeSet::new();
        while !pending.is_empty() {
            let ready = pending.iter().enumerate().find_map(|(at, action)| {
                let Action::MoveUnit { index, .. } = action else { return None; };
                if pending[..at].iter().any(|earlier| matches!(earlier, Action::MoveUnit { index: other, .. } if other == index)) { return None; }
                crate::transition::check_place(&self.position, action).is_ok().then_some(at)
            });
            if let Some(at) = ready {
                self.push(pending.remove(at))?;
                continue;
            }
            let action = step_aside(&self.position, &pending, &aside, self.red)
                .ok_or_else(|| format!("cannot exchange deployment space for {pending:?}"))?;
            if let Action::MoveUnit { index, .. } = action {
                aside.insert(index);
            }
            self.push(action)?;
        }
        Ok(())
    }

    fn buy_next(&mut self) -> Result<(), String> {
        let index = self.position.next_index.unit;
        let unit = self.purchase_type(index)?;
        let name = crate::catalog::unit_type_from_id(unit)
            .ok_or("unknown purchased unit")?
            .0;
        if crate::landing::landing(&self.position, name, self.red).is_none() {
            return Err(format!("default purchase of {name} has no landing space"));
        }
        self.push(Action::BuyUnit {
            unit,
            position: None,
            rotated: false,
        })?;
        if self.wanted(index)?.recovered {
            let recovery = self
                .target
                .battle_skills
                .iter()
                .find(|slot| {
                    RECOVERY_SKILLS.contains(&slot.id)
                        && slot
                            .release
                            .as_ref()
                            .is_some_and(|release| release.target == SkillTarget::Unit(index))
                })
                .ok_or_else(|| {
                    format!("recovered formation {index} has no Field Recovery release")
                })?;
            self.recover(Action::ReleaseCommanderSkill {
                index: recovery.index,
                id: recovery.id,
                target: SkillTarget::Unit(index),
            })?;
        }
        Ok(())
    }

    fn remaining(mut self) -> Result<Vec<Action>, String> {
        for wanted in self.target.active_units() {
            let index = wanted.index;
            let held = self.position.worn_equipment(index);
            let equipment = self.target.worn_equipment(index);
            if !equipment.starts_with(&held) {
                return Err(format!("formation {index} changes equipment fitting order"));
            }
            for equipment in &equipment[held.len()..] {
                self.push(Action::UseEquipment {
                    equipment: *equipment,
                    index,
                })?;
            }
        }
        self.moves()?;
        for wanted in self.target.active_units() {
            let index = wanted.index;
            while self.unit(index)?.level.unwrap_or(1) < wanted.level.unwrap_or(1) {
                self.upgrade(index)?;
            }
            self.train(index)?;
        }
        for wanted in &self.target.contraptions {
            if wanted.index < self.position.next_index.contraption {
                continue;
            }
            if wanted.index != self.position.next_index.contraption {
                return Err("contraption allocator has a gap".into());
            }
            let NativeFormation::Contraption(id) = resolve_contraption_type(&wanted.type_name)
                .ok_or("unknown contraption type")?
                .native
            else {
                return Err("not a contraption".into());
            };
            self.push(Action::ReleaseContraption {
                contraption: id,
                position: wanted.position,
                extra_position: None,
            })?;
        }
        self.infrastructure()?;
        for unit in self.target.unlocked_units.clone() {
            if !self.position.unlocked_units.contains(&unit) {
                self.push(Action::UnlockUnit { unit })?;
            }
        }
        let mut battle: Vec<_> = self
            .target
            .battle_skills
            .iter()
            .filter(|slot| slot.battle_order().is_some())
            .collect();
        battle.sort_by_key(|slot| slot.battle_order());
        for slot in battle {
            self.push(Action::ReleaseCommanderSkill {
                index: slot.index,
                id: slot.id,
                target: slot.release.as_ref().unwrap().target.clone(),
            })?;
        }
        if self.position != *self.target {
            let actual = serde_json::to_value(&self.position).map_err(|error| error.to_string())?;
            let expected = serde_json::to_value(self.target).map_err(|error| error.to_string())?;
            let different: Vec<_> = actual
                .as_object()
                .unwrap()
                .iter()
                .filter(|(key, value)| expected.get(key) != Some(*value))
                .map(|(key, value)| format!("{key}: produced {value}, expected {}", expected[key]))
                .collect();
            return Err(different.join(", "));
        }
        Ok(self.actions)
    }

    fn upgrade(&mut self, index: i32) -> Result<(), String> {
        if !self.unit(index)?.exp.is_some_and(Experience::is_full)
            && let Some(action) = self
                .pending_skill(index, &crate::transition::TRAINING_SKILLS)
                .into_iter()
                .next()
        {
            self.push(action)?;
        }
        self.push(Action::UpgradeUnit { index })
    }

    fn train(&mut self, index: i32) -> Result<(), String> {
        for action in self.pending_skill(index, &crate::transition::TRAINING_SKILLS) {
            self.push(action)?;
        }
        Ok(())
    }

    fn pending_skill(&self, index: i32, ids: &[i32]) -> Vec<Action> {
        let target = SkillTarget::Unit(index);
        self.target
            .battle_skills
            .iter()
            .filter(|slot| {
                ids.contains(&slot.id)
                    && slot
                        .release
                        .as_ref()
                        .is_some_and(|release| release.target == target)
                    && !self
                        .position
                        .battle_skills
                        .iter()
                        .any(|held| held.index == slot.index && held.release.is_some())
            })
            .map(|slot| Action::ReleaseCommanderSkill {
                index: slot.index,
                id: slot.id,
                target: target.clone(),
            })
            .collect()
    }
}

/// Finds the existing swap-space waypoint without changing a formation's region.
fn step_aside(
    position: &SideState,
    pending: &[Action],
    stepped_aside: &BTreeSet<i32>,
    red: bool,
) -> Option<Action> {
    let needed: Vec<_> = pending
        .iter()
        .filter_map(|action| match action {
            Action::MoveUnit {
                index,
                position: to,
                rotated,
            } => position
                .units
                .iter()
                .find(|entry| entry.index == *index)
                .and_then(|entry| crate::landing::unit_rect(&entry.type_name, *to, *rotated)),
            _ => None,
        })
        .collect();
    pending.iter().enumerate().find_map(|(at, action)| {
        let Action::MoveUnit { index, .. } = action else { return None; };
        if stepped_aside.contains(index) || pending[..at].iter().any(|earlier| matches!(earlier, Action::MoveUnit { index: other, .. } if other == index)) { return None; }
        let standing = position.active_units().find(|entry| entry.index == *index)?;
        let rotated = standing.rotated.unwrap_or(false);
        crate::landing::free_spot(position, &standing.type_name, rotated, Region::of(standing.position?), red, Some(*index), &needed)
            .map(|position| Action::MoveUnit { index: *index, position, rotated })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{Position, UnitSource};
    use crate::r#match::{Allowances, NextIndex, PanelSkill};

    fn side() -> SideState {
        SideState {
            supply: 10_000,
            unlocked_units: (1..=32).chain([2002]).collect(),
            shop: Allowances {
                buys_remaining: 3,
                unlocks_remaining: 1,
                contraptions_remaining: 8,
            },
            next_index: NextIndex {
                unit: 2,
                contraption: 0,
            },
            units: [0, 1]
                .into_iter()
                .map(|index| StateUnit {
                    record_position: None,
                    record_rotated: None,
                    type_name: "marksman".into(),
                    index,
                    position: Some(Position {
                        x: index * 20,
                        y: -160,
                    }),
                    level: None,
                    exp: None,
                    rotated: None,
                    equipments: Vec::new(),
                    travelling: None,
                    source: UnitSource::Joined,
                    recovered: false,
                    value: Some(100),
                })
                .collect(),
            ..SideState::default()
        }
    }

    fn slot(id: i32) -> PanelSkill {
        PanelSkill {
            index: 0,
            id,
            cooldown: 0,
            standing: Vec::new(),
            release: None,
        }
    }

    fn fill_bar(unit: &mut StateUnit) {
        let mut bar = Experience::at_level(0, &unit.type_name, unit.level.unwrap_or(1)).unwrap();
        bar.current = bar.maximum;
        unit.exp = Some(bar);
    }

    fn add_training(opened: &mut SideState, unit: i32) -> Action {
        let index = i32::try_from(opened.battle_skills.len()).unwrap();
        opened.battle_skills.push(PanelSkill {
            index,
            ..slot(1_100_001)
        });
        Action::ReleaseCommanderSkill {
            index,
            id: 1_100_001,
            target: SkillTarget::Unit(unit),
        }
    }

    fn moved(index: i32, x: i32, y: i32) -> Action {
        Action::MoveUnit {
            index,
            position: Position { x, y },
            rotated: false,
        }
    }

    fn bought(unit: i32, x: i32, y: i32) -> Action {
        Action::BuyUnit {
            unit,
            position: Some(Position { x, y }),
            rotated: false,
        }
    }

    fn default_purchase(unit: i32) -> Action {
        Action::BuyUnit {
            unit,
            position: None,
            rotated: false,
        }
    }

    fn close(opened: &SideState, actions: &[Action]) -> SideState {
        actions.iter().fold(opened.clone(), |position, action| {
            crate::transition::step_placing(
                Economy::embedded().unwrap(),
                &position,
                action,
                None,
                &mut crate::landing::placement(false),
            )
            .unwrap()
        })
    }

    fn normalized(opened: &SideState, closed: &SideState) -> Vec<Action> {
        let economy = Economy::embedded().unwrap();
        let actions = deployment(economy, opened, closed, None, false).unwrap();
        assert_eq!(
            crate::transition::deployed(economy, opened, &actions, false, None).unwrap(),
            *closed
        );
        actions
    }

    fn recorded_history(opened: &SideState, history: &[Action]) -> DeploymentHistory {
        let mut dependencies = DeploymentHistory::default();
        let mut position = opened.clone();
        for action in history {
            let next = close(&position, std::slice::from_ref(action));
            dependencies.record(&position, &next, action);
            position = next;
        }
        dependencies
    }

    #[test]
    fn cloak_history_keeps_crossings_and_research_between_move_runs() {
        let mut opened = side();
        opened.units[0].type_name = "phantom_ray".into();
        opened.units[0].position.as_mut().unwrap().x = -100;
        opened.units[0].value = None;
        let research = Action::UpgradeTechnology {
            unit: 25,
            tech: 3925,
        };
        let history = vec![
            moved(0, -150, -260),
            moved(0, -200, -260),
            moved(0, 330, 50),
            moved(0, 330, 100),
            moved(0, 330, 150),
            research.clone(),
            moved(0, 330, 200),
            moved(0, 330, 250),
        ];
        let ended = close(&opened, &history);
        let standard = normalized(&opened, &ended);
        assert_eq!(standard.first(), Some(&research));
        let actions =
            normalized_with_history(&opened, &ended, &recorded_history(&opened, &history));
        assert_eq!(
            actions,
            vec![
                moved(0, -200, -260),
                moved(0, 330, 50),
                moved(0, 330, 150),
                research,
                moved(0, 330, 250),
            ]
        );
    }

    #[test]
    fn purchased_cloak_units_keep_research_after_flank_deployment() {
        let opened = side();
        let history = vec![
            bought(25, -150, -260),
            moved(2, 330, 50),
            Action::UpgradeTechnology {
                unit: 25,
                tech: 3925,
            },
        ];
        let ended = close(&opened, &history);
        assert_eq!(
            normalized_with_history(&opened, &ended, &recorded_history(&opened, &history),),
            history
        );
    }

    #[test]
    fn other_initialization_warnings_also_retain_history() {
        for (name, tech, initial_techs, officers, destination) in [
            ("void_eye", 4430, vec![], vec![20021], (330, 50)),
            ("hound", 4228, vec![], vec![], (330, 50)),
            ("wraith", 110_181, vec![4418], vec![20021], (-200, -160)),
        ] {
            let unit = unit_id_from_type(name).unwrap();
            let mut opened = side();
            opened.units[0].type_name = name.into();
            opened.units[0].position.as_mut().unwrap().x = -100;
            opened.units[0].value = None;
            opened.techs = initial_techs;
            opened.officers = officers;
            let history = vec![
                moved(0, destination.0, destination.1),
                Action::UpgradeTechnology { unit, tech },
            ];
            let ended = close(&opened, &history);
            assert_eq!(
                normalized_with_history(&opened, &ended, &recorded_history(&opened, &history),),
                history,
                "{name}"
            );
        }
    }

    #[test]
    fn ordinary_history_still_reconstructs_a_legal_endpoint_plan() {
        let opened = side();
        let history = vec![moved(0, 20, -160), moved(0, -100, -160)];
        let ended = close(&opened, &history);
        assert_eq!(
            normalized_with_history(&opened, &ended, &recorded_history(&opened, &history),),
            vec![moved(0, -100, -160)],
        );
    }

    #[test]
    fn fallback_refuses_history_that_fails_to_replay_the_endpoint() {
        let economy = Economy::embedded().unwrap();
        let opened = side();
        let mut closed = opened.clone();
        closed.supply += 1;
        let history = recorded_history(&opened, &[moved(0, -100, -160)]);
        let error =
            deployment_with_history(economy, &opened, &closed, None, false, &history).unwrap_err();
        assert!(error.contains("supply"), "{error}");
        assert!(
            error.contains("historical actions do not reproduce"),
            "{error}"
        );
    }

    #[test]
    fn move_compaction_does_not_cross_another_units_move() {
        let opened = side();
        let history = vec![
            moved(0, -100, -160),
            moved(1, 100, -160),
            moved(0, -150, -160),
        ];
        assert_eq!(
            collapse_same_region_moves(
                Economy::embedded().unwrap(),
                &opened,
                &history,
                false,
                None,
            )
            .unwrap(),
            history
        );
    }

    fn normalized_with_history(
        opened: &SideState,
        closed: &SideState,
        dependencies: &DeploymentHistory,
    ) -> Vec<Action> {
        let economy = Economy::embedded().unwrap();
        let actions =
            deployment_with_history(economy, opened, closed, None, false, dependencies).unwrap();
        let reproduced =
            crate::transition::deployed(economy, opened, &actions, false, None).unwrap();
        assert_eq!(&reproduced, closed);
        assert_eq!(
            deployment_with_history(
                economy,
                opened,
                &reproduced,
                None,
                false,
                &recorded_history(opened, &actions)
            )
            .unwrap(),
            actions
        );
        actions
    }

    #[test]
    fn up_front_module_fitting_is_standard_and_full_price_upgrades_keep_history() {
        let mut opened = side();
        opened.units[0].type_name = "melting_point".into();
        opened.units[0].level = Some(4);
        opened.equipments.push(ENHANCEMENT_MODULE);
        fill_bar(&mut opened.units[0]);
        let train = add_training(&mut opened, 0);
        let upgrade = Action::UpgradeUnit { index: 0 };
        let module = Action::UseEquipment {
            equipment: ENHANCEMENT_MODULE,
            index: 0,
        };
        for (history, cost) in [
            (
                vec![
                    module.clone(),
                    upgrade.clone(),
                    train.clone(),
                    upgrade.clone(),
                ],
                200,
            ),
            (
                vec![
                    upgrade.clone(),
                    module.clone(),
                    train.clone(),
                    upgrade.clone(),
                ],
                300,
            ),
            (
                vec![
                    upgrade.clone(),
                    train.clone(),
                    upgrade.clone(),
                    module.clone(),
                ],
                400,
            ),
        ] {
            let closed = close(&opened, &history);
            assert_eq!(opened.supply - closed.supply, cost);
            let pure = deployment(Economy::embedded().unwrap(), &opened, &closed, None, false);
            if cost == 200 {
                assert_eq!(pure.unwrap(), history);
            } else {
                assert!(pure.unwrap_err().contains("supply"));
            }
            assert_eq!(
                normalized_with_history(&opened, &closed, &recorded_history(&opened, &history)),
                history
            );
        }
    }

    #[test]
    fn staggered_module_fittings_fall_back_without_changing_the_paid_cost() {
        let mut opened = side();
        opened.units[0].type_name = "melting_point".into();
        opened.officers.push(10540);
        opened.equipments = vec![ENHANCEMENT_MODULE; 2];
        fill_bar(&mut opened.units[0]);
        let train_first = add_training(&mut opened, 0);
        let train_second = add_training(&mut opened, 0);
        let upgrade = Action::UpgradeUnit { index: 0 };
        let module = Action::UseEquipment {
            equipment: ENHANCEMENT_MODULE,
            index: 0,
        };
        let history = [
            upgrade.clone(),
            module.clone(),
            train_first,
            upgrade.clone(),
            module,
            train_second,
            upgrade,
        ];
        let closed = close(&opened, &history);
        assert_eq!(opened.supply - closed.supply, 300);
        assert!(
            deployment(Economy::embedded().unwrap(), &opened, &closed, None, false)
                .unwrap_err()
                .contains("supply")
        );
        assert_eq!(
            normalized_with_history(&opened, &closed, &recorded_history(&opened, &history)),
            history
        );
    }

    #[test]
    fn a_purchase_and_its_immediate_move_remain_separate_actions() {
        let opened = side();
        let purchase = Action::BuyUnit {
            unit: 1,
            position: None,
            rotated: false,
        };
        let movement = Action::MoveUnit {
            index: 2,
            position: Position { x: -100, y: -160 },
            rotated: true,
        };
        let expected = [purchase, movement];
        let closed = close(&opened, &expected);
        let actions = normalized(&opened, &closed);
        assert_eq!(actions, expected);
        assert!(!serde_yaml::to_string(&actions).unwrap().contains("level:"));
    }

    #[test]
    fn a_vacated_waypoint_does_not_block_a_later_purchase() {
        let mut opened = side();
        opened.units[0].position = Some(Position { x: -350, y: 285 });
        let closed = close(
            &opened,
            &[moved(0, 350, 55), moved(0, 90, -35), bought(15, 350, 55)],
        );
        let actions = normalized(&opened, &closed);
        assert_eq!(
            actions,
            [default_purchase(15), moved(0, 90, -35), moved(2, 350, 55)]
        );
        let other = close(&opened, &[moved(0, 90, -35), bought(15, 350, 55)]);
        assert_eq!(normalized(&opened, &other), actions);
    }

    #[test]
    fn an_exchange_uses_legal_space_before_either_final_move() {
        let opened = side();
        let closed = close(&opened, &[moved(0, 20, -160), moved(1, 0, -160)]);
        assert_eq!(normalized(&opened, &closed).len(), 3);
    }

    #[test]
    fn default_purchase_refuses_a_full_board_instead_of_moving_early() {
        let mut opened = side();
        while let Some(position) = crate::landing::landing(&opened, "marksman", false) {
            let mut held = opened.units[0].clone();
            held.index = opened.next_index.unit;
            held.position = Some(position);
            opened.next_index.unit += 1;
            opened.units.push(held);
        }
        let history = [moved(0, 310, 50), default_purchase(2)];
        let closed = close(&opened, &history);
        let error =
            deployment(Economy::embedded().unwrap(), &opened, &closed, None, false).unwrap_err();
        assert!(
            error.contains("default purchase of marksman has no landing space"),
            "{error}"
        );
        assert_eq!(
            normalized_with_history(&opened, &closed, &recorded_history(&opened, &history)),
            history
        );
    }

    #[test]
    fn existing_and_new_formations_move_after_equipment_and_before_upgrades() {
        let mut opened = side();
        opened.units[0].type_name = "wasp".into();
        opened.units[0].source = UnitSource::Legacy;
        opened.equipments.push(crate::mobility::DEPLOYMENT_MODULE);
        let research = Action::UpgradeTechnology {
            unit: 6,
            tech: 1606,
        };
        let equip = Action::UseEquipment {
            index: 0,
            equipment: crate::mobility::DEPLOYMENT_MODULE,
        };
        let train = add_training(&mut opened, 2);
        let upgrade = Action::UpgradeUnit { index: 2 };
        let history = [
            research.clone(),
            moved(0, 100, -160),
            bought(1, -100, -160),
            train.clone(),
            upgrade.clone(),
            equip.clone(),
        ];
        let closed = close(&opened, &history);
        assert_eq!(
            normalized(&opened, &closed),
            [
                default_purchase(1),
                research,
                equip,
                moved(0, 100, -160),
                moved(2, -100, -160),
                train,
                upgrade
            ]
        );
    }

    #[test]
    fn a_purchase_vacates_its_default_landing_before_interceptor_placement() {
        let opened = side();
        let landed = close(&opened, &[default_purchase(1)]);
        let position = landed
            .units
            .iter()
            .find(|held| held.index == 2)
            .unwrap()
            .position
            .unwrap();
        let interceptor = Action::ReleaseContraption {
            contraption: 30_001,
            position,
            extra_position: None,
        };
        assert!(crate::transition::check_place(&landed, &interceptor).is_err());
        let history = [default_purchase(1), moved(2, -100, -160), interceptor];
        assert_eq!(normalized(&opened, &close(&opened, &history)), history);
    }

    #[test]
    fn jump_drive_precedes_the_existing_formations_move() {
        let mut opened = side();
        opened.units[0].type_name = "wasp".into();
        opened.units[0].source = UnitSource::Legacy;
        let tech = Action::UpgradeTechnology {
            unit: 6,
            tech: 1606,
        };
        let closed = close(&opened, &[tech.clone(), moved(0, 100, -160)]);
        assert_eq!(normalized(&opened, &closed), [tech, moved(0, 100, -160)]);
    }

    #[test]
    fn recovery_depends_on_research_of_its_last_formation() {
        let mut opened = side();
        opened.units.truncate(1);
        opened.battle_skills.push(slot(900_001));
        let tech = Action::UpgradeTechnology {
            unit: 2,
            tech: 10202,
        };
        let recover = Action::ReleaseCommanderSkill {
            index: 0,
            id: 900_001,
            target: SkillTarget::Unit(0),
        };
        let closed = close(&opened, &[tech.clone(), recover.clone()]);
        assert_eq!(normalized(&opened, &closed), [tech, recover]);
    }

    #[test]
    fn recovery_uses_the_module_only_if_an_upgrade_benefits() {
        let mut opened = side();
        opened.battle_skills.push(slot(900_001));
        opened.equipments.push(ENHANCEMENT_MODULE);
        fill_bar(&mut opened.units[0]);
        let recover = Action::ReleaseCommanderSkill {
            index: 0,
            id: 900_001,
            target: SkillTarget::Unit(0),
        };
        let optimized = [
            Action::UseEquipment {
                equipment: ENHANCEMENT_MODULE,
                index: 0,
            },
            Action::UpgradeUnit { index: 0 },
            recover.clone(),
        ];
        assert_eq!(normalized(&opened, &close(&opened, &optimized)), optimized);
        let paid = [Action::UpgradeUnit { index: 0 }, recover.clone()];
        let closed = close(&opened, &paid);
        assert!(
            deployment(Economy::embedded().unwrap(), &opened, &closed, None, false)
                .unwrap_err()
                .contains("supply")
        );
        assert_eq!(
            normalized_with_history(&opened, &closed, &recorded_history(&opened, &paid)),
            paid
        );
        assert_eq!(
            normalized(&opened, &close(&opened, std::slice::from_ref(&recover))),
            [recover]
        );
    }

    #[test]
    fn elite_recruitment_keeps_each_purchase_level_and_price() {
        let opened = side();
        let elite = Action::ActiveEnergyTowerSkill { skill: 4 };
        let closed = close(
            &opened,
            &[bought(1, -100, -160), elite.clone(), bought(15, 100, -160)],
        );
        let actions = normalized(&opened, &closed);
        let purchases: Vec<_> = actions
            .iter()
            .enumerate()
            .filter_map(|(at, action)| match action {
                Action::BuyUnit { .. } => Some(at),
                _ => None,
            })
            .collect();
        let activation = actions.iter().position(|action| action == &elite).unwrap();
        assert!(purchases[0] < activation && activation < purchases[1]);
    }

    #[test]
    fn elite_recruitment_uses_the_economic_cut_for_a_recovered_purchase() {
        let mut opened = side();
        opened.battle_skills.push(slot(900_001));
        opened.equipments.push(ENHANCEMENT_MODULE);
        let train = add_training(&mut opened, 3);
        let taken = [
            default_purchase(1),
            Action::ActiveEnergyTowerSkill { skill: 4 },
            default_purchase(2),
            Action::UseEquipment {
                equipment: ENHANCEMENT_MODULE,
                index: 3,
            },
            train,
            Action::UpgradeUnit { index: 3 },
            Action::ReleaseCommanderSkill {
                index: 0,
                id: 900_001,
                target: SkillTarget::Unit(3),
            },
        ];
        let closed = close(&opened, &taken);
        assert_eq!(closed.units[3].level, Some(3));
        assert!(closed.units[3].recovered);
        assert_eq!(normalized(&opened, &closed), taken);
    }

    #[test]
    fn elite_recruitment_buys_a_higher_rank_with_an_empty_bar() {
        let opened = side();
        let elite = Action::ActiveEnergyTowerSkill { skill: 4 };
        let buy = default_purchase(2);
        let closed = close(&opened, &[elite.clone(), buy.clone()]);
        assert_eq!(closed.units[2].level, Some(2));
        assert_eq!(closed.units[2].exp.unwrap().current, 0);
        assert_eq!(normalized(&opened, &closed), [elite, buy]);
    }

    #[test]
    fn a_recovered_purchase_keeps_its_recorded_type_and_upgrade() {
        let mut opened = side();
        opened.battle_skills.push(slot(900_001));
        let train = add_training(&mut opened, 2);
        let history = [
            bought(15, -100, -160),
            train,
            Action::UpgradeUnit { index: 2 },
            Action::ReleaseCommanderSkill {
                index: 0,
                id: 900_001,
                target: SkillTarget::Unit(2),
            },
        ];
        let closed = close(&opened, &history);
        let actions =
            normalized_with_history(&opened, &closed, &recorded_history(&opened, &history));
        assert!(matches!(actions[0], Action::BuyUnit { unit: 15, .. }));
        assert!(actions.contains(&Action::UpgradeUnit { index: 2 }));
        assert_eq!(normalized(&opened, &closed), actions);
    }

    #[test]
    fn consumed_redeployment_survives_its_target_being_recovered() {
        let mut opened = side();
        opened.battle_skills.push(slot(1_000_001));
        opened.battle_skills.push(PanelSkill {
            index: 1,
            ..slot(900_001)
        });
        let taken = [
            Action::ReleaseCommanderSkill {
                index: 0,
                id: 1_000_001,
                target: SkillTarget::Unit(0),
            },
            Action::ReleaseCommanderSkill {
                index: 1,
                id: 900_001,
                target: SkillTarget::Unit(0),
            },
        ];
        assert_eq!(normalized(&opened, &close(&opened, &taken)), taken);
    }

    #[test]
    fn training_keeps_its_target_when_multiple_formations_upgrade() {
        let mut opened = side();
        opened.battle_skills.push(slot(1_100_001));
        fill_bar(&mut opened.units[0]);
        let taken = [
            Action::UpgradeUnit { index: 0 },
            Action::ReleaseCommanderSkill {
                index: 0,
                id: 1_100_001,
                target: SkillTarget::Unit(1),
            },
            Action::UpgradeUnit { index: 1 },
        ];
        let closed = close(&opened, &taken);
        assert!(
            closed
                .units
                .iter()
                .all(|unit| unit.exp == Experience::at_level(0, &unit.type_name, 2))
        );
        let written = serde_yaml::to_string(&closed).unwrap();
        let mut read: SideState = serde_yaml::from_str(&written).unwrap();
        read.shop = closed.shop;
        assert_eq!(normalized(&opened, &read), taken);
    }

    #[test]
    fn recovery_restores_continuous_training_and_existing_module_refunds() {
        let mut opened = side();
        opened.units[0].exp = Experience::of(1, "marksman", 1).unwrap();
        opened.units[0].equipments.push(ENHANCEMENT_MODULE);
        opened.battle_skills = vec![
            slot(900_001),
            PanelSkill {
                index: 1,
                ..slot(1_100_001)
            },
            PanelSkill {
                index: 2,
                ..slot(1_100_001)
            },
        ];
        let taken = [
            Action::ReleaseCommanderSkill {
                index: 1,
                id: 1_100_001,
                target: SkillTarget::Unit(0),
            },
            Action::UpgradeUnit { index: 0 },
            Action::ReleaseCommanderSkill {
                index: 2,
                id: 1_100_001,
                target: SkillTarget::Unit(0),
            },
            Action::UpgradeUnit { index: 0 },
            Action::ReleaseCommanderSkill {
                index: 0,
                id: 900_001,
                target: SkillTarget::Unit(0),
            },
        ];
        assert_eq!(normalized(&opened, &close(&opened, &taken)), taken);
    }

    #[test]
    fn purchase_levels_allow_a_later_upgrade_without_reversing_elite_recruitment() {
        let mut opened = side();
        let train = add_training(&mut opened, 2);
        let closed = close(
            &opened,
            &[
                bought(1, -100, -160),
                bought(10, 100, -160),
                Action::ActiveEnergyTowerSkill { skill: 4 },
                train,
                Action::UpgradeUnit { index: 2 },
            ],
        );
        let actions = normalized(&opened, &closed);
        let activation = actions
            .iter()
            .position(|a| matches!(a, Action::ActiveEnergyTowerSkill { skill: 4 }))
            .unwrap();
        assert_eq!(
            actions[..activation]
                .iter()
                .filter(|a| matches!(a, Action::BuyUnit { .. }))
                .count(),
            2
        );
    }

    #[test]
    fn research_follows_default_purchases_and_precedes_unified_movement() {
        let opened = side();
        let research = Action::UpgradeTechnology { unit: 6, tech: 206 };
        let purchases = [bought(6, 310, 50), bought(1, -100, -160)];
        let closed = close(
            &opened,
            &[purchases[0].clone(), research.clone(), purchases[1].clone()],
        );
        assert_eq!(
            normalized(&opened, &closed),
            [
                default_purchase(6),
                default_purchase(1),
                research,
                moved(2, 310, 50),
                moved(3, -100, -160)
            ]
        );
    }

    #[test]
    fn a_reinforced_formation_can_be_recovered_using_the_recorded_choice() {
        let economy = Economy::embedded().unwrap();
        let mut opened = side();
        opened.reinforce_choice = Some(Offers::UNANSWERED);
        opened.battle_skills = vec![slot(RECOVERY_SKILLS[0])];
        let offers = Offers {
            dealt: vec![102_115, 102_212],
            refund: 300,
        };
        let choice = Action::ChooseReinforceItem {
            index: 1,
            id: crate::r#match::ReinforceChoice::Card(102_212),
        };
        let history = [
            choice.clone(),
            Action::ReleaseCommanderSkill {
                index: 0,
                id: RECOVERY_SKILLS[0],
                target: SkillTarget::Unit(3),
            },
        ];
        let closed =
            crate::transition::deployed(economy, &opened, &history, false, Some(&offers)).unwrap();
        let actions = deployment(economy, &opened, &closed, Some(&offers), false).unwrap();
        assert_eq!(actions[0], choice);
        assert_eq!(
            crate::transition::deployed(economy, &opened, &actions, false, Some(&offers)).unwrap(),
            closed
        );
    }

    #[test]
    fn rapid_resupply_is_the_upfront_budget_stage() {
        let mut opened = side();
        opened.supply = 200;
        let actions = [
            Action::ActiveEnergyTowerSkill { skill: 1 },
            Action::UpgradeTechnology {
                unit: 2,
                tech: 10202,
            },
        ];
        assert_eq!(normalized(&opened, &close(&opened, &actions)), actions);
    }

    #[test]
    fn optional_rapid_resupply_precedes_spending_even_when_supply_is_sufficient() {
        let opened = side();
        let purchase = bought(1, -100, -160);
        let loan = Action::ActiveEnergyTowerSkill { skill: 1 };
        let closed = close(&opened, &[purchase.clone(), loan.clone()]);
        assert_eq!(
            normalized(&opened, &closed),
            [loan, default_purchase(1), moved(2, -100, -160)]
        );
    }

    #[test]
    fn rapid_resupply_follows_the_reinforcement_choice() {
        let economy = Economy::embedded().unwrap();
        let mut opened = side();
        opened.reinforce_choice = Some(Offers::UNANSWERED);
        let offers = Offers {
            dealt: vec![],
            refund: 50,
        };
        let choice = Action::ChooseReinforceItem {
            index: 0,
            id: crate::r#match::ReinforceChoice::Refund(50),
        };
        let loan = Action::ActiveEnergyTowerSkill { skill: 1 };
        let purchase = bought(1, -100, -160);
        let closed = crate::transition::deployed(
            economy,
            &opened,
            &[choice.clone(), purchase.clone(), loan.clone()],
            false,
            Some(&offers),
        )
        .unwrap();
        let actions = deployment(economy, &opened, &closed, Some(&offers), false).unwrap();
        assert_eq!(
            actions,
            [choice, loan, default_purchase(1), moved(2, -100, -160)]
        );
        assert_eq!(
            crate::transition::deployed(economy, &opened, &actions, false, Some(&offers)).unwrap(),
            closed
        );
    }

    #[test]
    fn purchases_depend_on_unlock_allowance_and_shop_level() {
        let mut opened = side();
        opened.shop.buys_remaining = 0;
        opened.unlocked_units.retain(|unit| *unit != 6);
        let closed = close(
            &opened,
            &[
                Action::UnlockUnit { unit: 6 },
                Action::ActiveEnergyTowerSkill { skill: 3 },
                Action::ActiveEnergyTowerSkill { skill: 4 },
                bought(6, 310, 50),
            ],
        );
        let actions = normalized(&opened, &closed);
        assert!(matches!(
            actions.as_slice(),
            [
                Action::UnlockUnit { unit: 6 },
                Action::ActiveEnergyTowerSkill { skill: 3 },
                Action::ActiveEnergyTowerSkill { skill: 4 },
                Action::BuyUnit {
                    unit: 6,
                    position: None,
                    ..
                },
                Action::MoveUnit {
                    index: 2,
                    position: Position { x: 310, y: 50 },
                    ..
                },
            ]
        ));
    }

    #[test]
    fn movement_precedes_devices_and_unused_unlocks_precede_combat() {
        let mut opened = side();
        opened.unlocked_units.retain(|unit| *unit != 25);
        opened.tower_strengthen_levels = vec![0, 0];
        opened.battle_skills.push(slot(300_001));
        let closed = close(
            &opened,
            &[
                Action::UnlockUnit { unit: 25 },
                Action::ActiveEnergyTowerSkill { skill: 5 },
                Action::ActiveBlueprint { id: 4 },
                Action::StrengthenTower { tower: 0 },
                Action::ReleaseContraption {
                    contraption: 10_001,
                    position: Position { x: -100, y: -100 },
                    extra_position: None,
                },
                moved(0, 100, -160),
                Action::ReleaseCommanderSkill {
                    index: 0,
                    id: 300_001,
                    target: SkillTarget::Area(vec![Position { x: 0, y: -60 }]),
                },
            ],
        );
        let actions = normalized(&opened, &closed);
        assert!(matches!(
            actions.as_slice(),
            [
                Action::MoveUnit { index: 0, .. },
                Action::ReleaseContraption { .. },
                Action::ActiveEnergyTowerSkill { skill: 5 },
                Action::ActiveBlueprint { id: 4 },
                Action::StrengthenTower { tower: 0 },
                Action::UnlockUnit { unit: 25 },
                Action::ReleaseCommanderSkill { .. },
            ]
        ));
    }

    #[test]
    fn a_recovery_blueprint_preserves_earlier_panel_allocation() {
        let opened = side();
        let closed = close(
            &opened,
            &[
                Action::ActiveBlueprint { id: 1 },
                Action::ActiveBlueprint { id: 2 },
                Action::ReleaseCommanderSkill {
                    index: 1,
                    id: 900_001,
                    target: SkillTarget::Unit(0),
                },
            ],
        );
        assert!(matches!(
            normalized(&opened, &closed).as_slice(),
            [
                Action::ActiveBlueprint { id: 1 },
                Action::ActiveBlueprint { id: 2 },
                Action::ReleaseCommanderSkill { index: 1, .. },
            ]
        ));
    }
}
