//! Checks the deployment transition against what the game actually did.
//!
//! [`crate::transition::check`] compares a round's decisions against the round
//! after it, which is the only comparison a replay's snapshots allow. A native
//! observation states a position before each decision and after it, so the same
//! transition can be checked one decision at a time, from a position the game
//! itself was in rather than from one a round boundary reconstructed.
//!
//! Two checks come out of that, and they answer different questions.
//!
//! The **step check** applies one decision to the position it was taken from
//! and compares every field of the result. It is the strong one: a field that
//! reproduces here reproduces from a real position, and a failure names the one
//! decision that broke it.
//!
//! The **round check** applies a round's whole standing sequence to the
//! position the round opened with and compares the position it closed with. It
//! is the one that catches an error the step check cannot see, because a
//! retraction is a record and not a decision: the step check reads the
//! position after an undo out of the observation, while the round check has to
//! reach the same place without it.

use crate::battle::{Action, SideState};
use crate::economy::{Economy, OpeningKind};
use crate::layout::Position;
use crate::observe::{Observed, Record, Seat, net_records};
use crate::transition::{Unsettled, step_placing};
use std::collections::BTreeMap;

/// What checking one observation found.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// Decisions whose every field reproduced.
    pub closed: usize,
    /// Decisions that reproduced some field wrongly.
    pub failed: usize,
    /// Decisions this build's tables cannot settle, by reason.
    pub unsettled: Vec<(String, usize)>,
    /// Records that state no decision, by native type.
    pub retractions: usize,
    pub failures: Vec<Failure>,
}

impl Report {
    /// The decisions the oracle could actually decide.
    #[must_use]
    pub fn checked(&self) -> usize {
        self.closed + self.failed
    }

    fn unsettle(&mut self, reason: &Unsettled) {
        let reason = format!("{reason:?}");
        if let Some(entry) = self.unsettled.iter_mut().find(|(held, _)| *held == reason) {
            entry.1 += 1;
        } else {
            self.unsettled.push((reason, 1));
        }
    }
}

/// One field a decision did not reproduce.
#[derive(Debug, PartialEq, Eq)]
pub struct Failure {
    pub sequence: i64,
    pub round: i32,
    pub side: &'static str,
    pub native_type: String,
    pub field: &'static str,
    pub expected: String,
    pub actual: String,
}

/// Applies each recorded decision to the position it was taken from.
///
/// # Errors
///
/// Returns an error when a record cannot be read as the documents it describes,
/// which is a fault in the observation or in the reader rather than in the
/// transition.
pub fn step_check(economy: &Economy, records: &[Record]) -> Result<Report, String> {
    let mut report = Report::default();
    for record in records {
        if record.kind != "action" {
            continue;
        }
        let (Some(before), Some(after), Some(team), Some(round)) = (
            record.before.as_ref(),
            record.after.as_ref(),
            record.team,
            record.round,
        ) else {
            continue;
        };
        let seat = Seat::from_team(team)?;
        let held = before.side_state(seat)?;
        let reached = after.side_state(seat)?;
        let actions = record.actions(seat, specialist(economy, &held, &reached))?;
        if actions.is_empty() {
            report.retractions += 1;
            continue;
        }
        match fold(economy, &held, &actions, &arrivals(&held, &reached)) {
            Err(reason) => report.unsettle(&reason),
            Ok(produced) => {
                let mut failures = compare(&produced, &reached);
                if failures.is_empty() {
                    report.closed += 1;
                } else {
                    report.failed += 1;
                    report
                        .failures
                        .extend(failures.drain(..).map(|(field, expected, actual)| Failure {
                            sequence: record.sequence,
                            round,
                            side: seat.name(),
                            native_type: record.native_type.clone().unwrap_or_default(),
                            field,
                            expected,
                            actual,
                        }));
                }
            }
        }
    }
    Ok(report)
}

/// The opening specialist a round 0 choice produced.
///
/// The record logs the team half alone. The officer half is whichever opening
/// officer the position holds afterwards and did not hold before, and exactly
/// one officer of a side is an opening officer.
fn specialist(economy: &Economy, held: &SideState, reached: &SideState) -> Option<i32> {
    reached.techs.officers.iter().copied().find(|officer| {
        !held.techs.officers.contains(officer)
            && economy
                .advance_team(*officer)
                .is_some_and(|team| team.kind == OpeningKind::Officer)
    })
}

/// Applies one record's decisions in order.
///
/// A move carries several units in one record and resolves to one decision per
/// unit, so a record is a short sequence rather than a single step.
///
/// Where a card's formations land is taken from the position the game reached,
/// because no table decides it. Everything else about those formations, and
/// every other field, is still the transition's to produce, so borrowing the
/// one thing the build cannot say keeps the rest under test.
fn fold(
    economy: &Economy,
    held: &SideState,
    actions: &[Action],
    granted: &BTreeMap<i32, Position>,
) -> Result<SideState, Unsettled> {
    let mut produced = held.clone();
    for action in actions {
        let mut placement = |index: i32| granted.get(&index).copied();
        produced = step_placing(economy, &produced, action, &mut placement)?;
    }
    Ok(produced)
}

/// Where each formation a position gained arrived.
///
/// A card's squads land where the board put them, so the oracle reads that one
/// input back rather than inventing it. Taking it from the position the grant
/// itself produced, and not from the one a round closed with, is what keeps a
/// later move of the same formation under test.
fn arrivals(held: &SideState, reached: &SideState) -> BTreeMap<i32, Position> {
    reached
        .formations
        .iter()
        .filter(|entry| {
            !held
                .formations
                .iter()
                .any(|placed| placed.formation.index == entry.formation.index)
        })
        .map(|entry| (entry.formation.index, entry.formation.position))
        .collect()
}

/// Every field of a side's position, compared one at a time.
///
/// `exp` and `reactor_core` are compared like the rest: a decision that writes
/// either is reported unsettled rather than excused, so leaving them in cannot
/// hide one.
fn compare(produced: &SideState, reached: &SideState) -> Vec<(&'static str, String, String)> {
    let mut failures = Vec::new();
    let mut differ = |field: &'static str, expected: String, actual: String| {
        if expected != actual {
            failures.push((field, expected, actual));
        }
    };
    differ(
        "supply",
        produced.supply.to_string(),
        reached.supply.to_string(),
    );
    differ(
        "reactor_core",
        produced.reactor_core.to_string(),
        reached.reactor_core.to_string(),
    );
    differ(
        "shop.buys_remaining",
        produced.shop.buys_remaining.to_string(),
        reached.shop.buys_remaining.to_string(),
    );
    differ(
        "shop.unlocks_remaining",
        produced.shop.unlocks_remaining.to_string(),
        reached.shop.unlocks_remaining.to_string(),
    );
    differ(
        "shop.unlocked_units",
        format!("{:?}", produced.shop.unlocked_units),
        format!("{:?}", reached.shop.unlocked_units),
    );
    differ(
        "blueprints",
        format!("{:?}", produced.blueprints),
        format!("{:?}", reached.blueprints),
    );
    differ(
        "energy_tower_skills",
        format!("{:?}", produced.energy_tower_skills),
        format!("{:?}", reached.energy_tower_skills),
    );
    differ(
        "tower_strengthen_levels",
        format!("{:?}", produced.tower_strengthen_levels),
        format!("{:?}", reached.tower_strengthen_levels),
    );
    differ(
        "equipment",
        format!("{:?}", produced.equipment),
        format!("{:?}", reached.equipment),
    );
    differ(
        "next_index",
        format!("{:?}", produced.next_index),
        format!("{:?}", reached.next_index),
    );
    differ(
        "techs",
        format!("{:?}", produced.techs),
        format!("{:?}", reached.techs),
    );
    // A snapshot says a slot was released and not where in the round or at
    // what, so the panel compares on what a snapshot can state.
    differ("battle_skills", panel(produced), panel(reached));
    // An indexed collection is compared entry by entry, so a failure names the
    // one object that differs rather than the whole board around it.
    differ(
        "formations",
        first_difference(
            produced.formations.iter().map(|entry| format!("{entry:?}")),
            reached.formations.iter().map(|entry| format!("{entry:?}")),
        )
        .map_or_else(String::new, |(left, _)| left),
        first_difference(
            produced.formations.iter().map(|entry| format!("{entry:?}")),
            reached.formations.iter().map(|entry| format!("{entry:?}")),
        )
        .map_or_else(String::new, |(_, right)| right),
    );
    differ(
        "constructions",
        format!("{:?}", produced.constructions),
        format!("{:?}", reached.constructions),
    );
    differ(
        "contraptions",
        format!("{:?}", produced.contraptions),
        format!("{:?}", reached.contraptions),
    );
    differ(
        "airdrop_shields",
        format!("{:?}", produced.airdrop_shields),
        format!("{:?}", reached.airdrop_shields),
    );
    differ(
        "terrains",
        format!("{:?}", produced.terrains),
        format!("{:?}", reached.terrains),
    );
    failures
}

/// The panel as a snapshot states it: a slot, its skill, its cooldown, and
/// whether this round released or used it, which the game does not tell apart.
fn panel(state: &SideState) -> String {
    state
        .battle_skills
        .iter()
        .map(|skill| {
            format!(
                "{}:{}:{}:{}",
                skill.index,
                skill.id,
                skill.cooldown,
                u8::from(skill.release.is_some() || skill.used)
            )
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// The position a round opened with, which is the last of its initializations.
///
/// A round is entered, rolled back to its snapshot, and entered again, so the
/// first entry carries fields the rollback had not yet restored. The last one
/// is the position the round's own decisions are taken from.
#[must_use]
pub fn round_opening(records: &[Record], round: i32) -> Option<&Observed> {
    records
        .iter()
        .rfind(|record| record.kind == "initialization" && record.round == Some(round))
        .and_then(|record| record.after.as_ref())
}

/// The position a round's deployment closed with.
#[must_use]
pub fn round_terminal(records: &[Record], round: i32) -> Option<&Observed> {
    records
        .iter()
        .find(|record| record.kind == "round_end" && record.round == Some(round))
        .and_then(|record| record.terminal.as_ref())
}

/// The first entry two collections disagree on, as the pair of spellings.
///
/// A board is long and a decision touches one object of it, so naming that
/// object is what makes a failure readable. An entry present on one side alone
/// reads as the empty string on the other.
fn first_difference(
    left: impl Iterator<Item = String>,
    right: impl Iterator<Item = String>,
) -> Option<(String, String)> {
    let left: Vec<String> = left.collect();
    let right: Vec<String> = right.collect();
    (0..left.len().max(right.len())).find_map(|index| {
        let (held, reached) = (left.get(index), right.get(index));
        (held != reached).then(|| {
            (
                held.cloned().unwrap_or_default(),
                reached.cloned().unwrap_or_default(),
            )
        })
    })
}

/// What checking whole rounds found.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct RoundReport {
    /// Deployments whose closing position was reached exactly.
    pub closed: usize,
    /// Deployments that reached a different closing position.
    pub failed: usize,
    /// Deployments holding a decision this build's tables cannot settle.
    pub unsettled: usize,
    pub failures: Vec<Failure>,
}

impl RoundReport {
    #[must_use]
    pub fn checked(&self) -> usize {
        self.closed + self.failed
    }
}

/// Applies each round's standing decisions to the position it opened with.
///
/// This is the check a per-decision comparison cannot make. The step check
/// reads the position after a retraction out of the observation, so it never
/// has to reach that position itself; here the collapse has already removed the
/// retractions, and the closing position has to come out of the decisions that
/// are left.
///
/// # Errors
///
/// Returns an error when a record cannot be read as the documents it describes.
pub fn round_check(economy: &Economy, records: &[Record]) -> Result<RoundReport, String> {
    let mut report = RoundReport::default();
    let rounds: Vec<i32> = {
        let mut seen: Vec<i32> = records
            .iter()
            .filter(|record| record.kind == "round_end")
            .filter_map(|record| record.round)
            // Round 0 is the opening choice and has no deployment.
            .filter(|round| *round >= 1)
            .collect();
        seen.sort_unstable();
        seen.dedup();
        seen
    };
    for round in rounds {
        let (Some(opening), Some(terminal)) = (
            round_opening(records, round),
            round_terminal(records, round),
        ) else {
            continue;
        };
        for seat in [Seat::Blue, Seat::Red] {
            let taken: Vec<&Record> = records
                .iter()
                .filter(|record| {
                    record.round == Some(round)
                        && record.team == Some(i32::from(seat == Seat::Red))
                        && matches!(record.kind.as_str(), "action" | "finish_deploy")
                })
                .collect();
            let held = opening.side_state(seat)?;
            let reached = terminal.side_state(seat)?;
            // Where each grant of the round arrived, read off the record that
            // produced it rather than off the position the round closed with:
            // a formation moved after it arrived would otherwise look as if it
            // had been summoned where it ended up.
            let mut granted = BTreeMap::new();
            for record in &taken {
                let (Some(before), Some(after)) = (record.before.as_ref(), record.after.as_ref())
                else {
                    continue;
                };
                granted.extend(arrivals(
                    &before.side_state(seat)?,
                    &after.side_state(seat)?,
                ));
            }
            let mut actions = Vec::new();
            for record in net_records(&taken) {
                actions.extend(record.actions(seat, specialist(economy, &held, &reached))?);
            }
            match fold(economy, &held, &actions, &granted) {
                Err(_) => report.unsettled += 1,
                Ok(produced) => {
                    let mut failures = compare(&produced, &reached);
                    if failures.is_empty() {
                        report.closed += 1;
                    } else {
                        report.failed += 1;
                        report.failures.extend(failures.drain(..).map(
                            |(field, expected, actual)| Failure {
                                sequence: 0,
                                round,
                                side: seat.name(),
                                native_type: "deployment".into(),
                                field,
                                expected,
                                actual,
                            },
                        ));
                    }
                }
            }
        }
    }
    Ok(report)
}
