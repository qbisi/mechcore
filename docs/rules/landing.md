# Landing

Where the board puts a unit that no decision places: a purchase, a
reinforcement card's squads, an opening's force, and an officer's delivery. A
battle states where a purchase's moves end, so it reads this rule only for the
other three. And where the board lets a decision put one: a move, and a
contraption placed. [`action.md`](../spec/document/action.md) and the round-opening
deliveries of [`battle.md`](../spec/document/battle.md) rest on it.

## The rule

A new unit lands on its side's main deployment region, the rectangle of the
main region of the map's `MapLayout` territory, and the game places it in the
world frame, where red's region is blue's turned half a turn.

1. The preferred position is the region's centre, which
   [`config/opening.yaml`](../../config/opening.yaml) states per map, with the
   unit's corner aligned to the ten-metre grid. The corner is the centre
   less half the footprint, divided by ten and rounded half to even, then
   multiplied back. A footprint of odd tens therefore shifts the landing by
   five metres, and the shift is the world's: a unit lands at world `x = 5` on
   either side, which is local `5` for blue and `-5` for red.
2. The unit lands at the preferred position when nothing there overlaps
   it. Otherwise it lands at the free grid position nearest it, searched with
   world `x` ascending in the outer loop and world `y` ascending in the inner
   one, and the first position found wins a tie.

A position is free when the footprint lies inside the region and overlaps no
unit, construction or contraption already on it, and neither of the side's
towers, with positive area; edges may touch, and a shield or missile takes no
part. A tower occupies the square of its `MapData` radius around its centre,
both of which [`config/towers.yaml`](../../config/towers.yaml) states: a
placement on it is refused as `RegionLimit`. A unit's footprint exchanges width
and height when it is rotated. Squads handed out together land one at a time,
each clear of the ones before it.

## Moving and placing

A move is refused unless the unit's footprint, at its new position and facing,
lies inside the region the position lies in and overlaps nothing standing in
that region but the unit itself; the refusal is `RegionLimit`. A region is one
of three: the main deployment region and the two flanks, and a flank faces the
other way, so a rotation there exchanges width and height the other way round.
Only the target region is asked: a unit may move between any two regions
directly, and where it came from does not matter. What stands in a region is
what the landing rule avoids there, in the same terms: the side's units,
constructions and contraptions, a shield and a missile excepted, and in the
main region its two towers. A contraption is placed under the same test.

One move may carry several units, and each ignores the others it carries, so
a batch can trade two units' places where two single moves cannot.


`TerritoryManager.GetAvailiblePositionForNewActor` takes the region's centre,
builds the element's rectangle around it, aligns the rectangle's minimum corner
with `MapUtility.WorldToGrid` and multiplies the grid coordinates by ten.
`WorldToGrid` is `FPoint.RoundToInt(value / 10)`, and `FPoint.Round` keeps the
integer part below a half, rounds up above it, and at exactly a half rounds to
the even integer. It then asks `MapRegion.GetAvailiblePositionForElement` for a
position, preferring that one. That method walks every grid position of the
region in steps of ten, `x` outer and `y` inner, keeps those
`MapRegion.IsAvailible` accepts, and returns the one whose distance to the
preferred position is strictly least. `MapRect.Overlaps` compares strictly, so
touching rectangles do not overlap.

## Evidence

### Replayed

- Every arrival of this version's corpus lands where the rule puts it: a unit
  card's squads, a side's opening force, and the squads a specialist delivers as
  a round opens: `scripts/verify-battles.py`.
- Every round of this version's corpus settles into an order in which the rule
  allows each move, purchase and contraption where it stands, and verification
  applies each under it: `scripts/verify-battles.py`.

### Read

- A new unit's preferred position is the region's centre with its corner
  aligned to the grid: `TerritoryManager.GetAvailiblePositionForNewActor`,
  `MapUtility.WorldToGrid`.
- Alignment rounds half to even: `FPoint.RoundToInt`, `FPoint.Round`.
- The free position is the nearest one, walking `x` outer and `y` inner, the
  first found winning a tie: `MapRegion.GetAvailiblePositionForElement`,
  `MapRegion.IsAvailible`.
- Touching rectangles do not overlap: `MapRect.Overlaps`.
- A move is refused unless each unit's footprint lies inside the region its
  target lies in and overlaps nothing of that region the move does not carry:
  `PAP_MoveUnit.Check`, `TerritoryManager.CanMoveUnits`,
  `TerritoryManager.CanMoveUnitToPosition`, `MapRegion.IsAvailible`.
- A contraption is placed under the same test:
  `ContraptionManager.CanRelease`.

### Not established

- **A full region.** No recording has shown a region with no free position.
- **A flank's own bounds and the round it opens.** The build keeps both in the
  map asset; the flank rectangles are this repository's, and no recorded move
  has fallen outside them.
- **Inactive grid cells.** `MapRegion.IsAvailible` also asks whether the grid
  cells under two rectangles are active, and which cells a deployment leaves
  inactive was not read.
