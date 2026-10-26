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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub forecast: Option<StrikeForecast>,
}

/// Planned ground arrivals reconstructed by the simulator, not MCFR events.
#[derive(Debug, Serialize)]
pub struct StrikeForecast {
    pub radius: i64,
    pub points: Vec<StrikePoint>,
}

/// One planned impact in world centimetres at its simulation tick.
#[derive(Debug, Serialize)]
pub struct StrikePoint {
    pub tick: u64,
    pub x: i64,
    pub z: i64,
}

impl Deployment {
    /// Attaches a simulator plan, converting Q32.32 metres to page centimetres.
    pub fn set_strike_forecast(
        &mut self,
        radius: i64,
        points: impl IntoIterator<Item = (u64, mechcore_mcfr::QVec3)>,
    ) {
        self.forecast = Some(StrikeForecast {
            radius: centimetres(radius),
            points: points
                .into_iter()
                .map(|(tick, point)| StrikePoint {
                    tick,
                    x: centimetres(point.x),
                    z: centimetres(point.z),
                })
                .collect(),
        });
    }
}

#[derive(Deserialize)]
struct Effects {
    terrain_skills: Vec<SkillPresentation>,
    damage_skills: Vec<DamageSkillPresentation>,
}

#[derive(Deserialize)]
struct DamageSkillPresentation {
    effect_range: i64,
    #[serde(flatten)]
    skill: SkillPresentation,
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
                "missile_strike"
                | "heavy_missile_strike"
                | "nuke"
                | "orbital_javelin"
                | "orbital_bombardment"
                | "lightning_storm" => DeploymentGeometry::Circle,
                _ => continue,
            };
            let (from, to) = match (geometry, release.positions.as_slice()) {
                (DeploymentGeometry::Line, [from, to]) => (from, to),
                (DeploymentGeometry::Circle, [point]) => (point, point),
                _ => continue,
            };
            let Some((row, radius)) = effects
                .terrain_skills
                .iter()
                .map(|row| (row, row.sub_effect_range))
                .chain(effects.damage_skills.iter().map(|row| {
                    let radius = match geometry {
                        DeploymentGeometry::Line => row.skill.sub_effect_range,
                        DeploymentGeometry::Circle => row.effect_range,
                    };
                    (&row.skill, radius)
                }))
                .find(|(row, _)| {
                    battle_skill_type_from_id(row.id) == Some(release.type_name.as_str())
                })
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
                radius: centimetres(radius),
                start_ms: scaled(row.start_time, 1000),
                interval_ms: scaled(row.sub_effect_interval_time, 1000),
                count: row.sub_effect_count,
                forecast: None,
            });
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::{DeploymentGeometry, deployments};

    #[test]
    fn single_impacts_have_circle_ranges_and_do_not_admit_buff_categories() {
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
  - {name: electromagnetic_blast, positions: [{x: 0, y: 0}]}
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
    fn random_strikes_use_the_outer_footprint_and_preserve_their_release_side() {
        let layout = mechcore_document::parse_embedded_yaml(
            br"
kind: layout
round: 1
blue:
  units: []
  battle_skills:
  - {name: orbital_bombardment, positions: [{x: -25, y: 40}]}
red:
  units: []
  battle_skills:
  - {name: lightning_storm, positions: [{x: 90, y: 70}]}
",
        )
        .unwrap();
        let releases = deployments(Some(&layout)).unwrap();
        assert_eq!(releases.len(), 2);
        for release in &releases {
            assert_eq!(release.geometry, DeploymentGeometry::Circle);
            // The preview covers the entire random strike, not one 30 m hit.
            assert_eq!(release.radius, 13000);
            assert_eq!(release.start_ms, 3000);
            assert_eq!((release.x0, release.z0), (release.x1, release.z1));
        }
        assert_eq!((releases[0].count, releases[0].interval_ms), (15, 1000));
        assert_eq!((releases[1].count, releases[1].interval_ms), (30, 400));
        assert_eq!(
            (releases[0].team, releases[0].x0, releases[0].z0),
            (0, -2500, 4000)
        );
        assert_eq!(
            (releases[1].team, releases[1].x0, releases[1].z0),
            (1, -9000, -7000)
        );
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
