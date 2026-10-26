//! Read-only skill slots and unequipped items held at deployment.

use std::collections::{BTreeMap, BTreeSet};

use mechcore_document::{layout, r#match::SideState};
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize)]
pub struct Arsenal {
    pub skills: Vec<SkillSlot>,
    pub equipment: Vec<HeldItem>,
}

#[derive(Debug, Serialize)]
pub struct SkillSlot {
    pub id: i32,
    pub cooldown: i32,
    pub used: bool,
    pub released: bool,
}

#[derive(Debug, Serialize)]
pub struct HeldItem {
    pub id: i32,
    pub durability: Option<i32>,
}

/// The match's completed deployment retains unused inventory and skill slots.
#[must_use]
pub fn arsenal(state: &SideState) -> Arsenal {
    let mut slots: Vec<_> = state.battle_skills.iter().collect();
    slots.sort_by_key(|slot| slot.index);
    Arsenal {
        skills: slots
            .into_iter()
            .map(|slot| SkillSlot {
                id: slot.id,
                cooldown: slot.cooldown,
                used: slot.used,
                released: slot.release.is_some(),
            })
            .collect(),
        equipment: state
            .equipment
            .iter()
            .map(|item| HeldItem {
                id: item.id,
                durability: item.durability,
            })
            .collect(),
    }
}

/// A standalone fight knows releases, not the account's unused skill slots.
pub(crate) fn released(side: &layout::Side) -> Result<Arsenal, serde_json::Error> {
    let catalog = catalog()?;
    let skills = side
        .battle_skills
        .iter()
        .filter_map(|entry| {
            let layout::BattleSkillEntry::Release(release) = entry else {
                return None;
            };
            let (&id, _) = catalog
                .skills
                .iter()
                .find(|(_, asset)| asset.kind == release.type_name)?;
            Some(SkillSlot {
                id,
                cooldown: 0,
                used: false,
                released: true,
            })
        })
        .collect();
    Ok(Arsenal {
        skills,
        equipment: Vec::new(),
    })
}

#[derive(Deserialize, Serialize)]
pub(crate) struct Catalog {
    skills: BTreeMap<i32, Asset>,
    equipment: BTreeMap<i32, Asset>,
    images: BTreeMap<String, String>,
}

#[derive(Deserialize, Serialize)]
struct Asset {
    kind: String,
    name: String,
    effect: String,
    icon: String,
}

fn catalog() -> Result<Catalog, serde_json::Error> {
    serde_json::from_str(include_str!("../web/arsenal.json"))
}

pub(crate) fn assets<'a>(
    panels: impl Iterator<Item = &'a Arsenal>,
) -> Result<Catalog, serde_json::Error> {
    let mut catalog = catalog()?;
    let mut skills = BTreeSet::new();
    let mut equipment = BTreeSet::new();
    for panel in panels {
        skills.extend(panel.skills.iter().map(|slot| slot.id));
        equipment.extend(panel.equipment.iter().map(|item| item.id));
    }
    catalog.skills.retain(|id, _| skills.contains(id));
    catalog.equipment.retain(|id, _| equipment.contains(id));
    let images: BTreeSet<_> = catalog
        .skills
        .values()
        .chain(catalog.equipment.values())
        .map(|asset| asset.icon.as_str())
        .collect();
    catalog
        .images
        .retain(|name, _| images.contains(name.as_str()));
    Ok(catalog)
}
