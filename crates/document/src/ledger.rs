//! Checks that a battle's supply adds up, round by round.
//!
//! A state's `supply` is what the side can spend, so two consecutive states
//! and the decisions between them have to satisfy one identity:
//!
//! ```text
//! supply(round + 1) = supply(round) - spent(round) + income(round + 1)
//! ```
//!
//! Income is the map's own row plus what the side's officers add, less what an
//! energy tower Rapid Supply owes from the round before. Spending prices the
//! turn's actions through [`crate::economy`].
//!
//! Not every round can be checked. A side holding Field Recovery, or an officer
//! that pays a bounty for destroying a giant, is paid by the fight in an amount
//! no document records, so its rounds are counted apart rather than failed.

use crate::battle::{Action, Battle, SideState, Turn};
use crate::catalog::unit_id_from_type;
use crate::economy::{Economy, MapSupply, Officer};
use std::collections::BTreeMap;

/// Commander skills that turn what the fight destroyed into supply.
const RECOVERY_SKILLS: [i32; 4] = [900_001, 900_002, 900_003, 900_004];

/// What checking one battle found.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// Round transitions whose identity holds.
    pub closed: usize,
    /// Round transitions whose identity does not.
    pub failed: usize,
    /// Round transitions where the fight pays the side an unrecorded amount.
    pub fight_pays: usize,
    /// Round transitions holding a decision this build has no price for.
    pub unpriced: usize,
    pub failures: Vec<Failure>,
}

impl Report {
    /// The transitions the ledger could actually decide.
    #[must_use]
    pub fn checked(&self) -> usize {
        self.closed + self.failed
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Failure {
    pub round: i32,
    pub side: &'static str,
    /// What the next state should hold under the identity.
    pub expected: i32,
    /// What it holds.
    pub actual: i32,
}

/// Checks every round transition of a battle.
///
/// # Errors
///
/// Returns an error when the battle names a map this build has no supply row
/// for, since the round's income cannot then be stated at all.
pub fn check(battle: &Battle, economy: &Economy) -> Result<Report, String> {
    let map = economy.map(battle.map_id).ok_or_else(|| {
        format!(
            "map {} has no supply row in this build's economy",
            battle.map_id
        )
    })?;
    let mut report = Report::default();
    for pair in battle.turns.windows(2) {
        let [turn, next] = pair else { continue };
        for (name, state, following, actions) in [
            (
                "blue",
                &turn.state.sides.blue,
                &next.state.sides.blue,
                &turn.actions.blue,
            ),
            (
                "red",
                &turn.state.sides.red,
                &next.state.sides.red,
                &turn.actions.red,
            ),
        ] {
            record(
                &mut report,
                economy,
                &Transition {
                    round: turn.round,
                    side: name,
                    state,
                    following,
                    actions,
                    next_round: next.round,
                    map,
                },
            );
        }
    }
    Ok(report)
}

struct Transition<'a> {
    round: i32,
    side: &'static str,
    state: &'a SideState,
    following: &'a SideState,
    actions: &'a [Action],
    next_round: i32,
    map: MapSupply,
}

fn record(report: &mut Report, economy: &Economy, transition: &Transition<'_>) {
    if paid_by_the_fight(economy, transition.following) {
        report.fight_pays += 1;
        return;
    }
    let mut purse = Purse::new(economy, &transition.state.techs.officers);
    let Some(spent) = spend(economy, &mut purse, transition.state, transition.actions) else {
        report.unpriced += 1;
        return;
    };
    let owed: i32 = transition
        .actions
        .iter()
        .filter_map(|action| match action {
            Action::ActiveEnergyTowerSkill { skill } => economy.energy_tower_skill(*skill),
            _ => None,
        })
        .map(|skill| skill.owed)
        .sum();
    let earned = round_income(
        economy,
        transition.next_round,
        &transition.following.techs.officers,
        transition.map,
    ) - owed;
    let expected = transition.state.supply - spent + earned;
    if expected == transition.following.supply {
        report.closed += 1;
    } else {
        report.failed += 1;
        report.failures.push(Failure {
            round: transition.round,
            side: transition.side,
            expected,
            actual: transition.following.supply,
        });
    }
}

/// Whether the fight can pay this side an amount no document records.
fn paid_by_the_fight(economy: &Economy, state: &SideState) -> bool {
    state
        .battle_skills
        .iter()
        .any(|skill| RECOVERY_SKILLS.contains(&skill.id))
        || state.techs.officers.iter().any(|officer| {
            economy
                .officer(*officer)
                .is_some_and(|row| row.kill_bounty != 0)
        })
}

/// The income a round grants, which arrives before any of its decisions.
///
/// The map's row sets the base, and the officers the side holds at the round's
/// start add to it. An officer taken later in the round cannot have raised an
/// income that was already granted.
#[must_use]
pub fn round_income(economy: &Economy, round: i32, officers: &[i32], map: MapSupply) -> i32 {
    if round < 1 {
        return 0;
    }
    let base = map
        .first_round_supply
        .saturating_add((round - 1).saturating_mul(map.round_supply_increase))
        .min(map.max_round_supply);
    let extra: i32 = officers
        .iter()
        .filter_map(|officer| economy.officer(*officer))
        .map(|row| {
            row.round_supply + if round == 1 { row.first_round_supply } else { 0 }
        })
        .sum();
    base + extra
}

/// A side's prices, as the officers it holds change them.
struct Purse<'a> {
    economy: &'a Economy,
    officers: Vec<Officer>,
}

impl<'a> Purse<'a> {
    fn new(economy: &'a Economy, officers: &[i32]) -> Self {
        Self {
            economy,
            officers: officers
                .iter()
                .filter_map(|officer| economy.officer(*officer).cloned())
                .collect(),
        }
    }

    /// An officer taken this round discounts everything bought after it.
    fn add(&mut self, card: i32) -> Option<&Officer> {
        let officer = self.economy.officer(card)?.clone();
        self.officers.push(officer);
        self.officers.last()
    }

    fn modifier(&self, field: fn(&Officer) -> i32, unit: Option<i32>) -> i32 {
        self.officers
            .iter()
            .filter(|officer| match unit {
                None => true,
                Some(unit) => officer.units.is_empty() || officer.units.contains(&unit),
            })
            .map(field)
            .sum()
    }

    fn buy(&self, unit: i32) -> Option<i32> {
        let price = self.economy.unit(unit)?.supply;
        Some((price + self.modifier(|officer| officer.unit_supply, Some(unit))).max(0))
    }

    fn unlock(&self, unit: i32) -> Option<i32> {
        let price = self.economy.unit(unit)?.unlock_supply;
        Some((price + self.modifier(|officer| officer.unlock_supply, Some(unit))).max(0))
    }

    fn upgrade(&self, unit: i32) -> Option<i32> {
        let price = self.economy.unit(unit)?.upgrade_supply;
        Some((price + self.modifier(|officer| officer.upgrade_supply, Some(unit))).max(0))
    }

    fn technology(&self, technology: i32) -> Option<i32> {
        let price = self.economy.technology(technology)?;
        Some((price + self.modifier(|officer| officer.technology_supply, None)).max(0))
    }
}

/// Prices one round's decisions, or reports that one has no price.
fn spend(
    economy: &Economy,
    purse: &mut Purse<'_>,
    state: &SideState,
    actions: &[Action],
) -> Option<i32> {
    let mut roster: BTreeMap<i32, i32> = state
        .formations
        .iter()
        .filter_map(|formation| {
            unit_id_from_type(&formation.type_name).map(|unit| (formation.index, unit))
        })
        .collect();
    let mut next_index = state.next_index.unit;
    let mut towers: Vec<i32> = state.tower_strengthen_levels.clone();
    let mut total = 0;
    for action in actions {
        total += match action {
            Action::BuyUnit { unit, .. } => {
                roster.insert(next_index, *unit);
                next_index += 1;
                purse.buy(*unit)?
            }
            Action::UnlockUnit { unit } => purse.unlock(*unit)?,
            Action::UpgradeUnit { index } => purse.upgrade(*roster.get(index)?)?,
            Action::UpgradeTechnology { tech, .. } => purse.technology(*tech)?,
            Action::ActiveBlueprint { id } => economy.blueprint(*id)?,
            Action::ActiveEnergyTowerSkill { skill } => {
                let skill = economy.energy_tower_skill(*skill)?;
                skill.supply - skill.granted
            }
            Action::StrengthenTower { tower } => {
                let level = towers.get_mut(usize::try_from(*tower).ok()?)?;
                *level += 1;
                economy.tower_strengthen(*level)?
            }
            Action::ChooseReinforceItem { id, .. } => {
                let price = economy.card(*id)?;
                let granted = purse.add(*id).map_or(0, |officer| officer.granted_supply);
                price - granted
            }
            Action::ChooseAdvanceTeam { id, .. } => economy.card(*id)?,
            Action::DeclineReinforceItem
            | Action::MoveUnit { .. }
            | Action::ReleaseCommanderSkill { .. }
            | Action::ReleaseContraption { .. }
            | Action::UseEquipment { .. } => 0,
        };
    }
    Some(total)
}

/// The turns a report was built from, for a caller that wants to name them.
#[must_use]
pub fn transitions(turns: &[Turn]) -> usize {
    turns.len().saturating_sub(1) * 2
}

#[cfg(all(test, feature = "convert"))]
mod tests {
    use super::{check, transitions};
    use crate::convert::battle_from_grbr;
    use crate::economy::Economy;

    const TUFF: &str = "../../tests/grbr/2259_20260901--201562374_[crower]VS[[TUFF]MARLFAUX].grbr";
    const CAINE: &str = "../../tests/grbr/2259_20260901--201562557_[crower]VS[[BORK]  Caine].grbr";

    fn report_for(path: &str) -> super::Report {
        let battle = battle_from_grbr(&std::fs::read(path).unwrap()).unwrap();
        let economy = Economy::embedded().unwrap();
        let report = check(&battle, &economy).unwrap();
        assert_eq!(
            report.closed + report.failed + report.fight_pays + report.unpriced,
            transitions(&battle.turns),
            "every transition is accounted for exactly once"
        );
        report
    }

    #[test]
    fn every_checkable_round_of_the_tracked_matches_closes() {
        // Field Recovery pays both sides of these matches partway through, so
        // most transitions are set aside rather than decided.
        let report = report_for(TUFF);
        assert_eq!((report.closed, report.failed), (4, 0));
        assert_eq!(report.fight_pays, 12);
        assert_eq!(report.unpriced, 0);
    }

    #[test]
    fn a_ledger_failure_names_the_round_and_the_difference() {
        let report = report_for(CAINE);
        assert_eq!((report.closed, report.failed), (8, 2));
        let failure = &report.failures[0];
        assert_eq!(failure.side, "blue");
        assert_ne!(failure.expected, failure.actual);
    }
}
