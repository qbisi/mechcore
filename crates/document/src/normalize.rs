//! Reconstructs deployment decisions from their two endpoint positions.
//!
//! The sequence is a normal form of the deployment. History supplies only
//! upgrades preceding Enhancement Modules, whose fitting time changes prices.
//! Every emitted decision is stepped and compared with the requested endpoint.

use std::collections::{BTreeMap, BTreeSet};

use crate::catalog::{NativeFormation, resolve_contraption_type, unit_id_from_type};
use crate::economy::Economy;
use crate::layout::{Experience, Region};
use crate::ledger::{Purse, RECOVERY_SKILLS};
use crate::r#match::{Action, Offers, SideState, SkillTarget, StateUnit};

const ENHANCEMENT_MODULE: i32 = 13_030_004;

/// The level reached before each newly fitted Enhancement Module. Fitting
/// order distinguishes multiple modules on the same formation.
#[derive(Clone, Default)]
pub(crate) struct UpgradeDependencies(BTreeMap<(i32, usize), i32>);

impl UpgradeDependencies {
    /// Observe an action after it has been stepped in its historical order.
    pub(crate) fn record(&mut self, position: &SideState, action: &Action) {
        if let Action::UseEquipment {
            equipment: ENHANCEMENT_MODULE,
            index,
        } = action
            && let Some(unit) = position.units.iter().find(|unit| unit.unit.index == *index)
        {
            self.0.insert(
                (*index, unit.unit.equipment.len() - 1),
                unit.unit.level.unwrap_or(1),
            );
        }
    }
}

/// Formats a match, rebuilding each complete action segment from its endpoints.
/// Historical decisions supply the closing position and the upgrades that
/// precede fitting each Enhancement Module, rather than a compressed trace.
///
/// # Errors
/// Returns a malformed match, a historical decision that cannot be evaluated,
/// or a deployment whose endpoints have no standard representation.
pub fn match_yaml(bytes: &[u8]) -> Result<String, String> {
    let stated = crate::opening::stated(bytes)?.ok_or("not a match document")?;
    let mut game = crate::r#match::read(bytes)?.ok_or("not a match document")?;
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
            let declined = turn
                .state
                .reinforce_offers
                .as_ref()
                .map(|offers| offers.refund);
            let mut closed = opened.clone();
            let mut placement = crate::landing::placement(red);
            let mut dependencies = UpgradeDependencies::default();
            for action in actions.iter() {
                closed = crate::transition::step_placing(
                    crate::economy::Economy::embedded()?,
                    &closed,
                    action,
                    declined,
                    &mut placement,
                )
                .map_err(|error| {
                    format!(
                        "round {} {}: {action:?}: {error}",
                        turn.round,
                        if red { "red" } else { "blue" }
                    )
                })?;
                dependencies.record(&closed, action);
            }
            let conceded = actions.last() == Some(&Action::Concede);
            *actions = deployment(
                crate::economy::Economy::embedded()?,
                opened,
                &closed,
                turn.state.reinforce_offers.as_ref(),
                red,
                &dependencies,
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
    let mut values = crate::r#match::segment_values(&game)?;
    if !stated.ends_on_actions
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

/// Reconstructs the standard deployment sequence from its endpoints and the
/// historical upgrades that must precede fitting an Enhancement Module.
///
/// Choices that leave the same position use offer order, formation indices,
/// technology IDs and panel indices as tie breakers. A purchase names its
/// purchased level explicitly. Recovery may use an available Enhancement
/// Module; its economic result decides whether that optimization happened.
///
/// # Errors
/// Returns the first unresolved endpoint difference or an unrepresentable
/// decision dependency. No partially reconstructed sequence is returned.
pub(crate) fn deployment(
    economy: &Economy,
    opened: &SideState,
    ended: &SideState,
    offers: Option<&Offers>,
    red: bool,
    dependencies: &UpgradeDependencies,
) -> Result<Vec<Action>, String> {
    let choices: Vec<_> = offers.map_or_else(
        || vec![None],
        |offers| {
            offers
                .dealt
                .iter()
                .copied()
                .map(Some)
                .chain([None])
                .enumerate()
                .map(|(index, id)| {
                    Some(Action::ChooseReinforceItem {
                        index: i32::try_from(index).unwrap_or(i32::MAX),
                        id,
                    })
                })
                .collect()
        },
    );
    let mut errors = Vec::new();
    for choice in choices {
        let mut builder = Builder {
            economy,
            target: ended,
            red,
            declined: offers.map(|offers| offers.refund),
            position: opened.clone(),
            actions: Vec::new(),
            purchase_levels: BTreeMap::new(),
            upgrade_dependencies: dependencies.clone(),
            research: ended
                .techs
                .iter()
                .filter(|id| !opened.techs.contains(id))
                .copied()
                .collect(),
        };
        if let Some(choice) = choice
            && let Err(error) = builder.push(choice)
        {
            errors.push(error);
            continue;
        }
        if builder.position.officers != ended.officers
            || builder.position.battle_skills.iter().any(|slot| {
                !ended
                    .battle_skills
                    .iter()
                    .any(|wanted| wanted.index == slot.index && wanted.id == slot.id)
            })
        {
            continue;
        }
        for levels in builder.purchase_plans()? {
            let mut candidate = builder.clone();
            candidate.purchase_levels = levels;
            match candidate.finish() {
                Ok(actions) => return Ok(actions),
                Err(error) => errors.push(error),
            }
        }
    }
    errors.sort();
    errors.dedup();
    Err(format!(
        "deployment has no standard action sequence: {}",
        errors.join("; ")
    ))
}

#[derive(Clone)]
struct Builder<'a> {
    economy: &'a Economy,
    target: &'a SideState,
    red: bool,
    declined: Option<i32>,
    position: SideState,
    actions: Vec<Action>,
    research: BTreeSet<i32>,
    purchase_levels: BTreeMap<i32, i32>,
    upgrade_dependencies: UpgradeDependencies,
}

impl Builder<'_> {
    fn push(&mut self, action: Action) -> Result<(), String> {
        self.dependencies(&action)?;
        crate::transition::check_operation(self.economy, &self.position, &action)
            .map_err(|error| format!("{action:?}: {error}"))?;
        crate::transition::check_place(&self.position, &action)
            .map_err(|error| format!("{action:?}: {error}"))?;
        let step = |position: &SideState| {
            crate::transition::step_placing(
                self.economy,
                position,
                &action,
                self.declined,
                &mut crate::landing::placement(self.red),
            )
        };
        let mut next = step(&self.position);
        if next
            == Err(crate::transition::Unsettled::Refused(
                "spending more supply than the side holds",
            ))
            && self.can_activate(1)
        {
            self.push(Action::ActiveEnergyTowerSkill { skill: 1 })?;
            next = crate::transition::step_placing(
                self.economy,
                &self.position,
                &action,
                self.declined,
                &mut crate::landing::placement(self.red),
            );
        }
        self.position = next.map_err(|error| format!("{action:?}: {error}"))?;
        self.actions.push(action);
        Ok(())
    }

    fn can_activate(&self, skill: i32) -> bool {
        self.target.energy_tower_skills.contains(&skill)
            && !self.position.energy_tower_skills.contains(&skill)
    }

    fn purse(&self) -> Purse<'_> {
        let mut purse = Purse::new(self.economy, &self.position.officers);
        purse.raised = self
            .position
            .energy_tower_skills
            .iter()
            .filter_map(|id| self.economy.energy_tower_skill(*id))
            .map(|row| row.shop_unit_level)
            .sum();
        purse
    }

    fn dependencies(&mut self, action: &Action) -> Result<(), String> {
        match action {
            Action::UseEquipment {
                equipment: ENHANCEMENT_MODULE,
                index,
            } => {
                let held = self.unit(*index)?;
                if let Some(level) = self
                    .upgrade_dependencies
                    .0
                    .get(&(*index, held.unit.equipment.len()))
                    .copied()
                {
                    while self.unit(*index)?.unit.level.unwrap_or(1) < level {
                        self.upgrade(*index)?;
                    }
                }
            }
            Action::BuyUnit { unit, level, .. } => {
                if !self.position.unlocked_units.contains(unit) {
                    self.push(Action::UnlockUnit { unit: *unit })?;
                }
                if self.position.shop.buys_remaining == 0 && self.can_activate(3) {
                    self.push(Action::ActiveEnergyTowerSkill { skill: 3 })?;
                }
                if level.is_some_and(|level| level > self.purse().shop_level(*unit))
                    && self.can_activate(4)
                {
                    self.push(Action::ActiveEnergyTowerSkill { skill: 4 })?;
                }
            }
            Action::ReleaseCommanderSkill { index, id, target } => {
                self.panel(*index, *id)?;
                if RECOVERY_SKILLS.contains(id)
                    && let SkillTarget::Unit(index) = target
                {
                    let name = self.unit(*index)?.unit.type_name.clone();
                    if self
                        .position
                        .units
                        .iter()
                        .filter(|entry| entry.unit.type_name == name)
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

    fn purchase_plans(&self) -> Result<Vec<BTreeMap<i32, i32>>, String> {
        let raised: i32 = self
            .target
            .energy_tower_skills
            .iter()
            .filter_map(|id| self.economy.energy_tower_skill(*id))
            .map(|row| row.shop_unit_level)
            .sum();
        let mut plans = vec![(BTreeMap::new(), self.purse().raised)];
        for index in self.position.next_index.unit..self.target.next_index.unit {
            let wanted = self
                .target
                .units
                .iter()
                .find(|entry| entry.unit.index == index);
            let name = wanted.map_or("crawler", |entry| entry.unit.type_name.as_str());
            let unit = unit_id_from_type(name).ok_or("unknown purchased unit")?;
            let base = Purse::new(self.economy, &self.position.officers).shop_level(unit);
            let maximum = wanted.map_or(base + raised, |entry| entry.unit.level.unwrap_or(1));
            let mut next = Vec::new();
            for (plan, previous) in plans {
                for level in (base + previous..=(base + raised).min(maximum)).rev() {
                    let mut plan = plan.clone();
                    plan.insert(index, level);
                    next.push((plan, level - base));
                }
            }
            plans = next;
        }
        Ok(plans.into_iter().map(|(plan, _)| plan).collect())
    }

    fn finish(self) -> Result<Vec<Action>, String> {
        let recoveries = self.recoveries();
        let mut candidates = vec![self];
        for recovery in recoveries {
            let mut next = Vec::new();
            for candidate in candidates {
                let maximum = if let Action::ReleaseCommanderSkill {
                    target: SkillTarget::Unit(index),
                    ..
                } = recovery
                {
                    candidate.unit(index).map_or(0, |entry| {
                        crate::experience::MAX_LEVEL - entry.unit.level.unwrap_or(1)
                    })
                } else {
                    0
                };
                for upgrades in 0..=maximum {
                    for module in [true, false] {
                        if upgrades == 0 && module {
                            continue;
                        }
                        let mut recovered = candidate.clone();
                        if recovered.recover(&recovery, module, upgrades).is_ok() {
                            next.push(recovered);
                        }
                    }
                }
            }
            if next.is_empty() {
                return Err(format!("cannot reconstruct recovery {recovery:?}"));
            }
            candidates = next;
        }
        let mut errors = Vec::new();
        for mut candidate in candidates {
            let completed = (|| {
                for tech in candidate.research.clone() {
                    candidate.research(tech)?;
                }
                candidate.moves()?;
                while candidate.position.next_index.unit < candidate.target.next_index.unit {
                    candidate.buy_next(true)?;
                }
                candidate.remaining()
            })();
            match completed {
                Ok(actions) => return Ok(actions),
                Err(error) => errors.push(error),
            }
        }
        Err(errors.join("; "))
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

    fn recover(&mut self, recovery: &Action, module: bool, upgrades: i32) -> Result<(), String> {
        if let Action::ReleaseCommanderSkill {
            target: SkillTarget::Unit(index),
            ..
        } = recovery
        {
            let removed = self.unit(*index)?.clone();
            let lost_redeploy = self.target.battle_skills.iter().find(|slot| {
                crate::mobility::REDEPLOY_SKILLS.contains(&slot.id)
                    && slot.used
                    && self
                        .position
                        .battle_skills
                        .iter()
                        .any(|held| held.index == slot.index && !held.used)
            });
            if let Some(slot) = lost_redeploy
                && !self.target.units.iter().any(|entry| entry.redeployed)
            {
                self.push(Action::ReleaseCommanderSkill {
                    index: slot.index,
                    id: slot.id,
                    target: SkillTarget::Unit(*index),
                })?;
            }
            let owner =
                unit_id_from_type(&removed.unit.type_name).ok_or("unknown recovered unit")?;
            if module {
                if removed.unit.level.unwrap_or(1) >= crate::experience::MAX_LEVEL
                    || !self
                        .economy
                        .unit(owner)
                        .is_some_and(|row| row.wears_equipment)
                {
                    return Err("recovered unit cannot use a module upgrade".into());
                }
                let slots = self.economy.equipment_slots(&self.position.officers);
                let mut fitted = 0;
                while self.unit(*index)?.unit.equipment.len() < slots
                    && self
                        .position
                        .equipment
                        .iter()
                        .any(|item| item.id == ENHANCEMENT_MODULE)
                {
                    self.push(Action::UseEquipment {
                        equipment: ENHANCEMENT_MODULE,
                        index: *index,
                    })?;
                    fitted += 1;
                }
                if fitted == 0 && !removed.unit.equipment.contains(&ENHANCEMENT_MODULE) {
                    return Err("no free Enhancement Module".into());
                }
            }
            for _ in 0..upgrades {
                if self
                    .unit(*index)?
                    .unit
                    .exp
                    .is_some_and(|exp| !exp.is_full())
                    || self.spare_training()
                        && !self.unit(*index)?.unit.exp.is_some_and(Experience::is_full)
                {
                    self.train(*index)?;
                }
                self.push(Action::UpgradeUnit { index: *index })?;
            }
            if self.spare_training()
                && !self.unit(*index)?.unit.exp.is_some_and(Experience::is_full)
            {
                self.train(*index)?;
            }
        } else if module {
            return Err("only a formation can use an Enhancement Module".into());
        }
        self.push(recovery.clone())
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
        while self.research.contains(&tech)
            && !self
                .position
                .units
                .iter()
                .any(|entry| entry.unit.type_name == name)
        {
            if self.position.next_index.unit >= self.target.next_index.unit {
                return Err(format!(
                    "technology {tech} has no formation or remaining purchase"
                ));
            }
            self.buy_next(false)?;
        }
        if self.research.contains(&tech) {
            self.push(Action::UpgradeTechnology { unit, tech })?;
            self.research.remove(&tech);
        }
        Ok(())
    }

    fn unit(&self, index: i32) -> Result<&StateUnit, String> {
        self.position
            .units
            .iter()
            .find(|entry| entry.unit.index == index)
            .ok_or_else(|| format!("formation {index} is absent"))
    }

    fn wanted(&self, index: i32) -> Result<&StateUnit, String> {
        self.target
            .units
            .iter()
            .find(|entry| entry.unit.index == index)
            .ok_or_else(|| format!("target formation {index} is absent"))
    }

    fn free_to_move(&mut self, index: i32) -> Result<(), String> {
        let wanted = self.wanted(index)?.clone();
        if wanted.redeployed && !self.unit(index)?.redeployed {
            let skill = self
                .target
                .battle_skills
                .iter()
                .find(|slot| {
                    crate::mobility::REDEPLOY_SKILLS.contains(&slot.id)
                        && slot.used
                        && self
                            .position
                            .battle_skills
                            .iter()
                            .any(|held| held.index == slot.index && !held.used)
                })
                .ok_or("no unused Redeployment slot for moved formation")?;
            self.push(Action::ReleaseCommanderSkill {
                index: skill.index,
                id: skill.id,
                target: SkillTarget::Unit(index),
            })?;
        }
        if self.unit(index)?.movable(&self.position.techs) {
            return Ok(());
        }
        if !self.unit(index)?.movable(&self.position.techs)
            && wanted
                .unit
                .equipment
                .contains(&crate::mobility::DEPLOYMENT_MODULE)
        {
            // Preserve fitting order when the module follows another item.
            for equipment in wanted.unit.equipment {
                if self.unit(index)?.unit.equipment.contains(&equipment) {
                    continue;
                }
                self.push(Action::UseEquipment { equipment, index })?;
                if equipment == crate::mobility::DEPLOYMENT_MODULE {
                    break;
                }
            }
        }
        if !self.unit(index)?.movable(&self.position.techs) {
            return Err(format!(
                "formation {index} cannot move to its target position"
            ));
        }
        Ok(())
    }

    fn moves(&mut self) -> Result<(), String> {
        let mut pending = Vec::new();
        for held in self.position.units.clone() {
            let wanted = self.wanted(held.unit.index)?.clone();
            let moved = held.unit.position != wanted.unit.position
                || held.unit.rotated.unwrap_or(false) != wanted.unit.rotated.unwrap_or(false)
                || held.unit.travelling.unwrap_or(false) != wanted.unit.travelling.unwrap_or(false);
            if moved || wanted.redeployed {
                self.free_to_move(held.unit.index)?;
            }
            if !moved {
                continue;
            }
            if Region::of(held.unit.position) == Region::of(wanted.unit.position)
                && Region::of(wanted.unit.position).is_flank()
                && wanted.unit.travelling == Some(true)
                && held.unit.travelling != Some(true)
            {
                let aside = crate::landing::free_spot(
                    &self.position,
                    &held.unit.type_name,
                    held.unit.rotated.unwrap_or(false),
                    Region::Main,
                    self.red,
                    Some(held.unit.index),
                    &[],
                )
                .ok_or("no main-region waypoint to reproduce travelling")?;
                pending.push(Action::MoveUnit {
                    index: held.unit.index,
                    position: aside,
                    rotated: held.unit.rotated.unwrap_or(false),
                });
            }
            pending.push(Action::MoveUnit {
                index: held.unit.index,
                position: wanted.unit.position,
                rotated: wanted.unit.rotated.unwrap_or(false),
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

    fn buy_next(&mut self, move_now: bool) -> Result<(), String> {
        let index = self.position.next_index.unit;
        let wanted = self
            .target
            .units
            .iter()
            .find(|entry| entry.unit.index == index)
            .cloned();
        let name = wanted
            .as_ref()
            .map_or("crawler", |entry| entry.unit.type_name.as_str());
        let unit = unit_id_from_type(name).ok_or("unknown purchased unit")?;
        let level = *self
            .purchase_levels
            .get(&index)
            .ok_or("purchase allocator exceeds its plan")?;
        self.push(Action::BuyUnit {
            unit,
            level: Some(level),
            position: None,
            rotated: false,
        })?;
        if let Some(wanted) = wanted {
            if move_now
                && (self.unit(index)?.unit.position != wanted.unit.position
                    || wanted.unit.rotated.unwrap_or(false))
            {
                self.push(Action::MoveUnit {
                    index,
                    position: wanted.unit.position,
                    rotated: wanted.unit.rotated.unwrap_or(false),
                })?;
            }
        } else {
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
                .cloned()
                .ok_or_else(|| format!("allocator gap {index} has no Field Recovery release"))?;
            self.push(Action::ReleaseCommanderSkill {
                index: recovery.index,
                id: recovery.id,
                target: SkillTarget::Unit(index),
            })?;
        }
        Ok(())
    }

    fn remaining(mut self) -> Result<Vec<Action>, String> {
        for wanted in &self.target.units {
            let index = wanted.unit.index;
            let held = self.unit(index)?.unit.equipment.clone();
            if !wanted.unit.equipment.starts_with(&held) {
                return Err(format!("formation {index} changes equipment fitting order"));
            }
            for equipment in &wanted.unit.equipment[held.len()..] {
                self.push(Action::UseEquipment {
                    equipment: *equipment,
                    index,
                })?;
            }
        }
        for wanted in &self.target.units {
            let index = wanted.unit.index;
            while self.unit(index)?.unit.level.unwrap_or(1) < wanted.unit.level.unwrap_or(1) {
                self.upgrade(index)?;
            }
            if self.unit(index)?.unit.exp != wanted.unit.exp
                && wanted.unit.exp.is_some_and(Experience::is_full)
            {
                self.train(index)?;
            }
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
        let mut battle: Vec<_> = self
            .target
            .battle_skills
            .iter()
            .filter(|slot| !RECOVERY_SKILLS.contains(&slot.id) && slot.release.is_some())
            .collect();
        battle.sort_by_key(|slot| {
            (
                slot.release.as_ref().map(|release| release.order),
                slot.index,
            )
        });
        for slot in battle {
            self.push(Action::ReleaseCommanderSkill {
                index: slot.index,
                id: slot.id,
                target: slot.release.as_ref().unwrap().target.clone(),
            })?;
        }
        for unit in self.target.unlocked_units.clone() {
            if !self.position.unlocked_units.contains(&unit) {
                self.push(Action::UnlockUnit { unit })?;
            }
        }
        let actual = comparable(&self.position);
        let expected = comparable(self.target);
        if actual != expected {
            let actual = serde_json::to_value(actual).map_err(|error| error.to_string())?;
            let expected = serde_json::to_value(expected).map_err(|error| error.to_string())?;
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
        if self.unit(index)?.unit.exp.is_some_and(|exp| !exp.is_full())
            || self.spare_training() && !self.unit(index)?.unit.exp.is_some_and(Experience::is_full)
        {
            self.train(index)?;
        }
        self.push(Action::UpgradeUnit { index })
    }

    fn train(&mut self, index: i32) -> Result<(), String> {
        let slot = self
            .target
            .battle_skills
            .iter()
            .find(|slot| {
                crate::transition::TRAINING_SKILLS.contains(&slot.id)
                    && slot.used
                    && self
                        .position
                        .battle_skills
                        .iter()
                        .any(|held| held.index == slot.index && !held.used)
            })
            .ok_or_else(|| format!("no Intensive Training slot for formation {index}"))?;
        self.push(Action::ReleaseCommanderSkill {
            index: slot.index,
            id: slot.id,
            target: SkillTarget::Unit(index),
        })
    }

    fn spare_training(&self) -> bool {
        let available = self
            .target
            .battle_skills
            .iter()
            .filter(|slot| {
                crate::transition::TRAINING_SKILLS.contains(&slot.id)
                    && slot.used
                    && self
                        .position
                        .battle_skills
                        .iter()
                        .any(|held| held.index == slot.index && !held.used)
            })
            .count();
        let needed: usize = self
            .target
            .units
            .iter()
            .map(|wanted| {
                let Some(entry) = self
                    .position
                    .units
                    .iter()
                    .find(|entry| entry.unit.index == wanted.unit.index)
                else {
                    return usize::from(wanted.unit.exp.is_some_and(Experience::is_full));
                };
                let upgraded = wanted.unit.level.unwrap_or(1) > entry.unit.level.unwrap_or(1);
                usize::from(upgraded && entry.unit.exp.is_some_and(|exp| !exp.is_full()))
                    + usize::from(
                        wanted.unit.exp.is_some_and(Experience::is_full)
                            && (upgraded || entry.unit.exp != wanted.unit.exp),
                    )
            })
            .sum();
        available > needed
    }
}

fn comparable(state: &SideState) -> SideState {
    let mut state = state.clone();
    let mut order: Vec<_> = state
        .battle_skills
        .iter()
        .filter(|slot| slot.release.is_some())
        .map(|slot| {
            let release = slot.release.as_ref().unwrap();
            (
                RECOVERY_SKILLS.contains(&slot.id),
                release.order,
                slot.index,
            )
        })
        .collect();
    order.sort_by_key(|(recovery, old, index)| (!*recovery, if *recovery { *index } else { *old }));
    for (rank, (_, _, index)) in order.into_iter().enumerate() {
        state
            .battle_skills
            .iter_mut()
            .find(|slot| slot.index == index)
            .unwrap()
            .release
            .as_mut()
            .unwrap()
            .order = i32::try_from(rank).unwrap();
    }
    state
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
                .find(|entry| entry.unit.index == *index)
                .and_then(|entry| crate::landing::unit_rect(&entry.unit.type_name, *to, *rotated)),
            _ => None,
        })
        .collect();
    pending.iter().enumerate().find_map(|(at, action)| {
        let Action::MoveUnit { index, .. } = action else { return None; };
        if stepped_aside.contains(index) || pending[..at].iter().any(|earlier| matches!(earlier, Action::MoveUnit { index: other, .. } if other == index)) { return None; }
        let standing = position.units.iter().find(|entry| entry.unit.index == *index)?;
        let rotated = standing.unit.rotated.unwrap_or(false);
        crate::landing::free_spot(position, &standing.unit.type_name, rotated, Region::of(standing.unit.position), red, Some(*index), &needed)
            .map(|position| Action::MoveUnit { index: *index, position, rotated })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{Position, UnitPlacement, UnitSource};
    use crate::r#match::{Allowances, EquipmentItem, NextIndex, PanelSkill};

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
                    unit: UnitPlacement {
                        record_position: None,
                        record_rotated: None,
                        type_name: "marksman".into(),
                        index,
                        position: Position {
                            x: index * 20,
                            y: -160,
                        },
                        level: None,
                        exp: None,
                        rotated: None,
                        equipment: Vec::new(),
                        travelling: None,
                        source: UnitSource::Joined,
                    },
                    value: Some(100),
                    redeployed: false,
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
            used: false,
            standing: Vec::new(),
            release: None,
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
            level: None,
            position: Some(Position { x, y }),
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
        normalized_with_dependencies(opened, closed, &UpgradeDependencies::default())
    }

    fn recorded_dependencies(opened: &SideState, history: &[Action]) -> UpgradeDependencies {
        let mut dependencies = UpgradeDependencies::default();
        let mut position = opened.clone();
        for action in history {
            position = close(&position, std::slice::from_ref(action));
            dependencies.record(&position, action);
        }
        dependencies
    }

    fn normalized_with_dependencies(
        opened: &SideState,
        closed: &SideState,
        dependencies: &UpgradeDependencies,
    ) -> Vec<Action> {
        let economy = Economy::embedded().unwrap();
        let actions = deployment(economy, opened, closed, None, false, dependencies).unwrap();
        let reproduced =
            crate::transition::deployed(economy, opened, &actions, false, None).unwrap();
        assert_eq!(comparable(&reproduced), comparable(closed));
        assert_eq!(
            deployment(
                economy,
                opened,
                &reproduced,
                None,
                false,
                &recorded_dependencies(opened, &actions)
            )
            .unwrap(),
            actions
        );
        actions
    }

    #[test]
    fn enhancement_module_preserves_upgrades_before_and_after_fitting() {
        let mut opened = side();
        opened.units[0].unit.type_name = "melting_point".into();
        opened.units[0].unit.level = Some(4);
        opened.equipment.push(EquipmentItem {
            id: ENHANCEMENT_MODULE,
            durability: None,
        });
        let upgrade = Action::UpgradeUnit { index: 0 };
        let module = Action::UseEquipment {
            equipment: ENHANCEMENT_MODULE,
            index: 0,
        };
        for (history, cost) in [
            (vec![module.clone(), upgrade.clone(), upgrade.clone()], 200),
            (vec![upgrade.clone(), module.clone(), upgrade.clone()], 300),
            (vec![upgrade.clone(), upgrade.clone(), module.clone()], 400),
        ] {
            let closed = close(&opened, &history);
            assert_eq!(opened.supply - closed.supply, cost);
            assert_eq!(
                normalized_with_dependencies(
                    &opened,
                    &closed,
                    &recorded_dependencies(&opened, &history)
                ),
                history
            );
        }
    }

    #[test]
    fn successive_modules_keep_distinct_upgrade_dependencies() {
        let mut opened = side();
        opened.units[0].unit.type_name = "melting_point".into();
        opened.officers.push(10540);
        opened.equipment = vec![
            EquipmentItem {
                id: ENHANCEMENT_MODULE,
                durability: None
            };
            2
        ];
        let upgrade = Action::UpgradeUnit { index: 0 };
        let module = Action::UseEquipment {
            equipment: ENHANCEMENT_MODULE,
            index: 0,
        };
        let history = [
            upgrade.clone(),
            module.clone(),
            upgrade.clone(),
            module,
            upgrade,
        ];
        let closed = close(&opened, &history);
        assert_eq!(opened.supply - closed.supply, 300);
        assert_eq!(
            normalized_with_dependencies(
                &opened,
                &closed,
                &recorded_dependencies(&opened, &history)
            ),
            history
        );
    }

    #[test]
    fn a_vacated_waypoint_does_not_block_a_later_purchase() {
        let mut opened = side();
        opened.units[0].unit.position = Position { x: -350, y: 285 };
        let closed = close(
            &opened,
            &[moved(0, 350, 55), moved(0, 90, -35), bought(15, 350, 55)],
        );
        let actions = normalized(&opened, &closed);
        assert_eq!(actions[0], moved(0, 90, -35));
        assert_eq!(actions.len(), 3);
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
    fn jump_drive_precedes_the_existing_formations_move() {
        let mut opened = side();
        opened.units[0].unit.type_name = "wasp".into();
        opened.units[0].unit.source = UnitSource::Legacy;
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
    fn recovery_module_optimization_is_decided_by_the_recorded_economy() {
        let mut opened = side();
        opened.battle_skills.push(slot(900_001));
        opened.equipment.push(EquipmentItem {
            id: ENHANCEMENT_MODULE,
            durability: None,
        });
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
                Action::BuyUnit { level, .. } => Some((at, *level)),
                _ => None,
            })
            .collect();
        let activation = actions.iter().position(|action| action == &elite).unwrap();
        assert_eq!(purchases[0].1, Some(1));
        assert_eq!(purchases[1].1, Some(2));
        assert!(purchases[0].0 < activation && activation < purchases[1].0);
    }

    #[test]
    fn an_allocator_gap_is_filled_by_buying_and_recovering_a_crawler() {
        let mut opened = side();
        opened.battle_skills.push(slot(900_001));
        let closed = close(
            &opened,
            &[
                bought(15, -100, -160),
                Action::ReleaseCommanderSkill {
                    index: 0,
                    id: 900_001,
                    target: SkillTarget::Unit(2),
                },
            ],
        );
        let actions = normalized(&opened, &closed);
        assert!(matches!(actions[0], Action::BuyUnit { unit: 10, .. }));
        assert_eq!(actions.len(), 2);
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
    fn recovery_restores_continuous_training_and_existing_module_refunds() {
        let mut opened = side();
        opened.units[0].unit.exp = Experience::of(1, "marksman", 1).unwrap();
        opened.units[0].unit.equipment.push(ENHANCEMENT_MODULE);
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
        let opened = side();
        let closed = close(
            &opened,
            &[
                bought(1, -100, -160),
                bought(10, 100, -160),
                Action::ActiveEnergyTowerSkill { skill: 4 },
                Action::UpgradeUnit { index: 2 },
            ],
        );
        let actions = normalized(&opened, &closed);
        let levels: Vec<_> = actions
            .iter()
            .filter_map(|action| match action {
                Action::BuyUnit { level, .. } => *level,
                _ => None,
            })
            .collect();
        assert_eq!(levels, [1, 1]);
    }

    #[test]
    fn research_purchases_at_default_landing_and_moves_in_the_existing_unit_phase() {
        let opened = side();
        let research = Action::UpgradeTechnology { unit: 6, tech: 206 };
        let closed = close(
            &opened,
            &[bought(6, 310, 50), research.clone(), bought(1, -100, -160)],
        );
        let actions = normalized(&opened, &closed);
        assert!(matches!(
            actions[0],
            Action::BuyUnit {
                unit: 6,
                position: None,
                ..
            }
        ));
        assert_eq!(actions[1], research);
        assert_eq!(actions[2], moved(2, 310, 50));
        assert!(matches!(
            actions[3],
            Action::BuyUnit {
                unit: 1,
                position: None,
                ..
            }
        ));
    }

    #[test]
    fn rapid_resupply_is_a_dependency_of_the_first_unaffordable_operation() {
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
                    level: Some(2),
                    position: None,
                    ..
                },
                Action::MoveUnit { index: 2, .. },
            ]
        ));
    }

    #[test]
    fn remaining_enhancements_follow_devices_and_unused_unlocks_follow_combat() {
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
                Action::ReleaseContraption { .. },
                Action::ActiveEnergyTowerSkill { skill: 5 },
                Action::ActiveBlueprint { id: 4 },
                Action::StrengthenTower { tower: 0 },
                Action::ReleaseCommanderSkill { .. },
                Action::UnlockUnit { unit: 25 },
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
