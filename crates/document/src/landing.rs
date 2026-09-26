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
use crate::layout::{
    AMBUSH_LEFT_MAX_X, AMBUSH_LEFT_MIN_X, AMBUSH_MAX_Y, AMBUSH_MIN_Y, AMBUSH_RIGHT_MAX_X,
    AMBUSH_RIGHT_MIN_X, Position, Region,
};

/// The grid a position aligns to, in metres.
const GRID: i64 = 10;
/// The local main deployment region, `x=[-300,300], y=[-310,-10]`.
const MAIN_MIN: (i64, i64) = (-300, -310);
const MAIN_MAX: (i64, i64) = (300, -10);

/// A rectangle, as its two corners.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Rect {
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

    /// The same rectangle in a frame turned half a turn.
    const fn turned(self) -> Self {
        Self {
            min: (-self.max.0, -self.max.1),
            max: (-self.min.0, -self.min.1),
        }
    }
}

/// A region's rectangle in the side's own frame.
const fn bounds(region: Region) -> Rect {
    match region {
        Region::Main => Rect {
            min: MAIN_MIN,
            max: MAIN_MAX,
        },
        Region::LeftFlank => Rect {
            min: (AMBUSH_LEFT_MIN_X, AMBUSH_MIN_Y),
            max: (AMBUSH_LEFT_MAX_X, AMBUSH_MAX_Y),
        },
        Region::RightFlank => Rect {
            min: (AMBUSH_RIGHT_MIN_X, AMBUSH_MIN_Y),
            max: (AMBUSH_RIGHT_MAX_X, AMBUSH_MAX_Y),
        },
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

fn local(position: Position) -> (i64, i64) {
    (i64::from(position.x), i64::from(position.y))
}

/// A unit's footprint in a region: a flank faces the other way, so what a
/// rotation does there is undone.
fn unit_size(type_name: &str, rotated: bool, region: Region) -> Option<(i64, i64)> {
    let (width, height) = resolve_unit_type(type_name)?.footprint?;
    Some(if rotated ^ region.is_flank() {
        (height, width)
    } else {
        (width, height)
    })
}

/// Something standing in one of a side's regions: a unit, a construction, a
/// contraption or a tower, as a rectangle in the side's own frame.
struct Occupant {
    region: Region,
    rect: Rect,
    /// The unit's index, when it is one.
    unit: Option<i32>,
}

/// Everything a formation placed in a region cannot overlap. A region's
/// elements are the ones standing in it, and `MapRegion.IsAvailible` asks only
/// those of the target's region.
fn occupants(state: &SideState) -> Vec<Occupant> {
    let mut placed = Vec::new();
    for entry in &state.units {
        let formation = &entry.unit;
        let region = Region::of(formation.position);
        if let Some(size) = unit_size(
            &formation.type_name,
            formation.rotated == Some(true),
            region,
        ) {
            placed.push(Occupant {
                region,
                rect: Rect::around(local(formation.position), size),
                unit: Some(formation.index),
            });
        }
    }
    for construction in &state.constructions {
        if let Some(size) =
            resolve_construction_type(&construction.type_name).and_then(|spec| spec.footprint)
        {
            placed.push(Occupant {
                region: Region::of(construction.position),
                rect: Rect::around(local(construction.position), size),
                unit: None,
            });
        }
    }
    // The side's own towers stand in its region from the start.
    for center in crate::board::OWN_TOWERS_LOCAL {
        placed.push(Occupant {
            region: Region::Main,
            rect: Rect::around(
                center,
                (crate::board::TOWER_FOOTPRINT, crate::board::TOWER_FOOTPRINT),
            ),
            unit: None,
        });
    }
    for contraption in &state.contraptions {
        // A shield and a missile take part in no deployment collision.
        if matches!(contraption.type_name.as_str(), "shield" | "missile") {
            continue;
        }
        if let Some(size) =
            resolve_contraption_type(&contraption.type_name).and_then(|spec| spec.footprint)
        {
            placed.push(Occupant {
                region: Region::of(contraption.position),
                rect: Rect::around(local(contraption.position), size),
                unit: None,
            });
        }
    }
    placed
}

/// Whether moving unit `index` of `type_name` to `position` is one
/// `TerritoryManager.CanMoveUnitToPosition` allows on a side holding `state`:
/// its footprint inside the region the position lies in, overlapping nothing
/// of that region but the unit itself.
#[must_use]
pub(crate) fn unit_fits(
    state: &SideState,
    index: i32,
    type_name: &str,
    position: Position,
    rotated: bool,
) -> bool {
    let region = Region::of(position);
    let Some(size) = unit_size(type_name, rotated, region) else {
        return false;
    };
    let rect = Rect::around(local(position), size);
    rect.within(bounds(region))
        && !occupants(state).iter().any(|other| {
            other.region == region && other.unit != Some(index) && other.rect.overlaps(rect)
        })
}

/// Whether a contraption of `type_name` can be placed at `position` on a side
/// holding `state`: `ContraptionManager.CanRelease` asks what a move does, a
/// footprint inside its region overlapping nothing standing there. A shield and
/// a missile take part in no deployment collision.
#[must_use]
pub(crate) fn contraption_fits(state: &SideState, type_name: &str, position: Position) -> bool {
    if matches!(type_name, "shield" | "missile") {
        return true;
    }
    let Some(size) = resolve_contraption_type(type_name).and_then(|spec| spec.footprint) else {
        return false;
    };
    let region = Region::of(position);
    let rect = Rect::around(local(position), size);
    rect.within(bounds(region))
        && !occupants(state)
            .iter()
            .any(|other| other.region == region && other.rect.overlaps(rect))
}

/// Where a contraption of `type_name` at `position` stands, in the side's own
/// frame, when it takes part in deployment collisions.
#[cfg(feature = "convert")]
#[must_use]
pub(crate) fn contraption_rect(type_name: &str, position: Position) -> Option<Rect> {
    if matches!(type_name, "shield" | "missile") {
        return None;
    }
    let size = resolve_contraption_type(type_name)?.footprint?;
    Some(Rect::around(local(position), size))
}

/// Where a unit of `type_name` at `position` and `rotated` stands, in the
/// side's own frame, for a caller to keep clear.
#[cfg(feature = "convert")]
#[must_use]
pub(crate) fn unit_rect(type_name: &str, position: Position, rotated: bool) -> Option<Rect> {
    let size = unit_size(type_name, rotated, Region::of(position))?;
    Some(Rect::around(local(position), size))
}

/// The free grid position in `region` nearest its centre, for a unit of
/// `type_name` facing as `rotated` says, overlapping nothing standing there
/// but unit `ignore`, nor any of `avoid`; or nothing when the region has no
/// room.
///
/// `red` names the side: the grid and the search are the world's, and red's
/// frame is the world turned half a turn.
#[must_use]
pub(crate) fn free_spot(
    state: &SideState,
    type_name: &str,
    rotated: bool,
    region: Region,
    red: bool,
    ignore: Option<i32>,
    avoid: &[Rect],
) -> Option<Position> {
    let size = unit_size(type_name, rotated, region)?;
    let world = |rect: Rect| if red { rect.turned() } else { rect };
    let bound = world(bounds(region));
    let taken: Vec<Rect> = occupants(state)
        .into_iter()
        .filter(|other| other.region == region && (ignore.is_none() || other.unit != ignore))
        .map(|other| other.rect)
        .chain(avoid.iter().copied())
        .map(world)
        .collect();

    // The region's centre, with the formation's corner aligned to the grid.
    let centre = (
        i64::midpoint(bound.min.0, bound.max.0),
        i64::midpoint(bound.min.1, bound.max.1),
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
    let mut x = bound.min.0;
    while x < bound.max.0 {
        let mut y = bound.min.1;
        while y < bound.max.1 {
            let rect = Rect {
                min: (x, y),
                max: (x + size.0, y + size.1),
            };
            if rect.within(bound) && !taken.iter().any(|other| rect.overlaps(*other)) {
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

/// Where a formation of `type_name` lands on a side holding `state`, or
/// nothing when the unit has no footprint or the region has no room: the main
/// region's free position nearest its centre.
///
/// `red` names the side: the grid and the search are the world's, and red's
/// frame is the world turned half a turn.
#[must_use]
pub fn landing(state: &SideState, type_name: &str, red: bool) -> Option<Position> {
    free_spot(state, type_name, false, Region::Main, red, None, &[])
}

/// The board's rule as the placement [`crate::transition::step_placing`] asks
/// for, on one side.
pub fn placement(red: bool) -> impl FnMut(&SideState, &str) -> Option<Position> {
    move |state, type_name| landing(state, type_name, red)
}
