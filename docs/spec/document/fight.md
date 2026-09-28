# Fight definition

## Scope

A fight document is one fight and its result, written onto the
[layout](layout.md) the fight starts from: the layout, the seed it was fought
with, and what the fight left, each part of the result on the object it
belongs to. A simulator that fights the layout with the seed has to arrive at
the result.

```yaml
kind: fight
seed: 4242
round: 3
source: recording
ticks: 870
hash: {profile: mcfr-content-0.7.0, result: 380d721bf2aa581622f521e4386160a0b5eedfb16ffed7b477b7e288c31534ef}
blue:
  officers: [extended_range_marksman]
  units:
  - {name: marksman, index: 0, position: {x: 0, y: -50}, exp: 12/170/650}
  contraptions:
  - {name: interceptor, index: 0, position: {x: 5, y: -95}, retained: false}
  battle_skills:
  - {name: shield_airdrop, standing: {position: {x: -150, y: -150}}, retained: false}
red:
  core_damage: 37
  units:
  - {name: arclight, index: 0, position: {x: 0, y: -100}, exp: 0/40/750}
  battle_skills:
  - {name: sticky_oil_bomb, positions: [{x: -30, y: 150}, {x: 60, y: 150}], grid_rows: {2: [], 3: [], 4: []}}
```

Here the fight ended with blue's marksman at 170 of its 650, its interceptor
destroyed and the shield an earlier round airdropped gone; red's arclight
gained 40, red's reactor core lost 37, and the oil red released this round is
left on three of its seven points, each whole.

A fight is one edge of a [match](match.md): the layout is what a round's
deployment projects to, and the result is what the fight hands the next round,
the fields [match.md](match.md#transition-coverage) puts in the `fight` class.
This document defines how the result is written and how it relates to the
layout it is written onto. The layout's own fields are
[layout.md](layout.md)'s and are not restated here; the trajectory hash is
[mcfr.md](../mcfr/mcfr.md#the-hash)'s.

A fight document states the outcome of a fight, not how it got there. What
happened tick by tick is in a recording, and a fight document reaches it only
through the hash.

A fight document is valid whatever its source, and a `simulator` one is never
a fixture: it states what the simulator computed, which is what a fixture
checks the simulator against. Which fight documents a regression may pin is
[tests/README.md](../../../tests/README.md)'s to say.

## Relation to a layout

A fight document is a layout with the result written in:

```text
layout = project(fight)
```

The projection drops `source`, `ticks` and `hash`, each side's `core_damage`,
and every object's result fields, and keeps a unit's `exp` at its first term:
`before/after/maximum` becomes `before/maximum`, and nothing when `before` is
`0`. Every other field is copied unchanged, `seed` included. The projection of
a fight in normal form is a layout in normal form.

A fight document whose projection is not a valid layout is refused, whatever
its result. A document of `kind: layout` that carries a result field is
refused as a layout with a field it does not have; `kind` is what says a
document is a fight.

`seed` is required here, where a layout leaves it optional: a result is the
result of one seed.

A unit, a shield or an area the fight creates that is not in the layout has no
entry, and so no result. A fight only reports on what it started with.

## Root fields

After the layout's own root fields, `kind` apart, a fight states:

| Field | Meaning |
| --- | --- |
| `kind` | exactly `fight` |
| `seed` | the match seed the fight was fought with; required |
| `source` | where the result was read: `recording`, `replay` or `simulator` |
| `ticks` | the fight's logical ticks, the recording's `tick_count` |
| `hash` | `{profile, result}`: the recording's `hash_profile` and `result_hash` |

`source` says what the result is and so what it may be checked against:

- `recording`: read from an MCFR the game recorded from this layout and seed.
- `simulator`: what the simulator computed from this layout and seed. It has a
  trajectory as a recording does, and so a hash.
- `replay`: read from a native replay's next round, which states what the
  fight left behind but not how it went.

`ticks` and `hash` are present exactly when `source` is `recording` or
`simulator`, and absent for `replay`. `ticks` is at least `1`; `hash.result`
is 64 lowercase hex digits, and `hash.profile` names the definition that
computed it, as [mcfr.md](../mcfr/mcfr.md#the-hash) names profiles. A reader
checks the profile's form; a hash under a profile the checker does not compute
is not comparable, which is the checker's to report.

## Side fields

| Field | Meaning |
| --- | --- |
| `core_damage` | what the fight took off the side's reactor core; a non-negative integer, absent when `0` |

## Object fields

| Object | Field | Meaning |
| --- | --- | --- |
| `units` | `exp: before/after/maximum` | the layout's `current`, the experience the unit ends the fight with, and the level's full bar |
| `contraptions` | `retained` | whether the contraption stands when the fight ends |
| `battle_skills`, a standing `shield_airdrop` | `retained` | whether the shield stands when the fight ends |
| `battle_skills`, a `shield_airdrop` release | `retained` | whether the shield it airdropped stands when the fight ends |
| `battle_skills`, a `sticky_oil_bomb` release | `retained`, `grid_rows` | whether any of its area is left when the fight ends, and which of it |

### Experience

`exp` extends the layout's `current/maximum` gauge with the fight's end between
its two terms. `before` and `maximum` are the layout's, with the layout's rule
that `maximum` is the bar the [unit experience
index](../../rules/unit_experience.md) gives the unit's type and level; a
fight states that bar even where `before` is `0` and the projection keeps no
gauge.

`before <= after <= maximum`. A fight only adds experience, and a full bar
takes no further share of what a fight hands out
([unit_experience.md](../../rules/unit_experience.md#what-a-full-bar-means)),
so a bar filled during the fight ends it at `maximum`. `exp` is absent only
when `before` and `after` are both `0`.

### What is left standing

`retained` defaults to `true` and is written only as `false`. Whatever
disappears in a fight has a fight event behind it, a destruction or an
interception, so `false` is the value that says something happened.

A contraption of any kind carries `retained`.

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
  [`grid_rows`](layout.md#standing-entries) does, and an absent `grid_rows`
  means all seven survive whole. `retained: false` means none survives, and a
  release carries it or a non-empty `grid_rows`, never both. What a retained
  release states is what the next round's standing entry for the area states.
- Every other release carries nothing: its product is gone before the round
  that would carry it opens.

`retained` anywhere else, or `grid_rows` on any entry but a Sticky Oil Bomb
release, is refused. A standing area's own `grid_rows` stays inside its
`standing` as the layout writes it.

## Normal form

A fight document is in normal form when its projection is a layout in normal
form, each result field sits on the object it belongs to wherever the layout's
normal form puts that object, and:

- the root fields come in the layout's order, `kind`, `game_build`, `map_id`,
  `seed`, `round`, followed by `source`, `ticks`, `hash`, then `blue` and
  `red`;
- a side's `core_damage` comes before the side's layout fields, and is absent
  when `0`;
- a unit's `exp` stands where the layout's does, and is absent when both
  `before` and `after` are `0`;
- `retained` and `grid_rows` follow an entry's layout fields, `grid_rows`
  first; `retained` is absent when `true`, and a release's `grid_rows` that
  lists all seven points whole is absent, as a standing area's is.

The canonical writer spells a fight by the layout's three rules, so every
entry, its result included, is one line.

## Excluded fields

| Not written | Why |
| --- | --- |
| `winner` | a fight can end with neither side winning; `core_damage` says what each side lost |
| a unit's life at the end | a unit comes back whole next round, so its life hands nothing on; the hash covers it |
| damage and kill counters | the hash covers them, and no round reads them |
| the recording's per-tick state | a fight document states an outcome; the recording and its hash hold the path |
| instrument channels | they describe how a fight did what it did, and are outside the hash |
| the recording's build | the layout's `game_build` states the build the fight is read against, and a reader refuses another |
| a result for what the fight creates | it is not in the layout, so it has no entry to carry one |

## Unresolved

- Which command checks a fight document, and what it reports: whether a
  checker fights the projection and compares field by field, how it reports a
  difference, and what it says of a `replay` source it has no trajectory to
  compare.
- Whether a fight document states the reactor core a side started with. A
  side destroyed by the fight is recognizable only with that number, which the
  layout does not carry, so the document cannot say on its own that the fight
  ended the match.
