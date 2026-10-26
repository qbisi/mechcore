//! The map's committed skill deployments, read from the embedded
//! layout. Their presentation is independent of the damage or terrain.

use mechcore_document::{BattleSkillEntry, Layout, battle_skill_type_from_id};
use serde::{Deserialize, Serialize};

use crate::timeline::{centimetres, scaled};

const EFFECTS: &str = include_str!("../../../config/commander_skill_effects.yaml");

/// The shape of a committed release preview.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentGeometry {
    Line,
    Circle,
}

/// One release, in world centimetres and milliseconds. Standing oil is not
/// a new deployment. Multiple releases remain distinct even if their terrain
/// later becomes one visible surface.
#[derive(Debug, Serialize)]
pub struct Deployment {
    pub team: u32,
    pub ordinal: usize,
    pub kind: String,
    pub geometry: DeploymentGeometry,
    pub x0: i64,
    pub z0: i64,
    pub x1: i64,
    pub z1: i64,
    pub radius: i64,
    pub start_ms: i64,
    pub interval_ms: i64,
    pub count: u32,
}

#[derive(Deserialize)]
struct Effects {
    terrain_skills: Vec<SkillPresentation>,
    damage_skills: Vec<SkillPresentation>,
}

#[derive(Deserialize)]
struct SkillPresentation {
    id: i32,
    start_time: i64,
    sub_effect_interval_time: i64,
    sub_effect_count: u32,
    sub_effect_range: i64,
}

pub(crate) fn deployments(layout: Option<&Layout>) -> Result<Vec<Deployment>, serde_yaml::Error> {
    let Some(layout) = layout else {
        return Ok(Vec::new());
    };
    let effects: Effects = serde_yaml::from_str(EFFECTS)?;
    let mut result = Vec::new();
    for (team, side) in [(0, &layout.blue), (1, &layout.red)] {
        let turn = if team == 0 { 100 } else { -100 };
        for (ordinal, entry) in side.battle_skills.iter().enumerate() {
            let BattleSkillEntry::Release(release) = entry else {
                continue;
            };
            let geometry = match release.type_name.as_str() {
                "incendiary_bomb" | "sticky_oil_bomb" | "acid_blast" | "smoke_bomb"
                | "ion_blast" => DeploymentGeometry::Line,
                "missile_strike" | "heavy_missile_strike" | "nuke" | "orbital_javelin" => {
                    DeploymentGeometry::Circle
                }
                _ => continue,
            };
            let (from, to) = match (geometry, release.positions.as_slice()) {
                (DeploymentGeometry::Line, [from, to]) => (from, to),
                (DeploymentGeometry::Circle, [point]) => (point, point),
                _ => continue,
            };
            let Some(row) = effects
                .terrain_skills
                .iter()
                .chain(effects.damage_skills.iter())
                .find(|row| battle_skill_type_from_id(row.id) == Some(release.type_name.as_str()))
            else {
                continue;
            };
            result.push(Deployment {
                team,
                ordinal,
                kind: release.type_name.clone(),
                geometry,
                x0: i64::from(from.x) * turn,
                z0: i64::from(from.y) * turn,
                x1: i64::from(to.x) * turn,
                z1: i64::from(to.y) * turn,
                radius: centimetres(row.sub_effect_range),
                start_ms: scaled(row.start_time, 1000),
                interval_ms: scaled(row.sub_effect_interval_time, 1000),
                count: row.sub_effect_count,
            });
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::{DeploymentGeometry, deployments};

    #[test]
    fn single_impacts_have_circle_ranges_and_do_not_admit_other_point_categories() {
        let layout = mechcore_document::parse_embedded_yaml(
            br"
kind: layout
round: 1
blue:
  units: []
  battle_skills:
  - {name: missile_strike, positions: [{x: -35, y: 70}]}
  - {name: heavy_missile_strike, positions: [{x: 60, y: 100}]}
  - {name: nuke, positions: [{x: 0, y: 80}]}
  - {name: orbital_javelin, positions: [{x: 30, y: 70}]}
  - {name: orbital_bombardment, positions: [{x: 0, y: 0}]}
  - {name: photon_emission, positions: [{x: 0, y: 0}]}
red:
  units: []
  battle_skills:
  - {name: nuke, positions: [{x: 90, y: 40}]}
",
        )
        .unwrap();
        let releases = deployments(Some(&layout)).unwrap();
        assert_eq!(releases.len(), 5);
        let sizes: Vec<_> = releases.iter().map(|d| (d.radius, d.start_ms)).collect();
        assert_eq!(
            sizes,
            [
                (4000, 3000),
                (4000, 3000),
                (10000, 15000),
                (3000, 3000),
                (10000, 15000)
            ]
        );
        assert!(
            releases
                .iter()
                .all(|d| d.geometry == DeploymentGeometry::Circle
                    && d.x0 == d.x1
                    && d.z0 == d.z1
                    && d.count == 1
                    && d.interval_ms == 0)
        );
        assert_eq!((releases[4].x0, releases[4].z0), (-9000, -4000));
    }

    #[test]
    fn ion_preview_reads_the_damage_row_and_preserves_release_direction() {
        let layout = mechcore_document::parse_embedded_yaml(
            br"
kind: layout
round: 1
blue:
  units: []
  battle_skills:
  - {name: ion_blast, positions: [{x: -90, y: 40}, {x: 90, y: 40}]}
red:
  units: []
  battle_skills:
  - {name: ion_blast, positions: [{x: -90, y: 40}, {x: 90, y: 40}]}
",
        )
        .unwrap();
        let releases = deployments(Some(&layout)).unwrap();
        assert_eq!(releases.len(), 2);
        for d in &releases {
            assert_eq!(
                (d.radius, d.start_ms, d.interval_ms, d.count),
                (2000, 3000, 300, 60)
            );
        }
        assert_eq!((releases[0].x0, releases[0].x1), (-9000, 9000));
        assert_eq!((releases[1].x0, releases[1].x1), (9000, -9000));
        assert_eq!((releases[0].z0, releases[1].z0), (4000, -4000));
    }

    #[test]
    fn releases_keep_order_and_world_coordinates_without_replaying_standing_oil() {
        let layout = mechcore_document::parse_embedded_yaml(
            br"
kind: layout
round: 2
blue:
  units: []
  battle_skills:
  - {name: sticky_oil_bomb, standing: {control_points: [{x: -60, y: -40}, {x: 60, y: -40}]}}
  - {name: incendiary_bomb, positions: [{x: -60, y: -40}, {x: 60, y: -40}]}
  - {name: acid_blast, positions: [{x: -80, y: -20}, {x: 80, y: -20}]}
red:
  units: []
  battle_skills:
  - {name: smoke_bomb, positions: [{x: -60, y: -40}, {x: 60, y: -40}]}
  - {name: sticky_oil_bomb, positions: [{x: -20, y: -60}, {x: 20, y: -60}]}
",
        )
        .unwrap();
        let releases = deployments(Some(&layout)).unwrap();
        assert_eq!(releases.len(), 4);
        assert_eq!(releases[0].kind, "incendiary_bomb");
        assert_eq!(releases[0].ordinal, 1);
        assert_eq!(
            (
                releases[0].x0,
                releases[0].z0,
                releases[0].x1,
                releases[0].z1
            ),
            (-6000, -4000, 6000, -4000)
        );
        assert_eq!(releases[2].team, 1);
        assert_eq!(
            (
                releases[2].x0,
                releases[2].z0,
                releases[2].x1,
                releases[2].z1
            ),
            (6000, 4000, -6000, 4000)
        );
        assert_eq!(releases[3].ordinal, 1);
        assert!(
            releases
                .iter()
                .all(|d| d.radius > 0 && d.start_ms > 0 && d.count > 0)
        );
    }
}
