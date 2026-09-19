//! Where the board puts a formation no decision placed.
//!
//! A card's squads, an officer's delivery and an opening's force arrive without
//! a position, and the game chooses one. `docs/rules/landing.md` states the
//! rule, which is `TerritoryManager.GetAvailiblePositionForNewActor` and
//! `MapRegion.GetAvailiblePositionForElement`: the main deployment region's
//! centre, aligned to the ten-metre grid, or the free grid position nearest it.
//!
//! The game works in world coordinates, and the answer depends on it: the grid
//! aligns in the world frame, and the search walks it in ascending world `x`
//! and `y`. Red's frame is the world turned half a turn, so red's landings are
//! not blue's mirrored and the side has to be named.

use crate::battle::SideState;
use crate::catalog::{resolve_construction_type, resolve_contraption_type, resolve_unit_type};
use crate::layout::{Position, Region};

/// The grid a position aligns to, in metres.
const GRID: i64 = 10;
/// The local main deployment region, `x=[-300,300], y=[-310,-10]`.
const MAIN_MIN: (i64, i64) = (-300, -310);
const MAIN_MAX: (i64, i64) = (300, -10);

/// A world-frame rectangle, as its two corners.
#[derive(Clone, Copy)]
struct Rect {
    min: (i64, i64),
    max: (i64, i64),
}

impl Rect {
    fn around(center: (i64, i64), size: (i64, i64)) -> Self {
        Self {
            min: (center.0 - size.0 / 2, center.1 - size.1 / 2),
            max: (center.0 + size.0 / 2, center.1 + size.1 / 2),
        }
    }

    /// Positive-area overlap. Rectangles that only touch do not overlap.
    const fn overlaps(self, other: Self) -> bool {
        self.min.0 < other.max.0
            && other.min.0 < self.max.0
            && self.min.1 < other.max.1
            && other.min.1 < self.max.1
    }

    const fn within(self, region: Self) -> bool {
        self.min.0 >= region.min.0
            && self.max.0 <= region.max.0
            && self.min.1 >= region.min.1
            && self.max.1 <= region.max.1
    }
}

/// Rounds half to even, the way `FPoint.Round` does.
fn round_half_even(numerator: i64, denominator: i64) -> i64 {
    let floor = numerator.div_euclid(denominator);
    let twice_remainder = 2 * numerator.rem_euclid(denominator);
    match twice_remainder.cmp(&denominator) {
        std::cmp::Ordering::Less => floor,
        std::cmp::Ordering::Greater => floor + 1,
        std::cmp::Ordering::Equal => floor + floor.rem_euclid(2),
    }
}

/// Where a formation of `type_name` lands on a side holding `state`, or
/// nothing when the unit has no footprint or the region has no room.
///
/// `red` names the side: the grid and the search are the world's, and red's
/// frame is the world turned half a turn.
#[must_use]
pub fn landing(state: &SideState, type_name: &str, red: bool) -> Option<Position> {
    let size = resolve_unit_type(type_name)?.footprint?;
    let world = |position: Position| {
        let (x, y) = (i64::from(position.x), i64::from(position.y));
        if red { (-x, -y) } else { (x, y) }
    };
    let region = if red {
        Rect {
            min: (-MAIN_MAX.0, -MAIN_MAX.1),
            max: (-MAIN_MIN.0, -MAIN_MIN.1),
        }
    } else {
        Rect {
            min: MAIN_MIN,
            max: MAIN_MAX,
        }
    };
    let taken: Vec<Rect> = obstacles(state)
        .into_iter()
        .map(|(center, size)| Rect::around(world(center), size))
        .filter(|rect| rect.overlaps(region))
        .collect();

    // The region's centre, with the formation's corner aligned to the grid.
    let centre = (
        i64::midpoint(region.min.0, region.max.0),
        i64::midpoint(region.min.1, region.max.1),
    );
    let corner = |axis: usize| {
        let (centre, size) = if axis == 0 {
            (centre.0, size.0)
        } else {
            (centre.1, size.1)
        };
        round_half_even(2 * centre - size, 2 * GRID) * GRID + size / 2
    };
    let prefer = (corner(0), corner(1));

    let mut best: Option<((i64, i64), i64)> = None;
    let mut x = region.min.0;
    while x < region.max.0 {
        let mut y = region.min.1;
        while y < region.max.1 {
            let rect = Rect {
                min: (x, y),
                max: (x + size.0, y + size.1),
            };
            if rect.within(region) && !taken.iter().any(|other| rect.overlaps(*other)) {
                let center = (x + size.0 / 2, y + size.1 / 2);
                let distance = (center.0 - prefer.0).pow(2) + (center.1 - prefer.1).pow(2);
                if best.is_none_or(|(_, held)| distance < held) {
                    best = Some((center, distance));
                }
            }
            y += GRID;
        }
        x += GRID;
    }
    let (center, _) = best?;
    let local = if red { (-center.0, -center.1) } else { center };
    Some(Position {
        x: i32::try_from(local.0).ok()?,
        y: i32::try_from(local.1).ok()?,
    })
}

/// Everything on the side's main region a new formation cannot overlap, as
/// local centres and world-frame sizes.
fn obstacles(state: &SideState) -> Vec<(Position, (i64, i64))> {
    let mut placed = Vec::new();
    for entry in &state.formations {
        let formation = &entry.formation;
        if Region::of(formation.position).is_flank() {
            continue;
        }
        if let Some((width, height)) =
            resolve_unit_type(&formation.type_name).and_then(|spec| spec.footprint)
        {
            let size = if formation.rotated == Some(true) {
                (height, width)
            } else {
                (width, height)
            };
            placed.push((formation.position, size));
        }
    }
    for construction in &state.constructions {
        if let Some(size) =
            resolve_construction_type(&construction.type_name).and_then(|spec| spec.footprint)
        {
            placed.push((construction.position, size));
        }
    }
    for contraption in &state.contraptions {
        // A shield and a missile take part in no deployment collision.
        if matches!(contraption.type_name.as_str(), "shield" | "missile") {
            continue;
        }
        if let Some(size) =
            resolve_contraption_type(&contraption.type_name).and_then(|spec| spec.footprint)
        {
            placed.push((contraption.position, size));
        }
    }
    placed
}

/// The board's rule as the placement [`crate::transition::step_placing`] asks
/// for, on one side.
pub fn placement(red: bool) -> impl FnMut(&SideState, &str) -> Option<Position> {
    move |state, type_name| landing(state, type_name, red)
}
