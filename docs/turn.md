# Turn definition

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
they appear, and `tests/grbr/README.md` explains which replays are usable.

## Scope

A turn describes one deployment round: the state both players start it from, and
the decisions each of them takes from it.

```yaml
kind: turn
map_id: 1001
seed: 2038621361
round: 7
state: { ... }
actions: { ... }
```

`state` carries the [state document](state.md) shape without repeating `kind`,
since `kind` marks a document root and not a subtree. `map_id`, `seed` and
`round` stay at the turn's root for the same reason they sit at a state's root:
both sides share them.

Everything about what a state holds, how it projects onto a layout, and how it
is rebuilt from a replay belongs to that document. This one covers only the
second half of a turn.

A [battle](battle.md) is a whole match: the turns in order, plus the fields
every round of that match shares. A turn appearing there drops `map_id` and
`seed` as well, since the battle states them once.

## The two halves are the same information

A turn states a round twice over. `state.sides.<side>.battle_skills` records
which skills the side decided to release, in what order and at what target,
while `actions.<side>` records the same releases as a sequence of decisions. The
same holds for purchases, upgrades and moves.

That redundancy is deliberate and it is what makes a turn checkable. Applying
the actions to the state at the round's start has to reproduce the state at its
end, so a turn carries its own transition test rather than needing an external
oracle.

## Actions

Actions are two sequences, one per side, and a sequence carries order alone. No
timestamp is stored, because nothing reads one: applying a turn means applying
each side's actions in sequence, and the check that a turn reproduces its end
state never asks when an action happened.

The recorded order is the true order. The replay stamps each action with a
`LocalTime`, and across 234 single-side lists no list violates ascending time,
so reading the list in file order and dropping the stamp loses nothing. No merge
between the two sides is attempted either, since the interleaving of one side's
actions with the other's is not observable in a replay.

```yaml
actions:
  blue:
    - type: choose_reinforce_item
      offer: 3
      id: 1072213
    - type: buy_unit
      unit: 30
      position: {x: 0, y: -160}
    - type: release_commander_skill
      skill: 0
      target: {unit: 4}
  red:
    - ...
```

### Net decision

A turn stores the decisions that took effect, so a recorded list is collapsed
before it is written. The game keeps a retraction as an action of its own, and
there are two kinds.

Replaying the recorded list into a stack defines the collapse. Each action is
pushed. `Undo` pops the newest surviving action, and both disappear. `Redo`
pushes it back. `CancelReleaseCommanderSkill` removes the newest surviving
`ReleaseCommanderSkill` with the same `SkillIndex`, and disappears with it. What
remains is the turn's action list, and it contains no retraction of either kind.

The two rules are the game's own, read from `ActionHistoryController`, which
holds one `actionHostories` list and a `currentActionIndex` into it. Undo moves
that index back rather than deleting, a new action prunes everything past it,
and `TryGetCancelActionTarget` walks the list backwards from that index for the
newest `PAD_ReleaseCommanderSkill` whose field at `0x1C`, `SkillIndex`, equals
the cancel's own. It matches on the index alone, not on the skill `ID`.

The collapse is checked and not assumed. Across 35 undos none underflows the
stack, and the allocator agrees: predicting the `unitIndex` delta from net
purchases plus the units granted by cards explains 64 of 66 round transitions,
where ignoring undo explains 54. The cancel rule closes just as cleanly: all 14
cancels in the local set are preceded by a matching release and immediately
followed by another, so each one is a player replacing a skill's target. The
preceding release sits 1 to 14 positions back, which is why a cancel cannot be
treated as a stack pop.

What the collapse is not is the game's own file format. `FixActionWithUndo`, the
method `PlayReplayCommand.StartReplay` calls, deletes nothing: it merges a
round's actions from both players, sorts them, and then pads each player's own
list with a placeholder wherever the merged order runs ahead, so that the two
lists share an index space. It runs only for rounds a player's recorder reports
as containing an undo. The recorded lists therefore still hold every `Undo` and
every cancel, and collapsing them is this format's convention.

`FinishDeploy` is dropped. It appears exactly once per round-side, so it carries
no decision.

`BuyUnit` does not name the unit it creates. Its recorded `UIDX` is `-1` in
every purchase of the local set, so the index comes from the allocator, and
applying a turn has to hand out `next_index.unit` exactly as the game does.
That is one of the two things the allocator is in a state for.

`MoveUnit` is recorded as a batch. One action carries between one and seven
`moveUnitDatas` entries, 1096 of the 1164 carrying one. A turn writes one move
per unit: the collapse has already run by then, so the batch no longer has to
stay whole for an undo to pop it, and order is all that survives either way.

Declining a reinforcement card is recorded as `ChooseReinforceItem` with `ID`
zero at offer `-1`. A turn writes it as its own action, since an offer position
that names no card is not a choice of card.

`MoveUnit` keeps only the resulting position and rotation. The recorded
`positionRecord`, `rotateRecord` and `superDeployRecord` fields restate the
state before the move, which the turn already holds.

## Normal form

| Collection | Order |
| --- | --- |
| `actions.blue`, `actions.red` | as recorded, which is ascending `LocalTime` |

A state's own collections keep the orders that document defines.

## Excluded fields

| Field | Why it is not in the turn |
| --- | --- |
| `Time`, `LocalTime` | A sequence carries order, and nothing reads a timestamp |
| `PAD_Undo`, `PAD_Redo` | Removed by the net-decision collapse |
| `PAD_CancelReleaseCommanderSkill` | The same, together with the release it retracts |
| `PAD_FinishDeploy` | Exactly one per round-side, so it carries no decision |
| `PAD_MoveUnit.positionRecord` | Restates the state before the action |
| `PAD_MoveUnit.rotateRecord`, `superDeployRecord` | Same |
| `PAD_BuyUnit.UIDX` | Always `-1`; the allocator names the new unit |
| `PAD_ReleaseCommanderSkill.Positions` beside an object target | The player's click point, which names no state |

## Capabilities the adapter still lacks

A turn cannot be executed today.

The action space is the 19 `PAD_*` types, of which 16 occur in ranked replays.
The adapter installs state through 41 `MAD_*` Training Ground commands, and only
6 of those overlap the player action space. Installing an initial condition and
taking a legitimate decision are therefore different capabilities, and the
second one does not exist yet.

The transition check needs a card catalog, which a layout never needed. Without
one a `ChooseReinforceItem` that grants two units looks like state appearing
with no action behind it.

Half of that catalog now exists.
[`config/unit_reinforcements.yaml`](../config/unit_reinforcements.yaml) states,
for each of the 519 unit cards a standard match can offer, which unit it hands
out, how many squads of it, at what level, and from which round. No card in
this build mixes two kinds of unit. What is still missing is the opening
advance team, whose own units a converter would have to read the same way, and
the cards that grant a commander skill or an equipment rather than units.
