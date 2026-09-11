//! Applies a turn's decisions to the position they were taken from.
//!
//! `docs/spec/document/turn.md` calls this the turn's own transition test: a turn states its
//! round twice over, once as a position and once as the decisions taken from
//! it, and applying the second to the first has to reproduce the position the
//! next turn holds. [`apply`] is that application and [`check`] is that test.
//!
//! Only what the fight cannot touch is produced here. A roster, a reactor core
//! and a formation's experience are the fight's to decide; the two allocators,
//! a shop, a blueprint list, a technology list, a tower level, a skill panel,
//! an officer list and an equipment inventory are not.

use crate::battle::{Action, Battle, EquipmentItem, SideState, SkillTarget};
use std::collections::BTreeMap;
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
    /// The contraption allocator, which rises once per release and never falls.
    /// A contraption is consumed by the fight, but the index it took is not
    /// handed out again, so the allocator is the decisions' alone.
    pub next_contraption_index: i32,
    pub unlocked_units: Vec<i32>,
    pub technologies: Vec<i32>,
    pub blueprints: Vec<i32>,
    pub tower_strengthen_levels: Vec<i32>,
    /// The skill panel by ID, ascending. A slot's index and its cooldown are
    /// not settled here: a cooldown counts down through the fight.
    pub battle_skills: Vec<i32>,
    pub officers: Vec<i32>,
    /// What the side owns and no formation wears, in the state's normal form.
    /// A multiset: a side can own two copies of one item.
    pub equipment: Vec<EquipmentItem>,
    /// Items this round fitted that the side did not hold, ascending.
    ///
    /// Every fit has to take a copy out of the stock, so a non-empty list says
    /// the decisions describe a position that cannot be reached. It is carried
    /// rather than clamped away because a stock that is empty either way would
    /// otherwise let an unreachable round compare equal to a recorded one.
    pub equipment_shortfall: Vec<i32>,
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
    /// A list rather than a set: an officer card that may be taken again stacks
    /// rather than replacing itself, and a side can hold three copies of one.
    officers: Vec<i32>,
    skills: Vec<i32>,
    /// What the round's officers deliver. A list rather than a set: one
    /// officer hands out three copies of one item. A card's equipment is not
    /// here, because a card arrives at a point in the sequence and an officer
    /// arrives before it.
    equipment: Vec<i32>,
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

    let mut blueprints: BTreeSet<i32> = state.blueprints.iter().copied().collect();
    for action in actions {
        if let Action::ActiveBlueprint { id } = action {
            // A chain's second level replaces its first rather than joining it.
            blueprints.retain(|held| economy.blueprint_successor(*held) != Some(*id));
            blueprints.insert(*id);
        }
    }

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

    let (equipment, equipment_shortfall) = inventory(economy, state, &granted, actions);

    let released = actions
        .iter()
        .filter(|action| matches!(action, Action::ReleaseContraption { .. }))
        .count();

    Settled {
        next_unit_index: state.next_index.unit.saturating_add(allocated),
        next_contraption_index: state
            .next_index
            .contraption
            .saturating_add(i32::try_from(released).unwrap_or(i32::MAX)),
        unlocked_units: unlocked.into_iter().collect(),
        technologies: technologies.into_iter().collect(),
        blueprints: blueprints.into_iter().collect(),
        tower_strengthen_levels: levels,
        battle_skills: panel,
        officers,
        equipment,
        equipment_shortfall,
    }
}

/// What a side owns and no formation wears, after the round's decisions.
///
/// Four things move the stock and every one of them is a decision. A card and
/// an officer put an item in; recovering a formation puts back what it wore;
/// fitting takes one out. A card taken this round can be fitted in the same
/// round, and so can an item a recovery just returned, so the three inflows are
/// applied in the order the actions fall rather than all before the fits.
fn inventory(
    economy: &Economy,
    state: &SideState,
    granted: &Granted,
    actions: &[Action],
) -> (Vec<EquipmentItem>, Vec<i32>) {
    let mut shortfall = Vec::new();
    let mut stock = state.equipment.clone();
    // An officer delivers before any of the round's own decisions.
    stock.extend(granted.equipment.iter().map(|id| EquipmentItem {
        id: *id,
        durability: None,
    }));
    // What each formation wears, which a recovery hands back and a fit sets.
    let mut worn: BTreeMap<i32, i32> = state
        .formations
        .iter()
        .filter_map(|entry| Some((entry.formation.index, entry.formation.equipment?)))
        .collect();
    let panel: BTreeMap<i32, i32> = state
        .battle_skills
        .iter()
        .map(|slot| (slot.index, slot.id))
        .collect();
    for action in actions {
        match action {
            Action::ChooseReinforceItem { id: Some(id), .. }
                if economy.card_kind(*id) == Some(CardKind::Equipment) =>
            {
                stock.push(EquipmentItem {
                    id: *id,
                    durability: None,
                });
            }
            Action::ReleaseCommanderSkill {
                skill,
                target: SkillTarget::Unit(index),
            } if panel
                .get(skill)
                .is_some_and(|id| crate::ledger::RECOVERY_SKILLS.contains(id)) =>
            {
                if let Some(id) = worn.remove(index) {
                    stock.push(EquipmentItem {
                        id,
                        durability: None,
                    });
                }
            }
            Action::UseEquipment { equipment, unit } => {
                // Which copy leaves is arbitrary while every copy of an ID is
                // interchangeable, which is what an absent `durability` means.
                if let Some(position) = stock.iter().position(|item| item.id == *equipment) {
                    stock.remove(position);
                } else {
                    shortfall.push(*equipment);
                }
                worn.insert(*unit, *equipment);
            }
            _ => {}
        }
    }
    stock.sort();
    shortfall.sort_unstable();
    (stock, shortfall)
}

/// Reads out of a recorded state the same fields [`apply`] produces.
#[must_use]
pub fn settled(state: &SideState) -> Settled {
    let mut panel: Vec<i32> = state.battle_skills.iter().map(|slot| slot.id).collect();
    panel.sort_unstable();
    Settled {
        next_unit_index: state.next_index.unit,
        next_contraption_index: state.next_index.contraption,
        unlocked_units: sorted(&state.shop.unlocked_units),
        technologies: sorted(&state.techs.units),
        blueprints: sorted(&state.blueprints),
        tower_strengthen_levels: state.tower_strengthen_levels.clone(),
        battle_skills: panel,
        officers: sorted(&state.techs.officers),
        equipment: state.equipment.clone(),
        // A recorded position is one the match reached, so nothing is missing.
        equipment_shortfall: Vec::new(),
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
                    "next_index.contraption",
                    produced.next_contraption_index.to_string(),
                    held.next_contraption_index.to_string(),
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
                (
                    "equipment",
                    stock(&produced.equipment, &produced.equipment_shortfall),
                    stock(&held.equipment, &held.equipment_shortfall),
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

/// An inventory as a comparable line, naming what a fit could not find so that
/// an unreachable round cannot compare equal to a recorded one.
fn stock(held: &[EquipmentItem], shortfall: &[i32]) -> String {
    let held = items(held);
    if shortfall.is_empty() {
        held
    } else {
        format!("{held} missing {}", list(shortfall))
    }
}

/// An inventory as a comparable line, with the `-1` an absent durability means
/// left off so two spellings of one item cannot read as two items.
fn items(values: &[EquipmentItem]) -> String {
    values
        .iter()
        .map(|item| match item.durability {
            Some(durability) => format!("{}:{durability}", item.id),
            None => item.id.to_string(),
        })
        .collect::<Vec<_>>()
        .join(",")
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
/// Its squad, its commander skills and its equipment come in the officer's
/// `active_round`, and its unit joins the shop in the separate `unlock_round`. Both are absolute
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
        granted.equipment.extend(&officer.equipment);
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
    use crate::economy::{CardKind, Economy};

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

    /// A card taken and not fitted stays in the side's stock.
    #[test]
    fn an_unfitted_card_stays_in_stock() {
        let economy = Economy::embedded().unwrap();
        let taken = [Action::ChooseReinforceItem {
            offer: 0,
            id: Some(13_030_001),
        }];
        let settled = apply(&economy, 5, &SideState::default(), &taken);
        assert_eq!(
            settled.equipment,
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
        assert!(
            apply(&economy, 5, &SideState::default(), &taken)
                .equipment
                .is_empty()
        );
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
        let delivered = apply(&economy, 1, &state, &[]);
        assert_eq!(
            delivered
                .equipment
                .iter()
                .map(|item| item.id)
                .collect::<Vec<_>>(),
            vec![13_030_009; 3]
        );
        assert!(apply(&economy, 2, &state, &[]).equipment.is_empty());
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
            ..SideState::default()
        };
        let fitted = [Action::UseEquipment {
            equipment: 13_030_009,
            unit: 0,
        }];
        assert_eq!(apply(&economy, 1, &state, &fitted).equipment.len(), 2);
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
            }],
            ..SideState::default()
        };
        let recovered = [Action::ReleaseCommanderSkill {
            skill: 0,
            target: crate::battle::SkillTarget::Unit(5),
        }];
        assert_eq!(
            apply(&economy, 5, &state, &recovered)
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
        let settled = apply(&economy, 5, &state, &refitted);
        assert!(settled.equipment.is_empty());
        assert!(settled.equipment_shortfall.is_empty());
    }

    /// A fit with nothing to take is reported rather than clamped away.
    ///
    /// Both stocks are empty, so without the shortfall this unreachable round
    /// would compare equal to a recorded one.
    #[test]
    fn a_fit_the_side_cannot_afford_is_named() {
        let economy = Economy::embedded().unwrap();
        let fitted = [Action::UseEquipment {
            equipment: 13_030_004,
            unit: 0,
        }];
        let settled = apply(&economy, 5, &SideState::default(), &fitted);
        assert!(settled.equipment.is_empty());
        assert_eq!(settled.equipment_shortfall, vec![13_030_004]);
    }

    /// What the tracked replays do and do not say about the inventory.
    ///
    /// The tracked set pins fits, equipment cards, stock carried across a round
    /// boundary and any item whose source the transition cannot yet reproduce.
    #[test]
    fn tracked_equipment_coverage_and_failures_are_pinned() {
        let economy = Economy::embedded().unwrap();
        let (mut fits, mut cards) = (0, 0);
        let mut held = 0;
        let mut shortfalls = Vec::new();
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
                    let missing = apply(&economy, turn.round, state, actions).equipment_shortfall;
                    if !missing.is_empty() {
                        shortfalls.push(format!(
                            "{} round {} {side}: {missing:?}",
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
        shortfalls.sort();
        assert_eq!((fits, cards), (107, 86));
        assert_eq!(held, 22);
        assert_eq!(
            shortfalls,
            [
                "2259_20260910--67396394_[kulinichstas1985]VS[Menschlein].grbr round 4 blue: [13030001]",
                "2259_20260911--201618182_[🐙Noname🐙]VS[Rievin].grbr round 5 blue: [13030003]",
            ]
        );
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
            apply(&economy, 5, &state, &released).next_contraption_index,
            5
        );
        assert_eq!(apply(&economy, 5, &state, &[]).next_contraption_index, 3);
    }

    /// Every convertible replay the directory tracks, including the exact two
    /// equipment deliveries the current transition cannot reproduce.
    #[test]
    fn the_tracked_set_pins_transition_coverage_and_failures() {
        let economy = Economy::embedded().unwrap();
        let (mut closed, mut failed, mut battles, mut releases) = (0, 0, 0, 0);
        let mut failures = Vec::new();
        for entry in std::fs::read_dir("../../tests/grbr").expect("tracked replay directory") {
            let path = entry.expect("directory entry").path();
            if path.extension().is_none_or(|extension| extension != "grbr") {
                continue;
            }
            let Ok(battle) = battle_from_grbr(&std::fs::read(&path).unwrap()) else {
                continue;
            };
            let report = check(&battle, &economy);
            failures.extend(report.failures.iter().map(|failure| {
                format!("{}: {failure:?}", path.file_name().unwrap().to_string_lossy())
            }));
            closed += report.closed;
            failed += report.failed;
            battles += 1;
            releases += battle
                .turns
                .iter()
                .flat_map(|turn| turn.actions.blue.iter().chain(&turn.actions.red))
                .filter(|action| matches!(action, Action::ReleaseContraption { .. }))
                .count();
        }
        // The contraption allocator would close for free on a set that never
        // released one, so the set has to be known to move it.
        failures.sort();
        assert_eq!((battles, closed, failed, releases), (35, 5_110, 2, 323));
        assert_eq!(
            failures,
            [
                "2259_20260910--67396394_[kulinichstas1985]VS[Menschlein].grbr: Failure { round: 4, side: \"blue\", field: \"equipment\", expected: \" missing 13030001\", actual: \"\" }",
                "2259_20260911--201618182_[🐙Noname🐙]VS[Rievin].grbr: Failure { round: 5, side: \"blue\", field: \"equipment\", expected: \" missing 13030003\", actual: \"\" }",
            ]
        );
    }
}
