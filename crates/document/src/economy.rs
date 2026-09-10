//! What a decision costs, and what a round pays out.
//!
//! Every table here was extracted from one game build and lives in `config/`.
//! A price is never inferred: a decision this build has no price for is an
//! error rather than a zero, because a ledger that quietly prices something at
//! nothing closes by accident.

use serde::Deserialize;
use std::collections::BTreeMap;

const UNIT_PRICES: &str = include_str!("../../../config/unit_prices.yaml");
const UNIT_TECHS: &str = include_str!("../../../config/unit_techs.yaml");
const REINFORCE_ITEMS: &str = include_str!("../../../config/reinforce_items.yaml");
const UNIT_REINFORCEMENTS: &str = include_str!("../../../config/unit_reinforcements.yaml");
const OFFICERS: &str = include_str!("../../../config/officers.yaml");
const ECONOMY: &str = include_str!("../../../config/economy.yaml");

/// The prices and payouts of one build.
#[derive(Debug)]
pub struct Economy {
    units: BTreeMap<i32, UnitPrice>,
    technologies: BTreeMap<i32, i32>,
    cards: BTreeMap<i32, i32>,
    unit_reinforcements: BTreeMap<i32, UnitReinforcement>,
    officers: BTreeMap<i32, Officer>,
    blueprints: BTreeMap<i32, i32>,
    tower_strengthen: BTreeMap<i32, i32>,
    energy_tower_skills: BTreeMap<i32, EnergyTowerSkill>,
    maps: BTreeMap<i32, MapSupply>,
}

/// What one unit costs to buy, to unlock and to raise one level.
#[derive(Clone, Copy, Debug, Deserialize)]
pub struct UnitPrice {
    pub unit_id: i32,
    pub supply: i32,
    pub upgrade_supply: i32,
    #[serde(default)]
    pub unlock_supply: i32,
}

/// A card that hands a side units.
///
/// No card in this build mixes two kinds, so one unit and a squad count say
/// what arrives, at one level for all of them.
#[derive(Clone, Copy, Debug, Deserialize)]
pub struct UnitReinforcement {
    pub supply: i32,
    pub unit: i32,
    pub squads: i32,
    pub level: i32,
    /// The first round the card can be offered.
    pub from_round: i32,
}

/// An officer that changes what its side pays or earns.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Officer {
    #[serde(default)]
    pub unit_supply: i32,
    #[serde(default)]
    pub unlock_supply: i32,
    #[serde(default)]
    pub technology_supply: i32,
    #[serde(default)]
    pub upgrade_supply: i32,
    #[serde(default)]
    pub round_supply: i32,
    #[serde(default)]
    pub first_round_supply: i32,
    #[serde(default)]
    pub granted_supply: i32,
    /// A bounty the fight pays, which is why a side holding one is not checked.
    #[serde(default)]
    pub kill_bounty: i32,
    /// The units a discount applies to. Empty applies to every unit.
    #[serde(default)]
    pub units: Vec<i32>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
pub struct EnergyTowerSkill {
    pub supply: i32,
    /// What activating pays back at once.
    pub granted: i32,
    /// What it takes from the next round's income.
    pub owed: i32,
}

#[derive(Clone, Copy, Debug, Deserialize)]
pub struct MapSupply {
    pub first_round_supply: i32,
    pub round_supply_increase: i32,
    pub max_round_supply: i32,
}

#[derive(Deserialize)]
struct UnitPriceFile {
    units: Vec<UnitPrice>,
}

#[derive(Deserialize)]
struct UnitTechFile {
    units: Vec<UnitTechRow>,
}

#[derive(Deserialize)]
struct UnitTechRow {
    technologies: Vec<TechnologyPrice>,
}

#[derive(Deserialize)]
struct TechnologyPrice {
    id: i32,
    supply: i32,
}

#[derive(Deserialize)]
struct CardFile {
    items: Vec<CardPrice>,
}

#[derive(Deserialize)]
struct CardPrice {
    id: i32,
    supply: i32,
}

#[derive(Deserialize)]
struct UnitReinforcementFile {
    cards: Vec<UnitReinforcementRow>,
}

#[derive(Deserialize)]
struct UnitReinforcementRow {
    id: i32,
    #[serde(flatten)]
    card: UnitReinforcement,
}

#[derive(Deserialize)]
struct OfficerFile {
    officers: Vec<OfficerRow>,
}

#[derive(Deserialize)]
struct OfficerRow {
    id: i32,
    #[serde(flatten)]
    officer: Officer,
}

#[derive(Deserialize)]
struct EconomyFile {
    blueprints: Vec<BlueprintPrice>,
    tower_strengthen: Vec<TowerLevel>,
    energy_tower_skills: Vec<EnergyTowerRow>,
    maps: Vec<MapRow>,
}

#[derive(Deserialize)]
struct BlueprintPrice {
    id: i32,
    supply: i32,
}

#[derive(Deserialize)]
struct TowerLevel {
    level: i32,
    supply: i32,
}

#[derive(Deserialize)]
struct EnergyTowerRow {
    id: i32,
    #[serde(flatten)]
    skill: EnergyTowerSkill,
}

#[derive(Deserialize)]
struct MapRow {
    map_id: i32,
    #[serde(flatten)]
    supply: MapSupply,
}

fn parse<T: serde::de::DeserializeOwned>(source: &str, name: &str) -> Result<T, String> {
    serde_yaml::from_str(source).map_err(|error| format!("cannot read {name}: {error}"))
}

impl Economy {
    /// Loads the tables this build ships with.
    ///
    /// # Errors
    ///
    /// Returns an error when one of the embedded tables cannot be read, which
    /// means the file and the shapes here have drifted apart.
    pub fn embedded() -> Result<Self, String> {
        let units: UnitPriceFile = parse(UNIT_PRICES, "config/unit_prices.yaml")?;
        let techs: UnitTechFile = parse(UNIT_TECHS, "config/unit_techs.yaml")?;
        let cards: CardFile = parse(REINFORCE_ITEMS, "config/reinforce_items.yaml")?;
        let reinforcements: UnitReinforcementFile =
            parse(UNIT_REINFORCEMENTS, "config/unit_reinforcements.yaml")?;
        let officers: OfficerFile = parse(OFFICERS, "config/officers.yaml")?;
        let economy: EconomyFile = parse(ECONOMY, "config/economy.yaml")?;
        Ok(Self {
            units: units
                .units
                .into_iter()
                .map(|unit| (unit.unit_id, unit))
                .collect(),
            technologies: techs
                .units
                .into_iter()
                .flat_map(|row| row.technologies)
                .map(|tech| (tech.id, tech.supply))
                .collect(),
            cards: cards
                .items
                .into_iter()
                .map(|card| (card.id, card.supply))
                .collect(),
            unit_reinforcements: reinforcements
                .cards
                .into_iter()
                .map(|row| (row.id, row.card))
                .collect(),
            officers: officers
                .officers
                .into_iter()
                .map(|row| (row.id, row.officer))
                .collect(),
            blueprints: economy
                .blueprints
                .into_iter()
                .map(|blueprint| (blueprint.id, blueprint.supply))
                .collect(),
            tower_strengthen: economy
                .tower_strengthen
                .into_iter()
                .map(|level| (level.level, level.supply))
                .collect(),
            energy_tower_skills: economy
                .energy_tower_skills
                .into_iter()
                .map(|row| (row.id, row.skill))
                .collect(),
            maps: economy
                .maps
                .into_iter()
                .map(|row| (row.map_id, row.supply))
                .collect(),
        })
    }

    #[must_use]
    pub fn unit(&self, unit: i32) -> Option<UnitPrice> {
        self.units.get(&unit).copied()
    }

    #[must_use]
    pub fn technology(&self, technology: i32) -> Option<i32> {
        self.technologies.get(&technology).copied()
    }

    /// What taking a card costs, whichever kind of card it is.
    #[must_use]
    pub fn card(&self, card: i32) -> Option<i32> {
        self.cards.get(&card).copied().or_else(|| {
            self.unit_reinforcements
                .get(&card)
                .map(|reinforcement| reinforcement.supply)
        })
    }

    /// What a card hands out, when it hands out units.
    #[must_use]
    pub fn unit_reinforcement(&self, card: i32) -> Option<UnitReinforcement> {
        self.unit_reinforcements.get(&card).copied()
    }

    #[must_use]
    pub fn officer(&self, officer: i32) -> Option<&Officer> {
        self.officers.get(&officer)
    }

    #[must_use]
    pub fn blueprint(&self, blueprint: i32) -> Option<i32> {
        self.blueprints.get(&blueprint).copied()
    }

    /// What raising a tower to `level` costs.
    #[must_use]
    pub fn tower_strengthen(&self, level: i32) -> Option<i32> {
        self.tower_strengthen.get(&level).copied()
    }

    #[must_use]
    pub fn energy_tower_skill(&self, skill: i32) -> Option<EnergyTowerSkill> {
        self.energy_tower_skills.get(&skill).copied()
    }

    #[must_use]
    pub fn map(&self, map_id: i32) -> Option<MapSupply> {
        self.maps.get(&map_id).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::Economy;

    #[test]
    fn reads_every_embedded_table() {
        let economy = Economy::embedded().unwrap();
        let fortress = economy.unit(1).unwrap();
        assert_eq!((fortress.supply, fortress.unlock_supply), (400, 200));
        let marksman = economy.unit(2).unwrap();
        assert_eq!((marksman.supply, marksman.unlock_supply), (100, 0));
        assert_eq!(economy.technology(10201), Some(300));
        assert_eq!(economy.blueprint(4), Some(100));
        assert_eq!(economy.tower_strengthen(1), Some(100));
        assert_eq!(economy.map(1001).unwrap().first_round_supply, 200);
    }

    #[test]
    fn prices_a_card_of_every_kind() {
        let economy = Economy::embedded().unwrap();
        // A commander skill, an equipment, an officer and a unit card.
        assert_eq!(economy.card(300_001), Some(50));
        assert_eq!(economy.card(13_030_001), Some(50));
        assert_eq!(economy.card(20023), Some(300));
        assert_eq!(economy.card(1_072_213), Some(50));
    }

    #[test]
    fn a_unit_card_says_what_it_hands_out() {
        let economy = Economy::embedded().unwrap();
        let sledgehammer = economy.unit_reinforcement(1_072_213).unwrap();
        assert_eq!(
            (sledgehammer.unit, sledgehammer.squads, sledgehammer.level),
            (13, 2, 2)
        );
        assert_eq!(sledgehammer.from_round, 7);
        assert!(economy.unit_reinforcement(300_001).is_none());
    }

    #[test]
    fn reads_the_officers_that_change_a_price() {
        let economy = Economy::embedded().unwrap();
        let small = economy.officer(20023).unwrap();
        assert_eq!(small.unit_supply, -50);
        assert!(small.units.contains(&2) && !small.units.contains(&1));
        assert_eq!(economy.officer(10002).unwrap().round_supply, 50);
        assert_eq!(economy.officer(10005).unwrap().kill_bounty, 50);
    }

    #[test]
    fn rapid_supply_pays_now_and_owes_later() {
        let economy = Economy::embedded().unwrap();
        let rapid = economy.energy_tower_skill(1).unwrap();
        assert_eq!((rapid.supply, rapid.granted, rapid.owed), (0, 200, 300));
    }
}
