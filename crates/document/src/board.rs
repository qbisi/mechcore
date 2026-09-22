//! What stands on the board before anyone deploys.
//!
//! Each side's Energy Tower and Research Center are elements of its main
//! deployment region, and `MapRegion.IsAvailible` refuses a new element that
//! overlaps one exactly as it refuses one that overlaps a unit: a placement on
//! a tower is `PlayerActionCheckResult.RegionLimit` (16). Their deployment
//! footprint is a 20 m square on the tower's centre. The build keeps it in the
//! map asset, which this repository does not read, so it was measured against
//! the game: a 20 x 20 Marksman ten metres off a tower's centre, on either
//! axis, is refused, and one twenty metres off is placed; a Crawler whose edge
//! meets the square is placed.

/// The side of a tower's deployment footprint, in metres.
pub(crate) const TOWER_FOOTPRINT: i64 = 20;

/// The four towers, as world centres with their names: blue's at `y = -170`,
/// red's at `y = 170`, each side's Energy Tower on its own left.
pub(crate) const TOWERS: [((i64, i64), &str); 4] = [
    ((-140, -170), "blue Energy Tower"),
    ((140, -170), "blue Research Center"),
    ((140, 170), "red Energy Tower"),
    ((-140, 170), "red Research Center"),
];

/// A side's own two towers, as local centres: the same for either side,
/// because red's frame is the world turned half a turn.
pub(crate) const OWN_TOWERS_LOCAL: [(i64, i64); 2] = [(-140, -170), (140, -170)];
