//! Which recorded formation is which formation of the document.
//!
//! A recording numbers its formations by where their members stand rather than
//! by the order a layout declares them, so nothing a reader wants to say about
//! a recording can be written back into a document until the two are matched.
//! Both readers of a recording need that and neither of them is about it,
//! which is why it lives here.

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
/// snapshot and the nearest placement of the same type on the same side is its
/// own. A tie is refused rather than broken, because a formation that cannot
/// be named cannot be written back into a document.
pub(crate) fn formations(
    layout: &Layout,
    opened: &WorldSnapshot,
) -> Result<BTreeMap<u64, (Side, i32)>, Failure> {
    let mut grouped: BTreeMap<u64, Vec<&LiveUnitState>> = BTreeMap::new();
    for unit in &opened.live_units {
        grouped.entry(unit.formation_id).or_default().push(unit);
    }
    let mut named = BTreeMap::new();
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
        let name = type_name(first.unit_type_id)?;
        let centre = centre(members);
        let mut nearest: Option<(i64, i32)> = None;
        let mut tied = false;
        for placement in units_of(layout, side) {
            if placement.type_name != name {
                continue;
            }
            let distance = distance(world(placement.position, side), centre);
            match nearest {
                Some((best, _)) if distance > best => {}
                Some((best, _)) if distance == best => tied = true,
                _ => {
                    nearest = Some((distance, placement.index));
                    tied = false;
                }
            }
        }
        let Some((_, index)) = nearest.filter(|_| !tied) else {
            return Err(Failure::refused(format!(
                "formation {formation} of {name} on {} matches no single placement \
                 of the layout the recording embeds",
                side.name()
            )));
        };
        named.insert(*formation, (side, index));
    }
    Ok(named)
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
const fn world(position: Position, side: Side) -> (i64, i64) {
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
const fn distance(left: (i64, i64), right: (i64, i64)) -> i64 {
    let (x, z) = (left.0 - right.0, left.1 - right.1);
    x * x + z * z
}
