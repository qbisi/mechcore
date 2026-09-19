# Battle definition

## Scope

A battle is one match, written as a stream of YAML documents separated by
`---`. The stream opens with a header, holds the opening's decisions next, and
then alternates between the position a round opens with and the decisions each
side takes from it.

```yaml
kind: battle
map_id: 1001
seed: 2038621361
sides:
  blue: {offers: [...], constructions: [...], tech_loadout: {...}}
  red: {offers: [...], constructions: [...], tech_loadout: {...}}
---
kind: action
round: 0
blue:
- {type: choose_advance_team, offer: 1, name: vortex-fire_badger, specialist: giant_specialist}
red:
- {type: choose_advance_team, offer: 0, name: crawler-tarantula, specialist: supply_specialist}
---
kind: state
round: 1
sides:
  blue: { ... }
  red: { ... }
---
kind: action
round: 1
blue: [ ... ]
red: [ ... ]
---
kind: state
round: 2
...
```

Each document in the stream is a segment, and each segment names its `kind`.
This document defines the header, the grammar of the stream, and what holds
across its rounds. A state segment carries the [state](state.md) shape and an
action segment carries two [action](action.md) sequences; those two documents
define the segments themselves. A [layout](layout.md) is the projection of a
state onto what the fight simulates, and never appears in a battle.

A segment is a unit a reader can take on its own. The header and one round's
state are the position a side decides that round from, so a reader that wants
one round reads the header, that round's state and that round's actions, and
nothing between them.

A battle states decisions and the positions they were taken from. It does not
state a fight: what a fight did is visible only as the difference between one
round's state and the next.

## The stream

The segments come in one order:

```text
battle, action(0), state(1), action(1), state(2), action(2), ...
```

`round` counts up by one from each state to the next, and an action segment
carries the round of the state before it. Round zero is the opening, which has
decisions and no state; every later round has both.

A stream may end after any segment. That is what lets a stream grow: a match in
progress, or a reader that learns the next position later, appends the next
segment rather than rewriting the file. A stream that ends on an action segment
says that round's fight is not stated, and says nothing about whether it has
been fought.

Two segments end a match, and nothing may follow either of them:

- an action segment in which a side concedes, since the round is then not
  fought;
- a state segment in which a side's `reactor_core` is zero or below, since the
  fight that produced it destroyed that side.

Neither is an extra marker. A match's end is read from the last segment's own
content, so a stream never has to be reopened to append past a marker that
turned out to be premature.

A reader refuses a stream whose segments are out of order, whose rounds skip or
repeat, or that continues past the end of a match.

### A state segment is the position a round opens with

A battle states a round from before its first decision. No skill on its panel
carries a `release` or is `used`, and `travelling` is empty, because all three
are written by the round's own decisions. A state segment that holds a release
or a used skill is refused.

Each state segment repeats the whole position rather than what changed. That is
accepted rather than encoded away: a segment has to stand alone so that one round
can be lifted out of a match, and a delta would make every round depend on all
of its predecessors.

## The header

The header holds what every round of the match shares.

| Field | Source |
| --- | --- |
| `seed` | `BattleInfo.SystemSeed` |
| `map_id` | `BattleInfo.MapID` |

Both keep the meaning and the optionality a layout gives them.

The rest of the match header is a property of the standard 1v1 rule set rather
than of a match: the phase durations, the round cap, the advance team,
reinforcement and construction flags, the game and score modes. A battle states
the mode once by being this format, rather than restating its consequences in
every segment. A mode that changes them is a different format.

`FightTime` is the only header constant a simulator reads, and it is 120
seconds.

### Game rules are the premise, so the battle carries them

`game_rules` is optional and its absence means none. It is the one header
constant a battle states rather than drops, because exclusions across the other
documents rest on it being empty: the research queue is unreachable without rule
`999917`, equipment never wears out and so needs no `durability` without rule
`999903`, and the blueprint pool is `[1, 2, 3, 4, 5]` only while no rule enables
blueprint research. Each of those is a claim about this field.

A battle that names a rule is outside what this format defines. A reader refuses
such a document rather than reading the states inside it under premises the rule
breaks.

### The sides are positional

`blue` is `playerRecords[0]` and `red` is `playerRecords[1]`.

Nothing else names a seat. The record's own `team` is the same value for both
players, and its `Seat` names the client that recorded the file rather than a
side. Player names and account IDs are not fields: nothing reads them, and a
side is identified by which side it is.

### What a side brings to the match

Three per-side facts are properties of the match rather than of a round, and
they are what the header's `sides` holds. Two of them are dealt before either
player decides anything, and the third bounds every round.

#### The opening offers

```yaml
  blue:
    offers:
    - {team: void_eye-sledgehammer, specialist: cost_control_specialist}
    - {team: vortex-fire_badger, specialist: giant_specialist}
    - {team: marksman-sledgehammer, specialist: quick_supply_specialist}
    - {team: crawler-steel_ball, specialist: aerial_specialist}
```

The opening deals a side four combinations of a team of units and a
specialist officer, and taking one takes both. `offers` is the four in the
order shown to the player.

The deal is private: neither player sees the other's. That is why it sits under
a side here and not beside a state's `reinforce_offers`, which both players
choose from.

The deal belongs in the header rather than in a state because the opening has no
position worth stating. Every side of every match enters it holding nothing:
nothing bought, nothing researched, both towers at level zero and both
allocators at zero. The header is that position, and the offers are the one
part of it that differs between matches.

#### The construction layout is dealt, not built

```yaml
  blue:
    constructions:
    - {name: defensive_wall, index: 0, position: {x: -140, y: -55}}
    - {name: rapid_fire_turret, index: 1, position: {x: 140, y: -100}}
```

The map rolls a construction layout and deals it to both sides before the first
round. It is the one piece of the opening no decision produces, and without this
field a battle's buildings would appear out of nothing in whichever round it
starts from.

A state's own `constructions` is a different list with the same shape. That one
is live: buildings can be recovered or destroyed, so it only ever shortens.
This one is what the side started with and never moves. The first round's list
equals it, and every later round's is a subset of the same identities.

Each side reads the layout in its own frame, so the two are the same buildings
at mirrored coordinates rather than the same positions.

#### The custom tech loadout

Each player chooses, before the match, which technologies each unit may
research. A technology outside a unit's loadout cannot be researched in that
match, so the loadout bounds every `upgrade_technology` a round can hold.

```yaml
  blue:
    tech_loadout:
      fortress: [anti_air_barrage, launcher_overload, solid_shot, elite_marksman]
      marksman: [doubleshot, electromagnetic_shot, aerial_specialization, range_enhancement]
      war_factory: [high_explosive_ammo, missile_interceptor, range_enhancement, phoenix_production, steel_ball_production, sledgehammer_production]
      vortex: [grid_integration, range_enhancement, mobile_power_station, emergency_armor]
```

It is real state and not a catalogue: two players in one match hold different
loadouts. A row is keyed by the unit type's name, the one a unit's `name`
gives, and lists its technologies by name, as a state's `techs` does; the rows
are in ascending unit ID and a row's technologies in ascending technology ID,
which is the order the game lists both in. There is one row for each unit a standard 1v1 match can field: IDs
`1` through `31` and `2002`. The record also holds rows for Death Knell (`2001`)
and Experimental Death Knell (`4001`), which no standard 1v1 match fields, and
the conversion drops them.

A technology's name is unique only within its unit, so it is always written
under one: here under its row, in a state under its unit type, and in an
`upgrade_technology` beside the `unit` it names. Its owner cannot be read off the
end of its ID for every row, and says nothing at all about Mountain, whose ID is
above `2000`; it is resolved against the build's catalogue.

## The opening is round zero

The opening is the first action segment, and it holds one
`choose_advance_team` per side and nothing else. `offer` is the zero-based
position of the combination taken in that side's header `offers`, and `id` and
`specialist` name the team and specialist that combination holds. A decision
whose `id` and `specialist` are not what its offer holds is refused.

Round zero has no state segment because the header is its position, and the
first state segment is round 1, which already shows what the opening delivered.
The seam from the header to round 1 is checked like every other seam, below.

## Checking the deal against the seed

Every offer a battle states is drawn from the match's seed, so a battle can be
checked for having been dealt what it says without the replay it came from.

```bash
mechcore verify <battle.yaml>
```

Before checking the deal, verification reads every state field and every
action operand using the document types. Missing required fields, invalid
field types, unknown state/action fields and unknown action types are refused,
including fields the deal does not use. Tagged skill targets retain their
area, unit or construction meaning. Reading a complete position and decision
does not establish that the decision is legal there or that it reproduces the
next round's position.

The opening check computes initialization from `seed` and `map_id`, and compares
both complete `offers` arrays and both construction lists against it. It
compares the directly computed result, without searching alternative stream
positions, and requires each opening decision to name an offer in range and
the team and specialist that offer holds. It does not prove that a player chose that offer, and it does not validate
deployment or combat.

The reinforcement check advances the same stream through contiguous rounds from
round 1 and compares every round's complete ordered `reinforce_offers`. Each draw
uses that round's stated units, shop unlocks, active technologies and
officers, and the previous rounds' `choose_reinforce_item` decisions update the
pool. Offers being checked do not choose the stream position or seed the next
draw.

Round 1 carries no offers and no reinforcement choice. Every later round whose
decisions are stated requires one choice per side; a round that ends the stream
may omit it. An index and ID must name the predicted offer, or use the defined
decline form. Missing or reordered offers, invalid choices, unsupported inputs
and discontinuous rounds are refusals, and the first failing round is reported.

A successful report includes `reinforcement_rounds`,
`reinforcement_offers_checked`, and `reinforcements`, whose entries contain the
ordered offers, the ordinary or unit branch, and the random states and offsets
before and after generation. These checks are conditional on the stated round
inputs; they do not authenticate player decisions or validate transitions across
combat.

`mechcore opening <seed> <map_id>` predicts the same two offer arrays and
construction lists without a battle or replay. Its JSON result includes the
selected officer variants and unit reinforcement round pool, the reinforcement
state before and after the opening deal, and raw draw counts excluding seed
warm-up and including range rejection. The map stream reports its chosen
construction group, reversal flags in blue/red order, and raw draw count.
Unsupported maps and negative seeds are refused. This operation predicts
available options; it does not select an option for either player or predict
subsequent reinforcement deals.

## Between two states there is a fight

Consecutive states are not adjacent. Applying a round's decisions to its state
yields the position at the end of the deployment, projecting that position
yields the layout the fight starts from, and the fight is what produces the next
round's roster and reactor core. A battle is therefore the only document that
states an end-to-end simulator obligation, and the only one whose checks can be
cross-round.

### What a round reproduces

A round's decisions, applied to the state it opens with, have to reproduce the
state the next round opens with in everything the fight does not decide. The
application is a function of that state, the round and its decisions, and it
is three steps:

1. The decisions are stepped in order, which gives the position the deployment
   ends with. [`action.md`](action.md) defines each step, and its projection is
   the layout the fight starts from.
2. The fight runs. What it decides is not applied; the one thing it does to
   every position is empty the travelling set, and that is applied.
3. The next round opens on the result: it resets what lasts one round, pays
   the income less what an energy tower skill still owes, and makes the
   deliveries its officers' schedules name, landing each squad where [the
   board puts it](../../rules/landing.md).

The result is a whole position, board included, not a selection of fields.
Checking a round is comparing it against the next state leaf by leaf, which
the next section defines. The rules the function applies beyond the decisions
themselves are the ones no action states, and
[`action.md`](action.md#rules-no-action-states) carries them.

Both allocators are predicted even though the fight destroys what they hand
out. An index is never reissued, so an allocator records how many objects a
side has ever had rather than how many it still holds, and no fight can move
it.

A fit that finds nothing in the stock is refused rather than applied. An empty
stock is reached both by a round that balanced and by a round that fitted an
item the side never held, and the two must not compare equal.

The opening is a transition like the others, from the header onto round 1, with
no fight between them. The chosen team is not a step's to deliver: stepping the
opening answers what the position is immediately afterwards, and the team has
not arrived. It reaches the board when round 1 opens, which is not a decision,
so the opening's third step delivers it.

### Transition coverage

`mechcore verify <battle.yaml>` measures each transition against the whole next
position, not only the nine fields above. A transition starts from a round's
state and that round's decisions, steps the decisions in order, and opens the
next round on the result. Every leaf of the recorded next state is then put in
one of four classes:

| Class | Meaning |
| --- | --- |
| `equal` | Predicted, and the recorded value agrees |
| `unequal` | Predicted, and the recorded value differs, or the side's decisions contradict the position they were taken from |
| `unimplemented` | No rule predicts the leaf yet, or the build's tables cannot settle one of the side's decisions |
| `fight` | The fight decides the leaf |

A leaf is a scalar reached through mappings, a field of a unit, panel
slot, construction or contraption aligned by its `index`, or a whole list
otherwise, so an ID set and an inventory with repeats are one leaf each. A leaf
only one of the two positions has is still a leaf: a unit the prediction
lacks counts against it. The recorded next state is compared against and never
read by the prediction.

The `fight` class is the fixed set of fields below, in every transition that
ends in a fight, which is every one after round zero's. Standard 1v1 has no income
during the fight, so `supply` is not among them and is predicted like any other
field.

| Field | What the fight does to it |
| --- | --- |
| `reactor_core` | Damage |
| `units.exp` | Experience from the fight |
| `contraptions` | Which survive |
| `terrains` | Which remain |
| `airdrop_shields` | Which remain |

A leaf outside those fields is `unimplemented` when no rule produces it, even
where the unchanged value happens to agree; which fields those are changes as
rules are added, and the report names them. `reinforce_offers` is dealt from
the stream the header seeds, and is `equal` only where the deal agrees and
every field it is dealt from agrees on both sides too, since the deal is
checked against the recorded position.

The opening transition, from round zero onto round 1, starts from a position
the header deals rather than a state segment: the map's reactor core for that
seat, the side's `constructions`, and two towers at level zero. A standard 1v1
map unlocks no unit and hands out no commander skill before the opening. The
side's opening decision is stepped on it, and as round 1 opens the chosen team
arrives: its unit types join the shop, and its units land at level 1
where the board puts them, in the team's order. Nothing is fought in between,
so no leaf of this transition is in the `fight` class.

When a side's decisions cannot be stepped, nothing of that side's transition is
predicted. A decision naming what the position does not hold, or one the game
refuses there, is reported once at the path `actions`, and the side's leaves
outside the fight count as `unequal`. A decision the tables cannot price, or a
grant the board has no landing rule for, makes them `unimplemented`.

The report's `coverage` holds the four counts in `total`, by field group in
`fields`, whose key is a leaf's path with its `[index]` parts removed, and by
round and side in `transitions`, where `side: match` holds `reinforce_offers`.
`unequal` lists each disagreeing leaf with its round, side, path, predicted and
recorded values, and `untargeted_round` names a last round whose decisions have
no state after them, which is not a transition. A battle verifies only when no
leaf is `unequal` or `unimplemented`.

### Cross-round invariants

A segment checks itself. A battle checks the seams between states, and these
hold across every seam of a well-formed battle.

| Invariant |
| --- |
| `next_index.unit` rises or holds |
| `next_index.contraption` rises or holds |
| researched technologies are kept |
| researched technologies stay inside that side's `tech_loadout` |
| the skill panel is extended, never reordered |
| `shop.unlocked_units` are kept |
| `tower_strengthen_levels` rise or hold |
| `blueprints` are kept, or replaced by their own next level |
| `officers` are kept, or replaced by their own next level |
| `constructions` are kept or dropped, never added, and round 1's are the ones the header dealt |
| `reactor_core` falls or holds, except across the opening |

The two replacement rows are one mechanism seen twice. Activating a chain
blueprint replaces its predecessor rather than joining it, and the product
Officer it grants follows.

The reactor core rises only across the opening, by the amount the team and the
specialist carry between them. After the opening it only falls.

### Supply

Supply is one of the fields a round's transition predicts, and it satisfies
one identity across two rounds:

```text
supply(round + 1) = supply(round) - spent(round) + income(round + 1)
```

Spending is what the round's decisions cost, charged as each is taken;
[`action.md`](action.md) says what each costs, and the tables in `config/`
carry the amounts. Income is the schedule every versus map shares, plus what
the officers held as the next round opens add and what the equipment worn on
its board pays, less what an energy tower skill activated this round still
owes.

Nothing else reaches the supply between two rounds. Standard 1v1 pays nothing
during a fight: the only officers in this build that pay a bounty for
destroying a giant are neither dealt by an opening nor granted by a card. So
`supply` is not in the `fight` class, and a battle whose supply does not add up
fails to verify at that leaf.

The opening is the first term of that identity rather than an exception to it.
A side starts holding nothing, pays for the opening it takes, and round 1's
income arrives after.

## Converting a replay

```bash
mechcore convert <replay.grbr> <battle.yaml> [--force]
```

Conversion is offline. It reads the replay and nothing else, and it simulates
no fight.

The converter refuses rather than guesses. A replay from another build, a
downloaded one, a match mode other than `VS_1_1`, a `Test` match, a match
carrying game rules, rounds that are not the contiguous sequence both sides
share, an object this build's catalogues cannot name, and an action this format
has no representation for are each an error naming what was found.

The opening is read out of round 0: the replay records the team half of each
side's decision, the specialist half is read back from round 1's officers, and
the four offers are rebuilt from round 0's random state. A replay whose round 0
stands for anything but one team choice per side is refused, and so is a missing
or malformed random state, or a choice that disagrees with the rebuilt deal. A
replay that holds the opening alone is refused too: a battle is deployment
rounds, and one with none is not a document this format has a use for.

### A converted battle ends on its last round's decisions

A fight's result is recorded only in the snapshot that opens the next round.
The fight that ends a match opens none, so a replay holds no position after its
last round's decisions, and a converted battle's last segment is that round's
action segment. Its fight is not stated, because the source does not state it
and conversion does not simulate one.

A concession is the exception that needs no fight. The side's decision list ends
with `concede`, and the match ends with that segment. At most one concession
exists, because the first one ends the match. A replay in which a side decides
after conceding, a round follows a concession, or both sides concede is refused.

### What conversion rebuilds

Most fields are copied. Seven are not, and each is argued in the document that
owns it:

| Field | Why it is rebuilt |
| --- | --- |
| `supply` | The snapshot precedes the round's income, which is added back from the map settings the record itself carries, plus what the equipment on the board the round opens with pays, less the energy tower debt |
| `shop.buys_remaining`, `unlocks_remaining` | The recorded counters state the previous round's remainder. A round opens with two purchases, one more per Additional Deployment Slot held, and one unlock |
| `battle_skills[].cooldown` | The recorded cooldowns are the previous round's. A slot the previous round spent restarts at its skill's cooldown, and every other drops by one to zero |
| `energy_tower_skills` | The recorded list is a debt rather than an activation, so a round's start carries none |
| `equipment` | The recorded inventory includes fitted items, which the units already name |
| `movable` | No recorded field states it. Every unit of round 1 arrived with the opening, and a delivery arrived as its round opened; any other unit moves only if a Deployment Module or a Jump Drive frees it |
| Deliveries | The snapshot precedes what the round's officers deliver as it opens, so the squads, commander skills, equipment and unlocks each officer's schedule names are added to it, and a delivered squad lands where [the board puts it](../../rules/landing.md) |

Only `equipment` is rebuilt by conversion's own rule. The income, the
allowances, the cooldowns, the energy tower skills, `movable` and the
deliveries are what a round's opening does, and conversion makes that opening
rather than restating it: it reads the snapshot as it stands before the round
opens, marks the slots the previous round's actions spent and the Rapid Supply
it still owes for, and opens the round on it with the same rule a transition's
prediction ends with. A converted opening and a predicted one therefore cannot
follow two sets of rules. The opening pays the income schedule every versus map
shares, so a record whose map pays a different one is refused.

The energy tower debt is the one quantity two readings produce, and the two have
to agree. A round's decisions say what it owes, and the recorded list says the
same thing a round later, because the activation flag survives into the round
after the one that set it. Only the skill with a deferred half can be
snapshotted, so a round whose recorded list names anything else, or names
nothing where the round before activated one, is refused rather than converted
under whichever reading happens to be consulted. An activation in the last round
is never compared: no snapshot follows it.

`airdrop_shields` and `terrains` are read out of the panel rather than out of
the object lists. A skill that leaves an object standing keeps it in that
skill's `rangeItems`, and the snapshot opens the round, so an entry there is an
object that outlived the round which made it. The two kinds differ in what they
do with the entry's `round`: an oil terrain counts its remaining lifetime down
and is dropped at zero, while a Shield Airdrop is not time-limited, always
records zero, and simply loses its entry once the shield is gone. A `rangeItems`
entry belonging to any other skill is an error, since this format has not been
measured against it.

A shield the requested round releases is a decision in that round's action
segment, not an `airdrop_shields` entry, and the two never name the same object:
the state segment is taken before the round's own decisions.

`travelling` has no recorded source and needs none. The fight empties the
travelling set before the round it opens, so no state segment carries a
travelling unit. The field is absent because that is its value here, not
because the conversion could not find it.

## Normal form

| Collection | Order |
| --- | --- |
| segments | header, round zero's actions, then each round's state before its actions, ascending `round` |
| `offers` | as dealt; an opening decision's `offer` names a zero-based position in it |
| `constructions` | ascending `index` |
| `tech_loadout` | ascending unit ID, each row ascending technology ID |

A team is written as its two unit types joined by a hyphen, the one it holds
three of first, such as `vortex-fire_badger`. A specialist is an officer, and it
and the other rows [`state.md`](state.md#names) names are written by the game's
English names in snake case.
| `game_rules` | ascending rule ID |

A segment's own collections keep the orders [`state.md`](state.md) and
[`action.md`](action.md) define. Within a segment, `kind` comes first and
`round` second, so a reader scanning the stream finds both on the two lines
after each separator.

Three rules decide how every value is spelled, and none of them names a field.
A [layout](layout.md#normal-form) is spelled by the same three:

- a sequence item is written on one line, in flow style;
- a mapping or sequence whose members are all scalars is written in flow style
  on its key's line;
- every other value is written in block style.

```yaml
    shop:
      unlocked_units: [marksman, crawler, fire_badger, tarantula]
      buys_remaining: 2
      unlocks_remaining: 1
    next_index: {unit: 7, contraption: 0}
    units:
    - {name: vortex, index: 0, position: {x: -120, y: -100}, exp: 193, value: 100}
```

Actions, units and ID lists are what a battle holds by the thousand, and one
line each keeps a round on a screen and makes a diff name the item that
changed. A coordinate pair and an allocator are scalar mappings, so they fold
by the same rule; a side, a shop and a technology list mix shapes and stay
blocks. The spelling is part of the normal form, so one battle has one byte
sequence; a reader parses either style.

## Excluded fields

| Field | Why it is not in the battle |
| --- | --- |
| A round-zero state | Every side enters the opening holding nothing; the header is that position |
| A fight's result | The next state shows it; the last fight has no next state, see above |
| `BattleInfo.BattleID` | Names a server record; no rule reads it |
| `BattleRecord.Seat` | Which client recorded the file, not a property of the match |
| `PlayerRecord.name`, `id`, `ad` | Account identity; nothing reads it |
| `PlayerRecord.data.styleData` | Skins |
| `PlayerRecord.data.team`, `isLeader`, `type` | A side is named by which side it is |
| `PlayerRecord.data` supply and core settings | The map's row in `matchSettings` gives them, keyed by `map_id` |
| `BattleRecord.reinforceItems` | Carries no offer; the per-round array does |
| `PlayerRecord.seed` | Nothing draws from that stream, see the state document |
| `PAD_GiveUp.Time`, `LocalTime` | A concession is placed by its segment; no rule reads a clock |
| `BattleRecord.Version`, `CreateTime` | Provenance, see below |
| The 1v1 header constants | Properties of the mode, above |

A document's IDs are all resolved against one build's catalogue, so a converter
records which replay and which build a battle came from beside the document
rather than inside it. A `build` field no rule reads would state a fact the
reader cannot act on except by refusing the document.

## Unresolved

**Whether a battle states the fight that ends it.** A converted battle ends on
its last round's decisions, and the stream admits one more state segment after
them. What that segment would hold is open. The fight writes the roster with
its experience, the constructions, the contraptions and the reactor cores, and
nothing else: no income arrives, no allowance resets, no offer is dealt. A state
holding only those fields would close the last round, but it is not the shape
[`state.md`](state.md) defines, and its source would be something other than the
replay, which has to be shown to reproduce the fights the replay does record.

**Whether an appended segment says where it came from.** A stream can be
extended by something other than conversion, and a segment a simulator produced
reads the same as one a replay recorded. Marking the difference means a field
on the segment, and a field is only worth carrying if some reader refuses or
weighs a segment by it.

**Whether provenance belongs in the document.** A build mismatch is undetectable
from a battle alone, because the build is recorded beside the file. Making it
detectable means a `build` field, and a field is only worth carrying if a
mismatch is a refusal. That is the same question for every document kind and
should be answered once.

**What a battle does with a match that carries game rules.** Refusing is a floor
rather than a design. A rule changes premises the other documents rest on, so
admitting one means each of those documents saying what it does under that
rule, not just this one recording the rule's ID.
