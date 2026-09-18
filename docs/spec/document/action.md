# Action definition

## Scope

An action is one decision a side takes in a round. This document defines the
fourteen of them, what each carries and what each does to the position it was
taken from, and the action segment that carries one round's decisions for both
sides: that order is all a sequence carries, which recorded entries are
collapsed away before one is written, and which native fields are excluded.

A [battle](battle.md) is the stream the segments are written in, and places an
action segment after the state it was decided from. A [state](state.md) defines
the position an action reads and writes. A [layout](layout.md) is a projection
of a state and holds no actions at all.

Positions are side-local and use the [layout's coordinate
system](layout.md#coordinate-system). An action never states a side: it belongs
to the sequence it is in.

## Document shape

An action segment holds one round's decisions, one sequence per side.

```yaml
kind: action
round: 7
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

`round` is the round of the state segment before it, or zero for the opening,
which has no state. Both sides are present in every segment, and a side that
decided nothing holds an empty sequence.

An action is a mapping tagged by `type`, in `snake_case`. The remaining keys are
fixed per type, and every type but `concede` carries at least one operand.

```yaml
- type: buy_unit
  unit: 30
  position: {x: 0, y: -160}
- type: upgrade_unit
  index: 5
```

Every operand is a 32-bit integer or a position, except
`release_commander_skill`'s target, which is a tagged union, and `rotated`,
which is a boolean defaulting to false.

Four keys are not always present. `choose_reinforce_item` omits `id` when the
offer was declined, `choose_advance_team` omits `specialist` when the opening is
itself an officer, `release_contraption` omits `extra_position` unless the
contraption spans two points, and `move_unit` omits `rotated` when it is false.

An ID names a catalogue entry and is never a display name. `unit` is a unit
type, `index` a formation's deployment index, and `tech`, `id`, `skill`,
`equipment` and `contraption` their own catalogues. `tower` is a position in
`BuildingManager.buildings`, the key `tower_strengthen_levels` uses.

## What a transition writes

A round's decisions write three groups of state, and this document names which
group each action touches.

| Group | Fields |
| --- | --- |
| Settled | `next_index.unit`, `next_index.contraption`, `shop.unlocked_units`, `techs.units`, `techs.officers`, `blueprints`, `tower_strengthen_levels`, `battle_skills`, `equipment` |
| Supply | `supply` |
| Board | `formations`, `constructions`, `contraptions`, `airdrop_shields`, `terrains` |

Settled is the group no fight can touch, so a round's decisions determine it
outright. The board is what the decisions arrange and the fight then consumes.

All three groups are written one decision at a time, not only at a round's end.
A position is defined after every decision, so applying one decision to the
position it was taken from has to reach the position the next decision was
taken from. That is a stronger statement than the round transition
[`battle.md`](battle.md#what-a-round-reproduces) defines, and it is the one this
document's per-action rules are answerable to.

Two inputs no decision carries stop it being a total function, and both are
named rather than guessed.

- **Where a summoned formation lands.** A card's squads and an opening's force
  are placed clear of whatever already stands, so the same card in the same seat
  lands differently in two matches. The index each takes, its type, its level
  and what recovering it pays back are all settled; only the position is not.
- **What an experience release grants.** The commander skill that raises a
  formation's experience raises it by an amount no shipped table carries.

A decision that hits either is reported unsettled. It is not counted as
reproduced, and no value is invented for it.

An allocator is settled while the objects it names are board. A contraption is
destroyed by the fight and a formation can be, but neither index is handed out
again, so the two counters only ever rise and rise only by a decision.

`reinforce_offers` is granted by the round and written by no action. The rest of
that reading needs qualifying, because a decision can reach three fields that a
round otherwise owns.

- `reactor_core` is moved by the opening and by nothing else. Both halves of
  that one choice move it, so a team and its specialist are added together.
- `buys_remaining` counts down as purchases are made, and two decisions add to
  it: energy tower skill `3` and reinforcement card `10004` each grant one more.
  `unlocks_remaining` only ever counts down.
- A formation's `exp` is the fight's to grant, except that upgrading a formation
  discards it. A rank starts at zero however much the rank below it earned.

## The actions

### `choose_reinforce_item`

Answers the round's reinforcement offer. There are two answers and both are
choices, so both are this one action.

```yaml
- type: choose_reinforce_item
  offer: 3
  id: 1305003
- type: choose_reinforce_item
  offer: -1
```

`offer` is the offer's position in `reinforce_offers`, or `-1` for the decline,
which is a choice the round always makes available and never one of the items it
dealt. `id` names the item taken and is present exactly when `offer` is not
`-1`: what the decline hands back is built from the match's progress rather than
drawn from a catalogue, so it has no ID a document could carry.

A taken item grants the thing its own ID names, and which thing that is decides
the effect: an officer joins `techs.officers`, a commander skill joins
`battle_skills`, an equipment joins `equipment`, and a unit card hands out
squads. Squads advance `next_index.unit` and their unit type joins
`shop.unlocked_units`. The decline writes none of those.

A taken item costs its price, less what an officer taken this way grants back at
once. The decline is the one reinforcement choice that pays the side instead:
declining is an item of its own rather than the absence of one.

### `choose_advance_team`

```yaml
- type: choose_advance_team
  offer: 1
  id: 9910
  specialist: 20005
```

The opening, which is one decision with two halves: the team and the specialist
officer bound to it. `offer` is the combination's position in the side's
opening offers, which a battle's header states. `specialist` is optional and
absent when the team is itself an officer.

It is round zero's only decision, and no other round holds one.

A team of units hands out its force, advancing `next_index.unit` once per squad
and unlocking each unit type it is made of. A team that is an officer joins
`techs.officers` instead. The specialist joins `techs.officers` either way. An
opening also moves `reactor_core`, which is the fight's field and not settled
here.

### `buy_unit`

```yaml
- type: buy_unit
  unit: 30
  position: {x: 0, y: -160}
```

Buys one formation of `unit` and deploys it at `position`. Advances
`next_index.unit` by one and puts a formation on the board under that index.

The formation arrives at the shop's level for that unit, which an officer or an
Energy Tower skill can raise. Costs the unit's price plus one upgrade for each
level above the first.

### `upgrade_unit`

```yaml
- type: upgrade_unit
  index: 5
```

Raises the formation at `index` by one level. Costs one upgrade for its unit
type, less what the formation's equipment discounts, floored at zero.

### `unlock_unit`

```yaml
- type: unlock_unit
  unit: 30
```

Adds `unit` to `shop.unlocked_units`. Costs the unit's unlock price.

### `upgrade_technology`

```yaml
- type: upgrade_technology
  unit: 9
  tech: 3109
```

Researches `tech`, which belongs to `unit`, and adds it to `techs.units`.

The price rises with how many technologies that unit already holds: each one
already active adds a fixed step to the next one's own price. The count is per
unit and not per side.

### `active_blueprint`

```yaml
- type: active_blueprint
  id: 2
```

Activates a Research Center blueprint and adds it to `blueprints`. A blueprint
that grants a commander skill puts it on `battle_skills`.

A chain's second level replaces its first in `blueprints` rather than joining
it. A chain blueprint also hands the side an Officer, but a state names the
chain and not the Officer, so `techs.officers` does not move. A layout has no
blueprint list, so the projection onto one names the Officer instead. Both
documents are complete; they disagree on purpose.

### `active_energy_tower_skill`

```yaml
- type: active_energy_tower_skill
  skill: 3
```

Activates one Energy Tower skill for this round. Every one of them is a
one-round effect, so nothing carries into the next round's Settled group.

Costs the skill's price less what it grants back at once. A skill may also owe
against the next round's income, and one raises the shop's level for the rest of
this round, which makes every later purchase dearer.

### `strengthen_tower`

```yaml
- type: strengthen_tower
  tower: 0
```

Raises the tower at building-manager position `tower` by one level, writing
`tower_strengthen_levels[tower]`. Costs the price of the level reached.

A position keys a tower and does not name one: the two sides order their towers
oppositely, so nothing may read a tower's identity out of its position.
[`state.md`](state.md) carries the measurement.

### `use_equipment`

```yaml
- type: use_equipment
  equipment: 1305003
  unit: 3
```

Fits `equipment` to the formation at index `unit`. The item leaves `equipment`,
the side's list of what it owns and no formation wears, and becomes that
formation's.

Fitting is free, because the item was paid for when it was taken. What it can
change is later: an item may discount every upgrade of its formation, or pay its
side an income every round it is worn.

The inventory follows one identity across a round:

```text
equipment(R+1) = equipment(R)
               + what this round's cards granted
               + what this round's officers delivered
               + what recovering a formation handed back
               − what this round fitted
```

Every term is a multiset, and so is the inventory: a side can own two of one
item.

The terms are applied where they fall in the sequence rather than all before
the fits, because an item can arrive and be fitted in the same round, and a
recovered item can be fitted again later in the round it came back.

Every fit takes a copy out of the stock. A side that fits an item it does not
hold describes a position the match cannot reach, which is a stronger statement
than the stock merely staying where it was.

A formation also leaves the board by being destroyed, which is the fight's and
not a decision's. The identity covers what the decisions do to the stock.

### `move_unit`

```yaml
- type: move_unit
  index: 0
  position: {x: 85, y: -80}
  rotated: true
```

Moves the formation at `index` to `position`. `rotated` defaults to false and
faces the formation the other way. Free, and writes nothing but the board.

A move is also the only decision that writes `travelling`, and it writes it by
region rather than by coordinate. The coordinate system has three regions per
side: the main deployment half and the two flank rectangles. A move that ends
in the region it started in leaves `travelling` alone, so a formation shuffled
about inside one flank stays travelling and one shuffled about the main half
stays settled. A move that changes region is settled by the region it arrives
in: either flank sets `travelling`, and the main half clears it. The two flanks
are separate regions, so crossing from one to the other sets `travelling` on a
formation that had already settled.

The fight then empties the set, which is why `travelling` belongs to the
deployment that produced it rather than to the position the next round starts
from. A round's opening state carries no travelling formation, and the flank
regions open at round 2, so the earliest round in which any formation travels
is round 2.

A formation this round created travels only if a move takes it to a flank. A
purchase and a card both put their formation in the main half, so neither
arrives travelling.

### `release_commander_skill`

```yaml
- type: release_commander_skill
  skill: 0
  target: !unit 4
```

Releases the skill in panel slot `skill`. The slot is a panel index and not a
skill ID, which is what a retraction matches on.

`target` is a tagged union with exactly one of three forms, never two:

| Form | Means |
| --- | --- |
| `!area [{x, y}, ...]` | the points the skill covers, in order |
| `!unit <index>` | one of the side's own formations |
| `!construction <index>` | one of the side's own constructions |

What a release writes depends on the skill. It may put a construction, a
retained airdrop shield or a terrain on the board, or take one of the side's own
formations or constructions away. Taking a formation away returns what it wore
to `equipment`, where the same round can fit it to another formation. Releasing is free, except a skill that
recovers an object, which pays back what that object cost: for a formation, its
purchase price at the prices its side's officers made at the time, plus one
upgrade for every level above the first; for a construction, a fixed amount that
is a property of its type rather than of its history.

### `release_contraption`

```yaml
- type: release_contraption
  contraption: 10001
  position: {x: -130, y: -153}
```

Buys `contraption` from the shop and places it at `position`. Advances
`next_index.contraption` by one and puts the object on the board under that
index. Costs the contraption's own price.

`extra_position` is an optional second point, for a contraption that spans two.

### `concede`

```yaml
- type: concede
```

Gives up the match. It carries no operand and writes no field: the round is not
fought, so no position follows it.

It is the last decision its side takes, and the segment that holds it is the
last one its battle holds. What the side decided earlier in the round stands in
the sequence, and so does everything the other side decided.

## Rules no action states

Four rules govern a transition and no action names any of them. Applying
actions without them produces a position that looks right and is not.

- **A card or an officer allocates formations.** A card that hands out squads
  takes the next indices as it is taken, and an officer's squad arrives before
  any of the round's own decisions. Everything bought afterwards is filed one
  along, so ignoring this files later purchases under indices that belong to
  something else, and a recovery then names the wrong formation.
- **An officer delivers on a schedule of its own,** not when it arrives. Its
  squad, its commander skills and its equipment come in its `active_round`, and
  its unit joins the shop in its `unlock_round`. Both are absolute rounds and
  the two can differ.
- **A chain blueprint's second level replaces its first,** and the Officer it
  produces follows.
- **An officer list is a multiset.** An item that may be taken again stacks, so
  taking it a second time adds a copy rather than doing nothing.

## Sequences

A sequence carries order alone. No timestamp is stored, because nothing reads
one: applying a round means applying each side's actions in sequence, and the
check that a round reproduces its successor never asks when an action happened.

The two sequences of one segment are simultaneous and secret: neither player
sees the other's while the round is being deployed. No merge between them is
attempted, since the interleaving of one side's actions with the other's is not
observable in a replay.

### Net decision

A sequence stores the decisions that took effect, so a recorded list is
collapsed before it is written. The game keeps a retraction as an action of its own, and
there are two kinds.

Replaying the recorded list into a stack defines the collapse. Every recorded
action is pushed as an entry, and `Undo` pops the newest entry whether or not it
still stands for a decision. `Redo` pushes it back.
`CancelReleaseCommanderSkill` stops the newest standing
`ReleaseCommanderSkill` with the same `SkillIndex` from counting, but the
release stays on the stack as a spent entry, and the cancel is pushed as one
too. What remains standing is the sequence, and it contains no retraction of
either kind.

Counting the spent entries is the part that is easy to get wrong, and it is what
the game does: undo walks an index back over the recorded list, so an entry that
no longer means anything still costs an undo. A cancel matches its release on
the panel index alone and not on the skill ID, and the release it retracts can
sit many entries back, so a cancel is not a stack pop.

The collapse is lossy on purpose. A sequence states what a side decided, not
what it considered, so a document cannot express that a player took an action
and withdrew it.

What the collapse is not is the game's own file format. The method a replay
calls before playback deletes nothing: it merges a round's actions from both
players, sorts them, and pads each player's list with a placeholder wherever the
merged order runs ahead, so that the two lists share an index space. The
recorded lists still hold every retraction, and collapsing them is this format's
convention.

### What the collapse and the conversion drop

`FinishDeploy` carries no decision: it appears exactly once per round-side.

`BuyUnit` does not name the unit it creates. Its recorded index is always unset,
so the index comes from the allocator, and applying a round has to hand out
`next_index.unit` exactly as the game does. That is one of the two things the
allocator is in a state for.

`MoveUnit` is recorded as a batch carrying one or more units. A sequence holds
one move per unit: the collapse has already run by then, so the batch no longer has
to stay whole for an undo to pop it, and order is all that survives either way.
It keeps only the resulting position and rotation, since the recorded
before-state restates what the state segment already holds.

`GiveUp` is kept, as `concede`, and nothing it recorded besides its type
survives.

## What a decision reproduces

The nine fields [`battle.md`](battle.md#what-a-round-reproduces) compares are
what two round snapshots can decide between them. A recording that states a position before every decision and after it decides
more, because it asks a smaller question: not what the next round holds, but
what this position looks like one decision later.

That is the transition this document defines, and it is checked two ways
against the same recording.

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

Stepping the opening answers what the position is immediately after it, which is
not what round 1 holds: [`battle.md`](battle.md#what-a-round-reproduces) says
where the two frames differ.

## Normal form

| Collection | Order |
| --- | --- |
| `blue`, `red` | as taken |

`kind` comes first in a segment and `round` second, and `blue` precedes `red`.

## Excluded fields

| Field | Why it is not in an action |
| --- | --- |
| `Time`, `LocalTime` | A sequence carries order, and nothing reads a timestamp |
| `PAD_Undo`, `PAD_Redo` | Removed by the net-decision collapse |
| `PAD_CancelReleaseCommanderSkill` | The same, together with the release it retracts |
| `PAD_FinishDeploy` | Exactly one per round-side, so it carries no decision |
| `PAD_MoveUnit.positionRecord` | Restates the state before the action |
| `PAD_MoveUnit.rotateRecord`, `superDeployRecord` | Same |
| `PAD_BuyUnit.UIDX` | Unset; the allocator names the new unit |
| `PAD_ReleaseCommanderSkill.Positions` beside an object target | The player's click point, which names no state |

## Unresolved

**Whether releasing a construction is an action of its own.**
`PAD_ReleaseConstruction` is a real type carrying a construction ID, a
deployment index and a position, and no action here corresponds to it. Every
construction this schema can describe arrives through `release_commander_skill`
instead. Either the type is unreachable in a standard match, in which case
nothing is missing, or a fifteenth action belongs here.

**Where a deferred cost lives.** An Energy Tower skill can owe against the next
round's income, which makes the debt a fact about the next position and not only
about this round's decisions. A state can carry it as a field, or a reader can
derive it from the previous round's actions. The two are not equivalent: only
the first survives a position that arrives without the round before it.

**Whether the shop's allowances are settled.** `buys_remaining` and
`unlocks_remaining` are allowances that reset each round rather than counters an
action decrements, which is why they are listed above as granted by the round.
But the allowance is not constant, and what raises it is not established. If a
decision raises it, it belongs in the Settled group and some action has to say
so.

**Whether `extra_position` stays.** It is carried because the native release
action has the field. No contraption in this build is known to need a second
point, and an optional field that nothing populates is a claim the schema cannot
support.

**Whether a round can be executed rather than only checked.** Applying a round
produces the settled fields; the board is the other half, and taking a decision
in a live game is a capability distinct from installing its result. Until both
exist, an action segment can be verified against a recording but not replayed
into one.

**Whether a retraction is ever worth stating.** The collapse discards what a
player withdrew, on the grounds that a sequence records decisions rather than
deliberation. A reader that wanted to study how a position was arrived at rather
than what it became would need the recorded list instead, and the format has no
place to put it.
