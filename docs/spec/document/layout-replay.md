# Layout replay

## Scope

A layout replay is a `.grbr` file that states one [layout](layout.md) as a
replay the game fights. `mechcore replay convert <layout.yaml> <replay.grbr>`
writes it, `game.record_layout` writes one and records it, and the Adapter's
`record_replay_round` fights it as it fights any replay
([adapter.md](../adapter/adapter.md#record_replay_round)).

It rests on one property of the game's replay: a round opens from the
`PlayerRoundRecord` snapshot that record keeps for it, not from the actions of
the rounds before it. So a layout is a replay with one deployment round whose
snapshot is the layout and which carries no action, and the fight the game
plays from that round is the layout's fight.

This document defines what such a file states and what it leaves out. It is
not a definition of the `.grbr` format, which is the game's: a layout replay is
one file the game reads, written with the members the game's own replays
carry. Reading a replay the game saved is [battle.md](battle.md#converting-a-replay).

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

`playerRecords` holds two `PlayerRecord`s, blue first. Each opens every round up
to the layout's with a `PlayerRoundRecord` and no `actionRecords`. Round 0 is
the opening and holds nothing. The layout's round holds the side's units, each
a `NewUnitData`:

| Field | Value |
| --- | --- |
| `id` | the unit's type ID |
| `Index` | the formation's `index` |
| `Position` | the formation's `position` in the board's frame: blue's as written, red's negated |
| `IsRotate` | `rotated` |
| `Level` | 0, the first level |
| `Exp` | 0 |

`unitIndex` is one more than the highest index the side holds.

`matchDatas` holds one `MatchSnapshotData` per round up to the layout's, each
with an empty random state.

## What a layout replay states

A layout replay states a layout's units and nothing else of it. A layout that
holds any of the following is refused, and the refusal names each field:

- a round other than 1;
- officers, techs, energy tower skills, or a tower strengthen level above 0;
- constructions, contraptions, airdrop shields, terrains or battle skills;
- a unit whose level is not 1, or that holds experience, equipment, or is
  travelling;
- no seed.

## Normal form

A layout replay is a function of its layout, its seed and the build: the same
three always write the same bytes. Players come blue then red, rounds ascend,
and each side's units come in the layout's order.

## Excluded fields

| Field | Why it is not written |
| --- | --- |
| `PlayerRoundRecord.actionRecords` | The snapshot is the position the fight starts from; an action would move it |
| `PlayerSnapshotData.randomStateData`, `MatchSnapshotData.randomStateData` | A fight draws from `Match.roundRandom` and `FightTeam.random`, which the game derives from `SystemSeed` and the round ([state.md](state.md)) |
| `PlayerRecord.data.unitDatas` | A side's tech loadout, which offers a technology and fields none |
| `PlayerRecord.data.style` | Skins |
| `PlayerRecord.seed`, `id`, `name`, `ad` | Nothing a fight reads; the two players are named by side |
| `shop`, `supply`, `reactorCore` beyond their neutral values | Decide what a side may buy, not what fights |

## Unresolved

**Whether the rest of a layout goes into the snapshot as it stands.** The
snapshot is taken before a round opens, and a round's opening delivers what the
side's officers hand out and pays its income. A layout's officers written into
the snapshot would be delivered again, so the writer either states a layout's
opening as its pre-opening snapshot or states the officers some other way.

**Whether a battle's round is written as a layout replay or as the battle's
own replay.** A battle round's fight starts from what `doc project` writes, and
a layout replay of that projection fights it. A replay holding the battle's
whole stream would also carry its decisions, which the battle states in order
without the times, undos and pool bookkeeping the game's replay records.
