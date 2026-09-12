# Turn definition

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
is rebuilt from a replay belongs to that document. [`action.md`](action.md)
defines the thirteen decisions themselves: what each carries and what each does
to a state. This one covers how the two halves of a turn fit together.

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
      target: !unit 4
  red:
    - ...
```

No merge between the two sides is attempted, since the interleaving of one
side's actions with the other's is not observable in a replay.

### Net decision

A turn stores the decisions that took effect, so a recorded list is collapsed
before it is written. The game keeps a retraction as an action of its own, and
there are two kinds.

Replaying the recorded list into a stack defines the collapse. Every recorded
action is pushed as an entry, and `Undo` pops the newest entry whether or not it
still stands for a decision. `Redo` pushes it back.
`CancelReleaseCommanderSkill` stops the newest standing
`ReleaseCommanderSkill` with the same `SkillIndex` from counting, but the
release stays on the stack as a spent entry, and the cancel is pushed as one
too. What remains standing is the turn's action list, and it contains no
retraction of either kind.

Counting the spent entries is the part that is easy to get wrong, and it is what
the game does: undo walks an index back over the recorded list, so an entry that
no longer means anything still costs an undo. A cancel matches its release on
the panel index alone and not on the skill ID, and the release it retracts can
sit many entries back, so a cancel is not a stack pop.

The collapse is lossy on purpose. A turn states what a side decided, not what it
considered, so a document cannot express that a player took an action and
withdrew it.

What the collapse is not is the game's own file format. The method a replay
calls before playback deletes nothing: it merges a round's actions from both
players, sorts them, and pads each player's list with a placeholder wherever the
merged order runs ahead, so that the two lists share an index space. The
recorded lists still hold every retraction, and collapsing them is this format's
convention.

### What the collapse and the conversion drop

`FinishDeploy` carries no decision: it appears exactly once per round-side.

`BuyUnit` does not name the unit it creates. Its recorded index is always unset,
so the index comes from the allocator, and applying a turn has to hand out
`next_index.unit` exactly as the game does. That is one of the two things the
allocator is in a state for.

`MoveUnit` is recorded as a batch carrying one or more units. A turn writes one
move per unit: the collapse has already run by then, so the batch no longer has
to stay whole for an undo to pop it, and order is all that survives either way.
It keeps only the resulting position and rotation, since the recorded
before-state restates what the turn already holds.

`ChooseAdvanceTeam` belongs to round 0 and to no other round, and it is one
decision with two halves. The opening deals a side four combinations of a team
and a specialist officer, and taking one takes both. The same team appears with
different specialists in different matches, so the specialist is not a property
of the team; and no second action is recorded, so it is not a second decision
either.

The four combinations are dealt to each side privately. Neither player sees the
other's, which is why they are not `reinforce_offers`: that field sits above
`sides` precisely because both players choose from one array, and the opening is
the one offer that does not work that way. A turn states the one combination
taken and nothing about the three refused; the state's own `opening_offers` is
where the four belong, and [the state document](state.md) defines it.

## Normal form

| Collection | Order |
| --- | --- |
| `actions.blue`, `actions.red` | as taken |

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
| `PAD_BuyUnit.UIDX` | Unset; the allocator names the new unit |
| `PAD_ReleaseCommanderSkill.Positions` beside an object target | The player's click point, which names no state |

## What a turn reproduces

Applying a turn's decisions to its state has to reproduce the state the next
turn starts from, in everything the fight does not decide. That application is a
function rather than a comparison: it takes a position, a round and the round's
decisions, and returns the nine fields below. Checking a turn is then reading
the same nine out of the recorded next state and comparing.

| Field |
| --- |
| `next_index.unit` |
| `next_index.contraption` |
| `shop.unlocked_units` |
| `techs.units` |
| `techs.officers` |
| `blueprints` |
| `tower_strengthen_levels` |
| `battle_skills` |
| `equipment` |

`battle_skills` is compared by the IDs on the panel and not by how many slots it
has, and `techs.officers` by a multiset and not by a set. Both distinctions are
real: one officer can put two copies of a skill on the panel, and an officer
card that may be taken again stacks. A slot's index and its cooldown are left
out, because a cooldown counts down through the fight.

The rules the function applies beyond the actions themselves are the ones no
action states, and [`action.md`](action.md) carries them: a card or an officer
allocates formations, an officer delivers on a schedule of its own, a chain
blueprint's second level replaces its first, and an officer list is a multiset.
The fifth belongs here, because it is a property of the sequence rather than of
any decision in it: an undo steps back over a recorded entry rather than over a
standing decision.

Both allocators are here even though the fight destroys what they hand out. An
index is never reissued, so an allocator records how many objects a side has
ever had rather than how many it still holds, and no fight can move it.

`equipment` is compared together with what the round's fits could not find. An
empty stock is reached both by a round that balanced and by a round that fitted
an item the side never held, and the two must not compare equal.

A roster, a reactor core and a formation's experience are not checked, because
the fight decides them. The rest of the next position is the board, which
[`action.md`](action.md) names and which applying a turn does not yet produce.

`travelling` is the board field a turn's moves settle outright, and it is not
among the nine for a reason that is not incompleteness: the fight empties the
travelling set, so the field never reaches the next position to be compared
against. Applying a turn's moves gives the set the side's own deployment ended
with, and the next turn's state holds none whatever that set was. That makes it
a field a turn produces and no turn transition can check, which is the mirror
image of a reactor core.

## What a decision reproduces

The nine fields above are what two round snapshots can decide between them. A
recording that states a position before every decision and after it decides
more, because it asks a smaller question: not what the next round holds, but
what this position looks like one decision later.

That is the transition [`action.md`](action.md) defines, and it is checked two
ways against the same recording.

**One decision at a time.** Each recorded decision is applied to the position it
was taken from, and every field of the result is compared, the board included.
A failure names the one decision that caused it rather than the round it was in.

**A whole deployment.** A round's standing sequence, after the net-decision
collapse above, is applied to the position the round opened with and compared
against the position it closed with. This is the check the first one cannot
make: applying one decision at a time reads the position after a retraction out
of the recording, while a collapsed sequence has to reach the same place without
the retraction ever having happened.

```bash
mechcore verify <recording.jsonl>
find work/replay-corpus/observations -name '*.jsonl' | mechcore verify
```

runs both checks. `verify` reads whichever contract a file names for itself: a
recording written by `record_replay_battle`, whose format
[`adapter.md`](../adapter/adapter.md) defines, declares its schema on its header
record, and a layout declares `kind: layout` at its root. Nothing is inferred
from an extension.

A batch is a pipe rather than a flag. Paths come from the arguments, or from
standard input one per line when there are none, so expanding a directory stays
the shell's job and there is only ever one expander. One report per input goes
to standard output as a single JSON object per line, a refusal included, and
one unreadable input does not stop the rest. The exit code says whether every
input was valid.

The two frames disagree in one place, and the disagreement is a property of the
question rather than an error. Applying a turn answers what the next round
holds, so at round 0 it counts the squads the opening delivers; stepping that
same choice answers what the position is immediately afterwards, and the squads
have not arrived. They reach the board when round 1 opens, which is not a
decision and so is not a step.

## Unresolved

**Whether a turn can be executed rather than only checked.** Applying a turn
produces the seven fields above; the board is the other half, and taking a
decision in a live game is a capability distinct from installing its result.
Until both exist, a turn is a document that can be verified against a recording
but not replayed into one.

**Whether a retraction is ever worth stating.** The collapse discards what a
player withdrew, on the grounds that a turn records decisions rather than
deliberation. A reader that wanted to study how a position was arrived at rather
than what it became would need the recorded list instead, and the format has no
place to put it.
