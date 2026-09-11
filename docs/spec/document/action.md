# Action definition

## Scope

An action is one decision a side takes during a deployment round. This document
defines the thirteen of them: what each carries, and what each does to the
state the round started from.

A [turn](turn.md) holds the state and the two action sequences taken from it,
and owns everything about the sequence itself: that order is all a sequence
carries, which recorded entries are collapsed away before one is written, and
which native fields are excluded. A [state](state.md) defines the position an
action reads and writes. A [layout](layout.md) is a projection of a state and
holds no actions at all.

Positions are side-local and use the [layout's coordinate
system](layout.md#coordinate-system). An action never states a side: it belongs
to the sequence it is in.

## Document shape

An action is a mapping tagged by `type`, in `snake_case`. The remaining keys are
fixed per type, and every type carries at least one operand.

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

An allocator is settled while the objects it names are board. A contraption is
destroyed by the fight and a formation can be, but neither index is handed out
again, so the two counters only ever rise and rise only by a decision.

Three things are written by no action at all. `reactor_core` and a formation's
`exp` are the fight's. `reinforce_offers`, `opening_offers` and the shop's
`buys_remaining` and `unlocks_remaining` are granted by the round.

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

The round 0 opening, which is one decision with two halves: the team and the
specialist officer bound to it. `specialist` is optional and absent when the
team is itself an officer.

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

## Unresolved

**Whether giving up is an action.** `PAD_GiveUp` is the only player action type
in the build that overrides `IsExitMatchAction`, and the override returns true
unconditionally. That argues it ends a match rather than moving a position, and
so belongs in a battle's result rather than in an action sequence. Until it is
decided, a replay holding one has no representation at all.

**Whether releasing a construction is an action of its own.**
`PAD_ReleaseConstruction` is a real type carrying a construction ID, a
deployment index and a position, and no action here corresponds to it. Every
construction this schema can describe arrives through `release_commander_skill`
instead. Either the type is unreachable in a standard match, in which case
nothing is missing, or a fourteenth action belongs here.

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
