//! Deployment formations and the native artwork used by the side drawers.

use std::collections::{BTreeMap, BTreeSet};

use mechcore_document::layout::Side;
use serde::{Deserialize, Serialize};

/// One unit type at deployment, independent of combat losses and summons.
#[derive(Debug, Serialize)]
pub struct RosterEntry {
    pub kind: String,
    pub count: usize,
    /// Held formations at the start of this deployment, when history is known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count_before: Option<usize>,
    /// Known held formation values plus historical technology purchase costs.
    pub investment_supply: i64,
    pub techs: Vec<RosterTechnology>,
}

#[derive(Debug, Serialize)]
pub struct RosterTechnology {
    pub id: i32,
    pub active: bool,
    /// Researched during this round's deployment, when match history is known.
    pub activated_this_round: bool,
    /// Actual historical cost when active, current quote otherwise.
    /// Absent when the input does not carry enough deployment history.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub purchase_supply: Option<i32>,
}

/// Includes known unlocks and researched types even when no formation is held.
#[must_use]
pub fn unit_roster(
    side: &Side,
    unlocked: &[i32],
    loadout: Option<&BTreeMap<i32, Vec<i32>>>,
) -> Vec<RosterEntry> {
    let mut kinds = BTreeSet::<String>::new();
    for unit in &side.units {
        kinds.insert(unit.type_name.clone());
    }
    for id in unlocked.iter().copied().chain(
        side.techs
            .iter()
            .filter_map(|id| mechcore_document::names::technology_owner(*id)),
    ) {
        if let Some((name, _)) = mechcore_document::unit_type_from_id(id) {
            kinds.insert(name.into());
        }
    }
    kinds
        .into_iter()
        .map(|kind| {
            let selected = loadout.and_then(|rows| {
                rows.iter().find_map(|(id, techs)| {
                    mechcore_document::unit_type_from_id(*id)
                        .filter(|(name, _)| *name == kind)
                        .map(|_| techs)
                })
            });
            let techs = selected
                .unwrap_or(&side.techs)
                .iter()
                .copied()
                .filter(|id| {
                    mechcore_document::names::technology_owner(*id)
                        .and_then(mechcore_document::unit_type_from_id)
                        .is_some_and(|(name, _)| name == kind)
                })
                .map(|id| RosterTechnology {
                    id,
                    active: side.techs.contains(&id),
                    activated_this_round: false,
                    purchase_supply: None,
                })
                .collect();
            let count = side
                .units
                .iter()
                .filter(|unit| unit.type_name == kind)
                .count();
            RosterEntry {
                kind,
                count,
                count_before: None,
                investment_supply: 0,
                techs,
            }
        })
        .collect()
}

#[derive(Deserialize, Serialize)]
pub(crate) struct Catalog {
    units: BTreeMap<String, UnitAsset>,
    techs: BTreeMap<i32, TechAsset>,
    images: BTreeMap<String, String>,
}

#[derive(Deserialize, Serialize)]
struct UnitAsset {
    id: i32,
    name: String,
    portrait: String,
}

#[derive(Deserialize, Serialize)]
struct TechAsset {
    name: String,
    effect: String,
    icon: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    base_supply: Option<i32>,
}

/// A match embeds each used image once, shared by its sides and rounds.
pub(crate) fn assets<'a>(
    entries: impl Iterator<Item = &'a RosterEntry>,
) -> Result<Catalog, serde_json::Error> {
    let mut catalog: Catalog = serde_json::from_str(include_str!("../web/unit-panels.json"))?;
    let mut kinds = BTreeSet::new();
    let mut techs = BTreeSet::new();
    for entry in entries {
        kinds.insert(entry.kind.as_str());
        techs.extend(entry.techs.iter().map(|tech| tech.id));
    }
    catalog
        .units
        .retain(|kind, _| kinds.contains(kind.as_str()));
    catalog.techs.retain(|id, _| techs.contains(id));
    let economy = mechcore_document::economy::Economy::embedded()
        .map_err(<serde_json::Error as serde::de::Error>::custom)?;
    for (id, tech) in &mut catalog.techs {
        tech.base_supply = Some(economy.technology(*id).ok_or_else(|| {
            <serde_json::Error as serde::de::Error>::custom(format!(
                "technology {id} has no configured base supply price"
            ))
        })?);
    }
    let images: BTreeSet<_> = catalog
        .units
        .values()
        .map(|u| u.portrait.as_str())
        .chain(catalog.techs.values().map(|t| t.icon.as_str()))
        .collect();
    catalog
        .images
        .retain(|name, _| images.contains(name.as_str()));
    Ok(catalog)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn technology_assets_include_base_prices_for_active_and_inactive_choices() {
        let entries = [RosterEntry {
            kind: "rhino".into(),
            count: 1,
            count_before: None,
            investment_supply: 0,
            techs: vec![
                RosterTechnology {
                    id: 1109,
                    active: true,
                    activated_this_round: false,
                    purchase_supply: None,
                },
                RosterTechnology {
                    id: 905,
                    active: false,
                    activated_this_round: false,
                    purchase_supply: None,
                },
            ],
        }];
        let catalog = assets(entries.iter()).unwrap();
        let json = serde_json::to_value(catalog).unwrap();
        assert_eq!(json["techs"]["1109"]["base_supply"], 150);
        assert_eq!(json["techs"]["905"]["base_supply"], 200);
        assert_eq!(json["techs"].as_object().unwrap().len(), 2);
    }
}
