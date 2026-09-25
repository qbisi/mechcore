# Seeded opening initialization

These rules cover standard versus 1v1 with the shipped opening pools and no
game rules, on maps 1001, 1011, 1021, 1031 and 1032. The initialization flow determines both offer arrays and the initial defensive
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
`GRRandom.ServerRand(bottom, top)` returns `math_random(bottom + 1, top) - 1`.

## Reinforcement initialization before the opening

`ReinforcementSystem.Init` creates the match's reinforcement object, whose
`Init` applies `RandomTypeGroup` first to opening specialists, then to normal
reinforcement officers, and then runs `InitUnitReinforce`. Eligible officer
rows have the corresponding scope (2 for opening, 1 for normal) and an empty
`limitedScene` or one containing standard versus scene 1. Opening specialists all have `typeID = 0` and cause no
random calls here.

For normal officers, positive `typeID` values define groups. Groups are visited
in ascending type ID order and their members sorted by officer ID. A singleton
is retained without drawing. A group with multiple members calls
`ServerRand(0, member_count)` once to retain one member. Type zero rows do not
participate in this selection. The groups and their members, in draw order,
are `officer_groups` in [config/opening.yaml](../../config/opening.yaml).

`InitUnitReinforce` then filters `unitReinforceRoundPool` by the same scene,
sorts by ID and calls `ServerRand(0, count)` once over the eligible pools,
`unit_round_pools` in the same file. The stream now stands at the state recorded by round zero,
before `BattleOpeningController` deals blue and then red. Later reinforcement
consumption follows [reinforcement dealing](reinforcements.md).

[config/opening.yaml](../../config/opening.yaml) retains the group members and
pool IDs, rather than only their counts. All IDs and counts are integers read
from `ConfigDataContainer`; no measurement or scaling is involved.

## Defensive construction layout

A map's `MatchSetting.constructionGroupID` lists the groups it may lay, which
[config/opening.yaml](../../config/opening.yaml) states per map as `groups`.
`MapSystem.LoadConstructionLayout` chooses one with
`RandomElementSync` using the map stream. Each selected group permits
horizontal reversal and has `isOnlyMirrorSymmetry = false` and
`isCompact = false`. The map stream then calls `NextBool` once per side, blue
first. `GRRandom.NextBool` is `Next(1000) < 500`, including rejection, rather
than a single random bit. A true result negates that side's local x coordinate.

A group's `datas` list the constructions in creation and index order, each a
construction ID and a local position, all with `isRotate = false`; the same
file states them as `constructions`. `MapRegion.ConvertToWorldPoint` rotates
the group position by the main region's facing, then adds its center. The
supported maps resolve to one `MapLayout` whose blue main region faces 0 and
red's faces 2, so after the battle document rotates red by half a turn both
centers are the same point, the file's `centers`. Thus each side's document
position is `(±local_x, local_y + center_y)`. These are integer map
coordinates, without rounding or fitted offsets.

A seat starts with the reactor core `MatchSetting.GetReactorCore` reads from
the map's `reactorCores`: by seat index, except that a list of fewer than two
entries gives every seat its first; the file states each map's
`reactor_cores`, before the opening's team and specialist move it. The same
rows list no default shop unit and no commander skill, so a
side holds neither before its opening.

`scripts/extract_opening.py` reads the build's typed export: each map's
`matchSettings` row names its `MapData`, whose `layoutName` names the
`MapLayout` whose territories give the seats' main regions. The extractor
validates the supported flags and facings before emitting the initialization
table.

## Deal size

`BattleOpeningController.CalculateChooseCount` returns the minimum of
`Config.advanceTeamSetting.chooseCount` and each pool's size divided by the
number of players sharing it, `AdvanceTeamSetting.chooseCount` in `level0`.
The standard pools contain enough teams and specialists that the cap is
`chooseCount` itself.

## Diversity comparison

`BattleOpeningController.PrepareData` reads the selected map's `MatchSetting`
through `Config.GetMatchSetting`. It passes `advanceSameUnitMaximum` as
`ReinforcementSystem.RandAdvance`'s `diffUnitLimit`; `scripts/extract_opening.py`
checks that every supported map carries the same value. Despite its name, this argument is used as a lower bound on
unit diversity, not an upper bound on a repeated unit's squad count.

For each price tier touched by a candidate, the comparison adds the number of
distinct types already held, the new types in the candidate, and the remaining
picks **after** this pick. A total below `diffUnitLimit` rejects the candidate.
The number of remaining picks is `num - taken - 1`, with `taken` starting at
zero. In the native method, `not eax` followed by adding `num` computes
`num + ~taken`; arithmetic negation would count the current pick twice.

The constants are integers with no scaling or precision conversion.

The field values come from the shipped resources, and the field access and
comparison from the native instructions. Cpp2IL's ISIL labels the native `not`
as `Neg`; the native instruction determines the arithmetic. A seed determines
the alternatives offered, not which alternative either player chooses.

## Evidence

### Replayed

- The flow reproduces every recorded opening: the four offers each side was
  dealt and the construction layouts, in every replay of this version's corpus:
  `scripts/verify-battles.py`.

### Read

- The opening stream is the match's, seeded with the system seed, and the map
  draws from a stream of its own seeded the same, without drawing from the
  match's: `MapSystem.Init`.
- Initialization draws the officer groups and then the unit round pool, in
  that order: `ReinforcementSystem.Init`, `ReinforcementRandomObject_Normal.Init`,
  `ReinforcementRandomObject_Common.RandomTypeGroup`,
  `ReinforcementRandomObject_Common.InitUnitReinforce`.
- A range draw is `math_random(bottom + 1, top) - 1`: `GRRandom.ServerRand`.
- A map chooses one construction group and reverses each side's with one
  `NextBool`, blue first: `MapSystem.LoadConstructionLayout`,
  `MapSystem.LoadConstruction`, `GRRandom.NextBool`.
- A group position is rotated by the main region's facing and moved to its
  centre: `MapRegion.ConvertToWorldPoint`.
- A seat's reactor core is its map's by seat index, or the first for every
  seat when the list is shorter than two: `MatchSetting.GetReactorCore`.
- The deal size is the setting's choose count, capped by each pool's share:
  `BattleOpeningController.CalculateChooseCount`.
- The diversity limit is a lower bound on distinct types, counting the picks
  after this one: `BattleOpeningController.PrepareData`,
  `ReinforcementSystem.RandAdvance`.

### Not established

- **Modified pools, other modes and negative match seeds.**
