//! Checks that a turn's decisions produce the state the next turn starts from.
//!
//! `docs/turn.md` calls this the turn's own transition test: a turn states its
//! round twice over, once as a position and once as the decisions taken from
//! it, and applying the second to the first has to reproduce the position the
//! next turn holds.
//!
//! Only what the fight cannot touch is checked here. A roster, a reactor core
//! and a formation's experience are the fight's to decide; an allocator, a
//! shop, a blueprint list, a technology list, a tower level, a skill panel and
//! an officer list are not.

use crate::battle::{Action, Battle, SideState};
use crate::economy::{CardKind, Economy, OpeningKind};
use std::collections::BTreeSet;

/// What checking one battle found, counted per field rather than per round.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub closed: usize,
    pub failed: usize,
    pub failures: Vec<Failure>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Failure {
    pub round: i32,
    pub side: &'static str,
    /// The state field the decisions failed to reproduce.
    pub field: &'static str,
    pub expected: String,
    pub actual: String,
}

/// The round in which an opening specialist delivers what it hands out. The
/// officer arrives with the opening in round 0, and its squad and its skills
/// appear one round after that.
const OPENING_DELIVERY: i32 = 1;

/// What a round's decisions add to a side, beyond what they cost.
#[derive(Default)]
struct Granted {
    formations: usize,
    unlocked: BTreeSet<i32>,
    technologies: BTreeSet<i32>,
    blueprints: BTreeSet<i32>,
    officers: BTreeSet<i32>,
    skills: Vec<i32>,
}

/// Checks every round transition of a battle.
#[must_use]
pub fn check(battle: &Battle, economy: &Economy) -> Report {
    let mut report = Report::default();
    for pair in battle.turns.windows(2) {
        let [turn, next] = pair else { continue };
        for (side, state, following, actions) in [
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
            let granted = granted(economy, actions);
            let mut record = |field, expected: String, actual: String| {
                if expected == actual {
                    report.closed += 1;
                } else {
                    report.failed += 1;
                    report.failures.push(Failure {
                        round: turn.round,
                        side,
                        field,
                        expected,
                        actual,
                    });
                }
            };

            let mut granted = granted;
            if turn.round == OPENING_DELIVERY {
                opening_grants(economy, &state.techs.officers, &mut granted);
            }
            compare(&mut record, economy, state, following, actions, &granted);
        }
    }
    report
}


/// Applies one round's decisions to each field the fight cannot touch.
fn compare(
    record: &mut impl FnMut(&'static str, String, String),
    economy: &Economy,
    state: &SideState,
    following: &SideState,
    actions: &[Action],
    granted: &Granted,
) {
            let bought = actions
                .iter()
                .filter(|action| matches!(action, Action::BuyUnit { .. }))
                .count();
            let allocated = usize::try_from(state.next_index.unit).unwrap_or(0)
                + bought
                + granted.formations;
            record(
                "next_index.unit",
                allocated.to_string(),
                following.next_index.unit.to_string(),
            );

            let mut unlocked: BTreeSet<i32> =
                state.shop.unlocked_units.iter().copied().collect();
            unlocked.extend(actions.iter().filter_map(|action| match action {
                Action::UnlockUnit { unit } => Some(*unit),
                _ => None,
            }));
            unlocked.extend(&granted.unlocked);
            record(
                "shop.unlocked_units",
                list(&unlocked),
                list(&following.shop.unlocked_units.iter().copied().collect()),
            );

            let mut technologies: BTreeSet<i32> = state.techs.units.iter().copied().collect();
            technologies.extend(actions.iter().filter_map(|action| match action {
                Action::UpgradeTechnology { tech, .. } => Some(*tech),
                _ => None,
            }));
            technologies.extend(&granted.technologies);
            record(
                "techs.units",
                list(&technologies),
                list(&following.techs.units.iter().copied().collect()),
            );

            let mut blueprints: BTreeSet<i32> = state.blueprints.iter().copied().collect();
            for action in actions {
                if let Action::ActiveBlueprint { id } = action {
                    // A chain's second level replaces its first rather than
                    // joining it.
                    blueprints.retain(|held| {
                        economy.blueprint_successor(*held) != Some(*id)
                    });
                    blueprints.insert(*id);
                }
            }
            blueprints.extend(&granted.blueprints);
            record(
                "blueprints",
                list(&blueprints),
                list(&following.blueprints.iter().copied().collect()),
            );

            let mut levels = state.tower_strengthen_levels.clone();
            for action in actions {
                if let Action::StrengthenTower { tower } = action
                    && let Some(level) = usize::try_from(*tower)
                        .ok()
                        .and_then(|index| levels.get_mut(index))
                {
                    *level += 1;
                }
            }
            record(
                "tower_strengthen_levels",
                format!("{levels:?}"),
                format!("{:?}", following.tower_strengthen_levels),
            );

            let mut panel: Vec<i32> = state.battle_skills.iter().map(|slot| slot.id).collect();
            panel.extend(&granted.skills);
            panel.extend(actions.iter().filter_map(|action| match action {
                Action::ActiveBlueprint { id } => economy.blueprint_skill(*id),
                _ => None,
            }));
            let held: Vec<i32> = following.battle_skills.iter().map(|slot| slot.id).collect();
            record(
                "battle_skills",
                format!("{} slots", panel.len()),
                format!("{} slots", held.len()),
            );

            // A chain blueprint's officer is owned by `blueprints`, and the
            // state lists neither level of one, so activating one adds nothing
            // here.
            let mut officers: BTreeSet<i32> = state.techs.officers.iter().copied().collect();
            officers.extend(&granted.officers);
            record(
                "techs.officers",
                list(&officers),
                list(&following.techs.officers.iter().copied().collect()),
            );
}

fn list(values: &BTreeSet<i32>) -> String {
    values
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

/// Reads what the round's cards and openings hand out.
fn granted(economy: &Economy, actions: &[Action]) -> Granted {
    let mut granted = Granted::default();
    for action in actions {
        let card = match action {
            Action::ChooseReinforceItem { id, .. } => *id,
            Action::ChooseAdvanceTeam { id, specialist, .. } => {
                // The opening is one choice with two halves, and the officer
                // half hands out whatever that officer hands out.
                if let Some(specialist) = specialist {
                    // Only the officer itself arrives with the opening. What it
                    // hands out arrives a round later, which
                    // [`opening_grants`] applies.
                    granted.officers.insert(*specialist);
                }
                *id
            }
            _ => continue,
        };
        if let Some(team) = economy.advance_team(card) {
            match team.kind {
                OpeningKind::Units => {
                    // Picking a team unlocks the two unit types it is made of.
                    granted.formations += team.units.len();
                    granted.unlocked.extend(&team.units);
                }
                OpeningKind::Officer => {
                    granted.officers.insert(card);
                }
            }
        }
        if let Some(reinforcement) = economy.unit_reinforcement(card) {
            granted.formations += usize::try_from(reinforcement.squads).unwrap_or(0);
            granted.unlocked.insert(reinforcement.unit);
        }
        match economy.card_kind(card) {
            Some(CardKind::CommanderSkill) => granted.skills.push(card),
            Some(CardKind::Officer) => {
                granted.officers.insert(card);
            }
            _ => {}
        }
        grant_officer(economy, card, &mut granted);
    }
    granted
}

/// What the specialists a side already holds deliver this round.
fn opening_grants(economy: &Economy, officers: &[i32], granted: &mut Granted) {
    for officer in officers {
        if economy
            .advance_team(*officer)
            .is_some_and(|team| team.kind == OpeningKind::Officer)
        {
            grant_officer(economy, *officer, granted);
        }
    }
}

/// What an officer hands out the moment it arrives.
fn grant_officer(economy: &Economy, officer: i32, granted: &mut Granted) {
    let Some(officer) = economy.officer(officer) else {
        return;
    };
    granted.skills.extend(&officer.commander_skills);
    if let Some(opening) = officer.opening_unit {
        granted.formations += 1;
        granted.unlocked.insert(opening.unit);
    }
}

#[cfg(all(test, feature = "convert"))]
mod tests {
    use super::check;
    use crate::convert::battle_from_grbr;
    use crate::economy::Economy;

    const TUFF: &str = "../../tests/grbr/2259_20260901--201562374_[crower]VS[[TUFF]MARLFAUX].grbr";

    #[test]
    fn a_turn_reproduces_what_the_fight_does_not_touch() {
        let battle = battle_from_grbr(&std::fs::read(TUFF).unwrap()).unwrap();
        let economy = Economy::embedded().unwrap();
        let report = check(&battle, &economy);
        assert!(report.closed > report.failed, "{report:?}");
    }
}
