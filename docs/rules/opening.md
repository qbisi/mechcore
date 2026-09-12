# Seeded opening initialization

These rules cover build **1.11.1.3.2259**, standard versus 1v1 with the shipped
opening pools and no game rules, on maps 1001, 1011, 1021, 1031 and 1032.
The initialization flow determines both offer arrays and the initial defensive
construction layout. Player choices and subsequent reinforcement deals are
outside this scope.

## Two streams start from the same seed

`Match.random` is the opening/reinforcement stream, seeded with
`BattleInfo.SystemSeed`. `MapSystem.Init` creates its own `GRRandom` from
`Match.random.seed`, without drawing from the match stream. Construction draws
therefore do not advance the opening/reinforcement stream.

`GRRandom` delegates to Lua's `RanState`: xoshiro256** values, seeded with
`[seed, 0xff, 0, 0]` and sixteen discarded warm-up values. Range reduction masks
and rejects values outside the requested interval. A random call can consume
several raw values; initialization cannot be replaced with a fixed skip count.
`ReinforcePool.ServerRand(bottom, top)` returns `math_random(bottom + 1, top) - 1`.

## Reinforcement initialization before the opening

`ReinforcementSystem.Init` calls `NormalInit`, then `InitUnitReinforce`.
`NormalInit` applies `RandomTypeGroup` first to opening specialists, then to
normal reinforcement officers. Eligible officer rows have the corresponding
scope (2 for opening, 1 for normal) and an empty `limitedScene` or one containing
standard versus scene 1. Opening specialists all have `typeID = 0` and cause no
random calls here.

For normal officers, positive `typeID` values define groups. Groups are visited
in ascending type ID order and their members sorted by officer ID. A singleton
is retained without drawing. A group with multiple members calls
`ServerRand(0, member_count)` once to retain one member. Type zero rows do not
participate in this selection. The shipped eligible rows give 23 multi-member
groups. Their sizes in draw order are:

```text
4, 4, 2, 3, 3, 3, 3, 3, 2, 2, 3, 4, 4, 2, 5, 4, 2, 2, 3, 3, 2, 2, 2
```

`InitUnitReinforce` then filters `unitReinforceRoundPool` by the same scene,
sorts by ID and calls `ServerRand(0, count)` once. The eligible pool IDs are
1 through 55. The stream now stands at the state recorded by round zero,
before `BattleOpeningController` deals blue and then red. Later reinforcement
consumption follows [reinforcement dealing](reinforcements.md).

[config/opening.yaml](../../config/opening.yaml) retains the group members and
pool IDs, rather than only their counts. All IDs and counts are integers read
from `ConfigDataContainer`; no measurement or scaling is involved.

## Defensive construction layout

The supported `MatchSetting.constructionGroupID` lists are `[67, 68, 71]`, in
that order. `MapSystem.LoadConstructionLayout` chooses one with
`RandomElementSync` using the map stream. Each selected group permits
horizontal reversal and has `isOnlyMirrorSymmetry = false` and
`isCompact = false`. The map stream then calls `NextBool` once per side, blue
first. `GRRandom.NextBool` is `Next(1000) < 500`, including rejection, rather
than a single random bit. A true result negates that side's local x coordinate.

The group data lists the constructions in creation/index order:

| Group | Unit ID and local position |
| --- | --- |
| 67 | 1 at (140, 105), then 2 at (-140, 60) |
| 68 | 1 at (140, 105), then 3 at (-140, 60) |
| 71 | 1 at (140, 105) |

All entries have `isRotate = false`. These are the wall, anti-armor turret and
rapid-fire turret IDs in the construction catalogue. `MapRegion.ConvertToWorldPoint`
rotates the group position by the main region's facing, then adds its center.
The supported maps resolve to `map_layout_1V1_default`: its blue main rectangle
is `(-300, -310, 600, 300)`, facing 0; red's is `(-300, 10, 600, 300)`, facing 2.
After the battle document rotates red by half a turn, both centers are
`(0, -160)`. Thus each side's document position is `(±local_x, local_y - 160)`.
These are integer map coordinates, without rounding or fitted offsets.

`scripts/extract_opening.py` reads the configuration export and
`sharedassets0.assets`: MapData objects 243656, 243657 and 243659 link the map
names to MapLayout object 243676. The extractor validates the supported flags,
facings and shared layout before emitting the initialization table.

## Deal size

`BattleOpeningController.CalculateChooseCount` returns the minimum of
`Config.advanceTeamSetting.chooseCount` and each pool's size divided by the
number of players sharing it. `AdvanceTeamSetting.chooseCount` is **4** in
`level0`, MonoBehaviour path ID **146**, at byte zero of its serialized body.
The following `unitCountPerTeam` is **2**. The standard pools contain enough
teams and specialists that the cap remains four.

## Diversity comparison

`BattleOpeningController.PrepareData` reads the selected map's `MatchSetting`
through `Config.GetMatchSetting`. It passes the field at native offset `0xE0`,
`advanceSameUnitMaximum`, as `ReinforcePool.RandAdvance`'s `diffUnitLimit`.
The standard versus map rows in `ConfigDataContainer.matchSettings` carry **3**
for that field. Despite its name, this argument is used as a lower bound on
unit diversity, not an upper bound on a repeated unit's squad count.

For each price tier touched by a candidate, the comparison adds the number of
distinct types already held, the new types in the candidate, and the remaining
picks **after** this pick. A total below `diffUnitLimit` rejects the candidate.
The number of remaining picks is `num - taken - 1`, with `taken` starting at
zero. In the native method, `not eax` followed by adding `num` computes
`num + ~taken`; arithmetic negation would count the current pick twice.

The constants are integers with no scaling or precision conversion.

## Evidence and boundary

The field values come from the shipped resources. The field access and
comparison come from the native instructions of
`BattleOpeningController.CalculateChooseCount`,
`BattleOpeningController.PrepareData(PlayerController, List<AdvanceTeam>,
List<IReinforceItem>, int)` and `ReinforcePool.RandAdvance`. Cpp2IL's ISIL labels
the native `not` as `Neg`; the native instruction determines the arithmetic.

Artifact identities (SHA-256):

| Artifact | Hash |
| --- | --- |
| `GameAssembly.dylib` | `9d2e3f163f74728da73b5dac474f76a0ebdaa3852718fbeb45ad8dec6b4360e5` |
| `global-metadata.dat` | `2488ba4661958da42438e91cb382259077ef7077dfa2261e74ade1c9b1b1fb43` |
| `level0` | `9276c12f99c188854c588e603220472d98623a8ffcc8c03f84516f4adbdce265` |
| `sharedassets0.assets` | `097a8cc560fbe218fb1b5b01cf2b639b42670910286c7ebc5ea352024eda8cb3` |
| ConfigDataContainer JSON export, path ID 160 | `92849e4b0cba65bb03448cac0c868ef94fcbbd476eabb8f2bc33d484e820ac4f` |

The initialization call order and range branches come from
`ReinforcementSystem.Init`, `NormalInit`, `RandomTypeGroup` (including its sort
callbacks), and `InitUnitReinforce`. The independent map stream and its draw
order come from `MapSystem.Init`, `LoadConstructionLayout`, `LoadConstruction`,
`GRRandom.NextBool`, and `MapRegion.ConvertToWorldPoint`. Resources establish
the eligible pools, group flags, positions and map geometry.

Modified pools, other modes and negative match seeds are unverified.
[Reinforcement dealing](reinforcements.md) describes consumption after opening. A seed determines the alternatives offered, not
which alternative either player chooses. Reopen when the artifact identities
change, a supported map changes its initialization inputs, or a native state,
construction layout or offer array disagrees with this flow.
