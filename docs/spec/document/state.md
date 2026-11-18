# State definition

## Scope

A state describes one complete match position: everything both players hold at
one moment of one round. It is the largest of the four documents this format
defines, and the other three relate to it by projection and by composition.

`mechcore mask <state> --side blue|red --unit <type>` reads a standalone state
for geometric purchase positions; the [CLI contract](../mechcore/cli.md#mask)
defines its offsets, bit masks and decision-view boundary.
`mechcore preview <state>` also reads that position for initialized effective
attributes and initial target relationships, as the
[preview contract](../mechcore/cli.md#preview) defines.

```yaml
kind: state
map_id: 1001
seed: 2038621361
round: 7

reinforce_offers: [phoenix_2x_lv2, sabertooth_2x_lv2, steel_ball_2x_lv2, sledgehammer_2x_lv2, refund_400]

blue: {reinforce_choice: -1, ...}
red: {reinforce_choice: -1, ...}
```

`map_id` and `seed` carry the same meaning and the same optionality as in a
layout. They belong to the document rather than to a side because both sides
share them.

`reinforce_offers` lists the cards the round dealt, in the order it dealt them,
and then the decline as `refund_<amount>`, for example `refund_400`, naming
what declining pays in this round. A round that deals offers always offers the
decline after them. What the decline pays is the round's, not the position's,
and it is not dealt: it is the map's figure, or in a unit round the schedule's.
Writing it lets a position price its decline without replaying the deal. With
`rules.reinforcements: false` in the match header, every state omits the offer
and no decline is available; the shared stream stays at its opening checkpoint
and the pool delta stays empty.

Each side's `reinforce_choice` is absent in a round without offers. When
there are offers, it is `-1` until the side answers and then the zero-based
index of the card or refund in `reinforce_offers`. The refund's index is
the number of cards dealt. A round's opening position has an unanswered
choice whenever that round deals offers.

A state is defined after each action, not only at a round's ends. A
[match](match.md) writes the position each round opens with as a state
segment, followed by the [decisions](action.md) taken from it. A state segment
carries `kind: state` and `round`, and leaves `map_id` and `seed` to the
match's header, which states them once for every round.

## Random streams and the ordinary reinforcement pool

Every state carries three `random_state` arrays of four unsigned 64-bit words:
one at the state root for the shared reinforcement stream, and one under each
side for its player stream. None may be all zero. At a round opening these are
the streams after the reinforcement deal and the officers' deliveries, before
either side acts. A state's streams advance only when a rule draws from them.

`reinforce_pool` states the effective membership changes relative to the
ordinary pool initialized by the match header's seed:

```yaml
reinforce_pool:
  removed: [mass_produced_fortress]
  added: [extended_range_fortress]
```

Both lists use reinforcement card names, ordered by card ID without duplicates.
`removed` contains only initial members no longer held; `added` contains only
noninitial members now held. The lists are disjoint. Empty lists are omitted,
and `reinforce_pool` itself is omitted when both are empty. This is a set
difference, not an operation history. The initial pool is derived from the seed
and is not repeated in the header. A pool holds at most one member of each
positive card group.

An opening checkpoint includes the current round's condition refresh. The
round's nonrepeatable choices remove their cards before the next deal. Temporal
exclusion of level-4 commander skills in the following round follows from this
round's `reinforce_offers`, and is not a separate pool membership change.
The checkpoint, decisions and header suffice to continue the random streams
and the pool without earlier decisions; the fight still determines its own
part of the next opening position.

## Relation to a layout

A layout is not a different object from a state. It is the projection of a state
onto what the fight simulates:

```text
layout = project(state)
```

Because a state is defined after every action, so is the projection. Running an
action list forward and projecting after each step yields the layout a Training
Ground scene would install to reproduce the match at that moment, which gives a
finer failure signal than comparing a round's endpoints alone.

What the projection drops is everything the fight cannot observe: supply, the
unlocked units, the reinforcement offer, the random streams, the pool changes,
the allocators, and the parts of the skill panel that neither stand on the battlefield nor were released this round.

Each unit carries its `source`. In an opening state, an inherited unit is
`legacy` and leaves the field out; the first round's advance team is `joined`,
and a squad an officer delivers as the round opens is `delivered`. Units bought
or granted by the round's decisions are `joined` while applying those decisions.
Projection checks identities and sources against the opening roster and
allocator. It adds fitted equipment in slot order to each surviving unit
and lists recovered `delivered` squads in `Layout.recovered`.

The side copies `officers`, `techs`, `constructions`, `contraptions` and
`tower_strengthen_levels`. Two more fields are filtered:

| State source | Layout target | Projection |
| --- | --- | --- |
| `blueprints` | `blueprints` | the enhancement chains alone: `4`, `401`, `5`, `501` |
| `energy_tower_skills` | `energy_tower_skills` | copied, keeping `5` and `6` |

A valid blueprint list cannot hold both levels of one chain, so each chain
contributes at most one Officer. Blueprint IDs `1`, `2` and `3`, and Energy
Tower skill IDs `1`, `3` and `4`, reach no layout field: their fight-visible
consequences are already carried by the skill panel or by units, while
their economic consequences are not part of a layout.

`battle_skills` is projected rather than copied:

| Panel source | Layout `battle_skills` entry |
| --- | --- |
| each object in a slot's `standing` | one standing entry, `{name, standing}`, named by the slot's skill; all of them first, in the layout's normal-form order |
| each slot with an ordered `release` | one release entry, `{name, positions}`, after the standing entries, sorted by `release.order` |
| every other slot | nothing |

A release entry resolves the slot's native `id` to the layout's semantic name
and maps an area target to `positions`. Deployment releases have already
changed the state and have no layout entry. Projecting an ordered release
with an object target is refused.

A round's opening position carries no release at all, since a state is defined
after each action and a round opens before its first. The releases of a round
belong to the position its deployment closes with, which is what a captured
layout holds.

## Match level and side level

The reinforcement offer is dealt once per round and both players choose from the
same array, so it is the one field the two sides share rather than one each.

Round 1 is dealt no offer, and neither is the opening the game numbers round 0.
The field is absent there rather than an empty array, since no array was dealt.

### The opening offer is private, and it is not a state field

The opening is the exception, and it is not here. It deals each side four
combinations of an advance team and a specialist officer, privately: neither
player sees the other's, so it could never be `reinforce_offers`, which the two
sides share precisely because both choose from one array.

It is not under a side's state either, because the position it is dealt in is
not a round worth stating. A match's [header](match.md#the-opening-offers)
holds the four combinations under each side.

## Random state

No random stream is a state field. Four streams exist:

| Stream | Seed | Recorded |
| --- | --- | --- |
| `Match.random` | `BattleInfo.SystemSeed` | yes |
| `Player.random` | `PlayerRecord.seed` | yes |
| `Match.roundRandom` | `SystemSeed + round` | no |
| `FightTeam.random` | `(round + teamIndex) * 4444` | no |

The two recorded streams are excluded on the argument the reinforcement pool
follows: they decide what a later round is offered or handed, and this round's
offer is already stated by `reinforce_offers`, as this round's hand-out is by
the side's `equipment`. `Player.random` is what an officer that hands out one
of its items draws from; a [match](match.md) states each side's seed in its
header, and nothing else in a standard match draws from it. Neither is read by
a fight, so neither changes anything a layout can express. The two derived streams are computed from
the round, the team index and the match seed, all of which a document already
carries, so nothing has to store them either.

A state is not a machine that generates its own successors. It takes the offer
as an input, which is what lets it drop the streams that would produce one.

### What an installer must not do

Because no state carries either recorded stream, an installer leaves both
generators as the match's own initialisation left them. That is a legal
position, merely not the recorded one, and it is safe exactly while every offer
is supplied rather than rolled and no side holds an officer that draws what it
hands out, whose next item would then be the installed match's draw rather than
the recorded one.

Writing nothing must never be implemented as writing a zero state. The restore
path dereferences the word list without a guard and refuses a length mismatch,
so an empty list is an error rather than a default. Worse, four zero words are a
fixed point of the generator: every later draw is zero and stays zero. An
installer with no state to write skips the call rather than writing an empty
one.

## Side state

Each side carries the layout fields that project unchanged, the three that
reach a layout transformed, and the fields a layout has no reason to hold.

```yaml
    blue:
      reactor_core: 197
      supply: 50

      unlocked_units: [marksman, fang, crawler, arclight, wraith, sabertooth, typhoon, phantom_ray, hound, void_eye, vortex]

      blueprints: [sticky_oil_bomb, field_recovery, attack_enhancement_ii]
      energy_tower_skills: [rapid_resupply]
      tower_strengthen_levels: [1, 1]

      equipment:
        - {name: improved_firepower_control_system}

      battle_skills:
        - index: 0
          name: intensive_training
          cooldown: 0
          release:
            target: {unit: 8}
        - index: 1
          name: lightning_storm
          cooldown: 0

      next_index:
        unit: 29
        contraption: 15

      officers: [supply_specialist, efficient_light_manufacturing]
      techs:
        fang: [grenade_launcher]
        tarantula: [field_maintenance, spider_mine]
      units: [ ... ]
      constructions: [ ... ]
      contraptions: [ ... ]
```

A side always writes its `next_index` allocators and its `units` list.
It omits `constructions` and `contraptions` when their lists are empty.

### A unit carries its source and recovery value

```yaml
      units:
      - {name: crawler, index: 5, position: {x: -140, y: -135}, exp: 450/450, value: 100}
      - {name: marksman, index: 6, position: {x: 0, y: -160}, value: 0, source: delivered}
      - {name: fang, index: 7, value: 50, level: 2, source: joined, recovered: true}
```

`value` is what the side paid for the unit, at the prices its officers made
at the time, and it is what recovering the unit pays back. Two units of
one type and level can differ in it.

`source` uses the layout's three categories and defaults to `legacy`.
A match writes only two opening exceptions: `joined` for the first round's
advance team, and `delivered` for the squads its officers hand out that round.
Every surviving unit becomes `legacy` at the next round's opening, including
previous deliveries. Source describes arrival, not a permanent movement grant.

`movable` is not a document field. Deployment movement is derived internally:
a `joined` or `delivered` unit may move, and a `legacy` unit may move only if
its Deployment Module or Jump Drive frees it, or the panel records a
Redeploy release targeting it in this deployment.
[The mobility index](../../rules/mobility.md) gives the rule and its evidence.

When a unit is recovered, its row retains its index, name, source, value and
level and gains `recovered: true`. The row has no position, experience,
rotation, travelling flag or recorded pose. Its recovery release names the
same index. Only active rows carry positions, and only active rows take up
space or wear equipment. The next opening drops the tombstone and keeps the
index spent. Recovering a construction removes its row.

### The allocators

`next_index` holds the two live allocators. They are state, not a derived
maximum: a contraption is consumed when used, so its allocator runs far ahead of
what survives, and an index that vanishes is never handed out again.

What the allocators carry that the object lists cannot is history. Buying a unit
and undoing it returns the visible state and returns the allocator too, so two
positions that look identical can still differ here, which is what a state diff
has to be able to see.

`constructionIndex` is recorded by the game but is not an allocator under 1v1
rules, so it is left out: it takes its value for the match in round 1 and never
moves, and the construction list only ever shrinks. Each construction still
carries its own `index`, which is its identity and is unaffected.

### The two fixed towers

`tower_strengthen_levels` is a per-tower counter, keyed by the tower's position
in `BuildingManager.buildings`. A layout holds the same two numbers under the
same name keyed the same way, so the projection copies the list. Both key by
that position because it is what `PAD_StrengthenTower.Index` names and what the
restore path resolves.

**A position keys a tower; it does not name one.** The two sides do not agree on
which position holds which tower: on map 1021 blue holds the Energy Tower at
position 0 and red holds it at position 1. A side's buildings are appended in
the order its own territory lists them, nothing on that path sorts or compares
the building kind, and the two territories are mirror images. The order is map
data per side, so it may also differ from map to map.

So no constant names a position, and no code may assume one. A capture writes
each level under the position it read it from, and an installer strengthens the
tower at the position the layout keyed. Both check only that a side holds one
tower of each kind, in whichever order, so a side that lost a tower or grew one
fails loudly instead of having its levels written to the wrong building.

A side holds exactly two towers. Both draw from one shared catalogue whose four
rows raise the crystal's life, so a level is in `0..=4` for either tower.

### Equipment ownership and fitting order

The side's `equipment` lists every owned copy. Unfitted copies carry only
`name`. A fitted copy also carries `fitted`, with a standing unit's index
and its zero-based fitting slot.

```yaml
      equipment:
        - {name: laser_sights}
        - {name: laser_sights, fitted: {unit: 5, slot: 1}}
        - {name: heavy_armor, fitted: {unit: 5, slot: 0}}
```

A side can own several copies of an item, and a formation's fitting slots run
from zero in the order it fitted them. The example projects the unit's items
as `[heavy_armor, laser_sights]`. Duplicate slots and gaps are refused.
Recovering the unit returns its copies to stock by clearing their fittings.

A native replay writes each copy to `equipmentDatas` with permanent durability
`-1`, and each fitted copy to its unit's `equipments` list in slot order.
The stock on import comes from the inventory after accounting for the copies
listed on units.

### Supply and reactor core

Both start at a per-map constant and diverge only through play. The map's row in
`matchSettings`, keyed by `map_id`, gives `reactorCores`, `firstRoundSupply`,
`roundSupplyIncreaseValue` and `maxRoundSupply`.

`reactor_core` is recorded as it stands. It falls only as the outcome of a
fight, which no state can predict, and it rises only across the opening, by the
amount the advance team carries.

`supply` is what the side can spend at the moment the state describes. The
round's income has already been added to it, and every purchase, upgrade and
sale since has already been applied. It is the number a purchase's legality is
tested against and the number an installer writes.

That is deliberately not the number the replay stores, which is the residue from
before the round's income arrived. A converter rebuilds it; [the match
document](match.md) says how.

Installing a state is where the definition bites. Because the field already
includes the round's income, writing it directly leaves the game free to add
that income a second time. The game has a one-shot latch that suppresses exactly
that, and an installer has to set it even though no state document carries it.

### The shop

`unlocked_units` names each unit type the shop sells, by the name a unit's
`name` gives, in ascending unit ID. `locked_units` is dropped because it is the
complement against the build's unit catalogue. `MaxUnlockCount` is dropped
because it is the shipped constant plus a modifier no standard 1v1 source
provides.

**A round's allowances are not state fields.** A round allows two purchases,
one more for every Additional Deployment Slot (`10004`) the side holds, one
unlock, and eight contraption releases, which
[contraptions.md](../../rules/contraptions.md) states. Each is what the
round's own decisions spend, and each opens at a value the side's officers
fix. A state is the position a round opens with, so all three follow from its
`officers`, and a reader sets them there; they describe a position in the
middle of a round, after some of its decisions, and never cross from one round
into the next. What a decision does to them, and when one is refused for
spending past it, is [action.md](action.md)'s. The recorded counters could not
be copied in any case: the purchase and unlock counters describe the previous
round, because the snapshot precedes the round's own reset, and the record keeps
no contraption counter at all.

### Officers and technologies

`officers` lists the officers the side holds. An officer card that may be
taken again appears once per copy. `techs` holds unit technologies alone, grouped under the unit type they
belong to, as the replay groups them and as a match's `tech_loadout` does. A
unit that has researched nothing has no row.

The state of being unlocked but not active does not arise under standard 1v1
rules, so a technology is either listed or not.

### Names

A state names what it holds rather than numbering it, and a document that names
something writes the key `name`. A unit type is the name a unit's `name`
gives, and a contraption the name a layout gives it. An officer, a technology, a
blueprint, an energy tower skill, a commander skill and an equipment item are
the game's own English names in snake case, apostrophes dropped, which
[`config/names.yaml`](../../../config/names.yaml) holds: the
officer `supply_specialist`, the technology `grenade_launcher`, the blueprint
`field_recovery`, the energy tower skill `rapid_resupply`, the commander skill
`intensive_training`, the equipment item `photon_coating`.

What is named is what a standard 1v1 match can hand a side: the opening
specialists and the officers a reinforcement card grants, the commander skills
a card, one of those officers or a blueprint grants, and the equipment a card or
one of those officers hands out. Names are distinct within a kind, and a
technology's within its unit, which is the only place a state names one. One
clash is resolved by rule: the Mobile Beacon a card grants is
`mobile_beacon_card`, beside the blueprint's `mobile_beacon`.

A reinforcement card is named by what it grants, and a card of units as its
unit, squads and level, `sledgehammer_2x_lv2`. That leaves out the round the card
belongs to, which two cards can differ in alone, so a unit card's name is read
within the round of the segment holding it.

What a name stands for is still ordered by ID, the order the game lists it in,
and a name the build does not carry is refused.

### The research centre and the energy tower

Three native lists sit here. `blueprints` names what the research centre has
activated, `energy_tower_skills` which of the energy tower's skills this round
has activated, and `tower_strengthen_levels` how far each tower has been
reinforced.

The blueprint catalogue has two kinds. One is an upgrade chain: `4` and its
successor `401`, `5` and its successor `501`. The other grants a commander
skill. Under standard 1v1 rules the pool a match offers is `[1, 2, 3, 4, 5]`,
because a row that needs research is kept only when a game rule enables
blueprint research, and no standard match enables it.

That is why `research_queue` is not a field. It holds a blueprint from the
moment research starts until the skill joins the panel, and under standard 1v1
rules nothing can enter it. The field belongs to game rule `999917`, and it is
that rule, not this format, that should introduce it.

#### Why these do not fold into the skill panel

The panel plus the two chain levels do reproduce `blueprints`, by subtracting
the skills a side's officers grant and mapping each remaining entry back. That
is not enough to drop the field.

The panel is a union of three sources, and one of them is history rather than
state. Reinforcement drops add commander skills to the same panel from the same
ID space, and a state does not record which entries came from a drop. Recovering
`blueprints` would mean subtracting those, which works only while the two
catalogues do not collide.

That disjointness is maintained by hand rather than guaranteed. A game rule can
swap a blueprint for one granting the same skill at a different price and remove
the drop versions of the skills the research centre sells. Both are edits to
data, made to keep one skill from having two provenances. A state that
reconstructed `blueprints` from the panel would depend on those edits staying
correct, and would need the match's game rules to read them, which it does not
carry.

#### The chain half is duplicated by the officer list

Activating a chain blueprint also grants its product officer: `4` grants
`20310`, `401` grants `20311`, `5` grants `20300`, `501` grants `20301`. A chain
replaces rather than appends, so a side holding the second level lists it alone.

The blueprint list and those officers are two spellings of one fact.
`blueprints` owns it, and `officers` must not name `20300`, `20301`, `20310` or
`20311`. A layout spells it the same way, keeping the chains alone: a blueprint
that grants a skill reaches a fight only as that skill's release.

#### The energy tower keeps all five

Every energy tower skill lasts this round only, so this field is the set
activated in the round the state describes. Like everything else here it is defined after
each action, which means it is empty at a round's start and fills as the round's
actions are applied.

A state lists all five and a layout keeps only the two that reach a fight, `5`
and `6`. Skills `3` and `4` are recruitment and reach a fight only through the
units they produce, and `1` is economic. `1` is the one that must not be
dropped, because it is the half of a decision the next round pays for: it grants
supply now against a deduction from the next round's income. Carrying the skill
carries that debt, and nothing else in the document states it.

The recorded field is a different quantity and must not be copied into this one.
The game snapshots only skills with a deferred half, which admits `1` alone, and
it writes them a round late, because the activation flag survives into the
following round. So the recorded list is an input to reconstructing `supply`,
not a source for this field, which is rebuilt from the round's activations.

Installing one needs care in the other direction: the activation flag has to be
set without paying the immediate half a second time, and an installed state
whose flag is missing gives the next round too much supply.

## The skill panel

A layout lists only what a fight sees of the panel: the objects earlier releases
left standing, and this round's releases, in release order. A state lists the
whole panel, because an action references a skill by its panel slot and a slot
the state does not carry cannot be resolved.

```yaml
      battle_skills:
        - index: 0
          name: intensive_training
          cooldown: 1
        - index: 1
          name: shield_airdrop
          cooldown: 2
          standing:
            - {position: {x: -285, y: -22}}
        - index: 2
          name: lightning_storm
          cooldown: 0
          release:
            order: 0
            target: {area: [{x: 50, y: 44}, {x: 189, y: 38}]}
        - index: 3
          name: field_recovery
          cooldown: 0
          release:
            target: {unit: 12}
```

`index` is the position at which the skill joined the panel, and it is the key a
release names. It is lifelong identity, not a recyclable slot: the panel never
shrinks and never reorders, and each round's panel is a prefix extension of the
previous one. The panel is sorted by `index`, which duplicate skill IDs make the
only well-order available, since one panel can hold the same ID twice.

`cooldown` covers a skill that cannot be released this round. A layout cannot
express one, because a layout only names releases. It is counted as a round
opens: a slot the round before released, including deployment releases,
restarts at its skill's cooldown, and every other slot drops by one to zero. A
slot joins the panel at its skill's initial cooldown. [The commander skill
index](../../rules/commander_skills.md) gives both per skill.

`release` is present on a skill released this round. A battle release carries
its zero-based `order` among the side's battle releases and the target.
Field Recovery, Redeploy and Intensive Training carry their deployment target
and omit `order`. A round's opening position carries no releases.

`standing` lists the objects the slot's earlier releases left on the
battlefield, in the payload [the layout](layout.md#standing-entries) gives the
slot's skill, and is absent when there are none. Only a `shield_airdrop` or a
`sticky_oil_bomb` slot carries it; any other skill carrying it is refused by
name. A slot may carry more than one, since a Shield Airdrop is not
time-limited and the slot may release again while an earlier shield stands,
and a slot may carry `standing` and `release` together. Order within the list
carries nothing.

### The release target is exclusive

A release either covers an area or points at one object, never both.
[`action.md`](action.md) defines the three target forms, and a release here
carries the same union.

The recording does not look like this: a pointing release carries a position
beside its object index. That position is the player's click point rather than
the target's location, and a click point that resolves to an object has no
effect on the fight. Storing it would let one state have many documents, so a
state stores the resolved target and an executor supplies a coordinate, using
the target's centre, when the native call needs one.

The target form belongs to the release and not to the skill: one skill can point
at a unit in some releases and at a construction in others.

Area lengths are fixed per skill and range from one to three points, matching
the `Positions` column of the [battle skill index](../../rules/battle_skill.md).

## Normal form

A unit's `exp` is written `current/maximum` as a layout writes it, and is
absent when its `current` is `0`.

| Collection | Order |
| --- | --- |
| `battle_skills` | ascending `index` |
| `equipment` | ascending item ID, unfitted copies first, then fitted unit index and slot |
| `units`, `constructions`, `contraptions` | ascending `index` |
| `officers` | ascending ID, a multiset |
| `techs` | ascending unit ID, each unit's technologies ascending ID |
| `unlocked_units` | ascending unit ID |
| `blueprints`, `energy_tower_skills` | ascending ID |
| `reinforce_pool.removed`, `reinforce_pool.added` | ascending card ID, sets |
| a slot's `standing` | a shield by ascending `(x, y)`, an oil area by its control points |
| `reinforce_offers` | as dealt, then the decline; a choice names a position in it |
| `tower_strengthen_levels` | by building-manager position |

The rule behind the first three rows is that a collection is ordered by the key
the actions use to reference it, so document order and action resolution can
never disagree. The equipment table's order leaves each formation's fitting
order in its slots.

One cross-check goes with that order rather than in it. Each entry of
`next_index` must be greater than the highest index in the collection it
governs, so an allocator cannot drift away from the objects it has handed out.
The check is what a sentinel entry in the list itself would buy, without making
a typed collection carry an element of a second shape that a layout would then
also have to admit.

## Rebuilding a state without the game

Most of a round's state can be read out of a replay without running the game.
Three fields cannot be copied. Two are stale, because the snapshot precedes the
round's own reset: `supply` and each slot's `cooldown` state what stood before
the round's income and count-down arrived. The third, `energy_tower_skills`, is
not stale but a different quantity, and it is rebuilt from the round's actions.

The unit roster comes from the per-round player data rather than from the
match-level fight report, because the first two rounds precede the first fight
and so have no report to read.

The snapshot also precedes what the round's officers deliver as it opens, so
those deliveries are made on top of it: the squads, skills, equipment and
unlocks each officer's schedule names for the round. A delivered squad has no
recorded landing, and lands where [the board puts it](../../rules/landing.md).
Unit sources are classified at the round opening as
[`match.md`](match.md#what-conversion-rebuilds) states; movement permission is
an internal derivation, not a written field. Projecting a completed deployment
also preserves each legacy unit's opening position and rotation as layout
`recordPosition` and `recordRotated` when they differ from the final
`position` and `rotated`. Both are derived from the opening state by unit
index, before applying this round's actions, and omitted independently.

A slot's `standing` is copied from somewhere other than an object list. A
skill that leaves an object standing keeps it in that skill's `rangeItems`, so
it is read out of the recorded skill panel, onto the slot it is recorded under;
[the match document](match.md) states the rule.

## Excluded fields

| Field | Why it is not in the state |
| --- | --- |
| `playerData.preRoundFightResult` | Neither a decision nor a simulation input |
| `playerData.IsSpecialSupply` | A one-shot latch, always clear under 1v1 rules, see below |
| `playerData.researchQueue` | Unreachable without game rule `999917`, above |
| `matchDatas.deadCount` | Carries nothing a position needs |
| `NewUnitData.EquipmentID` | Always 0; the unit's `equipments` list holds what it wears |
| `NewUnitData.Durability` | Unused for units in this build |
| `EquipmentData.durability` | Always `-1` for equipment in the supported 1v1 rules |
| `NewUnitData.RoundCount`, `SellSupply` | Both follow from when the unit was bought and what it cost |
| `ConstructionSnapshotData.durability` | One entry per segment, and inert |
| `matchDatas.teamRanks` | Seat ordering; no mechanism reads it |
| `matchDatas.poolOPs` | Replaced by effective `reinforce_pool` membership changes |
| `matchDatas.RoundExcludeReinforce` | Next-round exclusion follows from this round's offers |

`IsSpecialSupply` is the latch the supply section names: when it is set, the
round's income is computed, the latch cleared, and the income not added. It
means "this supply was set directly, so do not stack this round's income on
top". It is excluded because nothing under 1v1 rules ever sets it, and it
matters anyway to an installer, which is where the supply section names it.

## Unresolved

**Whether a slot joining the panel reads its initial cooldown.** The
transition adds a slot at 0 rather than reading the table, and the table
gives Nuke, Lightning Storm and Ion Bombardment an `initial_cooldown` of 1.
Whether the game reads it, so that those skills join the panel already
cooling down when a specialist or a card hands them out, is not recorded.
