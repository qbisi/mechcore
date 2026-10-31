# Match replay

## Scope

A match replay is the `.grbr` file a [match](match.md) is written back as:

```bash
mechcore convert <match.yaml> --to grbr <replay.grbr> [--force]
```

Conversion requires a complete prefix including round-zero decisions; a partial
match is refused rather than supplied with invented history. Conversion is
idempotent across complete match prefixes in standard action normal form.
Such a match written as a replay converts back
([match.md](match.md#converting-a-replay)) to the same match, byte for byte.
A historical action sequence converts back in the normal form `format`
produces. `record_replay_round` fights any of a match replay's rounds.

The game deals a replayed round's offers again, from the pool a round's
snapshot restores and the match stream, and the round's recorded choice takes
the card at its index. So a match replay carries the pool as well as the
stream, and both are what the match's own rounds make: a match states no pool,
and needs none.

This document defines what a match replay holds and which matches cannot be
written as one. The file's framing and the snapshot members are the game's and
are shared with the [layout replay](layout-replay.md), which writes one fight
rather than a match. What conversion reads, and so what this has to write, is
[match.md](match.md#converting-a-replay) together with
`crates/document/src/convert.rs`.

## The file

The framing is the layout replay's
([layout-replay.md](layout-replay.md#the-file)): a `BinaryFormatter` stream of
one `GameRiver.Replay` holding the record as XML, with `battleID` `battle`, the
match's map, and the two players `{id: 1, name: blue}` and `{id: 2, name: red}`.

## The battle record

`BattleInfo` holds the match's `seed` as `SystemSeed`, its map, `VS_1_1` and the
standard 1v1 constants, with advance teams, reinforcement, unit reinforcement
and construction on, and no game rules and no match type. `Version` is the build
number and `Seat` is 0, a locally recorded replay.
`DeployTime` is the match's `deploy_time`, or the standard 100 seconds when it
states none.

Each `PlayerRecord`, blue first, holds the side's `name` (defaulting to `blue`
or `red`) and `seed`, the map's reactor core, the round income every versus
map shares, and its tech loadout as `unitDatas`.
Its rounds run from 0 to the match's last, one `PlayerRoundRecord` each, and
`matchDatas` holds one `MatchSnapshotData` per round.

## The random streams

The random states a replay records are where the match's seeds put them, which
is what conversion checks:

- the match stream: round 0 holds the state `SystemSeed` seeds, from which the
  opening is dealt; a round that deals reinforcement offers holds the state its
  deal starts from, and the round after it the state the deal ends on; any
  other round holds the one before it;
- each side's own stream: the side's seed, advanced once for every hand-out an
  earlier round's officers drew.

A round that deals offers lists them, as dealt, in its `reinforceItems`.
The match's checkpoints stand after the current deal and player deliveries;
replay snapshots stand before them. Replaying the header seeds verifies the
checkpoints and supplies the earlier stream states the replay requires.

## The reinforcement pool

Each round's snapshot holds the pool as the round opened, which is where the
match's deal leaves it ([reinforcements.md](../../rules/reinforcements.md)):

- `poolOPs` is the pool's log since the match began: each refresh's removals,
  each followed by the replacement drawn for it, and each nonrepeatable card an
  earlier round took, with the other variants of its group the pool still held;
- `RoundExcludeReinforce` holds every round a level-4 commander skill offered
  the round before excludes them from, each with every level-4 commander skill
  in the build's order.

A round's two choices are logged blue before red. The match logged them in the
order they were made, which a match does not keep; restoring a round sorts the
pool again, so the order decides nothing, and conversion accepts either
([match.md](match.md#converting-a-replay)).

The match carries effective `reinforce_pool.added` and `removed` sets rather
than `poolOPs`. Replay output reconstructs the historical log from the complete
match and checks the post-deal checkpoints. Continuing a match from a checkpoint
needs neither this log nor earlier choices.

## Round 0

The opening holds nothing: the map's reactor core and the dealt constructions.
Its one decision is `PAD_ChooseAdvanceTeam`, at the offer the side takes and
naming that offer's team; the specialist is read back from round 1's officers.
No `PAD_FinishDeploy` follows it: the game writes none in the opening, and
replaying one there finishes round 1's deployment before its decisions play.

## Every later round

A match's state segment is the position after the round's opening, and a
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
position that does not open onto the match's state is refused rather than
written. A chain blueprint's officer is written beside the blueprint, as the
game snapshots it. A panel slot's `standing` objects, the Shield Airdrops and
areas its earlier releases left, are written in that slot's `rangeItems`.

## The decisions

From round 1, a round's decisions are written in order, each side's ending
with `PAD_FinishDeploy` unless it concedes:

| Decision | Recorded as |
| --- | --- |
| `choose_reinforce_item` | `PAD_ChooseReinforceItem`, a decline at index −1 |
| `buy_unit` | `PAD_BuyUnit`, then `PAD_MoveUnit` of the formation it creates to its position and facing |
| `upgrade_unit`, `unlock_unit`, `upgrade_technology`, `active_blueprint`, `active_energy_tower_skill`, `strengthen_tower`, `use_equipment`, `release_contraption` | their own action |
| `move_unit` | `PAD_MoveUnit` of one formation |
| `release_commander_skill` | `PAD_ReleaseCommanderSkill` from its slot, at its positions or naming its unit or construction |
| `concede` | `PAD_GiveUp` |

The game places a purchase where the deployment area is free. A standard
match couples a purchase and its immediately following movement through the
purchase's position and rotation. Encoding expands that combined placement
into those two native operations. The formation created is the one the position
allocates next.

Match decisions use the standard phases in
[action.md](action.md#settling-a-round). Their dependencies already place unit
research, purchases, recovery and Energy Tower activations in native legal
order. Encoding retains Elite Recruitment's activation order. After the first
purchase of a type, it inserts that type's later technology activations before the purchase's movement, so flank initialization
sees them. The activations are omitted from their later match positions;
they are not repeated for later purchases. Rapid Resupply is already the
match's second phase, after reinforcement selection; encoding does not
activate it on demand. Purchase level is derived
from the officers and active Energy Tower skills at that point; it is not
an action parameter. Research requires a formation of its type.

A decision's `Time` is zero and its `LocalTime` the seconds since its round
opened, in the game's 0.05-second ticks, as every tracked replay records them.
A match keeps the order and not the clock, so a round's records are written
two seconds apart, the first at two seconds, and closer when the round's
`DeployTime`, or the opening's `PrepareTime` of 30, would not hold them.
Conversion reads none of them back.

## What a match replay refuses

A match is refused when a replay cannot hold it, and the refusal names why:

- a position after its last decisions, which no replay records;
- a side without an opening or without a seed;
- a deal that does not verify, such as a round two answers of one side take,
  since the random streams a replay records are where the deal leaves them;
- a round whose undone position does not open onto the match's, or whose
  decisions do not step, or put something where the board already has
  something, which the game would refuse;
- an object left standing whose skill the panel holds no slot of.

## Normal form

A match replay is a function of its match and the build: the same two always
write the same bytes. Players come blue then red, rounds ascend, and each
round's decisions follow the match's phases, with the native insertion points
stated above.

## Excluded fields

| Field | Why it is not written |
| --- | --- |
| `PAD_Undo`, `PAD_Redo`, `PAD_CancelReleaseCommanderSkill` | A match holds the net decisions they leave |
| `PAD_MoveUnit.positionRecord`, `rotateRecord`, `superDeployRecord` | They restate the position before the move |
| `PlayerRecord.id`, `ad`, `data.style` | Account identity and skins |
| `BattleInfo.BattleID`, `CreateTime` | Provenance, which a match does not carry |
| `NewUnitData.RoundCount` | No reading of the replay uses it |

## Unresolved

None.
