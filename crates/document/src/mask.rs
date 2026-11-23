//! Geometry-only purchase placement grids, using the layout compiler's checks.
//!
//! Each row is a low-bit x mask. Wide regions are split into at most 32-column
//! blocks, each with its own first-centre offset in the side's coordinates.

use std::collections::BTreeSet;

use serde::Serialize;

use crate::compile::{
    Placement, compile_placement_layout, grid_center_remainder, placement_footprint,
    validate_placement_collisions, validate_placement_footprints,
};
use crate::landing::{Rect, bounds as region_bounds, region_obstacles_excluding, unit_size};
use crate::layout::{Layout, Position, Region, UnitSource};
use crate::r#match::SideState;

const STEP: i64 = 10;
const WORD_BITS: usize = u32::BITS as usize;

/// One region and orientation, or a horizontal part of a wide region.
#[derive(Clone, Debug, Serialize)]
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
#[derive(Clone, Debug, Serialize)]
pub struct Mask {
    pub schema: &'static str,
    pub side: &'static str,
    pub unit: String,
    pub step: i64,
    pub grids: Vec<Grid>,
}

impl Mask {
    /// Whether `(position, rotated)` is set in this placement mask.
    #[must_use]
    pub fn contains(&self, position: Position, rotated: bool) -> bool {
        let region_name = match Region::of(position) {
            Region::Main => "main",
            Region::LeftFlank => "left_flank",
            Region::RightFlank => "right_flank",
        };
        let px = i64::from(position.x);
        let py = i64::from(position.y);
        for grid in &self.grids {
            if grid.region != region_name || grid.rotated != rotated {
                continue;
            }
            let ox = i64::from(grid.offset.x);
            let oy = i64::from(grid.offset.y);
            if px < ox || py < oy {
                continue;
            }
            let dx = px - ox;
            let dy = py - oy;
            if dx % self.step != 0 || dy % self.step != 0 {
                continue;
            }
            let Ok(col) = usize::try_from(dx / self.step) else {
                continue;
            };
            let Ok(row) = usize::try_from(dy / self.step) else {
                continue;
            };
            if col < grid.size_x && row < grid.size_y {
                return (grid.rows[row] & (1_u32 << col)) != 0;
            }
        }
        false
    }

    /// Finds the closest legal position to `target` in `region` that satisfies
    /// `predicate`, trying `preferred_rotated` first and optionally falling back
    /// to `!preferred_rotated`. Ties in squared distance break by smallest
    /// `y.abs()` (closer to the frontline), then lexicographically by `(x, y)`.
    #[must_use]
    pub fn nearest_in_region(
        &self,
        region: &str,
        target: Position,
        preferred_rotated: bool,
        allow_flip: bool,
        predicate: impl Fn(Position) -> bool,
    ) -> Option<(Position, bool)> {
        if let Some(pos) = self.nearest_for_rotation(region, target, preferred_rotated, &predicate)
        {
            return Some((pos, preferred_rotated));
        }
        if allow_flip {
            return self
                .nearest_for_rotation(region, target, !preferred_rotated, &predicate)
                .map(|pos| (pos, !preferred_rotated));
        }
        None
    }

    fn nearest_for_rotation(
        &self,
        region: &str,
        target: Position,
        rotated: bool,
        predicate: &impl Fn(Position) -> bool,
    ) -> Option<Position> {
        let mut best: Option<((i128, i64, i64, i64), Position)> = None;
        for grid in &self.grids {
            if grid.region != region || grid.rotated != rotated {
                continue;
            }
            for (row_idx, &word_init) in grid.rows.iter().enumerate() {
                let Ok(row_i64) = i64::try_from(row_idx) else {
                    continue;
                };
                let Ok(y) = i32::try_from(i64::from(grid.offset.y) + row_i64 * self.step) else {
                    continue;
                };
                let mut word = word_init;
                while word != 0 {
                    let col = word.trailing_zeros();
                    word &= word - 1;
                    let Ok(x) =
                        i32::try_from(i64::from(grid.offset.x) + i64::from(col) * self.step)
                    else {
                        continue;
                    };
                    let pos = Position { x, y };
                    if !predicate(pos) {
                        continue;
                    }
                    let dx = i128::from(pos.x) - i128::from(target.x);
                    let dy = i128::from(pos.y) - i128::from(target.y);
                    let key = (
                        dx * dx + dy * dy,
                        i64::from(pos.y).abs(),
                        i64::from(pos.x),
                        i64::from(pos.y),
                    );
                    if best.is_none_or(|(best_key, _)| key < best_key) {
                        best = Some((key, pos));
                    }
                }
            }
        }
        best.map(|(_, pos)| pos)
    }
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

/// Enumerates legal move destinations for standing formation `unit_index` on
/// `state`, excluding that formation's own current footprint from collisions.
///
/// Round-one flanks are omitted. Every set bit agrees with
/// `transition::check_place` for `Action::MoveUnit`.
///
/// # Errors
///
/// Returns an error when `unit_index` is absent or its type has no footprint.
pub fn move_unit(
    state: &SideState,
    round: i32,
    red: bool,
    unit_index: i32,
) -> Result<Mask, String> {
    move_unit_excluding(state, round, red, unit_index, &BTreeSet::new())
}

/// Enumerates legal move destinations for standing formation `unit_index` on
/// `state`, excluding `unit_index` and every formation in `ignored_units`
/// from collisions.
///
/// Round-one flanks are omitted.
///
/// # Errors
///
/// Returns an error when `unit_index` is absent or its type has no footprint.
pub fn move_unit_excluding(
    state: &SideState,
    round: i32,
    red: bool,
    unit_index: i32,
    ignored_units: &BTreeSet<i32>,
) -> Result<Mask, String> {
    let formation = state
        .active_units()
        .find(|entry| entry.index == unit_index)
        .ok_or_else(|| format!("formation {unit_index} is absent from state"))?;
    let unit = &formation.type_name;
    let side_name = if red { "red" } else { "blue" };
    let mut ignored = ignored_units.clone();
    ignored.insert(unit_index);
    let mut result = Mask {
        schema: "mechcore.mask",
        side: side_name,
        unit: unit.clone(),
        step: STEP,
        grids: Vec::new(),
    };
    for (name, region) in [
        ("main", Region::Main),
        ("left_flank", Region::LeftFlank),
        ("right_flank", Region::RightFlank),
    ] {
        if round == 1 && region.is_flank() {
            continue;
        }
        let ((min_x, min_y), (max_x, max_y)) = region.bounds();
        let region_rect = region_bounds(region);
        let obstacles = region_obstacles_excluding(state, &ignored, region);
        for rotated in [false, true] {
            let (width, height) = unit_size(unit, rotated, region)
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
                        let cx = offset_x
                            + i64::try_from(x).map_err(|_| "grid column exceeds i64")? * STEP;
                        let rect = Rect::around((cx, y), (width, height));
                        if rect.within(region_rect)
                            && !obstacles.iter().any(|obstacle| obstacle.overlaps(rect))
                        {
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

    #[test]
    fn move_unit_excludes_self_and_agrees_with_check_place() {
        use crate::r#match::{Action, StateUnit};
        let mut state = SideState::default();
        state.units.push(StateUnit {
            type_name: "marksman".into(),
            index: 0,
            value: Some(100),
            level: Some(1),
            source: UnitSource::Joined,
            recovered: false,
            position: Some(Position { x: 0, y: -150 }),
            exp: None,
            rotated: None,
            travelling: None,
            record_position: None,
            record_rotated: None,
        });
        state.units.push(StateUnit {
            type_name: "crawler".into(),
            index: 1,
            value: Some(100),
            level: Some(1),
            source: UnitSource::Joined,
            recovered: false,
            position: Some(Position { x: 0, y: -100 }),
            exp: None,
            rotated: None,
            travelling: None,
            record_position: None,
            record_rotated: None,
        });
        let mask = move_unit(&state, 1, false, 0).unwrap();
        assert!(mask.grids.iter().all(|g| g.region == "main"));
        // Own current position must be legal because unit 0 is excluded from its own obstacles.
        assert!(mask.contains(Position { x: 0, y: -150 }, false));
        // Neighbor unit 1's position must be blocked.
        assert!(!mask.contains(Position { x: 0, y: -100 }, false));
        for grid in &mask.grids {
            for row in 0..grid.size_y {
                for col in 0..grid.size_x {
                    let pos = Position {
                        x: grid.offset.x + i32::try_from(col).unwrap() * 10,
                        y: grid.offset.y + i32::try_from(row).unwrap() * 10,
                    };
                    let bit = (grid.rows[row] & (1_u32 << col)) != 0;
                    assert_eq!(mask.contains(pos, grid.rotated), bit);
                    let action = Action::MoveUnit {
                        index: 0,
                        position: pos,
                        rotated: grid.rotated,
                    };
                    assert_eq!(
                        crate::transition::check_place(&state, &action).is_ok(),
                        bit,
                        "disagreement at ({}, {}) rot={}",
                        pos.x,
                        pos.y,
                        grid.rotated
                    );
                }
            }
        }
    }

    fn test_unit(index: i32, type_name: &str, x: i32, y: i32) -> crate::r#match::StateUnit {
        crate::r#match::StateUnit {
            type_name: type_name.to_owned(),
            index,
            value: Some(100),
            level: Some(1),
            source: UnitSource::Joined,
            recovered: false,
            position: Some(Position { x, y }),
            exp: None,
            rotated: None,
            travelling: None,
            record_position: None,
            record_rotated: None,
        }
    }

    #[test]
    fn move_unit_excluding_clears_ignored_units_only() {
        let mut state = SideState::default();
        state.units.push(test_unit(0, "marksman", 0, -150));
        state.units.push(test_unit(1, "crawler", 0, -100));
        state.units.push(test_unit(2, "arclight", 50, -150));

        let ignored = BTreeSet::from([1]);
        let mask = move_unit_excluding(&state, 1, false, 0, &ignored).unwrap();
        assert!(mask.contains(Position { x: 0, y: -100 }, false));
        assert!(!mask.contains(Position { x: 50, y: -150 }, false));
    }

    #[test]
    fn nearest_in_region_picks_closest_cell_and_flips_when_blocked() {
        let mut state = SideState::default();
        state.units.push(test_unit(0, "crawler", -200, -150));
        state.units.push(test_unit(1, "marksman", -220, -100));
        state.units.push(test_unit(2, "marksman", -180, -100));

        let mask = move_unit(&state, 1, false, 0).unwrap();
        // Crawler (50x20 unrotated) aligns x centers to 5 mod 10 and y centers to 0 mod 10.
        let (pos, rot) = mask
            .nearest_in_region("main", Position { x: 2, y: -153 }, false, true, |_| true)
            .unwrap();
        assert_eq!(pos, Position { x: 5, y: -150 });
        assert!(!rot);

        // Restricting x == -200 leaves zero unrotated cells (width 50 requires x % 10 == 5),
        // so allow_flip=true falls back to rotated=true (20x50, x % 10 == 0, y % 10 == 5).
        let flipped = mask
            .nearest_in_region("main", Position { x: -200, y: -100 }, false, true, |p| {
                p.x == -200
            })
            .unwrap();
        assert_eq!(flipped, (Position { x: -200, y: -95 }, true));
        assert!(
            mask.nearest_in_region("main", Position { x: -200, y: -100 }, false, false, |p| {
                p.x == -200
            })
            .is_none()
        );
    }
}
