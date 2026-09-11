# Turn definition

## Status

The document is implemented. `crates/document` reads a turn out of a replay,
applies one, and checks the result; the field names below are a contract, not a
proposal. Sections marked **Unresolved** name what still has no evidence behind
it, and [`action.md`](action.md) carries the action space itself: what the
thirteen decisions are, what each carries, and what each does to a state.

Evidence for the claims here comes from the 59 GRBR replays the Steam
installation has recorded locally. 34 of them convert into battle documents,
giving 309 rounds, 618 player-rounds, 550 round transitions and 8,263 net
actions; `tests/grbr` tracks 6 in the repository, of which 4 convert, and a
count that says "tracked" means those. Test matches were set up with Training
Ground commands rather than by 1v1 rules and are excluded from every count.
`tests/grbr/README.md` explains which replays are usable.

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
`LocalTime`, and across the 495 single-side lists of the local set no list
violates ascending time, so reading the list in file order and dropping the
stamp loses nothing. The one list in the whole directory that does violate it
belongs to a Test match, where two `PAD_TestCommand` entries stamped `0` sit
among played actions, which is one more reason those matches are excluded.

No merge between the two sides is attempted either, since the interleaving of
one side's actions with the other's is not observable in a replay.

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

Replaying the recorded list into a stack defines the collapse. Every recorded
action is pushed as an entry, and `Undo` pops the newest entry whether or not it
still stands for a decision. `Redo` pushes it back.
`CancelReleaseCommanderSkill` stops the newest standing
`ReleaseCommanderSkill` with the same `SkillIndex` from counting, but the
release stays on the stack as a spent entry, and the cancel is pushed as one
too. What remains standing is the turn's action list, and it contains no
retraction of either kind.

Counting the spent entries is the part that is easy to get wrong, and it is
what the game does: undo walks an index back over the recorded list, so an entry
that no longer means anything still costs an undo.

The two rules are the game's own, read from `ActionHistoryController`, which
holds one `actionHostories` list and a `currentActionIndex` into it. Undo moves
that index back rather than deleting, a new action prunes everything past it,
and `TryGetCancelActionTarget` walks the list backwards from that index for the
newest `PAD_ReleaseCommanderSkill` whose field at `0x1C`, `SkillIndex`, equals
the cancel's own. It matches on the index alone, not on the skill `ID`.

The collapse is checked and not assumed. Over the local set every one of the
3850 field checks the transition test makes reproduces, and the allocator is the
field that pins the spent entries down. One player-round decides it. A side
chose a card, bought two units, moved one of them twice, released a skill,
cancelled it, released it again, unlocked a unit, and then pressed undo seven
times. Its next snapshot keeps the card and one of the two units, which is where
stepping back over seven recorded entries lands. Stepping back over seven
standing decisions lands two entries further, on the card, and the card is still
there.

The cancel rule closes just as cleanly. All 78 cancels in the local set are
preceded by a release carrying the same `SkillIndex`, and all 78 are followed
later in the same list by another release of that index, so each one is a player
replacing a skill's target. The re-release is usually the very next entry, but
not always: in two of the 78 a technology or a contraption comes between. The
preceding release sits 1 to 27 positions back, which is why a cancel cannot be
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

`ChooseAdvanceTeam` belongs to round 0 and to no other round, and it is one
decision with two halves. The opening deals a side four combinations of a team
and a specialist officer, and taking one takes both:

```yaml
actions:
  blue:
    - type: choose_advance_team
      offer: 2
      id: 9911
      specialist: 20005
```

`offer` is the position taken among the four, `id` names the team, whose
formations [`config/advance_teams.yaml`](../config/advance_teams.yaml) lists,
and `specialist` names the officer bound to it.

The two halves are one choice rather than two. The same team appears with
different specialists in different matches, so the specialist is not a property
of the team; and no second action is recorded, so it is not a second decision
either. The converter reads the specialist back from the officer list of the
round the opening produced, and exactly one officer of a side is an opening
specialist in all 290 player-rounds of the local set, so the reading is
unambiguous.

The four combinations are dealt to each side privately. Neither player sees the
other's, which is why they are not `reinforce_offers`: that field sits above
`sides` precisely because both players choose from one array, and the opening is
the one offer that does not work that way. The opening's only shared
information is the construction layout, which the map rolls once and deals to
both sides; both receive the same construction types in all 29 matches of the
local set.

A replay stores none of the four combinations, so a turn states the one taken
and nothing about the three refused. The state's own `opening_offers` is where
the four belong, and [the state document](state.md) defines it along with why a
replay can still show them: the file carries the random state and the pool log
the game rolls them from, so they are reproducible rather than recorded.

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

## What a turn reproduces

Applying a turn's decisions to its state has to reproduce the state the next
turn starts from, in everything the fight does not decide. That application is a
function rather than a comparison: it takes a position, a round and the round's
decisions, and returns the seven fields below. Checking a turn is then reading
the same seven out of the recorded next state and comparing. All seven reproduce
every time, over the four tracked replays and over the locally recorded ranked
matches alike: 462 of 462 field checks on the tracked set and 3850 of 3850 on
the local one.

| Field | Reproduced |
| --- | ---: |
| `techs.units` | 66/66 |
| `techs.officers` | 66/66 |
| `blueprints` | 66/66 |
| `tower_strengthen_levels` | 66/66 |
| `shop.unlocked_units` | 66/66 |
| `battle_skills` | 66/66 |
| `next_index.unit` | 66/66 |

`battle_skills` is compared by the IDs on the panel and not by how many slots it
has, and `techs.officers` by a multiset and not by a set. Both distinctions are
real: Missile Specialist puts two copies of Missile Strike on the panel, and an
officer card that may be taken again stacks, which is how one side in the local
set comes to hold three copies of `20022`. A slot's index and its cooldown are
left out, because a cooldown counts down through the fight.

Five rules the function had to learn are worth stating, because none of them is
visible in an action. Taking a team unlocks the two unit types it is made of.
A blueprint's second level replaces its first rather than joining it, and the
officer it produces follows. An officer hands out on a schedule of its own
rather than when it arrives, and the two halves can fall in different rounds:
Longbow Specialist unlocks Marksman in round 1 and hands out its rank 3 squad in
round 2, while Rhino Specialist unlocks in round 1 and waits until round 4. The
officer's own `activeRound` names the delivery round and `unitUnlockRound` the
unlock, both absolute, and `config/officers.yaml` carries them. An officer list
is a multiset, so a repeatable officer card adds a copy rather than doing
nothing. And an undo steps back over a recorded entry rather than over a
standing decision, which the net-decision section states.

A roster, a reactor core and a formation's experience are not checked, because
the fight decides them.

## Capabilities the adapter still lacks

A turn cannot be executed today.

The build defines 23 `PAD_*` types, of which 19 are decisions a player takes;
the other four are two abstract bases, the Training Ground container and a
sentinel whose every method throws. 16 of the 19 occur in the four tracked
ranked replays and 17 in the wider local set, which adds giving up. The two that
occur nowhere are redo and releasing a construction directly.
[`action.md`](action.md) defines the thirteen decisions the document keeps.

The adapter installs state through 33 `MAD_*` Training Ground commands, and only
6 of those name the same thing as a `PAD_*` type. It can perform 6 of the 13
document actions as decisions; the other 7, purchases and cards among them, it
can only install the result of. Installing an initial condition and taking a
legitimate decision are therefore different capabilities, and the second one
does not exist yet.

The transition check needs a card catalog, which a layout never needed. Without
one a `ChooseReinforceItem` that grants two units looks like state appearing
with no action behind it.

That catalog is now complete.
[`config/unit_reinforcements.yaml`](../config/unit_reinforcements.yaml) states,
for each of the 519 unit cards a standard match can offer, which unit it hands
out, how many squads of it, at what level, and from which round. No card in
this build mixes two kinds of unit.
[`config/reinforce_items.yaml`](../config/reinforce_items.yaml) covers the rest
of the pool, 84 officers, 21 commander skills and 18 equipment, each granting
the thing its own ID names, and
[`config/advance_teams.yaml`](../config/advance_teams.yaml) states the 45
openings. What is still missing is not a catalog but an executor: the deployment
half of a transition, which is the board group [`action.md`](action.md) names.
