# Layout replay

## Scope

A layout replay is a `.grbr` file that states one [layout](layout.md) as a
replay the game fights. `mechcore replay convert <layout.yaml> <replay.grbr>`
writes it, `game.record_layout` writes one and records it, and the Adapter's
`record_replay_round` fights it as it fights any replay
([adapter.md](../adapter/adapter.md#record_replay_round)).

It rests on one property of the game's replay: a round opens from the
`PlayerRoundRecord` snapshot that record keeps for it, not from the rounds
before it, and the round's recorded decisions are then played on it. So a
layout is a replay whose layout round opens from a snapshot of the layout's
side and whose decisions are the ones a layout states as made this round: the
battle skills it releases, the Energy Tower skills it activates, and the moves
that make a unit travel.

This document defines what such a file states. It is not a definition of the
`.grbr` format, which is the game's: a layout replay is one file the game
reads, written with the members the game's own replays carry. Reading a
replay the game saved is [battle.md](battle.md#converting-a-replay).

## The file

A `.grbr` is a .NET `BinaryFormatter` stream whose root is one
`GameRiver.Replay` from the `GRCore` library. Its members, in order:

| Member | Value |
| --- | --- |
| `battleID` | `layout` |
| `version` | the build number, the last component of `GAME_VERSION` |
| `seat` | 0 |
| `mapID` | the layout's `map_id`, or 1021 when it names none |
| `playerDatas` | a `List<Replay+PlayerData>` of two, `{id: 1, name: blue}` and `{id: 2, name: red}` |
| `realBattleRecordData` | the battle record below, as UTF-8 XML behind a byte order mark |
| `<CreateTime>k__BackingField` | the zero `DateTime` |

The record is the XML `XmlSerializer` writes for a `BattleRecord`.

## The battle record

`BattleInfo` holds the layout's `seed` as `SystemSeed`, the map, and the 1v1
constants a battle states once by being that format
([battle.md](battle.md#the-header)): `VS_1_1`, the phase durations, the round
cap. Advance teams, reinforcement and unit reinforcement are off, as they are in
the Training Ground; construction is on. `Version` is the build number and
`Seat` is 0.

`playerRecords` holds two `PlayerRecord`s, blue first, and `matchDatas` one
`MatchSnapshotData` per round up to the layout's, each with an empty random
state. A player's `data.unitDatas` is its loadout: one row per unit type the
side has technologies for, holding those technologies, since a technology is
researched only out of the loadout. Each player opens every round from 0 to the
layout's with a `PlayerRoundRecord`. Every round before the layout's is empty
and carries no decision.

Every position is written in the board's frame, which is blue's own: blue's as
the layout writes it, red's negated.

## The snapshot

The layout's round opens with this `playerData`:

| Member | Holds |
| --- | --- |
| `units` | every unit the round opens with, below |
| `unitIndex` | the unit allocator, below |
| `officers` | the side's officers, a chain blueprint's among them |
| `bluepints` | the chain blueprints those officers stand for |
| `commanderSkills` | the battle skill panel, below |
| `activeTechnologies` | the side's technologies, one `UnitData` row per unit type |
| `equipmentDatas` | every item a unit wears, each with durability −1 |
| `contraptions` | each contraption's `index`, type and position, and `contraptionIndex` one past the highest |
| `towerStrengthenLevels` | the two tower levels, 0 when the layout names none |
| `constructionSnapshotDatas` | each construction's `Index`, type and position, with one durability entry per segment: five for a Defensive Wall, one for anything else; `constructionIndex` one past the highest |
| `shop.unlockedUnits` | the unit types the round buys |
| `supply` | 10000 when the round buys or upgrades, 0 otherwise |

A unit is a `NewUnitData`: its type, `Index`, `Position`, `IsRotate`, the
equipment it wears, `Level` counted from 0 where a layout counts from 1, and
`Exp`, the experience within its level.

The panel holds one slot per battle skill the layout releases, in release
order, each ready this round. After them it holds one slot per object an
earlier release left standing, whose `rangeItems` entry the game restores: a
Shield Airdrop as one centre with no lifetime, and an oil area as the release's
two control points, the points still standing as a `ByteMask`, each standing
point's `12 x 12` grid or nothing for a whole one, and one round left. Red's
grids are turned half a turn, as its coordinates are.

## The round's decisions

The layout round's `actionRecords` hold, in this order:

1. each delivered squad upgraded to its layout level, fitted, and moved to its
   layout position and facing;
2. Mass Recruitment activated, when the purchases need its extra one;
3. each bought unit bought where the deployment area is free, upgraded,
   fitted, and moved to its layout position and facing;
4. each Energy Tower skill activated;
5. each battle skill released from its slot, at its positions, in the layout's
   order, which is the order its side's skills draw their scatter in;
6. `PAD_FinishDeploy`, when any decision precedes it.

A unit the round opens with does not move from round 2 on, so a unit that
travels, which is a unit a move took onto a flank from another region this
round, is bought during the round rather than held by the snapshot. A
purchase takes the unit allocator's next index, whatever the record asks for,
so every unit from the first index the round creates after its deliveries
through the last travelling one is bought, in index order: those units were all
created this round. The allocator opens at the first index the round creates.
Every other unit opens the round in the snapshot, a settled flank unit among
them. A round allows two purchases, one more for each Additional Deployment
Slot the side holds, and one more when Mass Recruitment is activated, which
the round does only when its purchases need it; it changes nothing a fight
sees.

A squad an officer's schedule hands out as the round opens arrives on top of
the snapshot, so the snapshot does not hold it again. It becomes the layout's
unit of the same type, at that squad's level or above, without experience: the
allocator opens at that unit's index, which the delivery takes, and the
round's decisions upgrade, fit and move it. Several squads take consecutive
indices in the order of the officers that deliver them. Without deliveries the
allocator opens one past the highest index the snapshot holds.

## What a layout replay refuses

A layout that compiles is refused only when a replay cannot open or play it,
and the refusal names each part:

- no seed;
- a squad an officer delivers as the round opens with no unit of the side to
  become, at consecutive indices;
- a unit the round buys with experience, since a round's decisions hand out
  none;
- an index among the units the round buys that no such unit holds;
- more purchases than the round allows;
- a travelling unit the deployment area has no free place to be bought at;
- a terrain no skill leaves, or a technology no unit owns.

## Normal form

A layout replay is a function of its layout, its seed and the build: the same
three always write the same bytes. Players come blue then red, rounds ascend,
a side's units and objects come in the layout's order, and decisions come in
the order above.

## Excluded fields

| Field | Why it is not written |
| --- | --- |
| `PlayerSnapshotData.randomStateData`, `MatchSnapshotData.randomStateData` | A fight draws from `Match.roundRandom` and `FightTeam.random`, which the game derives from `SystemSeed` and the round ([state.md](state.md)) |
| `PlayerRecord.data.style` | Skins |
| `PlayerRecord.seed`, `id`, `name`, `ad` | Nothing a fight reads; the two players are named by side |
| `MatchActionData.Time`, `LocalTime` | Decisions are numbered in order; nothing a fight reads keeps a clock |
| `reactorCore` and the shop's allowances | Decide what a side may do in later rounds, not what fights |

## Unresolved

**Whether a battle's round is written as a layout replay or as the battle's
own replay.** A battle round's fight starts from what `doc project` writes, and
a layout replay of that projection fights it. A replay holding the battle's
whole stream would also carry its decisions, which the battle states in order
without the times, undos and pool bookkeeping the game's replay records.
