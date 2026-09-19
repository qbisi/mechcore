# Landing

This index is pinned to game build 2259. It states where the board puts a
formation that no decision places: a purchase, a reinforcement card's squads,
an opening's force, and an officer's delivery. A battle states where a
purchase's moves end, so it reads this rule only for the other three. [`action.md`](../spec/document/action.md) and
the round-opening deliveries of [`battle.md`](../spec/document/battle.md) rest
on it.

## The rule

A new formation lands on its side's main deployment region, local
`x=[-300,300], y=[-310,-10]`, and the game places it in the world frame, where
red's region is blue's turned half a turn.

1. The preferred position is the region's centre, local `(0, -160)`, with the
   formation's corner aligned to the ten-metre grid. The corner is the centre
   less half the footprint, divided by ten and rounded half to even, then
   multiplied back. A footprint of odd tens therefore shifts the landing by
   five metres, and the shift is the world's: a unit lands at world `x = 5` on
   either side, which is local `5` for blue and `-5` for red.
2. The formation lands at the preferred position when nothing there overlaps
   it. Otherwise it lands at the free grid position nearest it, searched with
   world `x` ascending in the outer loop and world `y` ascending in the inner
   one, and the first position found wins a tie.

A position is free when the footprint lies inside the region and overlaps no
formation, construction or contraption already on it with positive area; edges
may touch, and a shield or missile takes no part. A formation's footprint
exchanges width and height when it is rotated. Squads handed out together land
one at a time, each clear of the ones before it.

## What the game does

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
touching rectangles do not overlap. All of this is read from the build's
IsilDump.

## Evidence

In the local observation set, 341 formations arrived without a purchase: card
squads, opening forces and officer deliveries, some landing clear of formations
that already stood at the centre. The rule places all 341 exactly where the game
did. With the rule in place of the recorded landings, the native oracle still
closes every decision and every deployment it checks.

Every purchase in the tracked replays lands where the rule puts it: all 1,632,
each read against the position the round had reached when it was bought.

The rule has not been observed on a full region, where no free position exists.
