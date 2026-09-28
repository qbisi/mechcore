//! Which recorded thing is which placement of the document.
//!
//! A recording numbers its formations by where their members stand rather than
//! by the order a layout declares them, and it records a building without
//! saying which construction released it, so nothing a reader wants to say
//! about a recording can be written back into a document until the two are
//! matched. Every reader of a recording needs that and none of them is about
//! it, which is why it lives here, matching by the one thing a recording and a
//! layout share: where a thing stands.

use std::collections::BTreeMap;

use mechcore_document::{Layout, Position, UnitPlacement};
use mechcore_mcfr::{LiveUnitState, McfrReader, WorldSnapshot};

use crate::{cli::Failure, turn::Side};

/// The first tick a recording holds, which is the scene the fight starts from.
pub(crate) const FIRST_TICK: u32 = 1;

/// The layout a recording embeds, which is the document its formations are
/// matched against.
pub(crate) fn layout(reader: &McfrReader) -> Result<Layout, Failure> {
    mechcore_document::parse_yaml(reader.layout_yaml().as_bytes())
        .map_err(|error| Failure::refused(format!("recording embeds no layout: {error}")))
}

pub(crate) fn snapshot(reader: &McfrReader, tick: u32) -> Result<WorldSnapshot, Failure> {
    reader
        .state(tick)
        .map_err(|error| Failure::refused(format!("recording has no tick {tick}: {error}")))
}

pub(crate) fn side_of(layout: &Layout, side: Side) -> &mechcore_document::Side {
    match side {
        Side::Blue => &layout.blue,
        Side::Red => &layout.red,
    }
}

pub(crate) fn units_of(layout: &Layout, side: Side) -> &[UnitPlacement] {
    &side_of(layout, side).units
}

/// Which layout formation each recorded formation is.
///
/// The two are matched by the one thing they share: a formation's members
/// stand in its own slot. Each formation's members are averaged at the given
/// snapshot, and a formation and a placement of the same type on the same side
/// are each other's when each is the other's nearest. A formation no placement
/// takes that way is not a placement's at all: the fight made it as it was
/// built, the way a skill or an officer hands a side units its formations do
/// not hold, and it is left out. A tie that decides a pairing is refused rather
/// than broken, because a formation that cannot be named cannot be written back
/// into a document.
pub(crate) fn formations(
    layout: &Layout,
    opened: &WorldSnapshot,
) -> Result<BTreeMap<u64, (Side, i32)>, Failure> {
    let mut grouped: BTreeMap<u64, Vec<&LiveUnitState>> = BTreeMap::new();
    for unit in &opened.live_units {
        grouped.entry(unit.formation_id).or_default().push(unit);
    }
    let mut recorded = Vec::new();
    for (formation, members) in &grouped {
        let first = members[0];
        let side = match first.team_id {
            0 => Side::Blue,
            1 => Side::Red,
            other => {
                return Err(Failure::refused(format!(
                    "recording holds team {other}, and a match has two"
                )));
            }
        };
        recorded.push((
            *formation,
            side,
            type_name(first.unit_type_id)?,
            centre(members),
        ));
    }
    let mut named = BTreeMap::new();
    for side in Side::BOTH {
        for placement in units_of(layout, side) {
            let at = world(placement.position, side);
            let name = placement.type_name.as_str();
            let of_type = || {
                recorded
                    .iter()
                    .filter(move |(_, owner, kind, _)| *owner == side && *kind == name)
            };
            let formation = match nearest(
                of_type().map(|(formation, _, _, centre)| (*formation, distance(at, *centre))),
            ) {
                Nearest::Nothing => continue,
                Nearest::One(formation) => formation,
                Nearest::Tied => {
                    return Err(Failure::refused(format!(
                        "placement {} of {name} on {} stands as near one recorded \
                         formation as another",
                        placement.index,
                        side.name()
                    )));
                }
            };
            let centre = of_type()
                .find(|(candidate, ..)| *candidate == formation)
                .map(|(.., centre)| *centre)
                .expect("the nearest formation is one of them");
            match nearest(
                units_of(layout, side)
                    .iter()
                    .filter(|other| other.type_name == name)
                    .map(|other| (other.index, distance(world(other.position, side), centre))),
            ) {
                Nearest::One(own) if own == placement.index => {
                    named.insert(formation, (side, placement.index));
                }
                Nearest::Tied => {
                    return Err(Failure::refused(format!(
                        "formation {formation} of {name} on {} stands as near one placement \
                         of the layout the recording embeds as another",
                        side.name()
                    )));
                }
                _ => {}
            }
        }
    }
    Ok(named)
}

/// Which candidate stands at the least distance.
enum Nearest<T> {
    /// There is no candidate.
    Nothing,
    One(T),
    /// Two candidates share the least distance.
    Tied,
}

fn nearest<T>(candidates: impl Iterator<Item = (T, i64)>) -> Nearest<T> {
    let mut best: Option<i64> = None;
    let mut answer = Nearest::Nothing;
    for (candidate, distance) in candidates {
        match best {
            Some(least) if distance > least => {}
            Some(least) if distance == least => answer = Nearest::Tied,
            _ => {
                best = Some(distance);
                answer = Nearest::One(candidate);
            }
        }
    }
    answer
}

fn type_name(unit_type: u32) -> Result<&'static str, Failure> {
    i32::try_from(unit_type)
        .ok()
        .and_then(mechcore_document::unit_type_from_id)
        .map(|(name, _)| name)
        .ok_or_else(|| {
            Failure::refused(format!(
                "recording holds unit type {unit_type}, which this build does not name"
            ))
        })
}

/// A side's placement in the world the fight runs in.
///
/// A layout states each side's board in its own frame, and red's is the same
/// board turned around, which is the transform the simulator applies when it
/// builds the scene.
pub(crate) const fn world(position: Position, side: Side) -> (i64, i64) {
    let (x, z) = (position.x as i64, position.y as i64);
    match side {
        Side::Blue => (x, z),
        Side::Red => (-x, -z),
    }
}

/// Where a formation's members stand on average, in world units.
fn centre(members: &[&LiveUnitState]) -> (i64, i64) {
    let count = i64::try_from(members.len()).unwrap_or(1).max(1);
    let sum = members.iter().fold((0_i64, 0_i64), |sum, unit| {
        (
            sum.0 + (unit.position.x >> 32),
            sum.1 + (unit.position.z >> 32),
        )
    });
    (sum.0 / count, sum.1 / count)
}

/// Squared distance, which orders as the distance does and needs no root.
pub(crate) const fn distance(left: (i64, i64), right: (i64, i64)) -> i64 {
    let (x, z) = (left.0 - right.0, left.1 - right.1);
    x * x + z * z
}

#[cfg(test)]
mod tests {
    use super::{Nearest, nearest};

    #[test]
    fn nearest_names_one_candidate_or_says_why_not() {
        assert!(matches!(nearest::<u8>([].into_iter()), Nearest::Nothing));
        assert!(matches!(
            nearest([(1, 9), (2, 4), (3, 7)].into_iter()),
            Nearest::One(2)
        ));
        assert!(matches!(
            nearest([(1, 4), (2, 4), (3, 7)].into_iter()),
            Nearest::Tied
        ));
        // A tie beaten by a nearer candidate is no tie.
        assert!(matches!(
            nearest([(1, 4), (2, 4), (3, 1)].into_iter()),
            Nearest::One(3)
        ));
    }
}
