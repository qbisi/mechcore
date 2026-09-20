//! What a side's decisions cost and what a round pays it.
//!
//! `Purse` prices a decision at the prices the side's officers make, and
//! [`round_income`] is the income a round opens with. The transition applies
//! both: [`crate::transition::step`] charges each decision as it is taken, and
//! [`crate::transition::open_round`] pays the income. Whether a battle's supply
//! adds up is then one of the leaves [`crate::coverage`] compares.

use crate::economy::{Economy, Officer, RoundSupply};

/// Commander skills that take one of the side's own formations away and pay
/// back what it cost.
pub(crate) const RECOVERY_SKILLS: [i32; 4] = [900_001, 900_002, 900_003, 900_004];

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
            row.round_supply
                + if round == 1 {
                    row.first_round_supply
                } else {
                    0
                }
        })
        .sum();
    base + extra
}

/// A side's prices, as the officers it holds change them.
pub(crate) struct Purse<'a> {
    economy: &'a Economy,
    officers: Vec<Officer>,
    /// What Elite Recruitment has added to the shop's level this round.
    pub(crate) raised: i32,
}

impl<'a> Purse<'a> {
    pub(crate) fn new(economy: &'a Economy, officers: &[i32]) -> Self {
        Self {
            economy,
            officers: officers
                .iter()
                .filter_map(|officer| economy.officer(*officer).cloned())
                .collect(),
            raised: 0,
        }
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

    pub(crate) fn buy(&self, unit: i32) -> Option<i32> {
        let price = self.economy.unit(unit)?.supply;
        Some((price + self.modifier(|officer| officer.unit_supply, Some(unit))).max(0))
    }

    /// The level a bought unit arrives at, which an officer can raise.
    ///
    /// Two officers that cover the same unit do not add their levels: Elite
    /// Specialist recruits everything at 2 and Elite Crawler recruits Crawlers
    /// at 5, and a side holding both buys a Crawler at 5 rather than at 6.
    pub(crate) fn shop_level(&self, unit: i32) -> i32 {
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

    pub(crate) fn unlock(&self, unit: i32) -> Option<i32> {
        let price = self.economy.unit(unit)?.unlock_supply;
        Some((price + self.modifier(|officer| officer.unlock_supply, Some(unit))).max(0))
    }

    pub(crate) fn upgrade(&self, unit: i32) -> Option<i32> {
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
    pub(crate) fn technology(&self, technology: i32, unit: i32, researched: i32) -> Option<i32> {
        let price = self.economy.technology(technology)?
            + researched * self.economy.technology_repeat_step();
        Some((price + self.modifier(|officer| officer.technology_supply, Some(unit))).max(0))
    }
}
