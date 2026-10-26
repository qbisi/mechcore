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
    Path,
}

impl DeploymentGeometry {
    fn for_skill(kind: &str) -> Option<Self> {
        Some(match kind {
            "incendiary_bomb" | "sticky_oil_bomb" | "acid_blast" | "smoke_bomb" | "ion_blast" => {
                Self::Line
            }
            "missile_strike"
            | "heavy_missile_strike"
            | "nuke"
            | "orbital_javelin"
            | "orbital_bombardment"
            | "lightning_storm"
            | "underground_threat"
            | "rhino_assault"
            | "wasp_swarm"
            | "mobilize_battleship"
            | "vulcans_descent"
            | "electromagnetic_impact"
            | "electromagnetic_blast"
            | "photon_emission"
            | "shield_airdrop" => Self::Circle,
            "mobile_beacon" | "mobile_beacon_card" => Self::Path,
            _ => return None,
        })
    }
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
    /// Ordered control points for a persistent waypoint path.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub path: Vec<[i64; 2]>,
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
#[allow(
    clippy::struct_field_names,
    reason = "fields name the extracted configuration tables"
)]
struct Effects {
    terrain_skills: Vec<SkillPresentation>,
    damage_skills: Vec<DamageSkillPresentation>,
    support_skills: Vec<PointSkillPresentation>,
    buff_skills: Vec<PointSkillPresentation>,
    shield_skills: Vec<PointSkillPresentation>,
    waypoint_skills: Vec<WaypointPresentation>,
}

#[derive(Deserialize)]
struct WaypointPresentation {
    id: i32,
    sub_effect_range: i64,
}

#[derive(Deserialize)]
struct PointSkillPresentation {
    id: i32,
    start_time: i64,
    effect_range: i64,
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
    // CS_SupportUnit.GetCountDownEffectLifeTime adds the native creator's
    // one-second APPEAR_DURATION to its configured start time.
    let points: Vec<_> = effects
        .support_skills
        .iter()
        .map(|row| (row, 1_i64 << 32))
        .chain(effects.buff_skills.iter().map(|row| (row, 0)))
        .chain(effects.shield_skills.iter().map(|row| (row, 0)))
        .map(|(row, appearance_delay)| SkillPresentation {
            id: row.id,
            start_time: row.start_time + appearance_delay,
            sub_effect_interval_time: 0,
            sub_effect_count: 1,
            sub_effect_range: row.effect_range,
        })
        .collect();
    let waypoints: Vec<_> = effects
        .waypoint_skills
        .iter()
        .map(|row| SkillPresentation {
            id: row.id,
            start_time: 0,
            sub_effect_interval_time: 0,
            sub_effect_count: 1,
            sub_effect_range: row.sub_effect_range,
        })
        .collect();
    let mut result = Vec::new();
    for (team, side) in [(0, &layout.blue), (1, &layout.red)] {
        let turn = if team == 0 { 100 } else { -100 };
        for (ordinal, entry) in side.battle_skills.iter().enumerate() {
            let BattleSkillEntry::Release(release) = entry else {
                continue;
            };
            let Some(geometry) = DeploymentGeometry::for_skill(&release.type_name) else {
                continue;
            };
            let (from, to) = match (geometry, release.positions.as_slice()) {
                (DeploymentGeometry::Line, [from, to])
                | (DeploymentGeometry::Path, [from, _, to]) => (from, to),
                (DeploymentGeometry::Circle, [point]) => (point, point),
                _ => continue,
            };
            let Some((row, radius)) = effects
                .terrain_skills
                .iter()
                .map(|row| (row, row.sub_effect_range))
                .chain(effects.damage_skills.iter().map(|row| {
                    let radius = match geometry {
                        DeploymentGeometry::Line | DeploymentGeometry::Path => {
                            row.skill.sub_effect_range
                        }
                        DeploymentGeometry::Circle => row.effect_range,
                    };
                    (&row.skill, radius)
                }))
                .chain(points.iter().map(|row| (row, row.sub_effect_range)))
                .chain(waypoints.iter().map(|row| (row, row.sub_effect_range)))
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
                path: if geometry == DeploymentGeometry::Path {
                    release
                        .positions
                        .iter()
                        .map(|p| [i64::from(p.x) * turn, i64::from(p.y) * turn])
                        .collect()
                } else {
                    Vec::new()
                },
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
    fn beacon_variants_preserve_three_ordered_points_in_world_coordinates() {
        let layout = mechcore_document::parse_embedded_yaml(
            br"
kind: layout
round: 2
blue:
  units: []
  battle_skills:
  - {name: mobile_beacon, positions: [{x: -100, y: -200}, {x: 50, y: 0}, {x: 50, y: 100}]}
red:
  units: []
  battle_skills:
  - {name: mobile_beacon_card, positions: [{x: 60, y: -120}, {x: 60, y: -120}, {x: -20, y: 80}]}
",
        )
        .unwrap();
        let releases = deployments(Some(&layout)).unwrap();
        assert_eq!(releases.len(), 2);
        assert!(releases.iter().all(|d| {
            d.geometry == DeploymentGeometry::Path
                && d.radius == 4000
                && d.start_ms == 0
                && d.interval_ms == 0
                && d.forecast.is_none()
        }));
        assert_eq!(
            releases[0].path,
            [[-10000, -20000], [5000, 0], [5000, 10000]]
        );
        assert_eq!(
            releases[1].path,
            [[-6000, 12000], [-6000, 12000], [2000, -8000]]
        );
    }

    #[test]
    fn buff_and_shield_previews_use_release_ranges_without_buff_lifetimes() {
        let layout = mechcore_document::parse_embedded_yaml(
            br"
kind: layout
round: 2
blue:
  units: []
  battle_skills:
  - {name: electromagnetic_impact, positions: [{x: -40, y: 60}]}
  - {name: electromagnetic_blast, positions: [{x: 40, y: 60}]}
  - {name: photon_emission, positions: [{x: 0, y: -110}]}
red:
  units: []
  battle_skills:
  - {name: shield_airdrop, positions: [{x: 30, y: -80}]}
  - {name: shield_airdrop, standing: {position: {x: -80, y: -100}}}
",
        )
        .unwrap();
        let releases = deployments(Some(&layout)).unwrap();
        assert_eq!(
            releases
                .iter()
                .map(|d| (d.radius, d.start_ms))
                .collect::<Vec<_>>(),
            [(6000, 2700), (13000, 2700), (11000, 600), (7000, 1000)]
        );
        assert!(releases.iter().all(|d| {
            d.geometry == DeploymentGeometry::Circle
                && (d.x0, d.z0) == (d.x1, d.z1)
                && d.count == 1
                && d.interval_ms == 0
                && d.forecast.is_none()
        }));
        assert_eq!((releases[0].x0, releases[0].z0), (-4000, 6000));
        assert_eq!(
            (releases[3].team, releases[3].x0, releases[3].z0),
            (1, -3000, 8000)
        );
    }

    #[test]
    fn summons_use_their_deployment_ranges_and_creator_appearance_delay() {
        let layout = mechcore_document::parse_embedded_yaml(
            br"
kind: layout
round: 1
blue:
  units: []
  battle_skills:
  - {name: underground_threat, positions: [{x: 0, y: 70}]}
  - {name: rhino_assault, positions: [{x: 0, y: 70}]}
  - {name: wasp_swarm, positions: [{x: 0, y: 70}]}
  - {name: mobilize_battleship, positions: [{x: 0, y: 70}]}
  - {name: vulcans_descent, positions: [{x: 0, y: 70}]}
red: {units: []}
",
        )
        .unwrap();
        let releases = deployments(Some(&layout)).unwrap();
        assert_eq!(
            releases.iter().map(|d| d.radius).collect::<Vec<_>>(),
            [3200, 2000, 2500, 2500, 2500]
        );
        assert!(
            releases
                .iter()
                .all(|d| d.geometry == DeploymentGeometry::Circle
                    && d.start_ms == 2300
                    && d.count == 1
                    && d.interval_ms == 0)
        );
    }

    #[test]
    fn single_impacts_have_circle_ranges() {
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
