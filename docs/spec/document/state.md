# State definition

## Scope

A state describes one complete match position: everything both players hold at
one moment of one round. It is the largest of the four documents this format
defines, and the other three relate to it by projection and by composition.

```yaml
kind: state
map_id: 1001
seed: 2038621361
round: 7

reinforce_offers: [phoenix_2x_lv2, sabertooth_2x_lv2, steel_ball_2x_lv2, sledgehammer_2x_lv2]

sides:
  blue: { ... }
  red: { ... }
```

`map_id` and `seed` carry the same meaning and the same optionality as in a
layout. They belong to the document rather than to a side because both sides
share them.

A state is defined after each action, not only at a round's ends. A
[battle](battle.md) writes the position each round opens with as a state
segment, followed by the [decisions](action.md) taken from it. A state segment
carries `kind: state` and `round`, and leaves `map_id` and `seed` to the
battle's header, which states them once for every round.

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
shop, the reinforcement offer, the allocators, and the parts of the skill panel
that were not released this round.

Eight side fields project unchanged: `officers`, `techs`, `units`,
`constructions`, `contraptions`, `airdrop_shields`, `terrains` and
`tower_strengthen_levels`. Two more reach a layout filtered rather than copied
straight:

| State source | Layout target | Projection |
| --- | --- | --- |
| `blueprints` | `blueprints` | the enhancement chains alone: `4`, `401`, `5`, `501` |
| `energy_tower_skills` | `energy_tower_skills` | copied, keeping `5` and `6` |

A valid blueprint list cannot hold both levels of one chain, so each chain
contributes at most one Officer. Blueprint IDs `1`, `2` and `3`, and Energy
Tower skill IDs `1`, `3` and `4`, reach no layout field: their fight-visible
consequences are already carried by the skill panel or by units, while
their economic consequences are not part of a layout.

`battle_skills` is projected rather than copied. The projection keeps only
entries with `release`, sorts them by `release.order`, resolves each native `id`
to the layout's semantic `type`, and maps an area target to `positions`. A unit
or construction target has no representation in the layout contract, so
projecting one is refused rather than dropped.

A round's opening position carries no release at all, since a state is defined
after each action and a round opens before its first. The releases of a round
belong to the position its deployment closes with, which is what a captured
layout holds.

## Match level and side level

The reinforcement offer is dealt once per round and both players choose from the
same array, so it is the one field above `sides`.

Round 1 is dealt no offer, and neither is the opening the game numbers round 0.
The field is absent there rather than an empty array, since no array was dealt.

### The opening offer is private, and it is not a state field

The opening is the exception, and it is not here. It deals each side four
combinations of an advance team and a specialist officer, privately: neither
player sees the other's, so it could never be `reinforce_offers`, which is above
`sides` precisely because both players choose from one array.

It is not under a side's state either, because the position it is dealt in is
not a round worth stating. A battle's [header](battle.md#the-opening-offers)
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
follows: they decide what a later round is offered, and this round's offer is
already stated by `reinforce_offers`. Neither is read by a fight, so neither
changes anything a layout can express. The two derived streams are computed from
the round, the team index and the match seed, all of which a document already
carries, so nothing has to store them either.

A state is not a machine that generates its own successors. It takes the offer
as an input, which is what lets it drop the streams that would produce one.

### What an installer must not do

Because no state carries either recorded stream, an installer leaves both
generators as the match's own initialisation left them. That is a legal
position, merely not the recorded one, and it is safe exactly while every offer
is supplied rather than rolled.

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

      shop:
        unlocked_units: [marksman, fang, crawler, arclight, wraith, sabertooth, typhoon, phantom_ray, hound, void_eye, vortex]
        buys_remaining: 3
        unlocks_remaining: 1

      blueprints: [sticky_oil_bomb, field_recovery, attack_enhancement_ii]
      energy_tower_skills: [rapid_resupply]
      tower_strengthen_levels: [1, 1]

      equipment:
        - {name: improved_firepower_control_system}

      battle_skills:
        - index: 0
          name: intensive_training
          cooldown: 1
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
      airdrop_shields: [ ... ]
      terrains: [ ... ]
```

The last six keys are the layout fields that project unchanged, elided here
because [the layout document](layout.md) already defines them. A side always
writes all six, empty where it holds nothing.

### A unit carries two fields a layout does not

```yaml
      units:
      - {name: crawler, index: 5, position: {x: -140, y: -135}, exp: 450/450, value: 100}
      - {name: hound, index: 9, position: {x: 0, y: -160}, value: 100, movable: true}
```

`value` is what the side paid for the unit, at the prices its officers made
at the time, and it is what recovering the unit pays back. Two units of
one type and level can differ in it.

`movable` says whether the side may move the unit this round, and is
absent when it may not. A unit moves in the round it arrives, whether it
was bought, handed out by a card, or delivered as the round opened, and stays
where it is in every later round unless something frees it: a Deployment Module
it wears, its unit's Jump Drive, or a Redeploy release this round.
[The mobility index](../../rules/mobility.md) gives the rule and its evidence.
It is a field rather than a derivation because the round a unit arrived in
is history no other field keeps. A layout does not carry it: the fight moves
every unit.

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

### Equipment is stock plus what is fitted

A state stores the difference rather than the record. `equipment` lists only
what the side owns and no unit wears, and a unit's own `equipment`
names what it carries. The two together enumerate everything owned, and neither
can be derived from the other: dropping the side list would lose an unfitted
item, and dropping the unit's field would lose which unit carries what.

Storing the recorded inventory whole would instead let one document contradict
itself, by listing an item no unit carries beside a unit carrying an
item the list omits.

`equipment` is a multiset: a side can own two copies of one item.

That difference is what an installer has to undo. The game restores equipment in
two steps, creating the inventory and then attaching items by replaying the fit,
so an installer must add the fitted items back to the inventory before it
attaches them.

`durability` is optional and its absence means `-1`. Equipment in a standard 1v1
does not wear out and survives every round. The field exists because game rule
`999903` hands out equipment that expires after one round; under that rule a
fitted item's durability would have no home in this format, and the rule that
introduces it is the one that should answer for it.

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
before the round's income arrived. A converter rebuilds it; [the battle
document](battle.md) says how.

Installing a state is where the definition bites. Because the field already
includes the round's income, writing it directly leaves the game free to add
that income a second time. The game has a one-shot latch that suppresses exactly
that, and an installer has to set it even though no state document carries it.

### The shop

A state stores two shop numbers, and both differ from what the replay holds.

`unlocked_units` names each unit type by the name a unit's `name` gives,
and lists them in ascending unit ID.

`locked_units` is dropped because it is the complement of `unlocked_units`
against the build's unit catalogue. `MaxUnlockCount` is dropped because it is
the shipped constant plus a modifier no standard 1v1 source provides.

The two counters that remain are stored as what is left, not as what was used,
because that is the number a legality check reads. The purchase allowance gates
a purchase directly: a buy is refused when the counter has fallen to zero.

A round opens with two purchases, one more for every Additional Deployment Slot
(`10004`) the side holds, and one unlock. Neither can be copied from a replay,
for the same reason `supply` cannot: the recorded counters describe the previous
round, because the snapshot is taken before the round's own reset.

### Officers and technologies

`officers` lists the officers the side holds. An officer card that may be
taken again appears once per copy. `techs` holds unit technologies alone, grouped under the unit type they
belong to, as the replay groups them and as a battle's `tech_loadout` does. A
unit that has researched nothing has no row.

The state of being unlocked but not active does not arise under standard 1v1
rules, so a technology is either listed or not.

### Names

A state names what it holds rather than numbering it, and a document that names
something writes the key `name`. A unit type is the name a unit's `name`
gives, and a contraption the name a layout gives it. An officer, a technology, a
blueprint, an energy tower skill, a commander skill and an equipment item are
the game's own English names in snake case, apostrophes dropped, which
[`config/names.yaml`](../../../config/names.yaml) holds for build 2259: the
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

Every energy tower skill is a 本回合 effect, so this field is the set activated
in the round the state describes. Like everything else here it is defined after
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

A layout lists released skills only, and its list order is the release order. A
state lists the whole panel, because an action references a skill by its panel
slot and a slot the state does not carry cannot be resolved.

```yaml
      battle_skills:
        - index: 0
          name: intensive_training
          cooldown: 1
        - index: 2
          name: lightning_storm
          cooldown: 0
          release:
            order: 1
            target: {area: [{x: 50, y: 44}, {x: 189, y: 38}]}
        - index: 3
          name: field_recovery
          cooldown: 0
          release:
            order: 2
            target: {unit: 12}
```

`index` is the position at which the skill joined the panel, and it is the key a
release names. It is lifelong identity, not a recyclable slot: the panel never
shrinks and never reorders, and each round's panel is a prefix extension of the
previous one. The panel is sorted by `index`, which duplicate skill IDs make the
only well-order available, since one panel can hold the same ID twice.

`cooldown` covers a skill that cannot be released this round. A layout cannot
express one, because a layout only names releases. It is counted as a round
opens: a slot the round before spent, by a release or a deployment skill,
restarts at its skill's cooldown, and every other slot drops by one to zero. A
slot joins the panel at its skill's initial cooldown. [The commander skill
index](../../rules/commander_skills.md) gives both per skill.

`release` is present on a skill released this round, and `order` states where in
the release sequence it falls. Moving release order into an explicit field is
what lets the collection be sorted at all: in a layout the array position *is*
the order, which is why `battle_skills` is the one collection a layout leaves as
written.

`used: true` marks a deployment skill this round spent. Such a skill does its
work on the position before the fight, as [`action.md`](action.md#release_commander_skill)
states, so its slot records only that it was spent: no target, no order, and
nothing a layout sees. The game flags a spent deployment skill and a released
one alike; the skill's ID is what tells them apart. A slot is never both `used`
and released, and a round's opening position carries neither.

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
| `equipment` | ascending item ID, then `durability`, a multiset |
| `units`, `constructions`, `contraptions` | ascending `index` |
| `officers` | ascending ID, a multiset |
| `techs` | ascending unit ID, each unit's technologies ascending ID |
| `shop.unlocked_units` | ascending unit ID |
| `blueprints`, `energy_tower_skills` | ascending ID |
| `airdrop_shields` | ascending `(x, y)` |
| `terrains` | ascending `type`, then control points |
| `reinforce_offers` | as dealt; a choice names a position in it |
| `tower_strengthen_levels` | by building-manager position |

The rule behind the first three rows is that a collection is ordered by the key
the actions use to reference it, so document order and action resolution can
never disagree. `equipment` sorts on the `durability` an absent field implies,
`-1`.

One cross-check goes with that order rather than in it. Each entry of
`next_index` must be greater than the highest index in the collection it
governs, so an allocator cannot drift away from the objects it has handed out.
The check is what a sentinel entry in the list itself would buy, without making
a typed collection carry an element of a second shape that a layout would then
also have to admit.

## Rebuilding a state offline

Most of a round's state can be read out of a replay without running the game.
Five fields cannot be copied. Four are stale, because the snapshot precedes the
round's own reset: `supply`, the two shop counters and each slot's `cooldown`
state what stood before the round's income, allowances and count-down arrived.
The fifth, `energy_tower_skills`, is not stale but a different quantity, and it
is rebuilt from the round's actions.

The unit roster comes from the per-round player data rather than from the
match-level fight report, because the first two rounds precede the first fight
and so have no report to read.

The snapshot also precedes what the round's officers deliver as it opens, so
those deliveries are made on top of it: the squads, skills, equipment and
unlocks each officer's schedule names for the round. A delivered squad has no
recorded landing, and lands where [the board puts it](../../rules/landing.md).
`movable` has no recorded source either, and is rebuilt as
[`battle.md`](battle.md#what-conversion-rebuilds) states.

`airdrop_shields` and `terrains` are the two fields that are copied from
somewhere other than an object list. A skill that leaves an object standing
keeps it in that skill's `rangeItems`, so both are read out of the recorded
skill panel; [the battle document](battle.md) states the rule.

## Excluded fields

| Field | Why it is not in the state |
| --- | --- |
| `playerData.preRoundFightResult` | Neither a decision nor a simulation input |
| `playerData.IsSpecialSupply` | A one-shot latch, always clear under 1v1 rules, see below |
| `playerData.researchQueue` | Unreachable without game rule `999917`, above |
| `matchDatas.deadCount` | Carries nothing a position needs |
| `NewUnitData.Durability` | Unused for units in this build |
| `NewUnitData.RoundCount`, `SellSupply` | Both follow from when the unit was bought and what it cost |
| `ConstructionSnapshotData.durability` | One entry per segment, and inert |
| `matchDatas.teamRanks` | Seat ordering; no mechanism reads it |
| `matchDatas.poolOPs` | Reinforcement pool bookkeeping for later rounds, see below |
| `matchDatas.RoundExcludeReinforce` | The same, per round rather than permanently |
| `matchDatas.randomStateData` | Decides a later offer roll only |
| `playerData.randomStateData` | Nothing draws from that stream |

`IsSpecialSupply` is the latch the supply section names: when it is set, the
round's income is computed, the latch cleared, and the income not added. It
means "this supply was set directly, so do not stack this round's income on
top". It is excluded because nothing under 1v1 rules ever sets it, and it
matters anyway to an installer, which is where the supply section names it.

`poolOPs` and `RoundExcludeReinforce` are the reinforcement pool's bookkeeping,
and both are excluded on one argument: they decide what a *later* round may be
offered, while this round's offer is already stated by `reinforce_offers`.

They differ in kind, and neither can stand in for the other. `poolOPs` is a log
of removals and promotions that changes pool membership permanently, undone only
by a matching add. `RoundExcludeReinforce` does not touch membership at all: an
excluded item stays in the pool and is merely invisible for one named round.

## Unresolved

**Whether a slot joining the panel reads its initial cooldown.** Every
commander skill of build 2259 has an `initial_cooldown` of 0, and the
transition adds a slot at 0 rather than reading the table. The two agree on
this build, so the question is what a later build does: whether Lightning
Storm and Nuclear Strike, which a specialist or a card can hand out, join the
panel ready to release or already cooling down. Reading the table only
answers it if the table is where a later build puts that difference, so this
waits on how a later build handles those two skills.

**Where a fitted item's durability would live.** `durability` belongs to the
side's inventory, and a unit's `equipment` names only an ID. Under the game
rule that makes equipment expire, a fitted item has a durability and this format
has nowhere to put it.

**Whether an installer may carry a random stream.** No state field holds one,
which rests on nothing drawing from the player stream and on the match stream
only deciding later offers. A use that rolls rather than supplies an offer would
break the second half of that, and the format would need a field for the four
words rather than a derivation.

