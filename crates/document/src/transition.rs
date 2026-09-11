//! Applies a turn's decisions to the position they were taken from.
//!
//! `docs/turn.md` calls this the turn's own transition test: a turn states its
//! round twice over, once as a position and once as the decisions taken from
//! it, and applying the second to the first has to reproduce the position the
//! next turn holds. [`apply`] is that application and [`check`] is that test.
//!
//! Only what the fight cannot touch is produced here. A roster, a reactor core
//! and a formation's experience are the fight's to decide; an allocator, a
//! shop, a blueprint list, a technology list, a tower level, a skill panel and
//! an officer list are not.

use crate::battle::{Action, Battle, SideState};
use crate::economy::{CardKind, Economy, OpeningKind};
use std::collections::BTreeSet;

/// The part of a side's next position that its own decisions settle.
///
/// Every field here is one a fight cannot touch, so a round's decisions
/// determine it outright. Comparing this against the same reading of a recorded
/// state is what [`check`] does, and building it is what a turn executor needs
/// before the fight fills in the rest.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Settled {
    /// The unit allocator, which counts what was handed out as well as bought.
    pub next_unit_index: i32,
    pub unlocked_units: Vec<i32>,
    pub technologies: Vec<i32>,
    pub blueprints: Vec<i32>,
    pub tower_strengthen_levels: Vec<i32>,
    /// The skill panel by ID, ascending. A slot's index and its cooldown are
    /// not settled here: a cooldown counts down through the fight.
    pub battle_skills: Vec<i32>,
    pub officers: Vec<i32>,
}

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

/// What a round's decisions add to a side, beyond what they cost.
#[derive(Default)]
struct Granted {
    formations: usize,
    unlocked: BTreeSet<i32>,
    technologies: BTreeSet<i32>,
    blueprints: BTreeSet<i32>,
    /// A list rather than a set: an officer card that may be taken again stacks
    /// rather than replacing itself, and a side can hold three copies of one.
    officers: Vec<i32>,
    skills: Vec<i32>,
}

/// Applies one round's decisions to the position they were taken from.
///
/// The result is everything about the next position that does not wait on the
/// fight. What a card or an officer hands out is part of the decisions even
/// though no action names it, which is why the economy tables are an argument.
#[must_use]
pub fn apply(economy: &Economy, round: i32, state: &SideState, actions: &[Action]) -> Settled {
    let mut granted = granted(economy, actions);
    officer_deliveries(economy, state, &mut granted, round);

    let bought = actions
        .iter()
        .filter(|action| matches!(action, Action::BuyUnit { .. }))
        .count();
    let allocated = i32::try_from(bought + granted.formations).unwrap_or(i32::MAX);

    let mut unlocked: BTreeSet<i32> = state.shop.unlocked_units.iter().copied().collect();
    unlocked.extend(actions.iter().filter_map(|action| match action {
        Action::UnlockUnit { unit } => Some(*unit),
        _ => None,
    }));
    unlocked.extend(&granted.unlocked);

    let mut technologies: BTreeSet<i32> = state.techs.units.iter().copied().collect();
    technologies.extend(actions.iter().filter_map(|action| match action {
        Action::UpgradeTechnology { tech, .. } => Some(*tech),
        _ => None,
    }));
    technologies.extend(&granted.technologies);

    let mut blueprints: BTreeSet<i32> = state.blueprints.iter().copied().collect();
    for action in actions {
        if let Action::ActiveBlueprint { id } = action {
            // A chain's second level replaces its first rather than joining it.
            blueprints.retain(|held| economy.blueprint_successor(*held) != Some(*id));
            blueprints.insert(*id);
        }
    }
    blueprints.extend(&granted.blueprints);

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

    let mut panel: Vec<i32> = state.battle_skills.iter().map(|slot| slot.id).collect();
    panel.extend(&granted.skills);
    panel.extend(actions.iter().filter_map(|action| match action {
        Action::ActiveBlueprint { id } => economy.blueprint_skill(*id),
        _ => None,
    }));
    panel.sort_unstable();

    // A chain blueprint's officer is owned by `blueprints`, and a state lists
    // neither level of one, so activating one adds nothing here.
    let mut officers = state.techs.officers.clone();
    officers.extend(&granted.officers);
    officers.sort_unstable();

    Settled {
        next_unit_index: state.next_index.unit.saturating_add(allocated),
        unlocked_units: unlocked.into_iter().collect(),
        technologies: technologies.into_iter().collect(),
        blueprints: blueprints.into_iter().collect(),
        tower_strengthen_levels: levels,
        battle_skills: panel,
        officers,
    }
}

/// Reads out of a recorded state the same fields [`apply`] produces.
#[must_use]
pub fn settled(state: &SideState) -> Settled {
    let mut panel: Vec<i32> = state.battle_skills.iter().map(|slot| slot.id).collect();
    panel.sort_unstable();
    Settled {
        next_unit_index: state.next_index.unit,
        unlocked_units: sorted(&state.shop.unlocked_units),
        technologies: sorted(&state.techs.units),
        blueprints: sorted(&state.blueprints),
        tower_strengthen_levels: state.tower_strengthen_levels.clone(),
        battle_skills: panel,
        officers: sorted(&state.techs.officers),
    }
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
            let produced = apply(economy, turn.round, state, actions);
            let held = settled(following);
            for (field, expected, actual) in [
                (
                    "next_index.unit",
                    produced.next_unit_index.to_string(),
                    held.next_unit_index.to_string(),
                ),
                (
                    "shop.unlocked_units",
                    list(&produced.unlocked_units),
                    list(&held.unlocked_units),
                ),
                (
                    "techs.units",
                    list(&produced.technologies),
                    list(&held.technologies),
                ),
                (
                    "blueprints",
                    list(&produced.blueprints),
                    list(&held.blueprints),
                ),
                (
                    "tower_strengthen_levels",
                    list(&produced.tower_strengthen_levels),
                    list(&held.tower_strengthen_levels),
                ),
                (
                    "battle_skills",
                    list(&produced.battle_skills),
                    list(&held.battle_skills),
                ),
                (
                    "techs.officers",
                    list(&produced.officers),
                    list(&held.officers),
                ),
            ] {
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
            }
        }
    }
    report
}

fn sorted(values: &[i32]) -> Vec<i32> {
    let mut values = values.to_vec();
    values.sort_unstable();
    values
}

fn list(values: &[i32]) -> String {
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
            // A declined offer names no item, so it falls to the arm below
            // and hands out nothing.
            Action::ChooseReinforceItem { id: Some(id), .. } => *id,
            // The opening is one choice with two halves, and each half is
            // either a force or an officer. Only the officer itself arrives
            // now; what it hands out waits for its own round, which
            // [`officer_deliveries`] applies.
            Action::ChooseAdvanceTeam { id, specialist, .. } => {
                match economy.advance_team(*id) {
                    Some(team) if team.kind == OpeningKind::Units => {
                        // Picking a team unlocks the two unit types it is
                        // made of.
                        granted.formations += team.units.len();
                        granted.unlocked.extend(&team.units);
                    }
                    Some(_) => granted.officers.push(*id),
                    None => {}
                }
                if let Some(specialist) = specialist.filter(|specialist| specialist != id) {
                    granted.officers.push(specialist);
                }
                continue;
            }
            _ => continue,
        };
        if let Some(reinforcement) = economy.unit_reinforcement(card) {
            granted.formations += usize::try_from(reinforcement.squads).unwrap_or(0);
            granted.unlocked.insert(reinforcement.unit);
        }
        match economy.card_kind(card) {
            Some(CardKind::CommanderSkill) => granted.skills.push(card),
            Some(CardKind::Officer) => granted.officers.push(card),
            _ => {}
        }
    }
    granted
}

/// What the officers a side holds hand out in this round.
///
/// An officer hands out on a schedule of its own rather than when it arrives.
/// Its squad and its commander skills come in the officer's `active_round`, and
/// its unit joins the shop in the separate `unlock_round`. Both are absolute
/// rounds: Longbow Specialist unlocks Marksman in round 1 and hands out its
/// rank 3 squad in round 2, while Rhino Specialist unlocks in round 1 and waits
/// until round 4. Every specialist in this build unlocks in round 1, so an
/// opening never hands out its squad in the round it is chosen.
///
/// A side holds the officers its state lists plus the ones this round's cards
/// and opening hand it, which is what makes round 0 reach the specialist the
/// opening just chose.
fn officer_deliveries(economy: &Economy, state: &SideState, granted: &mut Granted, round: i32) {
    let held: Vec<i32> = state
        .techs
        .officers
        .iter()
        .chain(granted.officers.iter())
        .copied()
        .collect();
    for officer in held {
        let Some(officer) = economy.officer(officer) else {
            continue;
        };
        if let Some(opening) = officer.opening_unit
            && opening.unlock_round == round
        {
            granted.unlocked.insert(opening.unit);
        }
        if officer.active_round != round {
            continue;
        }
        granted.skills.extend(&officer.commander_skills);
        if officer.opening_unit.is_some() {
            granted.formations += 1;
        }
    }
}

#[cfg(all(test, feature = "convert"))]
mod tests {
    use super::{apply, check};
    use crate::battle::{Action, SideState};
    use crate::convert::battle_from_grbr;
    use crate::economy::Economy;

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
        let settled = apply(&economy, 5, &state, &taken);
        assert_eq!(settled.officers, vec![20022, 20022, 20022]);
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
        assert_eq!(
            apply(&economy, 5, &state, &declined),
            apply(&economy, 5, &state, &[])
        );
    }

    /// Every ranked replay the directory tracks, so a rule that holds for one
    /// match alone cannot pass. The directory also keeps replays the converter
    /// refuses by name, and those are skipped here rather than asserted on.
    #[test]
    fn a_turn_reproduces_what_the_fight_does_not_touch() {
        let economy = Economy::embedded().unwrap();
        let (mut closed, mut battles) = (0, 0);
        for entry in std::fs::read_dir("../../tests/grbr").expect("tracked replay directory") {
            let path = entry.expect("directory entry").path();
            if path.extension().is_none_or(|extension| extension != "grbr") {
                continue;
            }
            let Ok(battle) = battle_from_grbr(&std::fs::read(&path).unwrap()) else {
                continue;
            };
            let report = check(&battle, &economy);
            assert_eq!(
                report.failures,
                Vec::new(),
                "{} does not reproduce",
                path.display()
            );
            closed += report.closed;
            battles += 1;
        }
        assert_eq!((battles, closed), (4, 462));
    }
}
