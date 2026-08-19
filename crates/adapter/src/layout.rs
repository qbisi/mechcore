use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Layout {
    sides: Sides,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Sides {
    blue: Side,
    red: Side,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Side {
    formations: Vec<Formation>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Formation {
    #[serde(rename = "type")]
    type_name: String,
    x: i32,
    y: i32,
    level: Option<i32>,
    rotated: Option<bool>,
    equipment: Option<i32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NativeFormation {
    Unit(i32),
    Construction(i32),
    Contraption(i32),
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Position {
    pub(crate) x: i32,
    pub(crate) y: i32,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Placement {
    pub(crate) type_name: String,
    pub(crate) native: NativeFormation,
    pub(crate) position: Position,
    pub(crate) level: Option<i32>,
    pub(crate) rotated: bool,
    pub(crate) equipment: Option<i32>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct SidePlan {
    pub(crate) formations: Vec<Placement>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Plan {
    pub(crate) blue: SidePlan,
    pub(crate) red: SidePlan,
}

const DEPLOYMENT_MIN_X: i64 = -300;
const DEPLOYMENT_MAX_X: i64 = 300;
const DEPLOYMENT_MIN_Y: i64 = -310;
const DEPLOYMENT_MAX_Y: i64 = -10;
const SHIELD_RADIUS: i64 = 70;

impl Plan {
    pub(crate) fn formation_count(&self) -> usize {
        self.blue.formations.len() + self.red.formations.len()
    }
}

pub(crate) fn compile(value: &Value) -> Result<Plan, String> {
    let layout: Layout = serde_json::from_value(value.clone())
        .map_err(|error| format!("invalid layout: {error}"))?;
    let blue = compile_side("blue", layout.sides.blue)?;
    let red = compile_side("red", layout.sides.red)?;
    validate_formation_footprints("blue", &blue.formations)?;
    validate_formation_footprints("red", &red.formations)?;
    validate_formation_collisions(&blue.formations, &red.formations)?;
    Ok(Plan { blue, red })
}

fn compile_side(side_name: &str, side: Side) -> Result<SidePlan, String> {
    Ok(SidePlan {
        formations: compile_formations(side_name, side.formations)?,
    })
}

fn compile_formations(
    side_name: &str,
    definitions: Vec<Formation>,
) -> Result<Vec<Placement>, String> {
    let placements = definitions
        .into_iter()
        .map(|formation| {
            let Formation {
                type_name,
                x,
                y,
                level,
                rotated,
                equipment,
            } = formation;
            let position = Position { x, y };
            let native = resolve_type(&type_name).ok_or_else(|| {
                format!(
                    "side {side_name} formation type {type_name:?} at ({}, {}) is unknown",
                    position.x, position.y
                )
            })?;
            match native {
                NativeFormation::Unit(unit_id) => {
                    let level = level.unwrap_or(1);
                    let rotated = rotated.unwrap_or(false);
                    if !(1..=9).contains(&level) {
                        return Err(format!(
                            "side {side_name} formation type {type_name:?} at ({}, {}) level must be 1..=9",
                            position.x, position.y
                        ));
                    }
                    if equipment.is_some_and(|id| id <= 0) {
                        return Err(format!(
                            "side {side_name} formation type {type_name:?} at ({}, {}) equipment must be a positive integer",
                            position.x, position.y
                        ));
                    }
                    Ok(Placement {
                        type_name,
                        native: NativeFormation::Unit(unit_id),
                        position,
                        level: Some(level),
                        rotated,
                        equipment,
                    })
                }
                NativeFormation::Construction(id) => {
                    reject_unit_fields(
                        side_name,
                        &type_name,
                        position,
                        level,
                        rotated,
                        equipment,
                    )?;
                    Ok(Placement {
                        type_name,
                        native: NativeFormation::Construction(id),
                        position,
                        level: None,
                        rotated: false,
                        equipment: None,
                    })
                }
                NativeFormation::Contraption(id) => {
                    reject_unit_fields(
                        side_name,
                        &type_name,
                        position,
                        level,
                        rotated,
                        equipment,
                    )?;
                    Ok(Placement {
                        type_name,
                        native: NativeFormation::Contraption(id),
                        position,
                        level: None,
                        rotated: false,
                        equipment: None,
                    })
                }
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    if !placements
        .iter()
        .any(|placement| matches!(placement.native, NativeFormation::Unit(_)))
    {
        return Err(format!(
            "side {side_name} formations must contain at least one valid unit"
        ));
    }
    Ok(placements)
}

fn validate_formation_footprints(side_name: &str, placements: &[Placement]) -> Result<(), String> {
    for placement in placements {
        match placement.native {
            NativeFormation::Contraption(10001) => {
                validate_shield_position(side_name, placement)?;
                continue;
            }
            NativeFormation::Contraption(20001) => {
                validate_missile_position(side_name, placement)?;
                continue;
            }
            _ => {}
        }
        let (width, height) = formation_footprint(placement)
            .ok_or_else(|| missing_footprint(side_name, placement))?;
        let x = i64::from(placement.position.x);
        let y = i64::from(placement.position.y);
        let min_x = x - width / 2;
        let max_x = x + width / 2;
        let min_y = y - height / 2;
        let max_y = y + height / 2;
        let required_x = grid_center_remainder(width).ok_or_else(|| {
            format!(
                "side {side_name} formation type {:?} at ({}, {}) has unsupported footprint width {width}",
                placement.type_name, placement.position.x, placement.position.y
            )
        })?;
        let required_y = grid_center_remainder(height).ok_or_else(|| {
            format!(
                "side {side_name} formation type {:?} at ({}, {}) has unsupported footprint height {height}",
                placement.type_name, placement.position.x, placement.position.y
            )
        })?;
        if x.rem_euclid(10) != required_x || y.rem_euclid(10) != required_y {
            return Err(format!(
                "side {side_name} formation type {:?} at ({}, {}) is off the native 10x10 grid: footprint {width}x{height} requires center x≡{required_x}, y≡{required_y} (mod 10)",
                placement.type_name, placement.position.x, placement.position.y,
            ));
        }
        if min_x < DEPLOYMENT_MIN_X
            || max_x > DEPLOYMENT_MAX_X
            || min_y < DEPLOYMENT_MIN_Y
            || max_y > DEPLOYMENT_MAX_Y
        {
            return Err(format!(
                "side {side_name} formation type {:?} at ({}, {}) footprint {width}x{height} exceeds round-one deployment boundary x=[{DEPLOYMENT_MIN_X},{DEPLOYMENT_MAX_X}], y=[{DEPLOYMENT_MIN_Y},{DEPLOYMENT_MAX_Y}]",
                placement.type_name, placement.position.x, placement.position.y
            ));
        }
    }
    Ok(())
}

fn validate_shield_position(side_name: &str, placement: &Placement) -> Result<(), String> {
    let min_x = DEPLOYMENT_MIN_X + SHIELD_RADIUS + 1;
    let max_x = DEPLOYMENT_MAX_X - SHIELD_RADIUS - 1;
    let min_y = DEPLOYMENT_MIN_Y + SHIELD_RADIUS + 1;
    let max_y = DEPLOYMENT_MAX_Y - SHIELD_RADIUS - 1;
    if position_within(placement.position, min_x, max_x, min_y, max_y) {
        Ok(())
    } else {
        Err(format!(
            "side {side_name} formation type {:?} at ({}, {}) places its radius-{SHIELD_RADIUS} edge outside the own-side deployment boundary: center must be within x=[{min_x},{max_x}], y=[{min_y},{max_y}]",
            placement.type_name, placement.position.x, placement.position.y
        ))
    }
}

fn validate_missile_position(side_name: &str, placement: &Placement) -> Result<(), String> {
    if position_within(
        placement.position,
        DEPLOYMENT_MIN_X,
        DEPLOYMENT_MAX_X,
        DEPLOYMENT_MIN_Y,
        DEPLOYMENT_MAX_Y,
    ) {
        Ok(())
    } else {
        Err(format!(
            "side {side_name} formation type {:?} at ({}, {}) is outside the own-side deployment boundary x=[{DEPLOYMENT_MIN_X},{DEPLOYMENT_MAX_X}], y=[{DEPLOYMENT_MIN_Y},{DEPLOYMENT_MAX_Y}]",
            placement.type_name, placement.position.x, placement.position.y
        ))
    }
}

fn position_within(position: Position, min_x: i64, max_x: i64, min_y: i64, max_y: i64) -> bool {
    let x = i64::from(position.x);
    let y = i64::from(position.y);
    (min_x..=max_x).contains(&x) && (min_y..=max_y).contains(&y)
}

fn grid_center_remainder(extent: i64) -> Option<i64> {
    match extent.rem_euclid(20) {
        0 => Some(0),
        10 => Some(5),
        _ => None,
    }
}

fn validate_formation_collisions(blue: &[Placement], red: &[Placement]) -> Result<(), String> {
    let mut world = Vec::with_capacity(blue.len() + red.len());
    world.extend(
        blue.iter()
            .filter(|placement| participates_in_collision(placement))
            .map(|placement| {
                (
                    "blue",
                    placement,
                    i64::from(placement.position.x),
                    i64::from(placement.position.y),
                )
            }),
    );
    world.extend(
        red.iter()
            .filter(|placement| participates_in_collision(placement))
            .map(|placement| {
                (
                    "red",
                    placement,
                    -i64::from(placement.position.x),
                    -i64::from(placement.position.y),
                )
            }),
    );

    for (index, &(left_side, left, left_x, left_y)) in world.iter().enumerate() {
        let (left_width, left_height) =
            formation_footprint(left).ok_or_else(|| missing_footprint(left_side, left))?;
        for &(right_side, right, right_x, right_y) in &world[index + 1..] {
            let (right_width, right_height) =
                formation_footprint(right).ok_or_else(|| missing_footprint(right_side, right))?;
            let overlaps_x = (left_x - right_x).abs() * 2 < left_width + right_width;
            let overlaps_y = (left_y - right_y).abs() * 2 < left_height + right_height;
            if overlaps_x && overlaps_y {
                return Err(format!(
                    "formations collide: {left_side} type {:?} at ({}, {}) and {right_side} type {:?} at ({}, {})",
                    left.type_name,
                    left.position.x,
                    left.position.y,
                    right.type_name,
                    right.position.x,
                    right.position.y
                ));
            }
        }
    }
    Ok(())
}

const fn participates_in_collision(placement: &Placement) -> bool {
    !matches!(
        placement.native,
        NativeFormation::Contraption(10001 | 20001)
    )
}

fn missing_footprint(side_name: &str, placement: &Placement) -> String {
    format!(
        "side {side_name} formation type {:?} at ({}, {}) has no deployment footprint",
        placement.type_name, placement.position.x, placement.position.y
    )
}

const fn formation_footprint(placement: &Placement) -> Option<(i64, i64)> {
    match placement.native {
        NativeFormation::Unit(id) => unit_footprint(id, placement.rotated),
        NativeFormation::Construction(id) => construction_footprint(id),
        NativeFormation::Contraption(id) => contraption_footprint(id),
    }
}

const fn unit_footprint(unit_id: i32, rotated: bool) -> Option<(i64, i64)> {
    let (width, height) = match unit_id {
        2 | 15 | 31 => (20, 20),
        5 | 14 | 18 | 19 | 21 | 24 | 26 => (30, 30),
        16 | 22 | 28 | 30 => (40, 20),
        1 | 3 | 4 | 23 | 27 => (40, 40),
        6 | 7 | 8 | 9 | 10 | 12 | 13 | 20 | 25 => (50, 20),
        11 => (50, 50),
        17 | 29 | 2002 => (70, 70),
        _ => return None,
    };
    Some(if rotated {
        (height, width)
    } else {
        (width, height)
    })
}

const fn construction_footprint(construction_id: i32) -> Option<(i64, i64)> {
    match construction_id {
        1 => Some((60, 10)),
        2 | 3 => Some((20, 20)),
        4 => Some((50, 10)),
        _ => None,
    }
}

const fn contraption_footprint(contraption_id: i32) -> Option<(i64, i64)> {
    match contraption_id {
        30001 => Some((30, 30)),
        _ => None,
    }
}

fn reject_unit_fields(
    side_name: &str,
    type_name: &str,
    position: Position,
    level: Option<i32>,
    rotated: Option<bool>,
    equipment: Option<i32>,
) -> Result<(), String> {
    if level.is_some() || rotated.is_some() || equipment.is_some() {
        Err(format!(
            "side {side_name} formation type {type_name:?} at ({}, {}) does not accept level, rotated, or equipment",
            position.x, position.y
        ))
    } else {
        Ok(())
    }
}

const fn resolve_type(type_name: &str) -> Option<NativeFormation> {
    match type_name.as_bytes() {
        b"fortress" => Some(NativeFormation::Unit(1)),
        b"marksman" => Some(NativeFormation::Unit(2)),
        b"vulcan" => Some(NativeFormation::Unit(3)),
        b"melting_point" => Some(NativeFormation::Unit(4)),
        b"rhino" => Some(NativeFormation::Unit(5)),
        b"wasp" => Some(NativeFormation::Unit(6)),
        b"mustang" => Some(NativeFormation::Unit(7)),
        b"steel_ball" => Some(NativeFormation::Unit(8)),
        b"fang" => Some(NativeFormation::Unit(9)),
        b"crawler" => Some(NativeFormation::Unit(10)),
        b"overlord" => Some(NativeFormation::Unit(11)),
        b"stormcaller" => Some(NativeFormation::Unit(12)),
        b"sledgehammer" => Some(NativeFormation::Unit(13)),
        b"hacker" => Some(NativeFormation::Unit(14)),
        b"arclight" => Some(NativeFormation::Unit(15)),
        b"phoenix" => Some(NativeFormation::Unit(16)),
        b"war_factory" => Some(NativeFormation::Unit(17)),
        b"wraith" => Some(NativeFormation::Unit(18)),
        b"scorpion" => Some(NativeFormation::Unit(19)),
        b"fire_badger" => Some(NativeFormation::Unit(20)),
        b"sabertooth" => Some(NativeFormation::Unit(21)),
        b"typhoon" => Some(NativeFormation::Unit(22)),
        b"sandworm" => Some(NativeFormation::Unit(23)),
        b"tarantula" => Some(NativeFormation::Unit(24)),
        b"phantom_ray" => Some(NativeFormation::Unit(25)),
        b"farseer" => Some(NativeFormation::Unit(26)),
        b"raiden" => Some(NativeFormation::Unit(27)),
        b"hound" => Some(NativeFormation::Unit(28)),
        b"abyss" => Some(NativeFormation::Unit(29)),
        b"void_eye" => Some(NativeFormation::Unit(30)),
        b"vortex" => Some(NativeFormation::Unit(31)),
        b"mountain" => Some(NativeFormation::Unit(2002)),
        b"defensive_wall" => Some(NativeFormation::Construction(1)),
        b"anti_armor_turret" => Some(NativeFormation::Construction(2)),
        b"rapid_fire_turret" => Some(NativeFormation::Construction(3)),
        b"magnetic_barrier" => Some(NativeFormation::Construction(4)),
        b"shield" => Some(NativeFormation::Contraption(10001)),
        b"missile" => Some(NativeFormation::Contraption(20001)),
        b"interceptor" => Some(NativeFormation::Contraption(30001)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn yaml_fixture(source: &str) -> Value {
        serde_yaml::from_str(source).unwrap()
    }

    #[test]
    fn compiles_unit_defaults_for_both_sides() {
        let plan = compile(&json!({
            "sides": {
                "blue": {"formations": [{"type": "marksman", "x": -20, "y": -50}]},
                "red": {"formations": [{"type": "marksman", "x": 20, "y": -180}]}
            }
        }))
        .unwrap();
        assert_eq!(plan.blue.formations[0].level, Some(1));
        assert!(!plan.blue.formations[0].rotated);
        assert_eq!(plan.red.formations[0].position, Position { x: 20, y: -180 });
        assert_eq!(plan.formation_count(), 2);
    }

    #[test]
    fn requires_nonempty_formations_for_both_sides() {
        let error = compile(&json!({
            "sides": {
                "blue": {"formations": []},
                "red": {"formations": [{"type": "marksman", "x": 0, "y": -50}]}
            }
        }))
        .unwrap_err();
        assert!(error.contains("at least one valid unit"));
    }

    #[test]
    fn rejects_fixture_whose_center_is_inside_but_footprint_crosses_boundary() {
        let value = yaml_fixture(include_str!(
            "../tests/fixtures/invalid-footprint-boundary.yaml"
        ));
        let error = compile(&value).unwrap_err();
        assert!(error.contains("sledgehammer"));
        assert!(error.contains("exceeds round-one deployment boundary"));
    }

    #[test]
    fn rejects_collision_fixture_and_names_both_units() {
        let value = yaml_fixture(include_str!(
            "../tests/fixtures/invalid-unit-collision.yaml"
        ));
        let error = compile(&value).unwrap_err();
        assert!(error.contains("formations collide"));
        assert!(error.contains("sledgehammer"));
        assert!(error.contains("crawler"));
    }

    #[test]
    fn rejects_unit_construction_collision_and_names_both_formations() {
        let value = yaml_fixture(include_str!(
            "../tests/fixtures/invalid-unit-construction-collision.yaml"
        ));
        let error = compile(&value).unwrap_err();
        assert!(error.contains("marksman"));
        assert!(error.contains("defensive_wall"));
    }

    #[test]
    fn rejects_center_that_does_not_match_its_footprint_grid_class() {
        let error = compile(&json!({
            "sides": {
                "blue": {"formations": [{"type": "marksman", "x": 5, "y": -50}]},
                "red": {"formations": [{"type": "marksman", "x": 0, "y": -50}]}
            }
        }))
        .unwrap_err();
        assert!(error.contains("off the native 10x10 grid"));
    }

    #[test]
    fn footprint_interface_covers_all_public_units_and_rotation() {
        assert_eq!(unit_footprint(2, false), Some((20, 20)));
        assert_eq!(unit_footprint(13, false), Some((50, 20)));
        assert_eq!(unit_footprint(13, true), Some((20, 50)));
        assert_eq!(unit_footprint(32, false), None);
    }

    #[test]
    fn resolves_representative_public_formation_types() {
        assert_eq!(resolve_type("marksman"), Some(NativeFormation::Unit(2)));
        assert_eq!(
            resolve_type("defensive_wall"),
            Some(NativeFormation::Construction(1))
        );
        assert_eq!(
            resolve_type("interceptor"),
            Some(NativeFormation::Contraption(30001))
        );
        assert_eq!(resolve_type("unit"), None);
    }
}
