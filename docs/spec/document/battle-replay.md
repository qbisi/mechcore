# Battle replay

## Scope

A battle replay is the `.grbr` file a [battle](battle.md) is written back as:

```bash
mechcore replay convert <battle.yaml> <replay.grbr> [--force]
```

Conversion is idempotent across the battle format. A battle written as a replay
converts back ([battle.md](battle.md#converting-a-replay)) to the same battle,
byte for byte, so the two directions are one mapping and not two
approximations of it. `record_replay_round` fights any of a battle replay's
rounds.

The game deals a replayed round's offers again, from the pool a round's
snapshot restores and the match stream, and the round's recorded choice takes
the card at its index. So a battle replay carries the pool as well as the
stream, and both are what the battle's own rounds make: a battle states no pool,
and needs none.

This document defines what a battle replay holds and which battles cannot be
written as one. The file's framing and the snapshot members are the game's and
are shared with the [layout replay](layout-replay.md), which writes one fight
rather than a match. What conversion reads, and so what this has to write, is
[battle.md](battle.md#converting-a-replay) together with
`crates/document/src/convert.rs`.

## The file

The framing is the layout replay's
([layout-replay.md](layout-replay.md#the-file)): a `BinaryFormatter` stream of
one `GameRiver.Replay` holding the record as XML, with `battleID` `battle`, the
battle's map, and the two players `{id: 1, name: blue}` and `{id: 2, name: red}`.

## The battle record

`BattleInfo` holds the battle's `seed` as `SystemSeed`, its map, `VS_1_1` and the
standard 1v1 constants, with advance teams, reinforcement, unit reinforcement
and construction on, and no game rules and no match type. `Version` is the build
number and `Seat` is 0, a locally recorded replay.

Each `PlayerRecord`, blue first, holds the side's `seed`, the map's reactor core,
the round income every versus map shares, and its tech loadout as `unitDatas`.
Its rounds run from 0 to the battle's last, one `PlayerRoundRecord` each, and
`matchDatas` holds one `MatchSnapshotData` per round.

## The random streams

The random states a replay records are where the battle's seeds put them, which
is what conversion checks:

- the match stream: round 0 holds the state `SystemSeed` seeds, from which the
  opening is dealt; a round that deals reinforcement offers holds the state its
  deal starts from, and the round after it the state the deal ends on; any
  other round holds the one before it;
- each side's own stream: the side's seed, advanced once for every hand-out an
  earlier round's officers drew.

A round that deals offers lists them, as dealt, in its `reinforceItems`.

## The reinforcement pool

Each round's snapshot holds the pool as the round opened, which is where the
battle's deal leaves it ([reinforcements.md](../../rules/reinforcements.md)):

- `poolOPs` is the pool's log since the match began: each refresh's removals,
  each followed by the replacement drawn for it, and each nonrepeatable card an
  earlier round took, with the other variants of its group the pool still held;
- `RoundExcludeReinforce` holds every round a level-4 commander skill offered
  the round before excludes them from, each with every level-4 commander skill
  in the build's order.

A round's two choices are logged blue before red. The match logged them in the
order they were made, which a battle does not keep; restoring a round sorts the
pool again, so the order decides nothing, and conversion accepts either
([battle.md](battle.md#converting-a-replay)).

## Round 0

The opening holds nothing: the map's reactor core and the dealt constructions.
Its one decision is `PAD_ChooseAdvanceTeam`, at the offer the side takes and
naming that offer's team; the specialist is read back from round 1's officers.

## Every later round

A battle's state segment is the position after the round's opening, and a
replay's snapshot is the position before it, so each snapshot is the state with
the opening undone:

- what the round's officers deliver is taken back: the commander skills they
  add to the panel, the items they add to the inventory, one of them drawn from
  the side's stream as conversion draws it, the squads they hand out, with the
  unit allocator moved back by as many, and the units they unlock;
- the round's income, what the equipment on the board pays and what the
  previous round's Rapid Resupply still owes are taken back out of the supply;
- a slot the previous round released is written active, which restarts it as
  the round opens, and every other slot counts down, so a slot on cooldown is
  written one round higher;
- the previous round's Rapid Resupply is the snapshot's `energyTowerSkills`;
- the inventory holds every item, the fitted ones among them.

Each undone position is opened again with conversion's own rule, and a
position that does not open onto the battle's state is refused rather than
written. A chain blueprint's officer is written beside the blueprint, as the
game snapshots it. A Shield Airdrop or an area an earlier round left standing
is written under a panel slot of the skill that leaves it.

## The decisions

A round's decisions are written in order, each side's ending with
`PAD_FinishDeploy`; a side that concedes finishes its deployment first, so its
round is fought before the match ends:

| Decision | Recorded as |
| --- | --- |
| `choose_reinforce_item` | `PAD_ChooseReinforceItem`, a decline at index −1 |
| `buy_unit` | `PAD_BuyUnit`, then `PAD_MoveUnit` of the formation it creates to its position and facing |
| `upgrade_unit`, `unlock_unit`, `upgrade_technology`, `active_blueprint`, `active_energy_tower_skill`, `strengthen_tower`, `use_equipment`, `release_contraption` | their own action |
| `move_unit` | `PAD_MoveUnit` of one formation |
| `release_commander_skill` | `PAD_ReleaseCommanderSkill` from its slot, at its positions or naming its unit or construction |
| `concede` | `PAD_GiveUp`, after the side's `PAD_FinishDeploy` |

The game places a purchase itself, where the deployment area is free, and a
battle states where the formation ends up, so a purchase is followed by the
move that puts it there; conversion folds the move back into the purchase. The
formation a purchase creates is the one the round's position, stepped decision
by decision, allocates next.

A battle holds its decisions in an order the board allows each one in
([action.md](action.md#settling-a-round)), so each is recorded where it stands
and none is held back or reordered. A move that clears another's way is one the
battle states.

## What a battle replay refuses

A battle is refused when a replay cannot hold it, and the refusal names why:

- a deployment clock, which only a match played on this platform states;
- a position after its last decisions, which no replay records;
- a side without an opening or without a seed;
- a round whose undone position does not open onto the battle's, or whose
  decisions do not step, or put something where the board already has
  something, which the game would refuse;
- an object left standing whose skill the panel holds no slot of.

## Normal form

A battle replay is a function of its battle and the build: the same two always
write the same bytes. Players come blue then red, rounds ascend, and each
round's decisions come in the battle's order.

## Excluded fields

| Field | Why it is not written |
| --- | --- |
| `MatchActionData.Time`, `LocalTime` | Decisions are numbered in order; a battle keeps no clock |
| `PAD_Undo`, `PAD_Redo`, `PAD_CancelReleaseCommanderSkill` | A battle holds the net decisions they leave |
| `PAD_MoveUnit.positionRecord`, `rotateRecord`, `superDeployRecord` | They restate the position before the move |
| `PlayerRecord.name`, `id`, `ad`, `data.style` | Account identity and skins |
| `BattleInfo.BattleID`, `CreateTime` | Provenance, which a battle does not carry |
| `NewUnitData.RoundCount` | No reading of the replay uses it |

## Unresolved

None.
