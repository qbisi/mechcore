# Layout replay

## Scope

A layout replay is a `.grbr` file that states one [layout](layout.md) as a
replay the game fights. `mechcore convert <layout.yaml> --to grbr <replay.grbr>`
writes it, `convert <layout.yaml> --to mcfr --backend game <out.mcfr>` writes one and records it, and the Adapter's
`record_replay_round` fights it as it fights any replay
([adapter.md](../adapter/adapter.md#record_replay_round)).

It rests on one property of the game's replay: a round opens from the
`PlayerRoundRecord` snapshot that record keeps for it, not from the rounds
before it, and the round's recorded decisions are then played on it. So a
layout is a replay whose layout round opens from a snapshot of the layout's
side and whose decisions are the ones a layout states as made this round: the
battle skills it releases, the Energy Tower skills it activates, the units
that join the side during the round, and the moves that put units in place.

This document defines what such a file states. It is not a definition of the
`.grbr` format, which is the game's: a layout replay is one file the game
reads, written with the members the game's own replays carry. Reading a
replay the game saved is [match.md](match.md#converting-a-replay).

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
constants a match states once by being that format
([match.md](match.md#the-header)): `VS_1_1`, the phase durations, the round
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
| `units` | every legacy unit the round opens with, below |
| `unitIndex` | the unit allocator, below |
| `officers` | the side's officers, a chain blueprint's among them |
| `bluepints` | the chain blueprints those officers stand for |
| `commanderSkills` | the battle skill panel, below |
| `activeTechnologies` | the side's technologies, one `UnitData` row per unit type |
| `equipmentDatas` | every item a unit wears, each with durability −1 |
| `contraptions` | each contraption's `index`, type and position, and `contraptionIndex` one past the highest |
| `towerStrengthenLevels` | the two tower levels, 0 when the layout names none |
| `constructionSnapshotDatas` | each construction's `Index`, type and position, with one durability entry per segment: five for a Defensive Wall, one for anything else; `constructionIndex` one past the highest |
| `shop.unlockedUnits` | nothing |
| `supply` | 10000 when the round upgrades a delivered squad, 0 otherwise |

A unit is a `NewUnitData`: its type, `Index`, `Position`, `IsRotate`, the
equipment it wears, `Level` counted from 0 where a layout counts from 1, and
`Exp`, the experience within its level.

The panel holds one slot per battle skill the layout releases, in release
order, each ready this round. After them it holds one slot per standing
`battle_skills` entry, standing shields before standing oil areas, each of the
entry's skill and holding the object in its `rangeItems` for the game to
restore: a
Shield Airdrop as one centre with no lifetime, and an oil area as the release's
two control points, the points still standing as a `ByteMask`, each standing
point's `12 x 12` grid or nothing for a whole one, and one round left. Red's
grids are turned half a turn, as its coordinates are.

## The round's decisions

The layout round's `actionRecords` hold, in this order:

1. each delivered squad upgraded to its layout level, fitted, and moved to its
   layout position and facing;
2. each unit that joins during the round, in index order, added, fitted, and
   moved to its layout position and facing;
3. each Energy Tower skill activated;
4. each battle skill released from its slot, at its positions, in the layout's
   order, which is the order its side's skills draw their scatter in;
5. `PAD_FinishDeploy`, when any decision precedes it.

The layout's `legacy_index` divides a side's units. One below it is legacy:
the snapshot holds it, but for a squad an officer delivers. Every other unit
joins during the round. A `MAD_AddUnit` adds it, carried by a
`PAD_TestCommand`, because a replay plays a match action as the test command
that carries it, as the Training Ground performs one. The action names the
side's seat, 0 for blue and 1 for red, the unit's type, its layout level and
its index, and asks for no fixed position in the side's main deployment area:
region 1 of blue's territory and 4 of red's. The game places it there, and the
move takes it to its layout position. A move onto a flank from another region
is what makes a unit travel, so a travelling unit is one that joins. A legacy
unit does not move, since a unit the round opens with does not from round 2
on, and a settled flank unit is one.

When the side's joining units are the allocator's next indices, from
`legacy_index` on, the action's index is `-1` and the allocator hands each its
own, as buying it did; otherwise each states its own, which leaves the
allocator at `legacy_index`. As the round ends the game enters the next
round's deployment, and an officer due then delivers its squad at the
allocator, so an allocator left behind would hand that squad an index a
joining unit already holds.

A squad an officer's schedule hands out as the round opens arrives on top of
the snapshot, so the snapshot does not hold it again. It becomes the layout's
unit of the same type, at that squad's level or above, without experience,
and its decisions upgrade, fit and move it. It is the last legacy unit: the
allocator opens at its index, which the delivery takes, one below
`legacy_index`. Without a delivery the allocator opens at `legacy_index`. A
standard 1v1 never deals a side two officers that deliver in one round, and
the order two would deliver in is not recorded, so a side that two deliver to
is refused.

## What a layout replay refuses

A layout that compiles is refused only when a replay cannot open or play it,
and the refusal names each part:

- no seed;
- two officers that each deliver a squad as the round opens;
- a squad an officer delivers as the round opens with no unit of the side to
  become;
- squads that are not the last legacy units;
- an officer that delivers a squad as the next round opens, beside joining
  units that skip an index, since they leave the allocator behind them;
- a unit that joins during the round with experience, since a round's
  decisions hand out none;
- a travelling legacy unit, since a replay moves none;
- a technology no unit owns.

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

None.
