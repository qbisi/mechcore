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
- {type: choose_reinforce_item, index: 3, name: sledgehammer_2x_lv2}
- {type: buy_unit, name: void_eye, position: {x: 0, y: -160}}
- {type: release_commander_skill, index: 0, name: intensive_training, target: {unit: 4}}
red:
- ...
```

`round` is the round of the state segment before it, or zero for the opening,
which has no state. Both sides are present in every segment, and a side that
decided nothing holds an empty sequence.

An action is a mapping tagged by `type`, in `snake_case`. The remaining keys are
fixed per type, and every type but `concede` carries at least one operand.

```yaml
- {type: buy_unit, name: void_eye, position: {x: 0, y: -160}}
- {type: upgrade_unit, index: 5}
```

`name` names what a decision is about: the unit type bought or unlocked, the
card, team, blueprint, energy tower skill, equipment item, commander skill or
contraption taken, fitted or released. It is the name [`state.md`](state.md#names)
gives that kind, and never an ID. `upgrade_technology` is the one decision about
two named things, and says `unit` and `tech`. Every other operand is a 32-bit
integer or a position, except `release_commander_skill`'s target, which is a
one-key mapping, and `rotated`, which is a boolean defaulting to false.

Two keys are not always present. `release_contraption` omits `extra_position`
unless the contraption spans two points, and `move_unit` omits `rotated` when it
is false.

`index` is a unit's deployment index, or a panel slot in
`release_commander_skill`. `tower` is a position in `BuildingManager.buildings`,
the key `tower_strengthen_levels` uses.

## What a transition writes

A round's decisions write three groups of state, and this document names which
group each action touches.

| Group | Fields |
| --- | --- |
| Settled | `next_index.unit`, `next_index.contraption`, `unlocked_units`, `techs`, `officers`, `blueprints`, `tower_strengthen_levels`, `battle_skills`, `equipment` |
| Supply | `supply` |
| Board | `units`, `constructions`, `contraptions`, `airdrop_shields`, `terrains` |

Settled is the group no fight can touch, so a round's decisions determine it
outright. The board is what the decisions arrange and the fight then consumes.

All three groups are written one decision at a time, not only at a round's end.
A position is defined after every decision, so applying one decision to the
position it was taken from has to reach the position the next decision was
taken from. That is a stronger statement than the round transition
[`battle.md`](battle.md#what-a-round-reproduces) defines, and it is the one this
document's per-action rules are answerable to.

Where a summoned unit lands is the board's rule rather than the
decision's. A card's squads, an opening's force and an officer's delivery each
land at the main deployment region's centre, aligned to the world's ten-metre
grid, or at the free grid position nearest it, which is why the same card lands
differently in two matches. [The landing index](../../rules/landing.md) states
the rule. It depends on which side is placing, because the grid and the search
are the world's, so a transition not told the side reports the landing
unsettled rather than inventing one.

An allocator is settled while the objects it names are board. A contraption is
destroyed by the fight and a unit can be, but neither index is handed out
again, so the two counters only ever rise and rise only by a decision.

`reinforce_offers` is granted by the round and written by no action. The rest of
that reading needs qualifying, because a decision can reach three things that a
round otherwise owns.

- `reactor_core` is moved by the opening and by nothing else. Both halves of
  that one choice move it, so a team and its specialist are added together.
- The round's allowances, which a state does not write, count down as they are
  spent, and a purchase, an unlock or a contraption release past its own is
  refused. Two decisions add a purchase: energy tower skill `3` and
  reinforcement card `10004` each grant one more. The card is an officer the side
  keeps, so every later round opens with the extra purchase too. The unlock and
  the contraption releases only ever count down.
- A unit's `exp` is the fight's to grant, except that upgrading a unit
  discards it and Intensive Training fills it. A rank starts at zero however much
  the rank below it earned.

## The actions

### `choose_reinforce_item`

Answers the round's reinforcement offer. There are two answers and both are
choices, so both are this one action.

```yaml
- {type: choose_reinforce_item, index: 3, name: photon_coating}
- {type: choose_reinforce_item, index: 4, name: decline_offer}
```

`index` is the position of what is taken in the round's `reinforce_offers`, and
`name` names it: a card dealt, or `decline_offer`, the decline the round always
offers after its cards. The two have to agree, so a decision that names a card
at another card's position, or the decline anywhere but after the cards, is
refused.

A card is named by what it grants, and a card of units as its unit, squads and
level, `sledgehammer_2x_lv2`. Two unit cards can share those and differ only in
the round that deals them, so a unit card's name is read within the round of
the segment that holds it.

A taken item grants the thing it names, and which thing that is decides
the effect: an officer joins `officers`, a commander skill joins
`battle_skills`, an equipment joins `equipment`, and a unit card hands out
squads. Squads advance `next_index.unit` and their unit type joins
`unlocked_units`. The decline writes none of those.

A taken item costs its price, less what an officer taken this way grants back at
once. The decline is the one reinforcement choice that pays the side instead:
declining is an item of its own rather than the absence of one. It pays the
`refund` the round's `decline_offer` states: 50 in an ordinary round, and in a
unit round the figure the match's unit reinforcement schedule states for that
round, which grows through the match.
[The reinforcement rules](../../rules/reinforcements.md#declining) give both.

### `choose_advance_team`

```yaml
- {type: choose_advance_team, index: 1, name: vortex-fire_badger, specialist: giant_specialist}
```

The opening, which is one decision with two halves: the team and the specialist
officer bound to it. `name` names the team as the header's offers do, and
`specialist` names the officer; both are required. `index` is the combination's
position in the side's opening offers, which a battle's header states.

It is round zero's only decision, and no other round holds one.

The team hands out its force as round 1 opens, advancing `next_index.unit` once
per squad and unlocking each unit type it is made of, and the specialist joins
`officers`. The deal draws a team only from teams of units and a specialist only
from specialist officers, so a decision pairing anything else is refused. An
opening also moves `reactor_core`, which is the fight's field and not settled
here.

### `buy_unit`

```yaml
- {type: buy_unit, name: void_eye, position: {x: 0, y: -160}, rotated: true}
```

Buys one unit of the unit type `name`. Advances `next_index.unit` by one and puts a
unit on the board under that index, at `position` and facing the way
`rotated` says, which defaults to false.

The game lands a purchase where [the board puts it](../../rules/landing.md) and
the player moves it from there, and a purchase states where those moves end
rather than where it landed: the round's moves of the unit it creates are
written into it, and not again as moves. A position on a flank therefore means
the unit was moved there, so the purchase sets its `travelling`, which a
move from the main half would have set. The purchase is refused where its
position is taken, as the move it stands for would be; the landing itself
always finds room or the round holds no purchase at all.

The unit arrives at the shop's level for its type, which an officer or an
Energy Tower skill can raise. Costs the unit's price plus one upgrade for each
level above the first.

### `upgrade_unit`

```yaml
- {type: upgrade_unit, index: 5}
```

Raises the unit at `index` by one level. Costs one upgrade for its unit
type, less what the unit's equipment discounts, floored at zero.

### `unlock_unit`

```yaml
- {type: unlock_unit, name: void_eye}
```

Adds the unit type `name` to `unlocked_units`. Costs the unit's unlock price.

### `upgrade_technology`

```yaml
- {type: upgrade_technology, unit: fang, tech: grenade_launcher}
```

Researches `tech`, which belongs to `unit`, and adds it to that unit's row of
`techs`. A technology's name is only unique within its unit, and `unit` is what
resolves it.

The price rises with how many technologies that unit already holds: each one
already active adds a fixed step to the next one's own price. The count is per
unit and not per side.

A Jump Drive, 高速引擎, frees every unit of its type to move in this round
and every later one, and sets their `movable`: `1606` for Wasp, `1611` for
Overlord and `1616` for Phoenix.

### `active_blueprint`

```yaml
- {type: active_blueprint, name: field_recovery}
```

Activates a Research Center blueprint and adds it to `blueprints`. A blueprint
that grants a commander skill puts it on `battle_skills`.

A chain's second level replaces its first in `blueprints` rather than joining
it. A chain blueprint also hands the side an Officer, but a state names the
chain and not the Officer, so `officers` does not move. A layout has no
blueprint list, so the projection onto one names the Officer instead. Both
documents are complete; they disagree on purpose.

### `active_energy_tower_skill`

```yaml
- {type: active_energy_tower_skill, name: mass_recruitment}
```

Activates one Energy Tower skill for this round. Every one of them is a
one-round effect, so nothing carries into the next round's Settled group.

Costs the skill's price less what it grants back at once. A skill may also owe
against the next round's income, and one raises the shop's level for the rest of
this round, which makes every later purchase dearer.

### `strengthen_tower`

```yaml
- {type: strengthen_tower, tower: 0}
```

Raises the tower at building-manager position `tower` by one level, writing
`tower_strengthen_levels[tower]`. Costs the price of the level reached.

A position keys a tower and does not name one: the two sides order their towers
oppositely, so nothing may read a tower's identity out of its position.
[`state.md`](state.md) carries the measurement.

### `use_equipment`

```yaml
- {type: use_equipment, name: photon_coating, index: 3}
```

Fits the item `name` to the unit at `index`, as `upgrade_unit` and
`move_unit` name one. The item leaves `equipment`,
the side's list of what it owns and no unit wears, and joins the end of
that unit's. A unit wears at most as many items as it has slots, which is the
same count [`layout.md`](layout.md#unit) gives a placement; fitting a full
unit is refused.

Fitting is free, because the item was paid for when it was taken. What it can
change is later: an item may discount every upgrade of its unit, or pay its
side an income every round it is worn. Fitting the Deployment Module,
`13040001`, frees its unit to move in this round and every later one, and
sets its `movable`.

The inventory follows one identity across a round:

```text
equipment(R+1) = equipment(R)
               + what this round's cards granted
               + what this round's officers delivered
               + what recovering a unit handed back
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

A unit also leaves the board by being destroyed, which is the fight's and
not a decision's. The identity covers what the decisions do to the stock.

### `move_unit`

```yaml
- {type: move_unit, index: 0, position: {x: 85, y: -80}, rotated: true}
```

Moves the unit at `index` to `position`. `rotated` defaults to false and
faces the unit the other way. Free, and writes nothing but the board.

Only a unit whose `movable` is true may move, and a move of any other is
refused rather than applied. A move is refused too where its place is taken:
the unit's footprint has to lie inside the region `position` lies in and
overlap nothing standing there but the unit itself, the rule
[landing.md](../../rules/landing.md#moving-and-placing) states. A unit is free to move in the round it
arrives. In a later round, what frees it is a decision of this document: fitting
a Deployment Module, researching its unit's Jump Drive, or releasing Redeploy at
it.

A move is also the only decision that writes `travelling`, and it writes it by
region rather than by coordinate. The coordinate system has three regions per
side: the main deployment half and the two flank rectangles. A move that ends
in the region it started in leaves `travelling` alone, so a unit shuffled
about inside one flank stays travelling and one shuffled about the main half
stays settled. A move that changes region is settled by the region it arrives
in: either flank sets `travelling`, and the main half clears it. The two flanks
are separate regions, so crossing from one to the other sets `travelling` on a
unit that had already settled.

The fight then empties the set, which is why `travelling` belongs to the
deployment that produced it rather than to the position the next round starts
from. A round's opening state carries no travelling unit, and the flank
regions open at round 2, so the earliest round in which any unit travels
is round 2.

A unit this round created travels only if it reaches a flank. A card puts
its unit in the main half, and a purchase whose position is on a flank is
one whose moves took it there.

### `release_commander_skill`

```yaml
- {type: release_commander_skill, index: 0, name: intensive_training, target: {unit: 4}}
```

Releases the skill in panel slot `index`, which holds the skill `name`. The slot
is what the game releases and what a retraction matches on; the skill is stated
beside it so that a release reads without the panel. A release whose slot does
not hold that skill at that point in the round is refused, and the panel can change
within a round, since a card or a blueprint adds a skill to it.

`target` is a mapping with exactly one of three keys, never two:

| Form | Means |
| --- | --- |
| `{area: [{x, y}, ...]}` | the points the skill covers, in order |
| `{unit: <index>}` | one of the side's own units |
| `{construction: <index>}` | one of the side's own constructions |

The key names the kind of target, as the list the index points into is named,
and the form is plain YAML rather than a tag such as `!unit 4`, so any YAML or
JSON reader takes a battle as it is.

What a release writes depends on the skill. It may put a construction, a
retained airdrop shield or a terrain on the board, or take one of the side's own
units or constructions away. Taking a unit away returns what it wore
to `equipment`, where the same round can fit it to another unit. Releasing is free, except a skill that
recovers an object, which pays back what that object cost: for a unit, its
purchase price at the prices its side's officers made at the time, plus one
upgrade for every level above the first; for a construction, a fixed amount that
is a property of its type rather than of its history.

A deployment skill is not a release the fight sees. It does its work on the
position during deployment, so the decision writes that work and marks the slot
`used`: the slot gains no `release`, carries no target or place in the release
order, and never reaches a layout.

Intensive Training, `1100001`, is one. It targets one of the side's own
units with `{unit: <index>}` and fills its experience bar, so `exp` becomes
`maximum/maximum`. A full
unit takes no further share of the experience a fight hands out;
[the unit experience index](../../rules/unit_experience.md) gives the amount a
full bar stands for per unit and level.

The skill cannot target a unit at level 9 or one whose bar is already
full, and a decision that does is refused rather than applied.

Redeploy, `1000001`, is the other. It targets one of the side's own units
with `{unit: <index>}` and sets its `movable`, so the unit may move for the rest of
the round.

### `release_contraption`

```yaml
- {type: release_contraption, name: shield, position: {x: -130, y: -153}}
```

Buys the contraption `name` names, by the name a layout gives it, from the shop
and places it at `position`. Advances
`next_index.contraption` by one and puts the object on the board under that
index. Costs the contraption's own price.

`extra_position` is an optional second point, for a contraption that spans two.

A contraption is refused where its place is taken, as a move is. A shield and a
missile take no part in deployment collisions and are never refused for it.

### `concede`

```yaml
- {type: concede}
```

Gives up the match once the side has finished deploying. It carries no operand
and writes no field. The round is still fought, and the match ends after it,
so no position follows it. The corpus's one concession is recorded after its
side's `FinishDeploy`, and its round's fight replays in full; a concession
recorded before `FinishDeploy`, which would leave the round unfought, is one
this format does not hold.

It is the last decision its side takes, and the segment that holds it is the
last one its battle holds. What the side decided earlier in the round stands in
the sequence, and so does everything the other side decided.

## Rules no action states

Four rules govern a transition and no action names any of them. Applying
actions without them produces a position that looks right and is not.

- **A card or an officer allocates units.** A card that hands out squads
  takes the next indices as it is taken, and an officer's squad arrives before
  any of the round's own decisions. Everything bought afterwards is filed one
  along, so ignoring this files later purchases under indices that belong to
  something else, and a recovery then names the wrong unit.
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
too. Undoing the cancel stands its release again, and redoing the cancel spends
it once more. What remains standing is the sequence, and it contains no
retraction of either kind.

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

`ReleaseCommanderSkill` records only the panel slot. The conversion reads the
skill that slot holds from the position the round has reached by then, which
is what `id` states.

`MoveUnit` is recorded as a batch carrying one or more units. A sequence holds
one move per unit: the collapse has already run by then, so the batch no longer has
to stay whole for an undo to pop it. It keeps only the resulting position and
rotation, since the recorded before-state restates what the state segment
already holds. Not even the moves' order survives: a sequence holds them in
the order [settling](#settling-a-round) gives them.

A unit's moves keep what they amount to, not the route. A move settles
`travelling` by the region it arrives in, and a round's opening holds no
travelling unit, so a unit that begins its moves in the main half
travels exactly when its last move ends on a flank, whichever way it went:

- A unit this round bought keeps none of its moves. The purchase carries
  where they end.
- Any other unit that begins its moves in the main half, one a card handed
  out included, keeps its last move alone.
- A unit that begins them on a flank keeps the last move of each stretch
  that ends in one region, the main half or one flank. Going to the main half
  and back travels where staying would not, so the route between regions
  matters for it.

The conversion steps the collapsed round from the position the round opened
with and refuses a replay where it does not end exactly where the recorded
round does.

### Settling a round

A sequence holds its decisions in an order the board allows each one in where
it stands, and the same order however the round was recorded, so conversion
settles the collapsed round before writing it:

1. The moves follow the side's other decisions, ascending by unit, a unit's own
   in the order it made them. A move of a unit that the round then takes off
   the board is dropped, since it ends up nowhere.
2. Each decision is then taken at the first point the position allows it and
   it steps. A purchase whose place a unit still holds waits until that unit
   has moved or left; a contraption waits the same way. Decisions that hand out
   a unit's index, purchases and reinforcement choices, keep their order, as
   the contraptions, which the game numbers as it places them, and a unit's own
   moves do.
3. When nothing left can be taken, the moves are waiting on one another, as two
   units trading places do. The first waiting unit with room first steps
   aside, within the region it stands in, to the free grid position nearest
   the region's centre that no waiting decision needs, found as
   [landing](../../rules/landing.md) finds one; that move is a decision of the
   sequence like any other.

A sequence settled this way is its own settling, so a battle written as a
replay and converted back settles to itself. Applying a round refuses a
decision whose place is taken, so every decision a battle holds is one the
game takes where it stands.

`GiveUp` is kept, as `concede`, and nothing it recorded besides its type
survives.

## What a decision reproduces

Applying a decision to the position it was taken from gives the whole next
position, board included: not what the next round holds, but what this
position looks like one decision later. That is the transition this document
defines.

A battle states no position between two decisions, so a decision is checked
through the round it belongs to. [`battle.md`](battle.md#what-a-round-reproduces)
steps a round's decisions in order and compares what the next round opens with
against the recorded state, and a layout captured live at the end of a
deployment is compared against the projection of that round's decisions,
stepped from the position it opened with.

Stepping the opening answers what the position is immediately after it, which is
not what round 1 holds: [`battle.md`](battle.md#what-a-round-reproduces) says
where the two frames differ.

## Normal form

| Collection | Order |
| --- | --- |
| `blue`, `red` | as [settled](#settling-a-round) |

`kind` comes first in a segment and `round` second, and `blue` precedes `red`.
Each action is written on one line as a flow mapping, by the spelling rules
[`battle.md`](battle.md#normal-form) states for every segment.

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
