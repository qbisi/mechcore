//! Deployment activation states and build-configured names for top HUD cards.

use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize)]
pub struct TowerCards {
    pub energy: Vec<TowerChoice>,
    pub blueprints: Vec<TowerChoice>,
}

#[derive(Debug, Serialize)]
pub struct TowerChoice {
    pub id: i32,
    pub name: String,
    pub active: bool,
    /// Activation cost, or the supply granted by Rapid Resupply.
    pub supply: i32,
}

#[derive(Deserialize)]
struct Names {
    energy_tower_skills: Vec<ChoiceName>,
    blueprints: Vec<ChoiceName>,
}

#[derive(Deserialize)]
struct ChoiceName {
    id: i32,
    name: String,
    supply: i32,
    #[serde(default)]
    granted: i32,
}

/// Missing activations, including layout-omitted economic skills, default off.
///
/// # Errors
/// Returns malformed embedded economy configuration.
pub fn tower_cards(energy: &[i32], blueprints: &[i32]) -> Result<TowerCards, serde_yaml::Error> {
    let names: Names = serde_yaml::from_str(include_str!("../../../config/economy.yaml"))?;
    let choices = |rows: Vec<ChoiceName>, held: &[i32]| {
        rows.into_iter()
            .map(|row| TowerChoice {
                id: row.id,
                active: held.contains(&row.id),
                name: row.name,
                supply: if row.granted == 0 {
                    row.supply
                } else {
                    row.granted
                },
            })
            .collect()
    };
    Ok(TowerCards {
        energy: choices(names.energy_tower_skills, energy),
        blueprints: choices(names.blueprints, blueprints),
    })
}
