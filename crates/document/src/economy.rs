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
const ADVANCE_TEAMS: &str = include_str!("../../../config/advance_teams.yaml");
const OFFICERS: &str = include_str!("../../../config/officers.yaml");
const ECONOMY: &str = include_str!("../../../config/economy.yaml");

/// The build every document this binary writes belongs to.
///
/// It is read from the embedded tables rather than written in the code, so a
/// binary built against another build's configuration cannot claim this one.
///
/// # Panics
///
/// Panics when the embedded economy does not state a build, which is a
/// configuration this binary could not have been built with.
#[must_use]
pub fn game_build() -> &'static str {
    static BUILD: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    BUILD.get_or_init(|| {
        #[derive(Deserialize)]
        struct Stated {
            game_build: String,
        }
        let stated: Stated =
            serde_yaml::from_str(ECONOMY).expect("the embedded economy states its build");
        stated.game_build
    })
}

/// The same, as a document's own field reads when it states nothing.
pub(crate) fn this_build() -> String {
    game_build().to_owned()
}

/// Refuses a document written against another build.
///
/// A document that states nothing is this build's, because a reader has no
/// other build to read it as. One that names another is refused rather than
/// read with the wrong tables under it.
///
/// # Errors
///
/// Returns the two builds when they differ.
pub(crate) fn require_this_build(stated: &str) -> Result<(), String> {
    if stated == game_build() {
        Ok(())
    } else {
        Err(format!(
            "document is written against game build {stated}, and this binary carries {}",
            game_build()
        ))
    }
}
const COMMANDER_SKILLS: &str = include_str!("../../../config/commander_skills.yaml");

/// The prices and payouts of one build.
#[derive(Debug)]
pub struct Economy {
    units: BTreeMap<i32, UnitPrice>,
    technologies: BTreeMap<i32, i32>,
    /// Which unit each technology belongs to, so a repeat can be counted.
    technology_owner: BTreeMap<i32, i32>,
    technology_repeat_step: i32,
    cards: BTreeMap<i32, Card>,
    advance_teams: BTreeMap<i32, AdvanceTeam>,
    unit_reinforcements: BTreeMap<i32, UnitReinforcement>,
    officers: BTreeMap<i32, Officer>,
    blueprints: BTreeMap<i32, Blueprint>,
    tower_strengthen: BTreeMap<i32, i32>,
    energy_tower_skills: BTreeMap<i32, EnergyTowerSkill>,
    round_supply: RoundSupply,
    constructions: BTreeMap<String, i32>,
    contraptions: BTreeMap<i32, i32>,
    reinforce_decline: i32,
    cooldowns: BTreeMap<i32, Cooldown>,
}

/// A commander skill's two cooldowns, in rounds.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
pub struct Cooldown {
    /// Where a slot starts when the skill joins the panel.
    #[serde(rename = "initial_cooldown")]
    pub initial: i32,
    /// Where a slot goes when a round spends the skill.
    #[serde(rename = "cooldown")]
    pub spent: i32,
}

#[derive(Deserialize)]
struct CommanderSkillFile {
    skills: Vec<CommanderSkillRow>,
}

#[derive(Deserialize)]
struct CommanderSkillRow {
    id: i32,
    #[serde(flatten)]
    cooldown: Cooldown,
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

/// What kind of thing a card is, and so what taking it changes.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CardKind {
    /// Grants the commander skill its own ID names.
    CommanderSkill,
    /// Grants the equipment its own ID names.
    Equipment,
    /// Grants the officer its own ID names.
    Officer,
    /// Hands out the units [`Economy::advance_team`] states.
    AdvanceTeam,
}

/// A card a match can offer.
#[derive(Clone, Copy, Debug, Deserialize)]
pub struct Card {
    pub kind: CardKind,
    pub supply: i32,
    /// What wearing this takes off the price of upgrading its formation.
    ///
    /// Upgrade Kit is the only item in this build that discounts one, by 100.
    #[serde(default)]
    pub upgrade_supply: i32,
    /// What wearing this adds to its side's income every round.
    ///
    /// Command Core is the only item in this build that pays one, at 50.
    #[serde(default)]
    pub round_supply: i32,
}

/// What a side can pick as its opening in round 0.
#[derive(Clone, Debug, Deserialize)]
pub struct AdvanceTeam {
    pub kind: OpeningKind,
    /// One entry per formation it hands out, empty for a specialist.
    #[serde(default)]
    pub units: Vec<i32>,
    /// What picking it does to the reactor core, which is how the stronger
    /// openings are paid for.
    #[serde(default)]
    pub reactor_core: i32,
}

/// The unit a specialist officer unlocks and hands out.
///
/// The two halves arrive in different rounds. The unit joins the shop in
/// `unlock_round`, and the squad itself in the officer's `active_round`.
#[derive(Clone, Copy, Debug, Deserialize)]
pub struct OpeningUnit {
    pub unit: i32,
    pub level: i32,
    pub unlock_round: i32,
}

/// The two shapes an opening takes.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OpeningKind {
    /// Hands out the formations the row lists.
    Units,
    /// Grants the officer its own ID names.
    Officer,
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
    /// The level a unit the officer covers arrives at when bought.
    ///
    /// Elite Specialist recruits every unit at level 2 and Elite Crawler
    /// recruits Crawlers at level 5. The levels are not free: buying pays the
    /// unit's price plus one upgrade for each level above the first.
    #[serde(default)]
    pub shop_unit_level: i32,
    #[serde(default)]
    pub round_supply: i32,
    #[serde(default)]
    pub first_round_supply: i32,
    #[serde(default)]
    pub granted_supply: i32,
    /// A bounty the fight pays. No opening deals and no card grants an
    /// officer with one in standard 1v1, which is why supply is predicted
    /// without a fight.
    #[serde(default)]
    pub kill_bounty: i32,
    /// Commander skills the officer puts on the panel when it arrives.
    #[serde(default)]
    pub commander_skills: Vec<i32>,
    /// Equipment it hands out when it arrives.
    #[serde(default)]
    pub equipment: Vec<i32>,
    /// The rounds the officer hands out what it hands out.
    ///
    /// Each is an absolute round rather than one counted from the officer's
    /// arrival, and it is the round the officer's own description names:
    /// Longbow Specialist reads "在第2回合免费获得1个3级长弓" and states 2, while
    /// Rhino Specialist states 4. `OfficerData.activeRound` is a list from
    /// build 2.0 on, and an officer hands out in every round it lists. Only an
    /// officer with something to hand out states any.
    #[serde(default)]
    pub active_round: Vec<i32>,
    /// A unit it unlocks and hands out a squad of.
    #[serde(default)]
    pub opening_unit: Option<OpeningUnit>,
    /// The units a discount applies to. Empty applies to every unit.
    #[serde(default)]
    pub units: Vec<i32>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
pub struct EnergyTowerSkill {
    /// How much higher a unit bought after this skill arrives. Elite
    /// Recruitment raises the shop by one level for the rest of the round,
    /// which is a price as well as a level.
    #[serde(default)]
    pub shop_unit_level: i32,
    pub supply: i32,
    /// What activating pays back at once.
    pub granted: i32,
    /// What it takes from the next round's income.
    pub owed: i32,
}

/// The income a round hands out, which every versus map shares.
#[derive(Clone, Copy, Debug, Deserialize)]
pub struct RoundSupply {
    pub first: i32,
    pub increase: i32,
    pub max: i32,
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
    unit_id: i32,
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
    #[serde(flatten)]
    card: Card,
}

#[derive(Deserialize)]
struct AdvanceTeamFile {
    teams: Vec<AdvanceTeamRow>,
}

#[derive(Deserialize)]
struct AdvanceTeamRow {
    id: i32,
    #[serde(flatten)]
    team: AdvanceTeam,
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
    reinforce_decline: i32,
    technology_repeat_step: i32,
    contraptions: Vec<ContraptionPrice>,
    constructions: Vec<ConstructionRecovery>,
    tower_strengthen: Vec<TowerLevel>,
    energy_tower_skills: Vec<EnergyTowerRow>,
    round_supply: RoundSupply,
}

#[derive(Deserialize)]
struct ContraptionPrice {
    id: i32,
    supply: i32,
}

#[derive(Deserialize)]
struct ConstructionRecovery {
    #[serde(rename = "type")]
    type_name: String,
    recovers: i32,
}

/// What a blueprint costs and what activating it grants.
#[derive(Clone, Copy, Debug, Deserialize)]
pub struct Blueprint {
    pub supply: i32,
    /// The commander skill it puts on the panel, if it grants one.
    #[serde(default)]
    pub grants_skill: Option<i32>,
    /// The officer it produces, if it is a level of an upgrade chain.
    #[serde(default)]
    pub grants_officer: Option<i32>,
}

#[derive(Deserialize)]
struct BlueprintPrice {
    id: i32,
    #[serde(flatten)]
    blueprint: Blueprint,
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
        let teams: AdvanceTeamFile = parse(ADVANCE_TEAMS, "config/advance_teams.yaml")?;
        let reinforcements: UnitReinforcementFile =
            parse(UNIT_REINFORCEMENTS, "config/unit_reinforcements.yaml")?;
        let officers: OfficerFile = parse(OFFICERS, "config/officers.yaml")?;
        let economy: EconomyFile = parse(ECONOMY, "config/economy.yaml")?;
        let skills: CommanderSkillFile = parse(COMMANDER_SKILLS, "config/commander_skills.yaml")?;
        Ok(Self {
            units: units
                .units
                .into_iter()
                .map(|unit| (unit.unit_id, unit))
                .collect(),
            technologies: techs
                .units
                .iter()
                .flat_map(|row| &row.technologies)
                .map(|tech| (tech.id, tech.supply))
                .collect(),
            technology_owner: techs
                .units
                .iter()
                .flat_map(|row| row.technologies.iter().map(|tech| (tech.id, row.unit_id)))
                .collect(),
            cards: cards
                .items
                .into_iter()
                .map(|card| (card.id, card.card))
                .collect(),
            advance_teams: teams
                .teams
                .into_iter()
                .map(|row| (row.id, row.team))
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
                .map(|row| (row.id, row.blueprint))
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
            constructions: economy
                .constructions
                .into_iter()
                .map(|row| (row.type_name, row.recovers))
                .collect(),
            contraptions: economy
                .contraptions
                .into_iter()
                .map(|row| (row.id, row.supply))
                .collect(),
            reinforce_decline: economy.reinforce_decline,
            technology_repeat_step: economy.technology_repeat_step,
            round_supply: economy.round_supply,
            cooldowns: skills
                .skills
                .into_iter()
                .map(|row| (row.id, row.cooldown))
                .collect(),
        })
    }

    /// A commander skill's cooldowns.
    #[must_use]
    pub fn cooldown(&self, skill: i32) -> Option<Cooldown> {
        self.cooldowns.get(&skill).copied()
    }

    #[must_use]
    pub fn unit(&self, unit: i32) -> Option<UnitPrice> {
        self.units.get(&unit).copied()
    }

    #[must_use]
    pub fn technology(&self, technology: i32) -> Option<i32> {
        self.technologies.get(&technology).copied()
    }

    /// The unit a technology belongs to.
    #[must_use]
    pub fn technology_owner(&self, technology: i32) -> Option<i32> {
        self.technology_owner.get(&technology).copied()
    }

    /// Every technology this build gives each unit, keyed by unit and in ID
    /// order.
    ///
    /// A battle's `tech_loadout` is an account's own choice of these, which a
    /// replay records. A match nobody handed one to carries all of them: it is
    /// the build's answer rather than an invented account's.
    #[must_use]
    pub fn unit_technologies(&self) -> BTreeMap<i32, Vec<i32>> {
        let mut rows: BTreeMap<i32, Vec<i32>> = BTreeMap::new();
        for (technology, unit) in &self.technology_owner {
            rows.entry(*unit).or_default().push(*technology);
        }
        rows
    }

    /// What each technology already active on a unit adds to the next one.
    #[must_use]
    pub const fn technology_repeat_step(&self) -> i32 {
        self.technology_repeat_step
    }

    /// What an equipment takes off the price of upgrading its formation.
    #[must_use]
    pub fn equipment_upgrade_supply(&self, equipment: i32) -> i32 {
        self.cards
            .get(&equipment)
            .map_or(0, |card| card.upgrade_supply)
    }

    /// What an equipment adds to its side's income every round it is worn.
    #[must_use]
    pub fn equipment_round_supply(&self, equipment: i32) -> i32 {
        self.cards
            .get(&equipment)
            .map_or(0, |card| card.round_supply)
    }

    /// What taking a card costs, whichever kind of card it is.
    #[must_use]
    pub fn card(&self, card: i32) -> Option<i32> {
        self.cards
            .get(&card)
            .map(|row| row.supply)
            .or_else(|| {
                self.unit_reinforcements
                    .get(&card)
                    .map(|reinforcement| reinforcement.supply)
            })
            // An opening is paid for with reactor core, never with supply.
            .or_else(|| self.advance_teams.contains_key(&card).then_some(0))
    }

    /// What taking a card changes.
    ///
    /// A unit card and an advance team are named by their own tables, so this
    /// answers for the three kinds whose ID is the thing they grant.
    #[must_use]
    pub fn card_kind(&self, card: i32) -> Option<CardKind> {
        self.cards.get(&card).map(|row| row.kind).or_else(|| {
            self.advance_teams
                .contains_key(&card)
                .then_some(CardKind::AdvanceTeam)
        })
    }

    /// The formations an opening hands out.
    #[must_use]
    pub fn advance_team(&self, team: i32) -> Option<&AdvanceTeam> {
        self.advance_teams.get(&team)
    }

    /// Every opening this build ships, in ascending ID, which is the order the
    /// game's own pools are sorted into before a deal draws from them.
    pub fn advance_teams(&self) -> impl Iterator<Item = (i32, &AdvanceTeam)> {
        self.advance_teams.iter().map(|(id, team)| (*id, team))
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

    /// What activating a blueprint costs.
    #[must_use]
    pub fn blueprint(&self, blueprint: i32) -> Option<i32> {
        self.blueprints.get(&blueprint).map(|row| row.supply)
    }

    /// The commander skill a blueprint puts on the panel.
    #[must_use]
    pub fn blueprint_skill(&self, blueprint: i32) -> Option<i32> {
        self.blueprints.get(&blueprint)?.grants_skill
    }

    /// The officer a chain blueprint produces.
    #[must_use]
    pub fn blueprint_officer(&self, blueprint: i32) -> Option<i32> {
        self.blueprints.get(&blueprint)?.grants_officer
    }

    /// The blueprint that replaces this one, if it is the first of a chain.
    #[must_use]
    pub fn blueprint_successor(&self, blueprint: i32) -> Option<i32> {
        match blueprint {
            4 => Some(401),
            5 => Some(501),
            _ => None,
        }
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

    /// What recovering a construction pays back, which its type alone decides.
    #[must_use]
    pub fn construction_recovery(&self, type_name: &str) -> Option<i32> {
        self.constructions.get(type_name).copied()
    }

    /// What releasing a contraption costs.
    #[must_use]
    pub fn contraption(&self, contraption: i32) -> Option<i32> {
        self.contraptions.get(&contraption).copied()
    }

    /// What declining the round's reinforcement pays back.
    ///
    /// Declining is itself an item rather than the absence of one, which is
    /// why it pays: `ReinforcementManager.GetGiveUpReinforce` hands back an
    /// `AddSupplyReinforceItem`.
    #[must_use]
    pub const fn reinforce_decline(&self) -> i32 {
        self.reinforce_decline
    }

    #[must_use]
    pub const fn round_supply(&self) -> RoundSupply {
        self.round_supply
    }
}

#[cfg(test)]
mod tests {
    use super::{CardKind, Economy, OpeningKind};

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
        assert_eq!(economy.round_supply().first, 200);
        assert_eq!(economy.round_supply().max, 4000);
        assert_eq!(economy.construction_recovery("defensive_wall"), Some(50));
        assert_eq!(
            economy.construction_recovery("anti_armor_turret"),
            Some(100)
        );
    }

    #[test]
    fn releasing_a_contraption_costs_its_own_price() {
        let economy = Economy::embedded().unwrap();
        assert_eq!(economy.contraption(10_001), Some(100));
        assert_eq!(economy.contraption(20_001), Some(50));
        assert_eq!(economy.contraption(30_001), Some(100));
    }

    #[test]
    fn an_equipment_can_pay_and_discount() {
        let economy = Economy::embedded().unwrap();
        assert_eq!(economy.equipment_round_supply(13_030_010), 50);
        assert_eq!(economy.equipment_upgrade_supply(13_030_004), -100);
        assert_eq!(economy.equipment_round_supply(13_030_004), 0);
    }

    #[test]
    fn declining_the_round_card_pays_back() {
        assert_eq!(Economy::embedded().unwrap().reinforce_decline(), 50);
    }

    #[test]
    fn prices_a_card_of_every_kind() {
        let economy = Economy::embedded().unwrap();
        // A commander skill, an equipment, an officer and a unit card.
        assert_eq!(economy.card(300_001), Some(50));
        assert_eq!(economy.card(13_030_001), Some(50));
        assert_eq!(economy.card(20023), Some(300));
        assert_eq!(economy.card(1_072_213), Some(50));
        assert_eq!(economy.card_kind(300_001), Some(CardKind::CommanderSkill));
        assert_eq!(economy.card_kind(13_030_001), Some(CardKind::Equipment));
        assert_eq!(economy.card_kind(20023), Some(CardKind::Officer));
        assert_eq!(economy.card_kind(9871), Some(CardKind::AdvanceTeam));
        // A unit card is named by its own table instead.
        assert_eq!(economy.card_kind(1_072_213), None);
    }

    #[test]
    fn an_opening_names_the_formations_it_hands_out() {
        let economy = Economy::embedded().unwrap();
        let team = economy.advance_team(9871).unwrap();
        assert_eq!(team.kind, OpeningKind::Units);
        assert_eq!(team.units, vec![2, 2, 2, 13, 13]);
        assert_eq!(team.reactor_core, -200);
        // A specialist is the same choice in the other shape.
        let specialist = economy.advance_team(20029).unwrap();
        assert_eq!(specialist.kind, OpeningKind::Officer);
        assert!(specialist.units.is_empty());
        assert_eq!(specialist.reactor_core, -200);
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
        // The catalogue's chains are the blueprints the tables grant an
        // officer through.
        for (blueprint, officer) in crate::catalog::CHAIN_BLUEPRINTS {
            assert_eq!(economy.blueprint_officer(blueprint), Some(officer));
        }
        assert_eq!(economy.officer(10005).unwrap().kill_bounty, 50);
    }

    #[test]
    fn rapid_supply_pays_now_and_owes_later() {
        let economy = Economy::embedded().unwrap();
        let rapid = economy.energy_tower_skill(1).unwrap();
        assert_eq!((rapid.supply, rapid.granted, rapid.owed), (0, 200, 300));
    }
}
