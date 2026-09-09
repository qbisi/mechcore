# State definition

## Status

This document is a design draft. No crate implements it yet, and the field names
below are proposals, not a contract. Sections marked **Unresolved** name what
still has no evidence behind it.

Evidence for the claims here comes from four ranked GRBR replays in
`tests/grbr`, 1095 recorded actions over 58 round-sides. Two computer matches in
the same directory were set up with Training Ground commands rather than by 1v1
rules and are excluded from every count.

A few claims are measured over a wider set, the 11 ranked matches and 202
player-rounds the Steam installation has recorded locally. Those say so where
they appear. `tests/grbr/README.md` explains which replays there are usable and
why the downloaded ones are not.

## Scope

A state document describes one complete match position: everything both players
hold at one moment of one round. It is the largest of the three documents this
format defines, and the other two are related to it by projection and by
composition.

```yaml
kind: state
map_id: 1001
seed: 2038621361
round: 7

reinforce_offers: [1072216, 1072221, 107228, 1072213]

sides:
  blue: { ... }
  red: { ... }
```

`map_id` and `seed` carry the same meaning and the same optionality as in a
layout. They belong to the document rather than to a side because both sides
share them.

A [turn](turn.md) carries this shape under its own `state` key, without
repeating `kind`, and adds the actions taken from it. `kind` marks a document
root, not a subtree.

## Relation to a layout

A layout is not a different object from a state. It is the projection of a
state onto what the fight simulates:

```text
layout = project(state)
```

The projection is defined at every point in the round, not only at its ends.
After each action the state changes, and projecting the new state yields the
layout that a Training Ground scene would have to install to reproduce the
match at that moment. Running an action list forward and projecting after every
step is therefore the natural way to check a [turn](turn.md) against a capture,
and it gives a much finer failure signal than comparing only the round's
endpoints.

What the projection drops is everything the fight cannot observe: supply,
the shop, the reinforcement offer, the allocators, and the parts of the skill
panel that were not released this round.

Six side fields project unchanged: `techs`, `formations`, `constructions`,
`contraptions`, `airdrop_shields` and `terrains`. The layout's
`research_center` and `energy_tower` objects do not occur in a state. They are
semantic views of three native state fields:

| State source | Layout target | Projection |
| --- | --- | --- |
| `blueprints` | `research_center.attack_level` | neither `4` nor `401` → `0`; `4` → `1`; `401` → `2` |
| `blueprints` | `research_center.defense_level` | neither `5` nor `501` → `0`; `5` → `1`; `501` → `2` |
| `tower_strengthen_levels` | `research_center.strength_level` | level at the Research Center's building-manager position |
| `tower_strengthen_levels` | `energy_tower.strength_level` | level at the Energy Tower's building-manager position |
| `energy_tower_skills` | `energy_tower.range_enhancement` | `5` is present |
| `energy_tower_skills` | `energy_tower.movement_enhancement` | `6` is present |

A valid blueprint list cannot hold both levels of one chain. Blueprint IDs `1`,
`2` and `3`, and Energy Tower skill IDs `1`, `3` and `4`, add no other tower
field to the projection: their fight-visible consequences are already carried
by the skill panel or formations, while their economic consequences are not
part of a layout. The building-manager position for each fixed tower remains the
one unresolved part of this projection.

`battle_skills` is projected rather than copied too. The projection keeps only
entries with `release`, sorts them by `release.order`, resolves each native `id`
to the layout's semantic `type`, and maps an area target to `positions`. A unit
or construction target has no representation in the current layout contract.

`kind` marks a document root, not a subtree. The shared per-side shapes do not
repeat it, because a seed, a map and a round are shared by both sides and belong
to the document that owns them.

## Match level and side level

One fact forces the state to have a level above `sides`.

The reinforcement offer is recorded once per round in `matchDatas`, not per
player, and both players choose from the same array. Across the corpus 49 of 50
`ChooseReinforceItem` actions satisfy `offers[Index] == ID` against that single
array; the remaining one is `ID=0, Index=-1`, the recorded form of declining.
It is the only field above `sides`, and the [scope](#scope) example shows it in
place.

The offer array is stored as recorded rather than rolled from a random state.
It is a function of that state, but reproducing the function means reproducing
the game's probability tables and draw order, and the layout format already
settled the general rule: a quantity the record states directly is stored, not
derived.

## Random state

No random stream is a state field. This section says what the four streams are,
why the two recorded ones are excluded, and what an installer still has to know
about them.

| Stream | Seed | Recorded |
| --- | --- | --- |
| `Match.random` | `BattleInfo.SystemSeed` | `matchDatas[round].randomStateData` |
| `Player.random` | `PlayerRecord.seed` | `playerData.randomStateData` |
| `Match.roundRandom` | `SystemSeed + round` | no |
| `FightTeam.random` | `(round + teamIndex) * 4444` | no |

The two recorded streams are excluded on the argument the reinforcement pool
follows: they decide what a later round is offered, and this round's offer is
already stated by `reinforce_offers`. Neither is read by a fight, so neither
changes anything a layout can express.

`Match.TakeRandomSnapshot` reads the field at offset `0x38`, which is
`Match.random`, and `PlayerSnapshotController.TakeSnapshot` reads offset `0xF8`,
which is `Player.random` rather than `Player.localRandom` at `0x100`. Both are
restored through `GRRandom.SetState`.

The measurements in this section are reproduced by
`work/research/random_state_support.py`, which reimplements the generator in
Python. The Rust one is `crates/simulation/src/random.rs`, already aligned
against a native attack stream.

### The derived stream is the one that is not recorded

`Match.GenerateRoundRandom(roundCount)` constructs `new GRRandom(seed + round)`,
taking the match seed from the match data and falling back to `1` on positive
overflow. That stream is a function of the seed and the round number, and the
game does not record it, which is the expected division: a derivable quantity is
recomputed and a historical one is snapshotted.

Only one of the two recorded streams turns out to be historical at all. The
match stream's position depends on how many draws were consumed. The player
stream never leaves its seed, so even the game's own snapshot of it carries no
information the seed does not.

### Neither recorded stream affects the fight

This matters for what a state is for. `FightTeam.RefreshRandomData` builds one
stream per team: it takes the round from `fightController.match`, adds the
team's own index, multiplies by `0x115C`, and stores the generator in the team's
`random` field. Formation composition seeds a separate generator with the match
seed plus the formation index. Neither formula has a term taken from a recorded
state.

Disassembly stops at construction. `FightTeam.GetRandom` has no indexed callers,
so the static call graph does not show combat consuming that stream, exactly as
it fails to show anything consuming the derived round stream. What supports the
claim is the simulator: it seeds a per-team generator with the same two
formulas and reproduces native captures tick by tick while never reading either
recorded state. A stream the fight depended on could not be missing from a
simulator that matches. That agreement is registered over a bounded domain, so
the claim is that no counterexample exists inside it.

The recorded states feed the meta layer only: `ReinforcePool`, `MapSystem`,
`MechPositionManager`, `UpgradeLevel` and `UserManager`. The match stream
changes what the next round offers, not how this round's battle resolves, which
is why excluding it costs a state nothing it claims to hold.

### The match stream is the seed advanced by a count nothing records

Every recorded match state is reachable from `SystemSeed`. Walking the stream
`GRRandom(SystemSeed)` generates and looking each snapshot up in it locates all
of them, over the 11 ranked matches and 101 round snapshots of the local set, the
furthest at 115 draws. The distances between consecutive snapshots are these:

| Position | Draws |
| --- | --- |
| Reaching the round 0 snapshot | 24 to 30 |
| Round 0 to round 1 | 17 to 24 |
| Round 1 to round 2 | 0 in every match |
| Each round from 2 on | 4 to 16, mean 9.2 over 68 transitions |

So the stream is reachable but not derivable. The count varies from match to
match at every position, which means `(seed, round)` does not determine it. A
document that wanted this stream would have to carry the four words, or the draw
count, which is only recoverable by searching the stream as this measurement
does. That is the cost excluding it avoids.

The floor of 4 is where the count meets the offer. Every round from 2 on records
exactly four reinforcement items, and no transition consumes fewer than four
draws, with nine consuming exactly four. That fits one draw per card, plus the
extra draws a rejection loop takes: `GRRandom` masks its projection up to a power
of two and redraws when the sample overshoots the range, so a weighted pick over
a pool whose weights do not sum to a power of two costs a variable number of
draws. Reproducing the count therefore means reproducing the pool and its
weights, which is the same reason `reinforce_offers` is stored rather than
rolled.

Two observations sit on top of that and are not explained. The zero between
rounds 1 and 2 says the round 2 offer was already rolled before the round 1
snapshot was taken, so a round's draws belong to the following round's cards.
And of the 68 later transitions, the counts 4, 5, 6 and 8 through 14 and 16 all
occur while 7 and 15 never do. Four independent rejection loops would put roughly
eight of the 68 at 7, so the internal structure of the roll is not simply one
masked draw per card. That is the thread to pull if the roll is ever reversed.

### The player stream is its seed

`playerData.randomStateData` equals the state of `GRRandom(PlayerRecord.seed)` in
all 202 ranked player-rounds of the local set, with no exception, and no seat's
value moves during its match. The four words are therefore one integer's worth of
information, and that integer is `PlayerRecord.seed`, which the record already
states once per player.

The mechanism explains the measurement rather than resting on it. A `Player`
holds two generators: `GetRandom` reads `0xF8` and `GetLocalRandom` reads
`0x100`. The snapshot takes `0xF8`. But `PrepareRandomData` and
`RefreshRandom(seedOffset)` both write `0x100`, seeding it with the player's own
seed plus a local offset, and `RefreshRandom` is what `UserManager` calls on
entering each deployment. The one thing that happens per round touches the other
field, which is why the snapshotted stream stays where it started.

Nothing reads it either. `GetLocalRandom` has no indexed callers at all, and the
one edge the call graph draws to `GetRandom`, from
`UserManager.OnEnterDeployment`, is an artifact of a trivial accessor being
inlined. That method loads its generator from `MatchModule.match` at `0x20` and
then from `0x38` on the `Match`, which is `Match.random`, the same field the
match snapshot takes; the round it passes alongside comes from `0x64`, the
match's `RoundCount`. The draw serves `GameRuleManager.TryGetRandomSupply`, so it
spends the match stream, and it belongs to a game rule whose `gameRules` list is
empty in every replay this machine holds.

No indexed call site reads `Player.random` at all, then. The stream exists, gets
snapshotted and gets restored, and nothing draws from it.

So this stream influences nothing observable in a 1v1, which is the strongest of
the three reasons it is not a field: it is not read, it is not history, and it is
not needed to state a position. An installer that wants the recorded generator
anyway can rebuild it from the replay's `PlayerRecord.seed`, and should assert
that `GRRandom(seed)` equals the recorded four words rather than trusting it. The
first replay where that assertion fails is a replay where the stream has a
position, and the question would have to be reopened.

A Training Ground match derives both seeds from the match. In the two such
replays here, seat 0 gets `SystemSeed` and seat 1 gets `SystemSeed + 1`, so an
installer setting up a local match can choose the seeds rather than copy them.
Ranked seeds show no such relation. The 22 of them differ from their match's
`SystemSeed` by amounts of either sign spanning the whole 32-bit range, none
appears in the first 200 draws of the match stream under five projections, and
the one account that plays in all 13 matches draws a different seed in each, so
it is not an account-level value either. A ranked seed comes from the server and
has to be recorded.

### An installer writes neither, and neither means zero

Because no state document carries either stream, an installer leaves both
generators as the match's own initialisation left them. That is a legal
position, merely not the recorded one. What it costs is that any roll the game
makes afterwards diverges from the record, which is safe exactly while every
offer is supplied rather than rolled.

Writing nothing must never be implemented as writing a zero state. `Match.ApplyRandomSnapshot`
dereferences `randomStateData` and its word list without a guard, and
`GRRandom.SetState` compares the incoming array's length against the internal
one and refuses a mismatch, so an empty list is an error rather than a default.
Worse, `[0, 0, 0, 0]` is a fixed point of xoshiro256\*\*: every later draw is
zero and stays zero. An installer that has no state to write must skip the call,
not write an empty one.

## Side state

Each side carries the six layout fields that project unchanged:
`techs`, `formations`, `constructions`, `contraptions`, `airdrop_shields` and
`terrains`. It adds the fields a layout has no reason to hold. The two layout
tower objects are deliberately absent: `blueprints`, `energy_tower_skills` and
`tower_strengthen_levels` are their source state, and the projection above
constructs the objects from those three fields. `battle_skills` also uses the
state panel shape defined below rather than the layout release shape.

```yaml
    blue:
      reactor_core: 197
      supply: 50

      shop:
        unlocked_units: [2, 9, 10, 15, 18, 21, 22, 25, 28, 30, 31]
        buys_remaining: 3
        unlocks_remaining: 1

      blueprints: [1, 2, 401]
      energy_tower_skills: [1]
      tower_strengthen_levels: [1, 1]

      equipment:
        - {id: 13030003}

      battle_skills:
        - index: 0
          id: 1100001
          cooldown: 1
        - index: 1
          id: 300005
          cooldown: 0

      next_index:
        unit: 29
        contraption: 15

      techs: { ... }
      formations: [ ... ]
      constructions: [ ... ]
      contraptions: [ ... ]
      airdrop_shields: [ ... ]
      terrains: [ ... ]
```

The last six keys are the layout fields that project unchanged, elided here
because [the layout document](layout.md) already defines them. A side always
writes all six, empty where it holds nothing.

`next_index` holds the two live allocators, recorded as `unitIndex` and
`contraptionIndex`. They are state, not a derived maximum. Comparing each
against the highest index actually present in the corpus:

| Allocator | Equal to highest index plus one | Higher |
| --- | ---: | ---: |
| `unitIndex` | 58 | 0 |
| `contraptionIndex` | 58 | 16 |

Contraptions are consumed when used, so the allocator runs far ahead of what
survives. One side moves through 2, 10, 15, 19 over four rounds while holding at
most three objects, and no index that vanishes is ever handed out again.

`unitIndex` happens to be recoverable in every round of this corpus, since the
newest unit always survived to appear in the roster. That is a property of these
four matches rather than a rule, and deriving one of two allocators would buy
nothing.

What the allocators carry that the object lists cannot is history. Buying a unit
and undoing it returns the visible state, and usually returns the allocator too,
but not always: of 66 round transitions, predicting the `unitIndex` delta from
net purchases explains 64 while ignoring undo explains 54, and one of the two
misses is a round where the allocator behaved as though the undo had not rolled
it back. Two positions that look identical can therefore differ here, which is
what a state diff has to be able to see.

`constructionIndex` is recorded by the game but is not an allocator under 1v1
rules, so it is left out. It reads 0 in round 0, takes its value for the match in
round 1, and never moves again; the construction list only ever shrinks, through
destruction. The value is not a property of the map either, since one match on
map 1001 deals each side two constructions and another deals one.
`MapSystem.LoadConstructionLayout` picks the group with
`IListExtensions.RandomElementSync` against a `GRRandom`, so the opening layout
is rolled, which also accounts for the match stream advancing between rounds 0
and 1. Nothing in a 1v1 match allocates from the result. Each construction still
carries its own `index`, which is its identity and is unaffected.

`tower_strengthen_levels` is a per-tower counter. `StrengthenTower` carries only
an `Index`, and the counter at that index rises by the number of net actions
naming it. All three transitions in the corpus close exactly.

It holds the same two numbers as the layout's `research_center.strength_level`
and `energy_tower.strength_level`, keyed differently. This document keys by
position in `BuildingManager.buildings`, because that is what
`PAD_StrengthenTower.Index` names and what `GetBuildingByIndex` resolves. A
layout keys by tower instead, and the adapter already bridges the two in
`resolve_core_tower`, which scans the same list for a `BuildingData.BuildingType`
of 1 for the energy tower or 2 for the research centre, requires exactly one
building of that kind, and returns its manager index.

The list is exactly two entries long in all 202 player-rounds of the local set,
so a 1v1 side holds precisely the two towers a layout names. Both draw from one
shared catalogue, `towerStrengthenDatas`, whose four rows cost 100, 150, 250 and
300 supply and raise the crystal's life, so a level is in `0..=4` for either
tower. Ranked play only ever reaches 2.

**Unresolved.** Which position is which tower is map data, and the binary does
not answer it. `MapSystem.LoadBuilding` walks the map's building entries, keeps
the ones `BuildingData.IsTowerData` accepts, constructs a `CrystalElement` for
each and appends it to the owning territory through `MapRegion.AddMapElement`.
`MapSystem.SetPlayerData` then walks each territory's element list in order and
hands every crystal to `BuildingManager.AddBuilding`, which appends to a plain
list. Nothing on that path sorts, and nothing compares `BuildingType`, so the
position is exactly the order in which the map asset lists its two towers. That
order lives outside the binary and may in principle differ from map to map.

A replay cannot settle it either, since a replay records no tower identity, and
the one fixture that would suggest an answer,
`tests/layouts/tuff-replay-round-7.yaml`, is referenced by nothing and carries
no provenance. Settling it takes one live readback of `GetBuildings` with each
entry's `BuildingType`, per map, which the adapter can already perform. Until
then the array is positional, a layout's fields are named, and only the
adapter's kind lookup connects them.

### Equipment is stock plus what is fitted

The replay records both halves. `playerData.equipmentDatas` is a list of
`{id, durability}`, and each `NewUnitData` carries an `EquipmentID` that is 0
when the unit carries nothing. The recorded inventory is everything the side
owns, fitted items included: over the 202 player-rounds of the local set, every
item fitted to a unit also appears in that side's inventory, without exception,
and 65 of the 71 rounds that hold any equipment at all list the same ID in both
places.

Owning something unfitted is rare but real. The inventory exceeds what is fitted
in 7 of 202 rounds, always by a single item, and one side carried an unfitted
`1308001` through four consecutive rounds. The same ID also appears twice in one
inventory in 2 rounds, so this is a multiset and not a set.

A state therefore stores the difference rather than the record. `equipment` lists
only what is not fitted, and a formation's own `equipment` names what it carries.
The two together enumerate everything owned, and neither can be derived from the
other: dropping the side list would lose an unfitted item, and dropping the
formation field would lose which unit carries what. Storing the recorded
inventory whole would instead let one document contradict itself, by listing an
item no formation carries beside a formation carrying an item the list omits.

That difference is what an installer has to undo. The game restores equipment in
two steps, creating the inventory in `ApplyEquipmentSnapshot` and then attaching
items by replaying `PAD_UseEquipment` from `RestoreEquipmentData`, so an adapter
must add the fitted items back to the inventory before it attaches them.

`durability` is `-1` in all 105 inventory entries of the local set, so a
normalized document omits it and an absent `durability` means `-1`. Equipment in
a standard 1v1 does not wear out and survives every round. The field exists because game rule `999903`
试验装备 hands out equipment that expires after one round, which is also what the
catalogue's `roundDuration` is for. Under that rule a fitted item's durability
would have no home in this format, and the rule that introduces it is the one
that should answer for it.

### Supply and reactor core

Both are per-map constants at the start and diverge only through play. The map's
row in `matchSettings`, keyed by the `MapID` the replay records, gives
`reactorCores`, `firstRoundSupply`, `roundSupplyIncreaseValue` and
`maxRoundSupply`. Every 1v1 map in build 2259 carries the same values.

| Setting | 1v1 value |
| --- | ---: |
| `reactorCores` | 4500 |
| `firstRoundSupply` | 200 |
| `roundSupplyIncreaseValue` | 200 |
| `maxRoundSupply` | 4000 |

`reactor_core` is recorded as it stands. It falls only as the outcome of a
fight, which no state can predict, and it can also rise: an advance team entry
carries its own `reactorCore` field, and the seat that opened one worth 100 in
the corpus shows 4600 at round 1 against the other seat's 4500.

`supply` is what the side can spend at the moment the state describes. The
round's income has already been added to it, and every purchase, upgrade and
sale since has already been applied. Like every other field here it is defined
after each action and not only at a round boundary, which is what makes the
projection in the opening section well defined at every point. It is the number
a purchase's legality is tested against and the number an installer writes.

That is deliberately not the number the replay stores. `playerData.supply` is
the residue from before the round's income is added, exactly like the two shop
counters. Round 1 records `0` for every side in the corpus while every side buys
two units in that same round, so the recorded value is not what the side had to
spend, and a document that copied it would be stating a quantity no rule reads.
At the opening of round 1 the field this document defines holds the map's
`firstRoundSupply`, 200, where the replay holds `0`.

Nothing has to bridge that gap yet. The offline replay-to-state conversion is
not being built, and both a live capture and an installer work against the
definition above rather than against the recorded field. What follows is only
where such a conversion would start.

The round's income arrives through `Player.AddRoundSupply(round, roundSupply)`.
It takes the round income, adds `extraFirstRoundSupply` when the round is 1,
adds `extraRoundSupply` and a modifier read from the player's dynamic `DataSet`,
floors the result at zero, and then adds it to `supply` unless the latch
described under excluded fields is set. A negative `roundSupply` argument makes
it ask `PlayerAgent.GetRoundSupply(round)` instead.

The `extraSupplyDatas` table is not part of this path in a ranked match. Its
`level1` through `level10` columns are read from `AIData`, so they are AI
difficulty handicaps rather than a per-player adjustment.

### The shop

The replay stores four shop numbers. A state stores two, and both differ from
what the replay holds.

`locked_units` is dropped because it is the complement of `unlocked_units`. The
union of the two lists is the same 32 unit IDs in all 74 ranked round-sides, and
the two never intersect, so one list plus the catalog determines the other.

`MaxUnlockCount` is dropped because it is 1 in all 74 ranked round-sides.
`Shop.UNLOCK_COUNT_PER_ROUND` is the constant 1, and the game adds a modifier
to it, so this is an assumption about standard 1v1 rather than an identity: a
source of unlock bonuses would break it.

The two counters that remain are stored as what is left, not as what was used,
because that is the number a legality check reads.

Neither can be read off the replay, for the same reason `supply` cannot: the
recorded counters describe the previous round. `UnlockCount` equals the allowance
minus the unlocks of the *preceding* round in all 58 transitions, with no
exception outside round 0, where it is `-1`. `BuyCount` behaves the same way
against the preceding round's purchases. The snapshot is taken before the round's
own reset, so a conversion would have to reconstruct these two rather than copy
them. That cost falls on a converter, not on the definition.

The purchase allowance itself is not in the replay at all.
`Shop.CalculateMaxBuyCount` computes it as `DataSet.GetDataInt(datas, 0) + 2`,
clamped at zero, where 2 is `Shop.BUY_COUNT_PER_ROUND` and the modifier is fed
by `OfficerData.ShopBuyCount` and `EnergyTowerSkillData.ShopBuyCountChangeValue`.
The allowance is real: purchases per round are exactly 2 in round 1 and never
exceed 3 in any later round, across all 58 round-sides. Which modifier raises it
to 3, and whether it can go higher, is not established.

The allowance does gate a purchase, and `buys_remaining` is exactly the number
the gate reads. `PAP_BuyUnit.Check` calls `ShopManager.CanBuyUnit(unitID)`, which
tests `TerritoryManager.CanAddUnit` for room in the deployment region and then
tail-calls a private overload; that tail call is an unresolved jump, which is why
the static call graph appears to stop at the region test. The private overload
tests the per-unit cap, then `Player.HasEnoughSupply`, and then, when
`ShopManager.hasBuyCountLimit` is set, returns a refusal if `Shop.buyCount` has
fallen to zero. The constructor sets that flag, and the only method that clears
it, `RemoveBuyCountLimit`, has no indexed callers.

`Shop.buyCount` is the same counter throughout. `Shop.Refresh` assigns it
`CalculateMaxBuyCount` at each round, `ReduceBuyCount` decrements it on a
purchase, and `AddBuyCount` restores it on an undo. Storing what is left rather
than what was used therefore stores the field itself.

`ShopManager.HasEnoughBuyCount` is not part of that path. Its one caller is
`MainUIMediator.OnClickFinishDeploy`, so it is the client asking whether the
player is about to end a round with purchases unspent.

The second counter in that check is not state here. `Shop.unitCounts` is a
per-card dictionary that only `Shop.Refresh` seeds, and only when
`BuyCountPerUnit` is above zero. It is not snapshotted, and ranked play shows it
is not binding: 122 of the 202 player-rounds of the local set buy the same unit
ID more than once in one round.

### Technologies stay flat

The replay groups technologies under the unit they belong to, as
`UnitData{id, techs, unlockedTechs}`. The layout stores one flat ascending array
instead, and a state keeps that.

The grouping is recoverable: a technology ID ends with the ID of the unit it
applies to, so `3925` and `425` both belong to unit `25` and `10204` to unit `4`.
All 80 entries in the corpus have an empty `unlockedTechs`, so the state of
being unlocked but not active does not arise in ranked play and the flat array
loses nothing.

### The research centre and the energy tower

Three native lists sit here. `blueprints` names what the research centre has
activated, `energy_tower_skills` what the energy tower has activated, and
`tower_strengthen_levels` how far each of the two towers has been reinforced.

The blueprint catalogue has 17 rows in two kinds. `bpType: 1` is an upgrade
chain: `4` 进攻强化 and its successor `401`, `5` 防御强化 and its successor `501`.
`bpType: 2` grants a commander skill, named by the row's `mapID`. A match setting
lists ten of them for a 1v1, `[1, 2, 3, 4, 5, 1001, 1003, 1004, 1005, 1006]`,
along with five energy tower skills, `[1, 3, 4, 5, 6]`.

The setting's list is not the pool. `PrepareBlueprint` keeps a row only when it
is the first level of its chain and either does not need research or the match
enables research. Written as the disassembly has it:

```
keep = IsFirstLevel(data) && (!NeedResearch(data) || IsEnableBlueprintResearch())
```

`NeedResearch` is a nonzero `researchTime`, which the five rows `1001` and `1003`
through `1006` carry and no other row does. `IsEnableBlueprintResearch` reads
`enableResearchSkill` off the match's game rules, and exactly one of the 13
shipped rules sets it: `999917` 战场技能研发, whose own text says those skills move
to the research centre instead of arriving as reinforcements. Every replay this
machine holds carries an empty `gameRules`, the tracked corpus and the wider
local set alike, so the pool a standard 1v1 actually offers is `[1, 2, 3, 4, 5]`,
none of which takes time to research.

The recorded blueprints agree: across 202 player-rounds
the only blueprints ever held are `1`, `2`, `3`, `4`, `5`, `401` and `501`, the
last two reached through the chain. No side ever holds one that needs research.

That is why `research_queue` is not a field here. Its type is
`ResearchData{blueprintID, startRount}`, the game's own spelling, and it holds a
blueprint from the moment research starts until `UpdateResearchProgress` calls
`Active` and the skill joins the panel. Under standard 1v1 rules nothing can
enter it, and all 202 player-rounds hold it empty. The field belongs to game rule
`999917`, and it is that rule, not this format, that should introduce it.

#### Why these do not fold into the skill panel

The panel plus the two chain levels do reproduce `blueprints`. Subtracting the
skills a side's officers grant, mapping each remaining panel entry back through
`mapID`, and adding the chain level rebuilds the recorded list in all 202
player-rounds of the local set. That is not enough to drop the field.

The panel is a union of three sources, and one of them is history rather than
state. Reinforcement drops add commander skills to the same panel from the same
ID space, and they dominate it: of the 14 skill IDs the local set shows, 3 come
from a blueprint, 2 from an officer, and the remaining 9 from a drop. Recovering
`blueprints` means subtracting those, and a state does not record which entries
came from a drop. It worked here only because the two catalogues do not collide.

That disjointness is maintained by hand rather than guaranteed. Game rule
`999917` is the visible seam: it swaps blueprint `1` for `1002`, which grants the
same skill `400002` at a different price and research time, and its
`excludeReinforce` list removes the drop versions of the skills the research
centre is about to sell. Both are edits to data, made to keep one skill from
having two provenances. A state that reconstructs `blueprints` from the panel
would depend on those edits staying correct, and would need the match's game
rules to read them, which it does not carry.

Price is not an argument either way. `GetActiveSupply` returns the blueprint's
own `supply` plus `BattleInfo.BlueprintIncreaseSupply` times the number already
activated, and that multiplier is 0 in all 22 shipped match settings and in all
six recorded matches.

#### The chain half is duplicated by the officer list

Activating a chain blueprint also adds its product officer: `4` adds `20310`,
`401` adds `20311`, `5` adds `20300`, `501` adds `20301`. In all 202
player-rounds of the local set the officer appears exactly when the blueprint
does. A chain replaces rather than appends, since `Active` goes through
`ReplaceBlueprint`, so a side holding 进攻强化II lists `401` alone and never `4`
beside it. That last point rests on the eight rounds where a chain reached level
two, and a downloaded replay spells it the other way, which is one of the reasons
`tests/grbr/README.md` rules that class out.

| Chain officers implied by `blueprints` | Officers listed | Player-rounds |
| --- | --- | ---: |
| none | none | 119 |
| 20300, 20310 | 20300, 20310 | 50 |
| 20310 | 20310 | 19 |
| 20300 | 20300 | 6 |
| 20300, 20311 | 20300, 20311 | 5 |
| 20301, 20310 | 20301, 20310 | 3 |

So the blueprint list and the product officers in the recording are two
spellings of one fact. `blueprints` owns it in a state, and `techs.officers` must
not name `20300`, `20301`, `20310` or `20311`. The projection carries the chain
half into a layout as `research_center.attack_level` and `defense_level` on 0 to
2; `docs/officers.md` already states that the derived officers must not also
appear there. Neither document enforces the invariant yet.

#### The energy tower keeps all five

A state lists every activated energy tower skill, and a layout keeps only the two
that reach a fight, `5` 强化瞄准 as `range_enhancement` and `6` 高速移动 as
`movement_enhancement`. The projection is behaving correctly: `3` and `4` are
recruitment and reach a fight only through the units they produce, and `1`
快速补给 is economic. `1` is the one that must not be dropped from a state, since
it pays `supplyChangeValue: 200` now against `nextRoundSupplyChangeValue: -300`
later, so it outlives the round that activated it. It is also the only energy
tower skill the local set shows, in 15 player-rounds.

## The skill panel

A layout lists released skills only, and its list order is the release order.
A state lists the whole panel, because an action references a skill by
its panel slot and a slot that the state does not carry cannot be resolved.

```yaml
      battle_skills:
        - index: 0
          id: 1100001
          cooldown: 1
        - index: 2
          id: 300005
          cooldown: 0
          release:
            order: 1
            target:
              area: [{x: 50, y: 44}, {x: 189, y: 38}]
        - index: 3
          id: 900001
          cooldown: 0
          release:
            order: 2
            target:
              unit: 12
```

`index` is the position at which the skill joined the panel, and it is the key
`ReleaseCommanderSkill.SkillIndex` names. It is lifelong identity, not a
recyclable slot: across all 202 panels of the local set the panel never shrinks
and never reorders, and each round's panel is a prefix extension of the previous
one. The panel is therefore sorted by `index`, which duplicate skill IDs make
the only well-order available, since 16 of those 202 panels hold the same ID
twice.

`cooldown` covers a skill that cannot be released this round. A layout cannot
express one, because a layout only names releases.

`release` is present on a skill released this round, and `order` states where in
the release sequence it falls. Moving release order into an explicit field is
what lets the collection be sorted at all: in a layout the array position *is*
the order, which is why `battle_skills` is the one collection the layout leaves
as written.

### The release target is exclusive

A release either covers an area or points at one object. The three target forms
are one enum, so a document cannot state two of them.

The recording does not look like this. Every one of the 101 recorded releases
carries at least one position, and a pointing release carries a position beside
its `UnitIndex` or `ConstructionIndex`:

| Recorded shape | Releases |
| --- | ---: |
| `Positions[1]` + `UnitIndex` | 46 |
| `Positions[1]` | 23 |
| `Positions[3]` | 20 |
| `Positions[1]` + `ConstructionIndex` | 6 |
| `Positions[2]` | 6 |

`UnitIndex` and `ConstructionIndex` never appear together, so those two are
exclusive as recorded. The position beside them is the player's click point, not
the target's location: of the 45 unit-targeted releases that survive undo
collapse, 7 name a unit whose position can be reconstructed from the same
round's action log, and all 7 differ from the click point by a few map units,
for instance a click at `(82, -171)` on a unit standing at `(85, -170)`.

A click point that resolves to a unit has no effect on the fight, and two
different click points on the same unit denote the same state. Storing it would
let one state have many documents, so the state stores the resolved target and
the executor supplies a coordinate, using the target's centre, when the native
call needs one.

The target form belongs to the release and not to the skill. Skill `900001`
points at a unit in some releases and at a construction in others.

Area lengths are fixed per skill and range from one to three points, matching
the `Positions` column of the [battle skill index](battle_skill.md).

## Normal form

| Collection | Order |
| --- | --- |
| `battle_skills` | ascending `index` |
| `equipment` | ascending `(id, durability)`, a multiset |
| `formations`, `constructions`, `contraptions` | ascending `index` |
| `techs.officers`, `techs.units` | ascending ID |
| `shop.unlocked_units` | ascending ID |
| `blueprints`, `energy_tower_skills` | ascending ID |
| `airdrop_shields` | ascending `(x, y)` |
| `terrains` | ascending `type`, then control points |
| `reinforce_offers` | as recorded; `ChooseReinforceItem` names a position in it |
| `tower_strengthen_levels` | by building-manager position |

The rule behind the first three rows is that a collection is ordered by the key
the actions use to reference it, so document order and action resolution can
never disagree. `equipment` may repeat an entry, since a side can own two copies
of one item, and it sorts on the `durability` an absent field implies, `-1`.

One cross-check goes with that order rather than in it. Each entry of
`next_index` must be greater than the highest index in the collection it
governs, so an allocator cannot drift away from the objects it has already
handed out. The check is what a sentinel entry in the list itself would buy,
without making a typed, homogeneous collection carry an element of a second
shape that a layout would then also have to admit.

## Rebuilding a state offline

Most of a round's state can be read out of a replay without running the game.
Three fields cannot be copied, because the snapshot is taken before the round's
own reset: `supply` and the two shop counters state what stood before the round's
income and allowances arrived, and each is treated in its own section above.
Nothing turns on closing that gap yet, since the conversion is not being built.

The unit roster comes straight from `playerData.units`, a list of `NewUnitData`
carrying `id`, `Index`, `RoundCount`, `Durability`, `Exp`, `Level`, `Position`,
`EquipmentID`, `IsRotate` and `SellSupply`. It is populated from round 1 onward
and empty only in round 0, before the opening team is placed. Fallen units leave
holes, so the roster is the survivors, and `max(Index)` is one less than that
side's `unitIndex` in all 180 populated player-rounds of the local set.

The same roster is recorded a second time at match level, in
`matchDatas[round].lastFightResult.Reports[seat].unitDatas`, one report per seat
in seat order. Where both exist the two agree exactly, in all 158 such
player-rounds. Rounds 0 and 1 have no `lastFightResult`, since they precede the
first fight, which is the reason to read the per-round list rather than the
report.

## Excluded fields

| Field | Why it is not in the state |
| --- | --- |
| `playerData.preRoundFightResult` | Neither a decision nor a simulation input |
| `playerData.IsSpecialSupply` | Inert in this build, see below |
| `playerData.researchQueue` | Unreachable without game rule `999917`, see above |
| `matchDatas.deadCount` | Zero in every round of the corpus |
| `matchDatas.teamRanks` | Seat ordering; no mechanism is known to read it |
| `matchDatas.poolOPs` | Reinforcement pool bookkeeping for later rounds, see below |
| `matchDatas.RoundExcludeReinforce` | The same, per round rather than permanently |
| `matchDatas.randomStateData` | Decides a later offer roll only, see the random state section |
| `playerData.randomStateData` | Nothing draws from that stream, see the same section |

`IsSpecialSupply` is `Player.isLockSupplyForSnapshot`, which
`PlayerSnapshotController.ApplySnapshot` passes straight back as the second
argument of `Player.SetSupply`. It is not an income correction. It is a one-shot
latch read by `AddRoundSupply`: when it is set, that method computes the round
income as usual, clears the latch, and skips adding the income to `supply`. It
means "this supply was set directly, so do not stack this round's income on
top".

It is excluded because it is always clear under 1v1 rules. `SetSupply` is its
only writer and every indexed call site passes `false`, including both of the
ones in the Training Ground command `MAP_ChangePlayerData`, so the only way it
becomes true is restoring a snapshot in which it already was. It is `false` in
all 90 player-rounds of the corpus, Training Ground matches included.

It matters anyway to anything that installs a state rather than reading one, so
it is named again under the adapter capabilities below.

`poolOPs` and `RoundExcludeReinforce` are the reinforcement pool's bookkeeping,
and both are excluded on one argument: they decide what a *later* round may be
offered, while this round's offer is already stated by `reinforce_offers`. A
state is not a self-contained machine that generates its own successors. It
takes the offer as an input, so it does not have to carry what would produce
one.

They are worth describing anyway, because an exporter meets them and because
neither is what it first looks like. `poolOPs` is
`ReinforcePool.m_ReinforceOperation`, a list of `(op, id)` pairs replayed by
`ApplayOperation`: `0` removes the id from every level list of
`m_ReinforceMap`, `1` reads the item's level and adds it to the list one above.
`SelectReinforce` writes removals when a card is taken, plus the taken card's
siblings in the same `typeID` group; `OnNewRound` writes both kinds as officer
availability is re-tested against the side's units. An item whose catalogue row
sets `canRepeated` is never removed, which is why an officer both players took
can be absent from the log.

`RoundExcludeReinforce` is `m_RoundExclude`, a map from round to a set of ids,
and it does not touch pool membership at all. `CheckAppear(id, round)` reads it
at draw time, alongside the item's own `earliestRound` and `latestRound` window,
so an excluded item stays in the pool and is merely invisible for that one
round. `RoundRand` writes it while rolling, into the key `round + 1`. The
recorded sets bear that out: over the 27 local matches the same 14 ids appear
every time, at a single round key in 12 matches and at two keys in 3, never
earlier than round 5.

Neither can stand in for the other. A removal is permanent and undone only by an
add; an exclusion expires when its round passes.
`work/research/reinforce_pool_support.py` reproduces the log measurements.

## Capabilities the adapter still lacks

A state cannot be captured today. The adapter's readback covers the layout
projection, so supply, the shop, the allocators, the full blueprint and Energy
Tower skill lists, and the panel cooldowns have no complete capture path.

A state can be rebuilt offline from a replay, as above, so the capture gap
blocks live work rather than corpus work.

Installing a state is a separate problem from capturing one, and `supply` is
where the two differ. The field already includes the round's income, so writing
it directly leaves the game free to add that income a second time, which is what
`Player.isLockSupplyForSnapshot` exists to prevent. An installer therefore has to
set that latch even though no state document carries it.
