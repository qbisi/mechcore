//! Geometry-only purchase placement grids, using the layout compiler's checks.
//!
//! Each row is a low-bit x mask. Wide regions are split into at most 32-column
//! blocks, each with its own first-centre offset in the side's coordinates.

use serde::Serialize;

use crate::compile::{
    Placement, compile_placement_layout, grid_center_remainder, placement_footprint,
    validate_placement_collisions, validate_placement_footprints,
};
use crate::layout::{Layout, Position, Region, UnitSource};

const STEP: i64 = 10;
const WORD_BITS: usize = u32::BITS as usize;

/// One region and orientation, or a horizontal part of a wide region.
#[derive(Debug, Serialize)]
pub struct Grid {
    pub region: &'static str,
    pub rotated: bool,
    /// The centre represented by row zero, bit zero, in own-side metres.
    pub offset: Position,
    pub size_x: usize,
    pub size_y: usize,
    pub rows: Vec<u32>,
}

/// A geometric answer, independent of supply, unlocks and shop allowances.
#[derive(Debug, Serialize)]
pub struct Mask {
    pub schema: &'static str,
    pub side: &'static str,
    pub unit: String,
    pub step: i64,
    pub grids: Vec<Grid>,
}

/// Enumerates new formation centres and both orientations on an observed layout.
///
/// This does not buy a unit. Empty sides are admitted, but existing placements
/// must satisfy the same compilation checks as layouts. Round-one flanks are
/// omitted; an occupied grid is retained even when all its rows are zero.
///
/// # Errors
///
/// Returns an unknown unit, an unsupported footprint, or an invalid layout.
pub fn purchase(layout: Layout, red: bool, unit: &str) -> Result<Mask, String> {
    let spec = crate::catalog::resolve_unit_type(unit)
        .ok_or_else(|| format!("unknown unit type {unit:?}"))?;
    let footprint = spec
        .footprint
        .ok_or_else(|| format!("unit {unit:?} has no footprint"))?;
    let mut plan = compile_placement_layout(layout)?;
    let side_name = if red { "red" } else { "blue" };
    let mut result = Mask {
        schema: "mechcore.mask",
        side: side_name,
        unit: unit.to_owned(),
        step: STEP,
        grids: Vec::new(),
    };
    for (name, region) in [
        ("main", Region::Main),
        ("left_flank", Region::LeftFlank),
        ("right_flank", Region::RightFlank),
    ] {
        if plan.round == 1 && region.is_flank() {
            continue;
        }
        let ((min_x, min_y), (max_x, max_y)) = region.bounds();
        for rotated in [false, true] {
            let mut candidate = Placement {
                type_name: unit.to_owned(),
                native: spec.native,
                footprint: Some(footprint),
                position: position(min_x.midpoint(max_x), min_y.midpoint(max_y))?,
                record_position: None,
                record_rotated: None,
                index: None,
                level: None,
                exp: None,
                rotated,
                equipment: Vec::new(),
                travelling: region.is_flank(),
                source: UnitSource::Joined,
            };
            let (width, height) = placement_footprint(&candidate)
                .ok_or_else(|| format!("unit {unit:?} has no footprint"))?;
            let first_x = first_centre(min_x + width / 2, width)?;
            let first_y = first_centre(min_y + height / 2, height)?;
            let last_x = max_x - width / 2;
            let last_y = max_y - height / 2;
            if first_x > last_x || first_y > last_y {
                continue;
            }
            let size_x = usize::try_from((last_x - first_x) / STEP + 1)
                .map_err(|_| "placement grid width exceeds usize")?;
            let size_y = usize::try_from((last_y - first_y) / STEP + 1)
                .map_err(|_| "placement grid height exceeds usize")?;
            for column in (0..size_x).step_by(WORD_BITS) {
                let offset_x =
                    first_x + i64::try_from(column).map_err(|_| "grid column exceeds i64")? * STEP;
                let block_width = WORD_BITS.min(size_x - column);
                let mut rows = Vec::with_capacity(size_y);
                for y in 0..size_y {
                    let y = first_y + i64::try_from(y).map_err(|_| "grid row exceeds i64")? * STEP;
                    let mut row = 0_u32;
                    for x in 0..block_width {
                        candidate.position = position(
                            offset_x
                                + i64::try_from(x).map_err(|_| "grid column exceeds i64")? * STEP,
                            y,
                        )?;
                        if validate_placement_footprints(
                            side_name,
                            std::slice::from_ref(&candidate),
                        )
                        .is_err()
                        {
                            continue;
                        }
                        let side = if red { &mut plan.red } else { &mut plan.blue };
                        side.units.push(candidate.clone());
                        let fits = validate_placement_collisions(&plan.blue, &plan.red).is_ok();
                        let side = if red { &mut plan.red } else { &mut plan.blue };
                        side.units.pop();
                        if fits {
                            row |= 1_u32 << x;
                        }
                    }
                    rows.push(row);
                }
                result.grids.push(Grid {
                    region: name,
                    rotated,
                    offset: position(offset_x, first_y)?,
                    size_x: block_width,
                    size_y,
                    rows,
                });
            }
        }
    }
    Ok(result)
}

fn position(x: i64, y: i64) -> Result<Position, String> {
    Ok(Position {
        x: i32::try_from(x).map_err(|_| "placement x exceeds i32")?,
        y: i32::try_from(y).map_err(|_| "placement y exceeds i32")?,
    })
}

fn first_centre(minimum: i64, extent: i64) -> Result<i64, String> {
    let remainder = grid_center_remainder(extent)
        .ok_or_else(|| format!("unsupported placement footprint extent {extent}"))?;
    Ok(minimum + (remainder - minimum).rem_euclid(STEP))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::UnitPlacement;

    fn layout(round: i32, populated: bool) -> Layout {
        let units = if populated {
            "[{name: rhino, index: 0, position: {x: 5, y: -155}}]"
        } else {
            "[]"
        };
        crate::parse_embedded_yaml(
            format!(
                "kind: layout\nround: {round}\nblue: {{units: {units}}}\nred: {{units: {units}}}\n"
            )
            .as_bytes(),
        )
        .unwrap()
    }

    #[test]
    fn every_bit_agrees_with_compiling_the_added_formation() {
        let original = layout(2, true);
        for red in [false, true] {
            let mask = purchase(original.clone(), red, "steel_ball").unwrap();
            let mut accepted = 0;
            let mut refused = 0;
            for grid in mask.grids {
                assert!(grid.size_x <= 32);
                assert_eq!(grid.rows.len(), grid.size_y);
                for (y, row) in grid.rows.iter().enumerate() {
                    for x in 0..grid.size_x {
                        let mut candidate = original.clone();
                        let own = if red {
                            &mut candidate.red
                        } else {
                            &mut candidate.blue
                        };
                        own.units.push(UnitPlacement {
                            type_name: "steel_ball".into(),
                            index: 1,
                            position: Position {
                                x: grid.offset.x + i32::try_from(x).unwrap() * 10,
                                y: grid.offset.y + i32::try_from(y).unwrap() * 10,
                            },
                            record_position: None,
                            record_rotated: None,
                            level: None,
                            exp: None,
                            rotated: Some(grid.rotated),
                            equipment: Vec::new(),
                            travelling: Some(grid.region != "main"),
                            source: UnitSource::Joined,
                        });
                        let marked = row & (1 << x) != 0;
                        assert_eq!(
                            marked,
                            crate::compile_layout(candidate).is_ok(),
                            "{} {} ({x}, {y})",
                            grid.region,
                            grid.rotated
                        );
                        if marked {
                            accepted += 1;
                        } else {
                            refused += 1;
                        }
                    }
                }
            }
            assert!(accepted > 0 && refused > 0);
        }
    }

    #[test]
    fn empty_deployment_tiles_wide_rows_and_preserves_rotation_offsets() {
        let mask = purchase(layout(1, false), false, "steel_ball").unwrap();
        assert!(mask.grids.iter().all(|g| g.region == "main"));
        assert!(
            mask.grids
                .iter()
                .any(|g| g.size_x == 32 && g.rows.iter().any(|r| r & (1 << 31) != 0))
        );
        assert_eq!(
            mask.grids
                .iter()
                .find(|g| !g.rotated)
                .unwrap()
                .offset
                .x
                .rem_euclid(10),
            5
        );
        assert_eq!(
            mask.grids
                .iter()
                .find(|g| g.rotated)
                .unwrap()
                .offset
                .x
                .rem_euclid(10),
            0
        );
        assert!(purchase(layout(1, false), true, "missing_unit").is_err());
        // An empty deployment is a mask input, not a fight that can execute.
        assert!(crate::compile_layout(layout(1, false)).is_err());
    }

    #[test]
    fn shield_does_not_take_placement_space() {
        let empty = layout(1, false);
        let mut shielded = empty.clone();
        shielded
            .blue
            .contraptions
            .push(crate::ContraptionPlacement {
                type_name: "shield".into(),
                index: 0,
                position: Position { x: 0, y: -150 },
            });
        assert_eq!(
            serde_json::to_value(purchase(empty, false, "rhino").unwrap()).unwrap(),
            serde_json::to_value(purchase(shielded, false, "rhino").unwrap()).unwrap(),
        );
    }
}
