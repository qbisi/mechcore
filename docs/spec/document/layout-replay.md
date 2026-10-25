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
layout is a replay whose layout round opens from a snapshot and applies the
commands needed to restore the layout's projected fight state. It preserves
the units' sources, experience and travelling membership, the battle skills
released this round, and the Energy Tower skills activated. It reconstructs
the stated position, not the deployment history that produced it.

The shared match-to-layout projection used by `convert` and live match
settlement warns when Stealth Cloak was acquired after Phantom Ray formations
entered flank deployment. The simulator always settles with technologies
activated before deployment, which can differ from that native history.
Projection and execution continue, and actual settlement follows the selected
backend's rules. Players and bots should activate technologies before flank
deployment to benefit from them earlier. No layout field records this order.

The same projection warns when Aerial Mode is acquired during a round with
Aerial Specialist and travelling Void Eyes. The native life corrections can
depend on research/flank order, and the state does not preserve that history.
This warning remains in canonical action order, since normalization may
already have erased the distinction. Execution still uses the projected
layout; match verification skips that round's fight comparison and checks
the other transitions normally.

This document defines what such a file states. It is not a definition of the
`.grbr` format, which is the game's: a layout replay is one file the game
reads, written with the members the game's own replays carry. Reading a
replay the game saved is [match.md](match.md#converting-a-replay).

## Reading a layout replay

`convert <replay.grbr> --to layout` restores the synthetic replay's selected
round from its legacy snapshot and native restoration commands, including
newly joined and delivered units, recovery, training, travelling, equipment,
Energy Tower activations and battle skills. The replay's final positive round
is its only available one; earlier snapshots are empty placeholders, not
captured battles.

The reader admits the canonical XML this writer emits for the pinned game
build. It re-encodes the restored layout and requires the embedded XML to
match, refusing any native field or command it would otherwise drop. This is
not a general interpreter for Training Ground test commands. The native game
backend opens the original file directly and does not require this projection.

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
`Exp`, the experience within its level. A synthetic first-round legacy unit
stores native `RoundCount=1`: `MechTeam.IsOldUnit` requires a positive count,
and first-round loading does not age the snapshot. Later rounds store zero
and their entry ages it before combat. A non-positive snapshot count does not
prevent that ageing. Joined units are therefore added by the round's actions
and retain their native new-unit state; their experience restoration is
described below.

The panel holds one slot per battle skill the layout releases, in release
order, each ready this round. After them it holds one slot per standing
`battle_skills` entry, standing shields before standing oil areas, each of the
entry's skill and holding the object in its `rangeItems` for the game to
restore: a
Shield Airdrop as one centre with no lifetime, and an oil area as the release's
two control points, the points still standing as a `ByteMask`, each standing
point's `12 x 12` grid or nothing for a whole one, and one round left. Red's
grids are turned half a turn, as its coordinates are. Next, when the round
recovers a delivered squad, it holds a Field Recovery slot, ready, which moves
no other slot. Finally it holds one ready Intensive Training slot per
joining or delivered unit whose full experience bar must be restored, in unit
index order, with `isActive=false` and `coolingRound=0`. These auxiliary slots
do not change the fight's skill indices.

## The round's decisions

The layout round's `actionRecords` hold, in this order:

1. each squad the layout lists in `recovered` recovered from the Field
   Recovery slot, by its index;
2. each delivered squad upgraded to its layout level, fitted, trained when
   needed, and moved to its layout position and facing;
3. each travelling legacy unit's position and travelling membership restored
   by native forced undo commands, followed by the current position of each
   other legacy unit whose `recordPosition` or `recordRotated` differs;
4. each unit that joins during the round, in index order, added, fitted,
   trained when needed, and
   moved to its layout position and facing;
5. each Energy Tower skill activated;
6. each battle skill released from its slot, at its positions, in the layout's
   order, which is the order its side's skills draw their scatter in;
7. `PAD_FinishDeploy`, when any decision precedes it.

A unit's `source` says where the round takes it from. A `legacy` unit the
snapshot holds; a `delivered` squad an officer delivers; a `joined` unit is
added during the round, including the advance team in round 1. A `MAD_AddUnit`
adds it, carried by a `PAD_TestCommand`, because a replay plays a match action
as the test command that carries it, as the Training Ground performs one. The action names the
side's seat, 0 for blue and 1 for red, the unit's type, its layout level and
its index, and asks for no fixed position in the side's main deployment area:
region 1 of blue's territory and 4 of red's. The game places it there, and the
move takes it to its layout position. A move onto a flank from another region
is what makes a joining unit travel. Every legacy snapshot uses
`recordPosition` and `recordRotated` when stated, otherwise `position` and
`rotated`. A settled legacy unit stays in that pose unless its current
position or rotation differs. A `PAD_TestCommand` carries `MAD_MoveUnit` to
the current position and rotation with `IsForce=true`, `IsUndo=true`, and the desired
`MoveUnitData.superDeployRecord` membership. The native undo path restores
travelling membership when regions differ. A travelling legacy unit whose
old and current positions share a region first visits the opposite flank,
then returns; its snapshot and preparation position remain unchanged. The
forced check permits restoration regardless of deployment mobility. A layout
need not state which skill or modifier made its position reachable in a match.

A newly created unit starts with zero experience. When its layout states a
full bar below level 9, a ready Intensive Training slot restores that bar
after any upgrades and before the move. A partial bar or nonzero experience
at level 9 cannot be restored this way and is refused. Legacy units read
their experience directly from the snapshot, with no training restriction.

The snapshot's allocator is where the round opened it: the first delivered or
recovered squad's index when an officer delivers one, since the delivery takes
the allocator's index, and otherwise one past the highest legacy unit's. When
the side's joining units are the allocator's next indices, from where the
deliveries left it, the action's index is `-1` and the allocator hands each its
own, as buying it did; otherwise each states its own, which leaves the
allocator where the deliveries left it. As the round ends the game enters the next
round's deployment, and an officer due then delivers its squad at the
allocator, so an allocator left behind would hand that squad an index a
joining unit already holds.

A squad an officer's schedule hands out as the round opens arrives on top of
the snapshot, so the snapshot does not hold it again. The allocator opens at
its index, which the delivery takes. The layout's `delivered` unit is the
squad, of its type, at its level or above, and its
decisions upgrade, fit, restore experience when needed, and move it. A squad the layout lists in `recovered`
was recovered before the fight, and a decision recovers it by that index,
which the game's own replay of the round agrees with tick for tick. Each
officer whose `opening_unit` the round activates delivers one squad, which
the layout states once, as a delivered unit or recovered. A standard 1v1 never
deals a side two officers that deliver in one round, and which delivers first
is the game's, not the layout's, so a side that two deliver to is refused.

## What a layout replay refuses

A layout that compiles is refused only when a replay cannot open or play it,
and the refusal names each part:

- no seed;
- two officers that each deliver a squad as the round opens;
- a delivered unit that is not of the delivering officer's squad's type or below
  its level;
- an officer that delivers a squad as the round opens and no delivered unit
  or recovered index for it, or a delivered unit or recovered index with no
  officer delivering one;
- an officer that delivers a squad as the next round opens, beside joining
  units that skip an index, since they leave the allocator behind them;
- a joining or delivered unit with nonzero experience that is not a full bar
  below level 9, since its source prevents snapshot restoration and Intensive
  Training cannot restore that bar;
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
