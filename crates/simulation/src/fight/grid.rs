//! `GridBlockInt`: a terrain cut into five-metre cells, as a battlefield
//! shield leaves it.
//!
//! A circle's cells are a template the build caches by radius
//! (`FightCacheData.GetGridBlockInt`): the cells of a circle centred at
//! `(r, r)` whose centre the circle contains. A grid of the circle where it
//! stands is that template placed on the world's five-metre lattice
//! (`GridBlockInt.Create`), its first cell where `CalculateGridDataForCheck`
//! rounds the circle's lower corner to. Comparing two grids shifts one into
//! the other's cells (`ConvertToLocalSpace`). `docs/rules/terrain.md` states
//! the rule.

use mechcore_mcfr::TerrainGridState;

use super::{fpoint_less_or_equal, native_q32_magnitude, q32_div, q32_mul};
use crate::{Error, Result};

/// `RangeItemEffectLayerGrid.gridSize`, five metres.
const GRID_SIZE: i64 = 5 << 32;
/// `gridHalfSize`: `gridSize` times `FPoint.C0_5`.
const GRID_HALF_SIZE: i64 = GRID_SIZE / 2;
/// `GridBlockInt.MAX_SIZE_HALF`: sixteen cells. A circle this wide or wider
/// is compared as a `GridBlockLong`, which is not read.
const MAX_SIZE_HALF: i64 = GRID_SIZE << 4;
/// A grid's 32 rows of 32 cells.
const ROWS: usize = 32;

/// A `GridBlockInt`: the centre of its first cell, its size in cells, and its
/// rows, `grids[y]` holding cell `x` at bit `31 - x`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::fight) struct GridBlock {
    position_x: i64,
    position_y: i64,
    size: (u32, u32),
    rows: [u32; ROWS],
}

/// A circle: its centre and radius, Q32.32 metres.
pub(in crate::fight) type Circle = (i64, i64, i64);

/// `FPoint.RoundToInt`: to the nearest whole, a half to the even one.
fn round_to_int(value: i64) -> i64 {
    let (whole, fraction) = (value >> 32, value & 0xFFFF_FFFF);
    match fraction.cmp(&0x8000_0000) {
        std::cmp::Ordering::Less => whole,
        std::cmp::Ordering::Greater => whole + 1,
        std::cmp::Ordering::Equal => whole + (whole & 1),
    }
}

/// `GridBlockInt.CalculateGridDataForCheck`: the centre of the first cell,
/// the circle's lower corner rounded to the lattice, and how many cells span
/// the circle.
fn grid_data((x, y, radius): Circle) -> (i64, i64, usize) {
    let corner = |centre: i64| {
        q32_mul(
            round_to_int(q32_div(centre.saturating_sub(radius), GRID_SIZE)) << 32,
            GRID_SIZE,
        )
        .saturating_add(GRID_HALF_SIZE)
    };
    let count = (q32_div(radius.saturating_mul(2), GRID_SIZE) + (1 << 32)) >> 32;
    (
        corner(x),
        corner(y),
        usize::try_from(count).unwrap_or_default(),
    )
}

/// `FightRange.Overlaps` of a circle and a rectangle: the rectangle's point
/// nearest the circle's centre strictly inside the circle.
fn circle_overlaps_rect((x, y, radius): Circle, (centre_x, centre_y, width, height): Rect) -> bool {
    let (half_width, half_height) = (q32_div(width, 2 << 32), q32_div(height, 2 << 32));
    let nearest_x = x.min(centre_x + half_width).max(centre_x - half_width);
    let nearest_y = y.min(centre_y + half_height).max(centre_y - half_height);
    let (dx, dy) = (nearest_x - x, nearest_y - y);
    q32_mul(dx, dx) + q32_mul(dy, dy) < q32_mul(radius, radius)
}

/// A `RectRange`: its centre and its size.
type Rect = (i64, i64, i64, i64);

impl GridBlock {
    /// `GridBlockInt..ctor`, the template `FightCacheData` keeps for a
    /// radius: the circle centred at `(r, r)`, each cell set whose centre it
    /// contains, and the size the set cells span.
    fn template(radius: i64) -> Result<Self> {
        let (position_x, position_y, count) = grid_data((radius, radius, radius));
        if count > ROWS {
            return Err(Error::new(
                "a terrain wider than a grid's 32 cells is not read".to_owned(),
            ));
        }
        let mut grid = Self {
            position_x,
            position_y,
            size: (0, 0),
            rows: [0; ROWS],
        };
        for (y, row) in grid.rows.iter_mut().enumerate().take(count) {
            let cell_y = position_y + (i64::try_from(y).unwrap_or_default() << 32) * 5;
            let mut any = false;
            for x in 0..count {
                let cell_x = position_x + (i64::try_from(x).unwrap_or_default() << 32) * 5;
                if fpoint_less_or_equal(
                    native_q32_magnitude(cell_x - radius, cell_y - radius),
                    radius,
                ) {
                    *row |= 1 << (31 - x);
                    grid.size.0 = grid.size.0.max(u32::try_from(x).unwrap_or_default());
                    any = true;
                }
            }
            if any {
                grid.size.1 = grid.size.1.max(u32::try_from(y).unwrap_or_default());
            }
        }
        grid.size = (grid.size.0 + 1, grid.size.1 + 1);
        Ok(grid)
    }

    /// `FightCacheData.GetGridBlockInt` then `GridBlockInt.Create`: the
    /// template of a circle's radius, placed where the circle stands.
    pub(in crate::fight) fn of_circle(circle: Circle) -> Result<Self> {
        let template = Self::template(circle.2)?;
        let (position_x, position_y, _) = grid_data(circle);
        Ok(Self {
            position_x,
            position_y,
            ..template
        })
    }

    /// The grid a recording's rows leave of a circle's
    /// (`GridBlockInt.Sync`): its cells the recording does not hold go.
    pub(in crate::fight) fn of_circle_holding(circle: Circle, recorded: &[u32]) -> Result<Self> {
        let mut grid = Self::of_circle(circle)?;
        let mut held = [0_u32; ROWS];
        for (x, row) in recorded.iter().enumerate().take(ROWS) {
            for (y, held) in held.iter_mut().enumerate() {
                if row & (1 << y) != 0 {
                    *held |= 1 << (31 - x);
                }
            }
        }
        for (row, held) in grid.rows.iter_mut().zip(held) {
            *row &= held;
        }
        Ok(grid)
    }

    /// `GridBlockInt.CalculateBounds`.
    fn bounds(&self) -> Rect {
        let width = q32_mul(i64::from(self.size.0) << 32, GRID_SIZE);
        let height = q32_mul(i64::from(self.size.1) << 32, GRID_SIZE);
        (
            self.position_x + width / 2,
            self.position_y + height / 2,
            width,
            height,
        )
    }

    /// `GridBlockInt.ConvertToLocalSpace`: `target`'s cells shifted onto
    /// this grid's. A column shift is masked to five bits, as C#'s is.
    fn convert_to_local(&self, target: &mut Self) {
        let cells = |from: i64, to: i64| q32_div(to - from, GRID_SIZE) >> 32;
        let dx = cells(self.position_x, target.position_x);
        let shift = u32::try_from(dx.unsigned_abs() & 31).unwrap_or_default();
        for row in &mut target.rows {
            *row = if dx < 0 { *row << shift } else { *row >> shift };
        }
        let dy = cells(self.position_y, target.position_y);
        let shifted = target.rows;
        for (index, row) in target.rows.iter_mut().enumerate() {
            let from = i64::try_from(index).unwrap_or_default() - dy;
            *row = usize::try_from(from)
                .ok()
                .and_then(|from| shifted.get(from))
                .copied()
                .unwrap_or(0);
        }
    }

    /// A circle's grid shifted onto this one's cells, when the circle comes
    /// near enough to have any.
    fn local_circle(&self, circle: Circle) -> Result<Option<Self>> {
        if !circle_overlaps_rect(circle, self.bounds()) {
            return Ok(None);
        }
        // `FPoint.op_GreaterThanOrEqual`, which counts 43 raw short as equal.
        if circle.2.saturating_add(43) >= MAX_SIZE_HALF {
            return Err(Error::new(
                "a circle of 80 metres or more meets a terrain's grid, which is not read"
                    .to_owned(),
            ));
        }
        let mut target = Self::of_circle(circle)?;
        self.convert_to_local(&mut target);
        Ok(Some(target))
    }

    /// `GridBlockInt.TryDisableGrid`: the cells a shield's own grid covers go.
    pub(in crate::fight) fn disable(&mut self, shield: Circle) -> Result<()> {
        if let Some(target) = self.local_circle(shield)? {
            for (row, cut) in self.rows.iter_mut().zip(target.rows) {
                *row &= !cut;
            }
        }
        Ok(())
    }

    /// `GridBlockInt.Overlaps` of a circle: whether the circle's own grid
    /// shares a cell with this one.
    pub(in crate::fight) fn overlaps(&self, circle: Circle) -> Result<bool> {
        Ok(self.local_circle(circle)?.is_some_and(|target| {
            self.rows
                .iter()
                .zip(target.rows)
                .any(|(row, other)| row & other != 0)
        }))
    }

    /// The grid as a recording reads it: the native rows taken as columns,
    /// so its row `i` holds cell `(i, j)` at bit `j`.
    pub(in crate::fight) fn recorded(&self) -> TerrainGridState {
        let mut rows = vec![0_u32; usize::try_from(self.size.1).unwrap_or_default()];
        for (x, column) in self
            .rows
            .iter()
            .copied()
            .enumerate()
            .take(usize::try_from(self.size.0).unwrap_or_default())
        {
            for (y, row) in rows.iter_mut().enumerate() {
                if column & (1 << (31 - y)) != 0 {
                    *row |= 1 << x;
                }
            }
        }
        TerrainGridState {
            origin_x: self.position_x,
            origin_y: self.position_y,
            size_x: self.size.0,
            size_y: self.size.1,
            rows,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_oil_is_twelve_cells_across() {
        let grid = GridBlock::of_circle((-60 << 32, -40 << 32, 30 << 32)).unwrap();
        let recorded = grid.recorded();
        assert_eq!((recorded.size_x, recorded.size_y), (12, 12));
        assert_eq!(
            (recorded.origin_x, recorded.origin_y),
            ((-875 << 32) / 10, (-675 << 32) / 10)
        );
        assert_eq!(recorded.rows[0], 0b0000_1111_0000);
    }
}
