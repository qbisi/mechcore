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

use crate::battle::{Action, Battle, SideState, SkillTarget, Turn};
use crate::catalog::unit_id_from_type;
use crate::economy::{Economy, Officer, RoundSupply};
use std::collections::BTreeMap;

/// Commander skills that take one of the side's own formations away and pay
/// back what it cost.
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
#[must_use]
pub fn check(battle: &Battle, economy: &Economy) -> Report {
    let map = economy.round_supply();
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
    report
}

struct Transition<'a> {
    round: i32,
    side: &'static str,
    state: &'a SideState,
    following: &'a SideState,
    actions: &'a [Action],
    next_round: i32,
    map: RoundSupply,
}

fn record(report: &mut Report, economy: &Economy, transition: &Transition<'_>) {
    if paid_by_the_fight(economy, transition.following) {
        report.fight_pays += 1;
        return;
    }
    let mut purse = Purse::new(economy, &transition.state.techs.officers);
    // A skill granted this round is only in the next state's panel, and the
    // slot has to resolve for a release to be priced at all.
    let mut panel: BTreeMap<i32, i32> = transition
        .following
        .battle_skills
        .iter()
        .map(|skill| (skill.index, skill.id))
        .collect();
    panel.extend(
        transition
            .state
            .battle_skills
            .iter()
            .map(|skill| (skill.index, skill.id)),
    );
    let Some(spent) = spend(
        economy,
        &mut purse,
        transition.state,
        &panel,
        transition.actions,
        transition.round,
    ) else {
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
    // A formation can carry an income as well as a stat: Command Core pays its
    // side 50 a round. What it pays for the next round is decided by the board
    // this round opened with, so fitting one mid-round first pays a round
    // later.
    let worn: i32 = transition
        .state
        .formations
        .iter()
        .filter_map(|entry| entry.formation.equipment)
        .map(|equipment| economy.equipment_round_supply(equipment))
        .sum();
    let earned = round_income(
        economy,
        transition.next_round,
        &transition.following.techs.officers,
        transition.map,
    ) + worn
        - owed;
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
    state.techs.officers.iter().any(|officer| {
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
pub fn round_income(economy: &Economy, round: i32, officers: &[i32], map: RoundSupply) -> i32 {
    if round < 1 {
        return 0;
    }
    let base = map
        .first
        .saturating_add((round - 1).saturating_mul(map.increase))
        .min(map.max);
    let extra: i32 = officers
        .iter()
        .filter_map(|officer| economy.officer(*officer))
        .map(|row| {
            row.round_supply + if round == 1 { row.first_round_supply } else { 0 }
        })
        .sum();
    base + extra
}

/// One formation, and what recovering it would pay back.
#[derive(Clone, Copy)]
struct Formation {
    unit: i32,
    /// The purchase price, at the prices the side's officers made at the time.
    paid: i32,
    level: i32,
    /// What it wears, since Upgrade Kit makes its upgrades cheaper.
    equipment: Option<i32>,
}

/// A side's prices, as the officers it holds change them.
struct Purse<'a> {
    economy: &'a Economy,
    officers: Vec<Officer>,
    /// What Elite Recruitment has added to the shop's level this round.
    raised: i32,
}

impl<'a> Purse<'a> {
    fn new(economy: &'a Economy, officers: &[i32]) -> Self {
        Self {
            economy,
            officers: officers
                .iter()
                .filter_map(|officer| economy.officer(*officer).cloned())
                .collect(),
            raised: 0,
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

    /// The level a bought unit arrives at, which an officer can raise.
    ///
    /// Two officers that cover the same unit do not add their levels: Elite
    /// Specialist recruits everything at 2 and Elite Crawler recruits Crawlers
    /// at 5, and a side holding both buys a Crawler at 5 rather than at 6.
    fn shop_level(&self, unit: i32) -> i32 {
        let officers = self
            .officers
            .iter()
            .filter(|officer| officer.units.is_empty() || officer.units.contains(&unit))
            .map(|officer| officer.shop_unit_level)
            .max()
            .unwrap_or(1)
            .max(1);
        officers + self.raised
    }

    fn unlock(&self, unit: i32) -> Option<i32> {
        let price = self.economy.unit(unit)?.unlock_supply;
        Some((price + self.modifier(|officer| officer.unlock_supply, Some(unit))).max(0))
    }

    fn upgrade(&self, unit: i32) -> Option<i32> {
        let price = self.economy.unit(unit)?.upgrade_supply;
        Some((price + self.modifier(|officer| officer.upgrade_supply, Some(unit))).max(0))
    }

    /// What researching a technology costs, given how many the unit already has.
    ///
    /// `UnitTechnologyManager.GetUpgradeCost` prices a technology as the step
    /// times the count already active plus its own supply, so the second
    /// technology on one unit costs more than the first.
    ///
    /// A technology discount is scoped like every other. Efficient Technology
    /// Research covers every unit, while Sabertooth Specialist covers only its
    /// own, so the discount asks about the unit the technology belongs to.
    fn technology(&self, technology: i32, unit: i32, researched: i32) -> Option<i32> {
        let price = self.economy.technology(technology)?
            + researched * self.economy.technology_repeat_step();
        Some((price + self.modifier(|officer| officer.technology_supply, Some(unit))).max(0))
    }
}

/// The formations a round opens with, and what recovering each would pay back.
///
/// Recovery pays what the formation cost: the price paid for it, at the prices
/// its side's officers made at the time, plus one upgrade for every level above
/// the first. The record's own `sell_supply` is the purchase half alone, so the
/// levels are added when the recovery is priced.
fn opening_roster(state: &SideState) -> BTreeMap<i32, Formation> {
    state
        .formations
        .iter()
        .filter_map(|entry| {
            let unit = unit_id_from_type(&entry.formation.type_name)?;
            Some((
                entry.formation.index,
                Formation {
                    unit,
                    paid: entry.value.unwrap_or(0),
                    level: entry.formation.level.unwrap_or(1),
                    equipment: entry.formation.equipment,
                },
            ))
        })
        .collect()
}

/// Files the squads the side's officers hand out this round.
///
/// A specialist delivers on its own schedule, which `crate::transition` states,
/// and the squad arrives before any of the round's own decisions. It has to be
/// filed for the same reason a card's squads do: it takes the next index, and
/// everything bought afterwards is filed one along from where it would be.
fn officer_squads(
    economy: &Economy,
    state: &SideState,
    round: i32,
    roster: &mut BTreeMap<i32, Formation>,
    next_index: &mut i32,
) -> Option<()> {
    for officer in &state.techs.officers {
        let Some(officer) = economy.officer(*officer) else {
            continue;
        };
        let Some(opening) = officer.opening_unit.filter(|_| officer.active_round == round) else {
            continue;
        };
        roster.insert(
            *next_index,
            Formation {
                unit: opening.unit,
                paid: economy.unit(opening.unit)?.supply,
                level: opening.level,
                equipment: None,
            },
        );
        *next_index += 1;
    }
    Some(())
}

/// Files the squads a card hands out, which it does as the card is taken.
///
/// Leaving them out does not just lose their recovery value: it shifts the
/// index every later purchase is filed under, so a recovery names the wrong
/// formation. Recovering one pays back the unit's own price, since the side
/// never bought it and no officer discount ever applied.
fn hand_out(
    economy: &Economy,
    card: i32,
    roster: &mut BTreeMap<i32, Formation>,
    next_index: &mut i32,
) -> Option<()> {
    let Some(reinforcement) = economy.unit_reinforcement(card) else {
        return Some(());
    };
    for _ in 0..reinforcement.squads {
        roster.insert(
            *next_index,
            Formation {
                unit: reinforcement.unit,
                paid: economy.unit(reinforcement.unit)?.supply,
                level: reinforcement.level,
                equipment: None,
            },
        );
        *next_index += 1;
    }
    Some(())
}

/// Prices one round's decisions, or reports that one has no price.
fn spend(
    economy: &Economy,
    purse: &mut Purse<'_>,
    state: &SideState,
    panel: &BTreeMap<i32, i32>,
    actions: &[Action],
    round: i32,
) -> Option<i32> {
    let mut roster = opening_roster(state);
    // How many technologies each unit already holds, which is what makes the
    // next one on that unit dearer.
    let mut researched: BTreeMap<i32, i32> = BTreeMap::new();
    for tech in &state.techs.units {
        if let Some(unit) = economy.technology_owner(*tech) {
            *researched.entry(unit).or_default() += 1;
        }
    }
    let mut next_index = state.next_index.unit;
    officer_squads(economy, state, round, &mut roster, &mut next_index)?;
    let mut towers: Vec<i32> = state.tower_strengthen_levels.clone();
    let mut total = 0;
    for action in actions {
        total += match action {
            Action::BuyUnit { unit, .. } => {
                // A unit that arrives above level 1 is paid for as a purchase
                // plus one upgrade per level above the first.
                let price = purse.buy(*unit)?;
                let level = purse.shop_level(*unit);
                roster.insert(
                    next_index,
                    Formation {
                        unit: *unit,
                        paid: price,
                        level,
                        equipment: None,
                    },
                );
                next_index += 1;
                price + (level - 1) * purse.upgrade(*unit)?
            }
            Action::UnlockUnit { unit } => purse.unlock(*unit)?,
            Action::UpgradeUnit { index } => {
                let formation = roster.get_mut(index)?;
                let (unit, worn) = (formation.unit, formation.equipment);
                formation.level += 1;
                // A discount can exceed the price, and an upgrade is never
                // paid backwards.
                (purse.upgrade(unit)?
                    + worn.map_or(0, |id| economy.equipment_upgrade_supply(id)))
                .max(0)
            }
            Action::UpgradeTechnology { tech, .. } => {
                let unit = economy.technology_owner(*tech)?;
                let count = researched.entry(unit).or_default();
                let price = purse.technology(*tech, unit, *count)?;
                *count += 1;
                price
            }
            Action::ActiveBlueprint { id } => economy.blueprint(*id)?,
            Action::ActiveEnergyTowerSkill { skill } => {
                let skill = economy.energy_tower_skill(*skill)?;
                // Elite Recruitment raises the shop for the rest of the round,
                // so a unit bought after it costs its extra level too.
                purse.raised += skill.shop_unit_level;
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
                // A card that hands out squads allocates them as it is taken,
                // which is what the indices of the rest of the round's
                // purchases are counted from. Recovering one pays back the
                // unit's own price: the side never bought it, so no officer
                // discount ever applied to it.
                hand_out(economy, *id, &mut roster, &mut next_index)?;
                price - granted
            }
            Action::ChooseAdvanceTeam { id, .. } => economy.card(*id)?,
            Action::ReleaseCommanderSkill { skill, target } => {
                // Field Recovery takes one of the side's own formations away
                // and pays back what that formation cost.
                match (panel.get(skill), target) {
                    (Some(id), SkillTarget::Unit(index)) if RECOVERY_SKILLS.contains(id) => {
                        let formation = *roster.get(index)?;
                        -(formation.paid
                            + (formation.level - 1) * purse.upgrade(formation.unit)?)
                    }
                    (Some(id), SkillTarget::Construction(index))
                        if RECOVERY_SKILLS.contains(id) =>
                    {
                        let placement = state
                            .constructions
                            .iter()
                            .find(|placement| placement.index == *index)?;
                        -economy.construction_recovery(&placement.type_name)?
                    }
                    (None, _) => return None,
                    _ => 0,
                }
            }
            // A contraption is bought from the shop as it is placed, so a
            // release is a purchase. Its price is the contraption's own.
            Action::ReleaseContraption { contraption, .. } => economy.contraption(*contraption)?,
            // Declining is an item of its own rather than the absence of one,
            // and the item it is pays supply.
            Action::DeclineReinforceItem => -economy.reinforce_decline(),
            // Fitting an item is free; the card was paid for when it was taken.
            // What it can change is the price of upgrading its formation.
            Action::UseEquipment { equipment, unit } => {
                if let Some(formation) = roster.get_mut(unit) {
                    formation.equipment = Some(*equipment);
                }
                0
            }
            Action::MoveUnit { .. } => 0,
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

    fn report_for(path: &str) -> super::Report {
        let battle = battle_from_grbr(&std::fs::read(path).unwrap()).unwrap();
        let economy = Economy::embedded().unwrap();
        let report = check(&battle, &economy);
        assert_eq!(
            report.closed + report.failed + report.fight_pays + report.unpriced,
            transitions(&battle.turns),
            "every transition is accounted for exactly once"
        );
        report
    }

    #[test]
    fn every_decidable_round_of_a_tracked_match_closes() {
        let report = report_for(TUFF);
        assert_eq!((report.closed, report.failed), (16, 0));
        assert_eq!(report.fight_pays, 0);
        assert_eq!(report.unpriced, 0);
    }

    /// The whole tracked set, so a price that only fits one match cannot pass.
    #[test]
    fn the_tracked_set_closes_every_round() {
        let economy = Economy::embedded().unwrap();
        let (mut closed, mut checked) = (0, 0);
        for entry in std::fs::read_dir("../../tests/grbr").expect("tracked replay directory") {
            let path = entry.expect("directory entry").path();
            if path.extension().is_none_or(|extension| extension != "grbr") {
                continue;
            }
            let Ok(battle) = battle_from_grbr(&std::fs::read(&path).unwrap()) else {
                continue;
            };
            let report = check(&battle, &economy);
            closed += report.closed;
            checked += report.checked();
        }
        assert_eq!((closed, checked), (66, 66));
    }

    #[test]
    fn recovering_a_construction_pays_its_fixed_price() {
        let economy = Economy::embedded().unwrap();
        assert_eq!(economy.construction_recovery("defensive_wall"), Some(50));
        // A unit's value is history; a construction's is its type.
        let battle = battle_from_grbr(&std::fs::read(TUFF).unwrap()).unwrap();
        let walls = battle.turns.iter().flat_map(|turn| {
            [&turn.state.sides.blue, &turn.state.sides.red]
                .into_iter()
                .flat_map(|side| side.constructions.iter())
        });
        assert!(walls.clone().count() > 0);
        assert!(
            walls
                .clone()
                .all(|placement| economy
                    .construction_recovery(&placement.type_name)
                    .is_some())
        );
    }

    /// A failure has to say which round it is and by how much it misses.
    /// Nothing in the tracked set fails any more, so the shape is checked
    /// against a state whose supply was moved by hand.
    #[test]
    fn a_ledger_failure_names_the_round_and_the_difference() {
        let economy = Economy::embedded().unwrap();
        let mut battle = battle_from_grbr(&std::fs::read(TUFF).unwrap()).unwrap();
        battle.turns[3].state.sides.red.supply += 50;
        let report = check(&battle, &economy);
        assert!(!report.failures.is_empty());
        let failure = &report.failures[0];
        assert!(failure.side == "blue" || failure.side == "red");
        assert!(failure.round > 0);
        assert_ne!(failure.expected, failure.actual);
    }
}
