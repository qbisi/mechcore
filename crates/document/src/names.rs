//! The names a match document gives what other documents number.
//!
//! An officer, a unit's technologies, a blueprint, an energy tower skill, a
//! commander skill and an equipment item are named by the game's own English
//! names in snake case, which `config/names.yaml` holds and
//! `scripts/extract/extract_names.py` extracts. A contraption is named as a layout
//! names it. An opening team is named by its two unit types,
//! `vortex-fire_badger`; the specialist dealt beside it is an officer.
//!
//! A reinforcement card is named by what it grants: an officer, a commander
//! skill or an equipment item by that one's name, and a card of units as
//! `scorpion_1x_lv2`, its unit, squads and level. Two unit cards can share
//! those and differ only in the round that deals them, so a unit card's name
//! is only read within its round: a match segment appends it as `@4`, and
//! a unit card's name read on its own is refused.
//!
//! The document still orders what it names by ID, so the fields keep the ID
//! and only their spelling is a name. A name this build does not carry is
//! refused, and so is a number where a name belongs.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::OnceLock;

const NAMES: &str = include_str!("../../../config/names.yaml");
const ADVANCE_TEAMS: &str = include_str!("../../../config/advance_teams.yaml");
const UNIT_REINFORCEMENTS: &str = include_str!("../../../config/unit_reinforcements.yaml");

/// One kind of row, named both ways.
#[derive(Default)]
struct Named {
    names: BTreeMap<i32, String>,
    ids: BTreeMap<String, i32>,
}

impl Named {
    fn insert(&mut self, id: i32, name: String) {
        self.ids.insert(name.clone(), id);
        self.names.insert(id, name);
    }

    fn name(&self, id: i32) -> Option<&str> {
        self.names.get(&id).map(String::as_str)
    }

    fn id(&self, name: &str) -> Option<i32> {
        self.ids.get(name).copied()
    }
}

struct Names {
    officers: Named,
    teams: Named,
    blueprints: Named,
    energy_tower_skills: Named,
    commander_skills: Named,
    equipment: Named,
    /// A unit card's name by ID, and its ID by name and round.
    unit_cards: BTreeMap<i32, String>,
    unit_card_ids: BTreeMap<(String, i32), i32>,
    /// A unit's technologies by unit ID: a name is only unique within one.
    technologies: BTreeMap<i32, Named>,
    /// Which unit each technology belongs to.
    owners: BTreeMap<i32, i32>,
}

#[derive(Deserialize)]
struct NameFile {
    officers: BTreeMap<i32, String>,
    technologies: BTreeMap<String, BTreeMap<i32, String>>,
    blueprints: BTreeMap<i32, String>,
    energy_tower_skills: BTreeMap<i32, String>,
    commander_skills: BTreeMap<i32, String>,
    equipment: BTreeMap<i32, String>,
}

#[derive(Deserialize)]
struct UnitCardFile {
    cards: Vec<UnitCard>,
}

#[derive(Deserialize)]
struct UnitCard {
    id: i32,
    unit: i32,
    squads: i32,
    level: i32,
    from_round: i32,
}

#[derive(Deserialize)]
struct TeamFile {
    teams: Vec<Team>,
}

#[derive(Deserialize)]
struct Team {
    id: i32,
    #[serde(default)]
    units: Vec<i32>,
}

fn names() -> &'static Names {
    static NAMES_: OnceLock<Names> = OnceLock::new();
    NAMES_.get_or_init(|| load().unwrap_or_else(|error| panic!("config/names.yaml: {error}")))
}

fn load() -> Result<Names, String> {
    let file: NameFile = serde_yaml::from_str(NAMES).map_err(|error| error.to_string())?;
    let mut officers = Named::default();
    for (id, name) in file.officers {
        officers.insert(id, name);
    }
    let mut technologies = BTreeMap::new();
    let mut owners = BTreeMap::new();
    for (unit, rows) in file.technologies {
        let unit = crate::catalog::unit_id_from_type(&unit)
            .ok_or_else(|| format!("{unit} is not a unit type"))?;
        let mut named = Named::default();
        for (id, name) in rows {
            owners.insert(id, unit);
            named.insert(id, name);
        }
        technologies.insert(unit, named);
    }
    let mut blueprints = Named::default();
    for (id, name) in file.blueprints {
        blueprints.insert(id, name);
    }
    let mut energy_tower_skills = Named::default();
    for (id, name) in file.energy_tower_skills {
        energy_tower_skills.insert(id, name);
    }
    let mut commander_skills = Named::default();
    for (id, name) in file.commander_skills {
        commander_skills.insert(id, name);
    }
    let mut equipment = Named::default();
    for (id, name) in file.equipment {
        equipment.insert(id, name);
    }
    let cards: UnitCardFile =
        serde_yaml::from_str(UNIT_REINFORCEMENTS).map_err(|error| error.to_string())?;
    let mut unit_cards = BTreeMap::new();
    let mut unit_card_ids = BTreeMap::new();
    for card in cards.cards {
        let (unit, _) = crate::catalog::unit_type_from_id(card.unit)
            .ok_or_else(|| format!("unit card {} holds unit {}", card.id, card.unit))?;
        let name = format!("{unit}_{}x_lv{}", card.squads, card.level);
        if unit_card_ids
            .insert((name.clone(), card.from_round), card.id)
            .is_some()
        {
            return Err(format!(
                "two unit cards are {name} in round {}",
                card.from_round
            ));
        }
        unit_cards.insert(card.id, name);
    }
    // A card is named by what it grants, so no two kinds may share an ID or
    // a name.
    let mut card_names = BTreeMap::new();
    for (id, name) in officers
        .names
        .iter()
        .chain(&commander_skills.names)
        .chain(&equipment.names)
    {
        if card_names.insert(name.clone(), *id).is_some() {
            return Err(format!("{name} names two reinforcement cards"));
        }
    }
    // A team is its two unit types, the one it holds three of first. The
    // file's officer rows are the specialists, which the deal draws from a
    // pool of their own and never as a team.
    let teams_file: TeamFile =
        serde_yaml::from_str(ADVANCE_TEAMS).map_err(|error| error.to_string())?;
    let mut teams = Named::default();
    for team in teams_file
        .teams
        .into_iter()
        .filter(|team| !team.units.is_empty())
    {
        let mut kinds: Vec<i32> = Vec::new();
        for unit in team.units {
            if !kinds.contains(&unit) {
                kinds.push(unit);
            }
        }
        let name = kinds
            .iter()
            .map(|unit| {
                crate::catalog::unit_type_from_id(*unit)
                    .map(|(name, _)| name)
                    .ok_or_else(|| format!("team {} holds unit {unit}", team.id))
            })
            .collect::<Result<Vec<_>, _>>()?
            .join("-");
        teams.insert(team.id, name);
    }
    Ok(Names {
        officers,
        teams,
        blueprints,
        energy_tower_skills,
        commander_skills,
        equipment,
        unit_cards,
        unit_card_ids,
        technologies,
        owners,
    })
}

/// The unit a technology belongs to.
#[must_use]
pub fn technology_owner(technology: i32) -> Option<i32> {
    names().owners.get(&technology).copied()
}

/// An equipment item's name, which a refusal quotes.
#[must_use]
pub fn equipment_name(id: i32) -> Option<&'static str> {
    names().equipment.name(id)
}

/// An officer's name, which a report quotes.
#[must_use]
pub fn officer_name(id: i32) -> Option<&'static str> {
    names().officers.name(id)
}

/// Every equipment item the build names, by ascending ID.
#[must_use]
pub fn equipment_ids() -> Vec<i32> {
    names().equipment.names.keys().copied().collect()
}

/// Every commander skill the build names, by ascending ID.
#[must_use]
pub fn commander_skill_ids() -> Vec<i32> {
    names().commander_skills.names.keys().copied().collect()
}

/// A kind of named row, as the serde helpers below use it.
pub(crate) trait Kind {
    const WHAT: &'static str;
    fn name(id: i32) -> Option<&'static str>;
    fn id(name: &str) -> Option<i32>;
}

macro_rules! kind {
    ($kind:ident, $field:ident, $what:literal) => {
        pub(crate) struct $kind;
        impl Kind for $kind {
            const WHAT: &'static str = $what;
            fn name(id: i32) -> Option<&'static str> {
                names().$field.name(id)
            }
            fn id(name: &str) -> Option<i32> {
                names().$field.id(name)
            }
        }
    };
}

kind!(Officer, officers, "officer");
kind!(AdvanceTeam, teams, "opening team");
kind!(Blueprint, blueprints, "blueprint");
kind!(EnergyTowerSkill, energy_tower_skills, "energy tower skill");
kind!(CommanderSkill, commander_skills, "commander skill");
kind!(Equipment, equipment, "equipment item");

/// A contraption, by the name a layout gives it.
pub(crate) struct Contraption;
impl Kind for Contraption {
    const WHAT: &'static str = "contraption";
    fn name(id: i32) -> Option<&'static str> {
        crate::catalog::contraption_type_from_id(id)
    }
    fn id(name: &str) -> Option<i32> {
        match crate::catalog::resolve_contraption_type(name)?.native {
            crate::catalog::NativeFormation::Contraption(id) => Some(id),
            _ => None,
        }
    }
}

/// A reinforcement card that grants a named thing. A unit card is not one:
/// its name needs its round, which [`card`] handles.
pub(crate) struct Card;
impl Kind for Card {
    const WHAT: &'static str = "reinforcement card";
    fn name(id: i32) -> Option<&'static str> {
        let names = names();
        names
            .officers
            .name(id)
            .or_else(|| names.commander_skills.name(id))
            .or_else(|| names.equipment.name(id))
            .or_else(|| names.unit_cards.get(&id).map(String::as_str))
    }
    fn id(name: &str) -> Option<i32> {
        let names = names();
        names
            .officers
            .id(name)
            .or_else(|| names.commander_skills.id(name))
            .or_else(|| names.equipment.id(name))
    }
}

/// Whether a name is a unit card's, `scorpion_1x_lv2`, which is read within
/// its round.
pub(crate) fn is_unit_card(name: &str) -> bool {
    name.rsplit_once("_lv").is_some_and(|(front, level)| {
        level.parse::<u32>().is_ok()
            && front.rsplit_once('_').is_some_and(|(_, squads)| {
                squads
                    .strip_suffix('x')
                    .is_some_and(|n| n.parse::<u32>().is_ok())
            })
    })
}

/// A card's ID from its name, a unit card's as `name@round`.
fn card_id(name: &str) -> Result<i32, String> {
    if let Some((card, round)) = name.split_once('@') {
        let round: i32 = round
            .parse()
            .map_err(|_| format!("{name} names no round"))?;
        return names()
            .unit_card_ids
            .get(&(card.to_owned(), round))
            .copied()
            .ok_or_else(|| format!("no unit card is {card} in round {round}"));
    }
    if is_unit_card(name) {
        return Err(format!("unit card {name} is only read within its round"));
    }
    Card::id(name).ok_or_else(|| format!("{name} is not a reinforcement card of this build"))
}

pub(crate) fn name_of<K: Kind, E: serde::ser::Error>(id: i32) -> Result<&'static str, E> {
    K::name(id).ok_or_else(|| E::custom(format!("{} {id} has no name in this build", K::WHAT)))
}

pub(crate) fn id_of<K: Kind, E: serde::de::Error>(name: &str) -> Result<i32, E> {
    K::id(name).ok_or_else(|| E::custom(format!("no {} of this build is named {name}", K::WHAT)))
}

/// A technology's name, within its unit.
pub(crate) fn technology_name<E: serde::ser::Error>(
    unit: i32,
    technology: i32,
) -> Result<&'static str, E> {
    names()
        .technologies
        .get(&unit)
        .and_then(|named| named.name(technology))
        .ok_or_else(|| {
            E::custom(format!(
                "technology {technology} is not one of unit {unit}'s"
            ))
        })
}

/// A technology's ID, from its name within its unit.
pub(crate) fn technology_id<E: serde::de::Error>(unit: i32, name: &str) -> Result<i32, E> {
    names()
        .technologies
        .get(&unit)
        .and_then(|named| named.id(name))
        .ok_or_else(|| E::custom(format!("{name} is not a technology of unit {unit}")))
}

/// A technology written by its name. Its ID says which unit it belongs to, so
/// writing needs nothing else; reading needs the unit, which only the
/// decision or the grouping around it has.
pub(crate) mod technology {
    use serde::{Serialize, Serializer};

    #[allow(clippy::trivially_copy_pass_by_ref)] // Required by serde's `with` shape.
    pub(crate) fn serialize<S: Serializer>(id: &i32, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::Error;
        let unit = super::technology_owner(*id)
            .ok_or_else(|| S::Error::custom(format!("technology {id} belongs to no unit")))?;
        super::technology_name::<S::Error>(unit, *id)?.serialize(serializer)
    }
}

/// Serde shapes for a named ID, one module per kind, since a `with` path
/// cannot take a type parameter.
macro_rules! shapes {
    ($module:ident, $kind:ident) => {
        #[allow(dead_code)] // Not every kind is written in every shape.
        pub(crate) mod $module {
            use super::$kind;
            use serde::{Deserialize, Deserializer, Serialize, Serializer};

            /// One named ID.
            pub(crate) mod one {
                use super::{Deserialize, Deserializer, Serialize, Serializer, $kind};

                #[allow(clippy::trivially_copy_pass_by_ref)] // Required by serde's `with` shape.
                pub(crate) fn serialize<S: Serializer>(
                    id: &i32,
                    serializer: S,
                ) -> Result<S::Ok, S::Error> {
                    crate::names::name_of::<$kind, S::Error>(*id)?.serialize(serializer)
                }

                pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
                    deserializer: D,
                ) -> Result<i32, D::Error> {
                    crate::names::id_of::<$kind, D::Error>(&String::deserialize(deserializer)?)
                }
            }

            /// An optional named ID.
            pub(crate) mod option {
                use super::{Deserialize, Deserializer, Serializer, $kind};

                #[allow(clippy::ref_option)] // Required by serde's `with` shape.
                pub(crate) fn serialize<S: Serializer>(
                    id: &Option<i32>,
                    serializer: S,
                ) -> Result<S::Ok, S::Error> {
                    match id {
                        Some(id) => serializer
                            .serialize_some(crate::names::name_of::<$kind, S::Error>(*id)?),
                        None => serializer.serialize_none(),
                    }
                }

                pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
                    deserializer: D,
                ) -> Result<Option<i32>, D::Error> {
                    Option::<String>::deserialize(deserializer)?
                        .map(|name| crate::names::id_of::<$kind, D::Error>(&name))
                        .transpose()
                }
            }

            /// A list of named IDs, kept in the order it holds.
            pub(crate) mod many {
                use super::{Deserialize, Deserializer, Serializer, $kind};

                pub(crate) fn serialize<S: Serializer>(
                    ids: &[i32],
                    serializer: S,
                ) -> Result<S::Ok, S::Error> {
                    serializer.collect_seq(
                        ids.iter()
                            .map(|id| crate::names::name_of::<$kind, S::Error>(*id))
                            .collect::<Result<Vec<_>, S::Error>>()?,
                    )
                }

                pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
                    deserializer: D,
                ) -> Result<Vec<i32>, D::Error> {
                    Vec::<String>::deserialize(deserializer)?
                        .iter()
                        .map(|name| crate::names::id_of::<$kind, D::Error>(name))
                        .collect()
                }
            }
        }
    };
}

shapes!(officer, Officer);
shapes!(advance_team, AdvanceTeam);
shapes!(blueprint, Blueprint);
shapes!(energy_tower_skill, EnergyTowerSkill);
shapes!(commander_skill, CommanderSkill);
shapes!(equipment, Equipment);
shapes!(contraption, Contraption);

/// Reinforcement cards by name. Writing needs nothing else; reading a unit
/// card needs its round, which a match segment appends as `name@round`.
pub(crate) mod card {
    use serde::{Deserialize, Deserializer, Serializer};

    fn name<E: serde::ser::Error>(id: i32) -> Result<&'static str, E> {
        crate::names::name_of::<super::Card, E>(id)
    }

    fn id<E: serde::de::Error>(name: &str) -> Result<i32, E> {
        super::card_id(name).map_err(E::custom)
    }

    /// Ordinary pool cards, whose names need no unit reinforcement round.
    pub(crate) mod many {
        use super::{Deserialize, Deserializer, Serializer};
        pub(crate) fn serialize<S: Serializer>(
            ids: &[i32],
            serializer: S,
        ) -> Result<S::Ok, S::Error> {
            serializer.collect_seq(
                ids.iter()
                    .map(|id| super::name::<S::Error>(*id))
                    .collect::<Result<Vec<_>, _>>()?,
            )
        }
        pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
            deserializer: D,
        ) -> Result<Vec<i32>, D::Error> {
            Vec::<String>::deserialize(deserializer)?
                .iter()
                .map(|name| super::id::<D::Error>(name))
                .collect()
        }
    }

    fn refund_amount(name: &str) -> Option<i32> {
        let amount = name.strip_prefix("refund_")?;
        if amount.is_empty() || !amount.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        amount.parse().ok()
    }

    /// A reinforcement answer: a card name or the round's `refund_<amount>`.
    pub(crate) mod choice {
        use super::{Deserialize, Deserializer, Serializer};
        use crate::r#match::ReinforceChoice;

        #[allow(clippy::trivially_copy_pass_by_ref)] // serde's `with` signature.
        pub(crate) fn serialize<S: Serializer>(
            choice: &ReinforceChoice,
            serializer: S,
        ) -> Result<S::Ok, S::Error> {
            match choice {
                ReinforceChoice::Card(card) => {
                    serializer.serialize_str(super::name::<S::Error>(*card)?)
                }
                ReinforceChoice::Refund(amount) if *amount >= 0 => {
                    serializer.serialize_str(&format!("refund_{amount}"))
                }
                ReinforceChoice::Refund(_) => Err(serde::ser::Error::custom(
                    "reinforcement refund is not negative",
                )),
            }
        }
        pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
            deserializer: D,
        ) -> Result<ReinforceChoice, D::Error> {
            let name = String::deserialize(deserializer)?;
            if let Some(amount) = super::refund_amount(&name) {
                return Ok(ReinforceChoice::Refund(amount));
            }
            super::id::<D::Error>(&name).map(ReinforceChoice::Card)
        }
    }

    /// A round's cards followed by its supply decline option.
    pub(crate) mod offers {
        use super::{Deserialize, Deserializer, Serializer};
        use crate::economy::CardKind;
        use crate::r#match::Offers;
        use serde::Serialize;

        /// Structured representation of one entry in `reinforce_offers`.
        #[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema, PartialEq, Eq)]
        #[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
        pub enum OfferEntry {
            Officer {
                name: String,
                cost: i32,
            },
            Equipment {
                name: String,
                cost: i32,
            },
            CommanderSkill {
                name: String,
                cost: i32,
            },
            Unit {
                name: String,
                unit: String,
                level: i32,
                squads: i32,
                cost: i32,
            },
            Supply {
                name: String,
                cost: i32,
            },
        }

        #[allow(clippy::ref_option)] // Required by serde's `with` shape.
        pub(crate) fn serialize<S: Serializer, O: std::borrow::Borrow<Offers>>(
            offers: &Option<O>,
            serializer: S,
        ) -> Result<S::Ok, S::Error> {
            match offers {
                Some(offers) => serializer.serialize_some(offers.borrow()),
                None => serializer.serialize_none(),
            }
        }

        pub(crate) fn write<S: Serializer>(
            offers: &Offers,
            serializer: S,
        ) -> Result<S::Ok, S::Error> {
            use serde::ser::Error;
            if offers.refund < 0 {
                return Err(S::Error::custom("reinforcement refund is not negative"));
            }
            let supply_cost = offers
                .refund
                .checked_neg()
                .ok_or_else(|| S::Error::custom("reinforcement refund overflows cost"))?;
            let economy = crate::economy::Economy::embedded().map_err(S::Error::custom)?;
            let mut entries = Vec::with_capacity(offers.dealt.len() + 1);
            for &card in &offers.dealt {
                let name = super::name::<S::Error>(card)?.to_owned();
                let cost = economy.card(card).ok_or_else(|| {
                    S::Error::custom(format!("reinforcement card {card} has no price"))
                })?;
                if let Some(ur) = economy.unit_reinforcement(card) {
                    let (unit, _) =
                        crate::catalog::unit_type_from_id(ur.unit).ok_or_else(|| {
                            S::Error::custom(format!(
                                "unit card {card} has unknown unit {}",
                                ur.unit
                            ))
                        })?;
                    entries.push(OfferEntry::Unit {
                        name,
                        unit: unit.to_owned(),
                        level: ur.level,
                        squads: ur.squads,
                        cost,
                    });
                } else {
                    let entry = match economy.card_kind(card) {
                        Some(CardKind::Officer) => OfferEntry::Officer { name, cost },
                        Some(CardKind::Equipment) => OfferEntry::Equipment { name, cost },
                        Some(CardKind::CommanderSkill) => OfferEntry::CommanderSkill { name, cost },
                        _ => {
                            return Err(S::Error::custom(format!(
                                "reinforcement card {card} has no offer kind"
                            )));
                        }
                    };
                    entries.push(entry);
                }
            }
            entries.push(OfferEntry::Supply {
                name: format!("refund_{}", offers.refund),
                cost: supply_cost,
            });
            entries.serialize(serializer)
        }

        pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
            deserializer: D,
        ) -> Result<Option<Offers>, D::Error> {
            use serde::de::Error;
            let Some(mut entries) = Option::<Vec<OfferEntry>>::deserialize(deserializer)? else {
                return Ok(None);
            };
            let refund = match entries.pop() {
                Some(OfferEntry::Supply { name, cost }) => {
                    let refund = super::refund_amount(&name).ok_or_else(|| {
                        D::Error::custom(
                            "reinforce_offers ends in supply entry refund_<nonnegative integer>",
                        )
                    })?;
                    if refund.checked_neg() != Some(cost) {
                        return Err(D::Error::custom("supply offer cost must equal -<refund>"));
                    }
                    refund
                }
                _ => {
                    return Err(D::Error::custom(
                        "reinforce_offers ends in supply entry refund_<nonnegative integer>",
                    ));
                }
            };
            let economy = crate::economy::Economy::embedded().map_err(D::Error::custom)?;
            let mut dealt = Vec::with_capacity(entries.len());
            for entry in entries {
                let id = match entry {
                    OfferEntry::Supply { .. } => {
                        return Err(D::Error::custom(
                            "supply offer must appear only as the final entry",
                        ));
                    }
                    OfferEntry::Officer { name, cost } => {
                        check_card::<D::Error>(economy, &name, CardKind::Officer, cost)?
                    }
                    OfferEntry::Equipment { name, cost } => {
                        check_card::<D::Error>(economy, &name, CardKind::Equipment, cost)?
                    }
                    OfferEntry::CommanderSkill { name, cost } => {
                        check_card::<D::Error>(economy, &name, CardKind::CommanderSkill, cost)?
                    }
                    OfferEntry::Unit {
                        name,
                        unit,
                        level,
                        squads,
                        cost,
                    } => {
                        let id = super::id::<D::Error>(&name)?;
                        let ur = economy.unit_reinforcement(id).ok_or_else(|| {
                            D::Error::custom(format!("{name} is not a unit reinforcement card"))
                        })?;
                        let expected_unit =
                            crate::catalog::unit_type_from_id(ur.unit).map(|(u, _)| u);
                        if expected_unit != Some(unit.as_str())
                            || ur.level != level
                            || ur.squads != squads
                        {
                            return Err(D::Error::custom(format!(
                                "unit offer {name} disagrees with unit {unit}, level {level}, squads {squads}"
                            )));
                        }
                        if economy.card(id) != Some(cost) {
                            return Err(D::Error::custom(format!(
                                "reinforcement offer {name} has wrong cost {cost}"
                            )));
                        }
                        id
                    }
                };
                dealt.push(id);
            }
            Ok(Some(Offers { dealt, refund }))
        }

        fn check_card<E: serde::de::Error>(
            economy: &crate::economy::Economy,
            name: &str,
            expected_kind: CardKind,
            cost: i32,
        ) -> Result<i32, E> {
            let id = super::id::<E>(name)?;
            if economy.card_kind(id) != Some(expected_kind) {
                return Err(E::custom(format!(
                    "reinforcement offer {name} has wrong card kind"
                )));
            }
            if economy.card(id) != Some(cost) {
                return Err(E::custom(format!(
                    "reinforcement offer {name} has wrong cost {cost}"
                )));
            }
            Ok(id)
        }
    }
}

pub use card::offers::OfferEntry;

/// Technologies grouped by the unit they belong to, `{fang: [name, ...]}`:
/// units in ID order, a unit's technologies in ID order. Stored flat, as IDs.
pub(crate) mod technologies {
    use serde::{Deserializer, Serializer};
    use std::collections::BTreeMap;

    pub(crate) fn serialize<S: Serializer>(ids: &[i32], serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::Error;
        let mut grouped: BTreeMap<i32, Vec<i32>> = BTreeMap::new();
        for id in ids {
            let unit = super::technology_owner(*id)
                .ok_or_else(|| S::Error::custom(format!("technology {id} belongs to no unit")))?;
            grouped.entry(unit).or_default().push(*id);
        }
        super::loadout::serialize(&grouped, serializer)
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Vec<i32>, D::Error> {
        let grouped = super::loadout::deserialize(deserializer)?;
        let mut ids: Vec<i32> = grouped.into_values().flatten().collect();
        ids.sort_unstable();
        Ok(ids)
    }
}

/// A mapping from unit type to that unit's technologies, both by name:
/// `{fortress: [anti_air_barrage, ...]}`, units and technologies in ID order.
///
/// Match headers write `tech_loadout` in compact form (`"full"` when every
/// standard 1v1 technology is enabled, otherwise an unpadded Base64url bitset
/// of enabled standard 1v1 technology IDs sorted ascending), while `--expanded`
/// formatting writes this mapping by name. Deserialization accepts both forms.
pub mod loadout {
    use serde::de::MapAccess;
    use serde::{Deserializer, Serializer};
    use std::borrow::Borrow;
    use std::collections::BTreeMap;
    use std::fmt;
    use std::sync::OnceLock;

    const BASE64URL: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

    struct TechCatalog {
        /// `(tech_id, unit_id)` for every standard 1v1 technology, sorted by `tech_id`.
        techs: Vec<(i32, i32)>,
        index_by_id: BTreeMap<i32, usize>,
        bytes_len: usize,
        encoded_len: usize,
        trailing_byte_mask: u8,
        trailing_b64_mask: u8,
    }

    fn catalog() -> Result<&'static TechCatalog, String> {
        static CATALOG: OnceLock<Result<TechCatalog, String>> = OnceLock::new();
        CATALOG
            .get_or_init(|| {
                let economy = crate::economy::Economy::embedded()?;
                let mut techs = Vec::new();
                for (unit, list) in economy.unit_technologies() {
                    for tech in list {
                        techs.push((tech, unit));
                    }
                }
                techs.sort_unstable_by_key(|(tech, _)| *tech);
                let mut index_by_id = BTreeMap::new();
                for (bit, (tech, _)) in techs.iter().enumerate() {
                    if index_by_id.insert(*tech, bit).is_some() {
                        return Err(format!("duplicate standard technology ID {tech}"));
                    }
                }
                let count = techs.len();
                let bytes_len = count.div_ceil(8);
                let encoded_len = (bytes_len * 4).div_ceil(3);
                let rem_bits = count % 8;
                let trailing_byte_mask = if rem_bits == 0 {
                    u8::MAX
                } else {
                    (1u8 << rem_bits) - 1
                };
                let b64_rem_bits = (bytes_len * 8) % 6;
                let trailing_b64_mask = if b64_rem_bits == 0 {
                    0
                } else {
                    (1u8 << (6 - b64_rem_bits)) - 1
                };
                Ok(TechCatalog {
                    techs,
                    index_by_id,
                    bytes_len,
                    encoded_len,
                    trailing_byte_mask,
                    trailing_b64_mask,
                })
            })
            .as_ref()
            .map_err(Clone::clone)
    }

    fn encode_b64url(bytes: &[u8]) -> String {
        let mut out = String::with_capacity((bytes.len() * 4).div_ceil(3));
        for chunk in bytes.chunks(3) {
            let b0 = chunk[0];
            let b1 = *chunk.get(1).unwrap_or(&0);
            let b2 = *chunk.get(2).unwrap_or(&0);
            let n = (u32::from(b0) << 16) | (u32::from(b1) << 8) | u32::from(b2);
            out.push(BASE64URL[((n >> 18) & 0x3f) as usize] as char);
            out.push(BASE64URL[((n >> 12) & 0x3f) as usize] as char);
            if chunk.len() >= 2 {
                out.push(BASE64URL[((n >> 6) & 0x3f) as usize] as char);
            }
            if chunk.len() == 3 {
                out.push(BASE64URL[(n & 0x3f) as usize] as char);
            }
        }
        out
    }

    fn b64url_value(byte: u8) -> Option<u8> {
        match byte {
            b'A'..=b'Z' => Some(byte - b'A'),
            b'a'..=b'z' => Some(byte - b'a' + 26),
            b'0'..=b'9' => Some(byte - b'0' + 52),
            b'-' => Some(62),
            b'_' => Some(63),
            _ => None,
        }
    }

    fn decode_compact(text: &str) -> Result<BTreeMap<i32, Vec<i32>>, String> {
        let cat = catalog()?;
        if text == "full" {
            return Ok(crate::economy::Economy::embedded()?.unit_technologies());
        }
        let bytes = text.as_bytes();
        if bytes.len() != cat.encoded_len {
            return Err(format!(
                "compact tech_loadout must be \"full\" or {} base64url characters, not {}",
                cat.encoded_len,
                bytes.len()
            ));
        }
        let mut sextets = Vec::with_capacity(bytes.len());
        for (at, &ch) in bytes.iter().enumerate() {
            let val = b64url_value(ch).ok_or_else(|| {
                format!(
                    "compact tech_loadout has invalid base64url character {:?} at {at}",
                    ch as char
                )
            })?;
            sextets.push(val);
        }
        if let Some(&last) = sextets.last()
            && (last & cat.trailing_b64_mask) != 0
        {
            return Err("compact tech_loadout has nonzero base64url padding bits".into());
        }
        let mut raw = Vec::with_capacity(cat.bytes_len);
        for chunk in sextets.chunks(4) {
            let (c0, c1) = (chunk[0], chunk[1]);
            raw.push((c0 << 2) | (c1 >> 4));
            if chunk.len() >= 3 {
                let c2 = chunk[2];
                raw.push((c1 << 4) | (c2 >> 2));
                if chunk.len() == 4 {
                    raw.push((c2 << 6) | chunk[3]);
                }
            }
        }
        debug_assert_eq!(raw.len(), cat.bytes_len);
        if (raw[cat.bytes_len - 1] & !cat.trailing_byte_mask) != 0 {
            return Err("compact tech_loadout sets bits past the technology table".into());
        }
        let mut enabled = 0usize;
        let mut map: BTreeMap<i32, Vec<i32>> = BTreeMap::new();
        for (bit, &(tech, unit)) in cat.techs.iter().enumerate() {
            if ((raw[bit / 8] >> (bit % 8)) & 1) != 0 {
                enabled += 1;
                map.entry(unit).or_default().push(tech);
            }
        }
        if enabled == cat.techs.len() {
            return Err("full tech_loadout must be written as `full`".into());
        }
        for list in map.values_mut() {
            list.sort_unstable();
        }
        Ok(map)
    }

    /// Writes a compact loadout (`"full"` or a 42-char base64url positive bitset).
    ///
    /// # Errors
    ///
    /// Returns an error when a unit or technology is outside this build's standard table.
    pub fn serialize_compact<S, M>(map: &M, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
        M: Borrow<BTreeMap<i32, Vec<i32>>>,
    {
        use serde::ser::Error;
        let cat = catalog().map_err(S::Error::custom)?;
        let mut buf = vec![0u8; cat.bytes_len];
        let mut count = 0usize;
        for (&unit, technologies) in map.borrow() {
            for &tech in technologies {
                let Some(&bit) = cat.index_by_id.get(&tech) else {
                    return Err(S::Error::custom(format!(
                        "technology {tech} is not in the standard tech table"
                    )));
                };
                if cat.techs[bit].1 != unit {
                    return Err(S::Error::custom(format!(
                        "technology {tech} belongs to unit {}, not {unit}",
                        cat.techs[bit].1
                    )));
                }
                let mask = 1u8 << (bit % 8);
                if (buf[bit / 8] & mask) == 0 {
                    buf[bit / 8] |= mask;
                    count += 1;
                }
            }
        }
        if count == cat.techs.len() {
            serializer.serialize_str("full")
        } else {
            serializer.serialize_str(&encode_b64url(&buf))
        }
    }

    /// Writes a loadout by name.
    ///
    /// # Errors
    ///
    /// Returns an error when a unit or a technology has no name in this build.
    pub fn serialize<S, M>(map: &M, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
        M: Borrow<BTreeMap<i32, Vec<i32>>>,
    {
        use serde::ser::Error;
        let mut rows = Vec::new();
        for (unit, technologies) in map.borrow() {
            let name = crate::catalog::unit_type_from_id(*unit)
                .map(|(name, _)| name)
                .ok_or_else(|| {
                    S::Error::custom(format!("unit ID {unit} has no type name in this build"))
                })?;
            let mut sorted = technologies.clone();
            sorted.sort_unstable();
            let named = sorted
                .iter()
                .map(|technology| super::technology_name::<S::Error>(*unit, *technology))
                .collect::<Result<Vec<_>, _>>()?;
            rows.push((name, named));
        }
        serializer.collect_map(rows)
    }

    /// Reads a loadout written either in compact bitset (`"full"` or base64url)
    /// or expanded map (`{unit: [tech, ...]}`) form.
    ///
    /// # Errors
    ///
    /// Returns an error when a compact encoding or technology name is invalid.
    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<BTreeMap<i32, Vec<i32>>, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = BTreeMap<i32, Vec<i32>>;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a compact tech_loadout string or unit-technology mapping")
            }

            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Self::Value, E> {
                decode_compact(value).map_err(E::custom)
            }

            fn visit_map<M: MapAccess<'de>>(self, mut access: M) -> Result<Self::Value, M::Error> {
                use serde::de::Error;
                let mut map = BTreeMap::new();
                while let Some((name, technologies)) = access.next_entry::<String, Vec<String>>()? {
                    let unit = crate::catalog::unit_id_from_type(&name).ok_or_else(|| {
                        M::Error::custom(format!("{name} is not a unit type of this build"))
                    })?;
                    let mut ids = technologies
                        .iter()
                        .map(|technology| super::technology_id::<M::Error>(unit, technology))
                        .collect::<Result<Vec<_>, _>>()?;
                    ids.sort_unstable();
                    map.insert(unit, ids);
                }
                Ok(map)
            }
        }
        deserializer.deserialize_any(Visitor)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use serde::Deserialize;

        #[derive(Debug, Deserialize, PartialEq, Eq)]
        struct Wrapper(#[serde(with = "super")] BTreeMap<i32, Vec<i32>>);

        #[derive(serde::Serialize)]
        struct CompactWrapper<'a>(
            #[serde(serialize_with = "super::serialize_compact")] &'a BTreeMap<i32, Vec<i32>>,
        );

        #[derive(serde::Serialize)]
        struct ExpandedWrapper<'a>(
            #[serde(serialize_with = "super::serialize")] &'a BTreeMap<i32, Vec<i32>>,
        );

        #[test]
        fn compact_bitset_round_trips_full_empty_and_partial_loadouts() {
            let economy = crate::economy::Economy::embedded().unwrap();
            let full = economy.unit_technologies();
            let full_yaml = serde_yaml::to_string(&CompactWrapper(&full)).unwrap();
            assert_eq!(full_yaml.trim(), "full");
            assert_eq!(serde_yaml::from_str::<Wrapper>(&full_yaml).unwrap().0, full);

            let empty = BTreeMap::new();
            let empty_yaml = serde_yaml::to_string(&CompactWrapper(&empty)).unwrap();
            assert_eq!(empty_yaml.trim().len(), 42);
            assert_eq!(
                serde_yaml::from_str::<Wrapper>(&empty_yaml).unwrap().0,
                empty
            );

            // Keep 4 per normal unit and up to 6 for factory/abyss/mountain.
            let mut custom = BTreeMap::new();
            for (unit, mut list) in full {
                list.truncate(crate::r#match::research_limit(unit));
                custom.insert(unit, list);
            }
            let custom_yaml = serde_yaml::to_string(&CompactWrapper(&custom)).unwrap();
            assert_eq!(custom_yaml.trim().len(), 42);
            assert_eq!(
                serde_yaml::from_str::<Wrapper>(&custom_yaml).unwrap().0,
                custom
            );

            // Expanded YAML map reads identically to compact.
            let expanded_yaml = serde_yaml::to_string(&ExpandedWrapper(&custom)).unwrap();
            assert_eq!(
                serde_yaml::from_str::<Wrapper>(&expanded_yaml).unwrap().0,
                custom
            );
        }

        #[test]
        fn compact_decoder_refuses_malformed_and_all_ones_strings() {
            assert!(decode_compact("A").is_err());
            assert!(decode_compact(&format!("{}=", "A".repeat(41))).is_err());
            assert!(decode_compact(&format!("{}!", "A".repeat(41))).is_err());
            // Last base64 char of 31 bytes has 4 unused low bits -> 'B' sets bit 0.
            assert!(decode_compact(&format!("{}B", "A".repeat(41))).is_err());
            // All-ones encoding for full table must be written as "full".
            let cat = catalog().unwrap();
            let mut full_bytes = vec![0xffu8; cat.bytes_len];
            full_bytes[cat.bytes_len - 1] = cat.trailing_byte_mask;
            let all_ones = encode_b64url(&full_bytes);
            assert!(decode_compact(&all_ones).is_err());
        }
    }
}
