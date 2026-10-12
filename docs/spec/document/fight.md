# Fight definition

## Scope

A fight document is one fight and its result: the [layout](layout.md) the
fight starts from, the seed it was fought with, and what the fight left, kept
apart. The layout is written as a layout writes it, and the result in a nested
`outcome` that names units and contraptions by their stable indices. A
simulator that fights the layout with the seed has to arrive at the result.

```yaml
kind: fight
source: game
seed: 4242
round: 3
blue:
  officers: [extended_range_marksman]
  units:
  - {name: marksman, index: 0, position: {x: 0, y: -50}, exp: 12/650}
  contraptions:
  - {name: interceptor, index: 0, position: {x: 5, y: -95}}
  battle_skills:
  - {name: shield_airdrop, standing: {position: {x: -150, y: -150}}}
red:
  units:
  - {name: arclight, index: 0, position: {x: 0, y: -100}}
  battle_skills:
  - {name: sticky_oil_bomb, positions: [{x: -30, y: 150}, {x: 60, y: 150}]}
outcome:
  blue:
    core_damage: 0
    units:
    - {index: 0, exp: 170/650}
    contraptions:
    - {index: 0, retained: false}
    battle_skills:
    - {at: 0, retained: false}
  red:
    core_damage: 37
    units:
    - {index: 0, exp: 40/750}
    battle_skills:
    - {at: 0, grid_rows: {2: [], 3: [], 4: []}}
ticks: 870
hash: 23:380d721bf2aa581622f521e4386160a0b5eedfb16ffed7b477b7e288c31534ef
```

Here the fight ended with blue's marksman at 170 of its 650, its interceptor
destroyed and the shield an earlier round airdropped gone; red's arclight
gained 40, red's reactor core lost 37, and the oil red released this round is
left on three of its seven points, each whole.

A fight is one edge of a [match](match.md): the layout is what a round's
deployment projects to, and the outcome is what the fight hands the next
round, the fields [match.md](match.md#transition-coverage) puts in the `fight`
class. This document defines how the outcome is written and how it refers to
the layout. The layout's own fields are [layout.md](layout.md)'s and are not
restated here; the trajectory hash is [mcfr.md](../mcfr/mcfr.md#the-hash)'s.

A fight document states the outcome of a fight, not how it got there. What
happened tick by tick is in a recording, and a fight document reaches it only
through the hash.

A fight document is valid whatever its source, and a `simulator` one is never
a fixture: it states what the simulator computed, which is what a fixture
checks the simulator against. Which fight documents a regression may pin is
[tests/README.md](../../../tests/README.md)'s to say.

## Relation to a layout

A fight document is a layout with its outcome beside it:

```text
layout = project(fight)
```

The projection drops `source`, `outcome`, `asserts`, `ticks` and `hash`, and names the
document `kind: layout`; every other field is copied unchanged, `seed`
included. No result is written inside the layout, so the projection takes
nothing out of `blue` or `red`, and the projection of a fight in normal form is
a layout in normal form.

A fight document whose projection is not a valid layout is refused, whatever
its outcome. `seed` is required here, where a layout leaves it optional: a
result is the result of one seed.

A unit, a shield or an area the fight creates that is not in the layout has no
path, and so no result. A fight only reports on what it started with.

## Root fields

| Field | Meaning |
| --- | --- |
| `kind` | exactly `fight` |
| `source` | who fought it: `game` or `simulator` |
| the layout's root fields | `game_build`, `map_id`, `seed`, `round`, `blue`, `red`, as [layout.md](layout.md) writes them; `seed` is required |
| `outcome` | required settlement for both sides, [below](#outcome) |
| `asserts` | what the document claims about how the fight went, [below](#asserts); absent when it claims nothing |
| `ticks` | the fight's logical ticks, the recording's `tick_count` |
| `hash` | `<profile>:<result>`: the recording's `hash_profile` and `result_hash`, as `23:e4ed…` |

`source` says who fought the fight, and so what the document is:

- `game`: the game fought it, through the Adapter, and recorded it. It is
  evidence of what the game does, and is what a fixture pins.
- `simulator`: the simulator fought it. It states what the simulator
  computed, and is never evidence.

The game fights a layout by writing it as a replay and fighting that replay's
round, and fights a native replay's round the same way; the two recordings of
one fight agree tick for tick, hash included. So a fight the game fought from
a replay and one it fought from a layout are both `game`, and a recording of
either that disagreed with the other would be the Adapter's error, not a
second kind of fight.

A recording says which it is: its `producer` is `game` or `simulator`
([mcfr.md](../mcfr/mcfr.md#file-metadata)), and a document read from it takes
the same name. The hash cannot say it, since both producers write the same
timeline for the same fight.

`ticks` and `hash` are the trajectory, and come together or not at all. A
document read from a recording carries both; one written without them states
the outcome alone, which a change to the rules that leaves every outcome where
it was does not move. `ticks` is at least `1`; `hash` is the profile's number,
a colon, and the result hash's 64 lowercase hex digits. The profile names the
definition that computed it, as [mcfr.md](../mcfr/mcfr.md#the-hash) numbers
profiles. A reader checks the form; a hash under a profile the checker does not
compute is not comparable, which is the checker's to report.

So a fight is checked by fighting its projection with its seed and comparing
what the fight arrives at with what the document states: the outcome, the
asserts when it makes any, and the trajectory when it carries one, each
reported on its own. The layout
needs no comparing, since both fights start from the one projection.
[`verify`](../mechcore/cli.md#verify) is the command that checks one.

## Outcome

`outcome` is a mapping with required `blue` and `red` entries. Each side
states its non-negative `core_damage`, including zero, and may list changed
`units`, `contraptions` and `battle_skills`. Missing settlement is invalid:
an absent core damage never means zero.

A unit or contraption result names the layout object's stable `index`, not
its position in a list. Indices can have gaps. A battle-skill result names
`at`, its position in the layout's normalized `battle_skills` list, counted
from zero; those entries have no stable native index. Each identity may occur
at most once in its outcome collection. Unknown identities, empty result
entries and fields the object cannot carry are refused.

| Object | Identity | Result | Meaning |
| --- | --- | --- | --- |
| side | `blue` or `red` | `core_damage` | what the fight took off the side's reactor core, as [reactor_damage.md](../../rules/reactor_damage.md) states; required, non-negative |
| unit | `index` | `exp` | final experience, as the layout's `current/maximum` gauge |
| contraption | `index` | `retained` | `false`: it does not stand when the fight ends |
| standing or released `shield_airdrop` | `at` | `retained` | `false`: its shield does not stand when the fight ends |
| released `sticky_oil_bomb` | `at` | `retained`, `grid_rows` | what is left of its area when the fight ends |

Core damage is always written. Other results state only what changed: no
`exp` equal to the unit's starting experience, no `retained: true`, and no
`grid_rows` that leaves all seven points whole. Empty result collections are
omitted. Both side entries and the outcome itself are always present.

### Experience

A unit's `exp` uses the layout's `current/maximum` spelling. Its maximum
must equal the [unit experience index](../../rules/unit_experience.md)'s bar
for its type and level. Its current is a whole number between the experience
the layout gives the unit, its starting gauge's current or zero, and that bar. A
fight only adds experience, and a full bar takes no further share of what a
fight hands out
([unit_experience.md](../../rules/unit_experience.md#what-a-full-bar-means)),
so a bar filled during the fight ends it at the bar. Its current is whole because the
fight's end cuts each formation's experience to one
([unit_experience.md](../../rules/unit_experience.md#what-a-kill-hands-out)).
An outcome gauge whose current equals the unit's start is refused, being unchanged.

### What is left standing

`retained` is written only as `false`. Whatever disappears in a fight has a
fight event behind it, a destruction or an interception, so `false` is the
value that says something happened.

In `battle_skills`, a result belongs to whatever of a skill can outlive the
fight's round, and nothing else carries one:

- A standing Shield Airdrop carries `retained`. A shield stands until it is
  destroyed, so one that is retained is a standing entry of the next round.
- A standing Sticky Oil Bomb area carries nothing. Oil lasts two rounds and a
  standing area is in its second, so it never carries on and its end is not a
  result.
- A Shield Airdrop released this round carries `retained`: `false` when the
  shield it airdropped is destroyed.
- A Sticky Oil Bomb released this round carries its area's result in the
  encoding a standing area uses. Its control points are its `positions`.
  `grid_rows` states which of the seven generated points survive and each
  clipped point's mask, exactly as a standing area's
  [`grid_rows`](layout.md#standing-entries) does. `retained: false` means none
  survives, and a release carries it or `grid_rows`, never both. What a
  retained release states is what the next round's standing entry for the area
  states.
- Every other release carries nothing: its product is gone before the round
  that would carry it opens.

## Asserts

The outcome says what a fight hands on and the hash that it went exactly as
recorded; neither says what a fixture was made to show. `asserts` does: each
is a query of the fight's recording and the rows the game's recording of the
fight answered it with.

```yaml
asserts:
- sql: |-
    SELECT tick, target__kind, target__id, amount
    FROM events
    WHERE type = 'damage' AND source__kind IS NULL AND tick = 63
    ORDER BY ordinal
  rows:
  - [63, building, 5, 3000]
  - [63, building, 6, 1112]
```

| Field | Meaning |
| --- | --- |
| `sql` | the query, as [`query`](../mechcore/cli.md#query) asks a recording |
| `rows` | what the game's recording answered, row by row, each cell as `query` writes it; absent until recorded |

An assert is checked by asking the fight being compared the same query and
comparing its rows. Whether it is bound to a tick, as the example is, or only
to the order things happened in, is the query's: what it selects, and what it
orders by. A claim that the two buffs' rates sum, whichever tick a hit lands
on, asks for no tick.

The rows are the game's. `verify --backend game --update` writes each
assert's rows from the game's recording, and nothing else writes them: an
assert written by hand states its `sql` and no `rows`, and does not verify
until the game has answered it. So an assert outlives the hash: a change to
the rules that moves the trajectory leaves every assert about what it did not
move standing, and says, by which fail, what moved.

## Normal form

A fight document is in normal form when its projection is a layout in normal
form, and:

- the root fields come in the order `kind`, `source`, then the layout's root
  fields in the layout's order, then `outcome`, `asserts`, `ticks`, `hash`;
- `outcome` lists blue before red; each side writes `core_damage` first,
  then `units`, `contraptions`, `battle_skills`;
- unit and contraption results sort by `index`, battle-skill results by `at`;
  each entry writes its identity first, and `grid_rows` before `retained`;
- core damage is explicit; unchanged object results and empty result lists
  are absent;
- `asserts` keep their order, each with `sql` before `rows`.

The layout and outcome use [layout.md](layout.md#normal-form)'s spelling:
sequence items are flow mappings on their own lines, scalar-only collections
are flow values, and other mappings are blocks. Assertion SQL uses a literal
block and each answer row is a flow sequence on its own line. Empty recorded
rows are `rows: []`; unrecorded rows are absent. Literal-block indicators
preserve SQL exactly: `|-` strips its final newline, `|` preserves one, and
`|+` preserves multiple. An explicit indentation indicator preserves leading
whitespace when needed. The writer never reformats or tokenizes SQL.
Strings containing control characters a literal block would normalize are
quoted with escapes, preserving their value.

## Excluded fields

| Not written | Why |
| --- | --- |
| `winner` | a fight can end with neither side winning; `core_damage` says what each side lost |
| a unit's life at the end | a unit comes back whole next round, so its life hands nothing on; the hash covers it |
| damage and kill counters | the hash covers them, and no round reads them |
| the recording's per-tick state | a fight document states an outcome; the recording and its hash hold the path |
| instrument channels | they describe how a fight did what it did, and are outside the hash |
| the recording's build | the layout's `game_build` states the build the fight is read against, and a reader refuses another |
| a result for what the fight creates | it is not in the layout, so it has no path to carry one |

## Unresolved

- Whether a fight document states the reactor core a side started with. A
  side destroyed by the fight is recognizable only with that number, which the
  layout does not carry, so the document cannot say on its own that the fight
  ended the match.
