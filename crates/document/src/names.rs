//! The names a battle document gives what other documents number.
//!
//! An officer, a unit's technologies, a blueprint, an energy tower skill, a
//! commander skill and an equipment item are named by the game's own English
//! names in snake case, which `config/names.yaml` holds and
//! `scripts/extract_names.py` extracts. A contraption is named as a layout
//! names it. An opening team is named by its two unit types,
//! `vortex-fire_badger`; the specialist dealt beside it is an officer.
//!
//! A reinforcement card is named by what it grants: an officer, a commander
//! skill or an equipment item by that one's name, and a card of units as
//! `scorpion_1x_lv2`, its unit, squads and level. Two unit cards can share
//! those and differ only in the round that deals them, so a unit card's name
//! is only read within its round: a battle segment appends it as `@4`, and
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
/// card needs its round, which a battle segment appends as `name@round`.
pub(crate) mod card {
    use serde::{Deserialize, Deserializer, Serializer};

    fn name<E: serde::ser::Error>(id: i32) -> Result<&'static str, E> {
        crate::names::name_of::<super::Card, E>(id)
    }

    fn id<E: serde::de::Error>(name: &str) -> Result<i32, E> {
        super::card_id(name).map_err(E::custom)
    }

    /// An optional card: a reinforcement choice's, absent when declined.
    pub(crate) mod option {
        use super::{Deserialize, Deserializer, Serializer};

        // Required by serde's `with` shape.
        #[allow(clippy::ref_option, clippy::trivially_copy_pass_by_ref)]
        pub(crate) fn serialize<S: Serializer>(
            card: &Option<i32>,
            serializer: S,
        ) -> Result<S::Ok, S::Error> {
            match card {
                Some(card) => serializer.serialize_some(super::name::<S::Error>(*card)?),
                None => serializer.serialize_none(),
            }
        }

        pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
            deserializer: D,
        ) -> Result<Option<i32>, D::Error> {
            Option::<String>::deserialize(deserializer)?
                .map(|name| super::id::<D::Error>(&name))
                .transpose()
        }
    }

    /// An optional list of cards: a round's reinforcement offers.
    pub(crate) mod offers {
        use super::{Deserialize, Deserializer, Serializer};

        #[allow(clippy::ref_option)] // Required by serde's `with` shape.
        pub(crate) fn serialize<S: Serializer, V: AsRef<[i32]>>(
            cards: &Option<V>,
            serializer: S,
        ) -> Result<S::Ok, S::Error> {
            match cards {
                Some(cards) => serializer.serialize_some(
                    &cards
                        .as_ref()
                        .iter()
                        .map(|card| super::name::<S::Error>(*card))
                        .collect::<Result<Vec<_>, _>>()?,
                ),
                None => serializer.serialize_none(),
            }
        }

        pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
            deserializer: D,
        ) -> Result<Option<Vec<i32>>, D::Error> {
            Option::<Vec<String>>::deserialize(deserializer)?
                .map(|names| {
                    names
                        .iter()
                        .map(|name| super::id::<D::Error>(name))
                        .collect()
                })
                .transpose()
        }
    }
}

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
pub(crate) mod loadout {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::borrow::Borrow;
    use std::collections::BTreeMap;

    pub(crate) fn serialize<S, M>(map: &M, serializer: S) -> Result<S::Ok, S::Error>
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

    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<BTreeMap<i32, Vec<i32>>, D::Error> {
        use serde::de::Error;
        let mut map = BTreeMap::new();
        for (name, technologies) in BTreeMap::<String, Vec<String>>::deserialize(deserializer)? {
            let unit = crate::catalog::unit_id_from_type(&name).ok_or_else(|| {
                D::Error::custom(format!("{name} is not a unit type of this build"))
            })?;
            let mut ids = technologies
                .iter()
                .map(|technology| super::technology_id::<D::Error>(unit, technology))
                .collect::<Result<Vec<_>, _>>()?;
            ids.sort_unstable();
            map.insert(unit, ids);
        }
        Ok(map)
    }
}
