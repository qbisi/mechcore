# Layout generation

## Scope

`mechcore generate` writes layouts for the game and the simulator to fight
alike, so that what the simulator does is checked across the space of fights
and not only across the fights a match or a fixture happened to reach. This
document defines that space, how a batch is drawn from it so that its
combinations spread, and what makes a batch reproducible.

A generated layout is a [layout](layout.md) like any other, and is legal by
the layout's own rules alone: it compiles, and the game fights it as a
[layout replay](layout-replay.md). Whether a match could reach it is not
asked. A position the Training Ground can stage is a position the fight has
to get right, so supply, prices and what the shop deals are outside this
contract. The one rule a generated side keeps beyond the layout's is the one
the layout already holds: a side holds at most one opening specialist.

This document does not say how a generated layout is fought, recorded or
compared. [`verify`](../mechcore/cli.md#verify) and
[`convert`](../mechcore/cli.md#convert) do that for a generated layout as for
any other, and a fight that parts from the game's lands as a fixture as
[tests/README.md](../../../tests/README.md) says.

## The space

A layout is drawn as an assignment of a value to each of a fixed list of
factors. A factor is one decision a fight can tell apart; its values are
finite, and each is legal on its own. Every factor below a side is a factor
of each side, so blue's and red's are different factors.

| Factor | Values |
| --- | --- |
| `round` | `1`, `2`, `3`, `4` |

`round` takes the rounds the rules turn on: the first, where no legacy unit is
old; the second, where a flank unit is travelling; and the rounds an opening
specialist delivers its squad in.

Each side has a lead unit, whose every property is a factor, and a second
unit, whose type and level are:

| Factor | Values |
| --- | --- |
| `lead.type` | every unit type the catalog names |
| `lead.level` | `1` through `9` |
| `lead.source` | `legacy`, `joined` |
| `lead.exp` | `empty`, `half`, `full`: none, half the level's bar, all of it |
| `lead.equipment` | none, or one item of the equipment catalog |
| `lead.techs` | none, one of its type's technologies, all of them |
| `lead.modification` | none, or the first through fourth officer that modifies its type, by ID |
| `lead.rotated` | `false`, `true` |
| `lead.depth` | `front`, `middle`, `back`, `flank` |
| `second.type` | none, or every unit type the catalog names |
| `second.level` | `1` through `9` |

`depth` is where the lead unit stands in its side's local frame: `front` in
`y=[-60,-10]`, `middle` in `y=[-180,-60]`, `back` in `y=[-310,-180]` of the
main half, and `flank` on one of the two flanks. A band is where the centre stands; a
footprint taller than its band stands as near it as the main half allows.
Every other unit and contraption stands anywhere in the main half.

A construction is never placed. The constructions a side may hold are the
ones the [opening](../../rules/opening.md#defensive-construction-layout)
lays for the layout's own seed on the map it is fought on, where they stand
and with their indices: all of them, or a part, each kept on an even draw,
as an earlier round's fighting leaves them. A construction no seed lays is
in no layout.

The side's own factors:

| Factor | Values |
| --- | --- |
| `officer.first`, `officer.second` | none, or a generic officer that changes a fight |
| `opening` | none, or an opening specialist that changes a fight |
| `blueprint` | none, or one of the four enhancement chains' levels |
| `energy_tower_skills` | none, `enhanced_range`, `high_mobility`, both |
| `tower_strengthen_levels` | none, both towers at a random level, both at `4` |
| `constructions` | none, the ones the layout's seed lays, or a part of them |
| `contraption` | none, `shield`, `interceptor`, `missile` |
| `battle_skill` | none, or a release of one battle skill the layout supports |

An officer changes a fight when [`config/officer_effects.yaml`](../../../config/officer_effects.yaml)
gives it an effect, or when it delivers a squad as a round opens. Each is in
one factor: one that modifies the units it lists one by one, `mech_type: 10`,
is `lead.modification`'s for the unit it lists; one of the opening pool is
`opening`'s; the four an enhancement chain hands out are `blueprint`'s; and
every other is generic, `officer.first`'s and `officer.second`'s. An officer that changes only supply, prices or what the shop
deals is in no factor.

An opening specialist that delivers a squad delivers it when `round` is one
of its rounds: the side then holds that squad as a `delivered` unit, of its
type and level, without experience. In any other round the specialist holds
nothing a fight sees beyond its effect.

### Values that need others

Some values say nothing, or are refused, beside some value of another factor.
The generator never assigns such a pair:

- `lead.exp` other than `empty` with `lead.source: joined`: a unit that joins
  during the round holds no experience;
- `lead.depth: flank` in round `1`, which has no flank;
- `lead.depth: flank` for a type whose footprint is wider than a flank both
  ways;
- `lead.depth: flank` with `lead.source: legacy` in round `2`, whose flank
  units are all travelling;
- `lead.modification` beyond the number of officers that modify `lead.type`;
- `lead.techs` other than none for a type that has no technologies;
- `second.level` with `second.type` none;
- the same officer in `officer.first` and `officer.second`, unless its card
  may be taken again.

A value that is legal with every other value on its own may still meet a
combination the layout refuses, such as a footprint no free cell takes. Those
are found by realizing the layout, below, and not listed here.

## Covering pairs

A batch is drawn so that every pair of values of two different factors that
the rules above admit appears together in at least one of its layouts. That is
a pairwise covering array: the number of layouts it takes grows with the
product of the two largest factors, not with the size of the space, and every
two decisions a fight can tell apart are seen together at least once.

The layouts are drawn one after another. For each, the generator builds a
fixed number of candidates and keeps the one that covers the most pairs no
earlier layout covered:

1. It starts a candidate from one uncovered pair, assigning both values.
2. It assigns the remaining factors one at a time in a shuffled order, each
   the value that covers the most uncovered pairs with the values already
   assigned, among the values the rules above admit beside them.
3. Ties are broken by the batch's random stream.

Once every pair is covered, or no candidate covers one, each further layout
is drawn by assigning every factor a value at random among those the rules
admit.

A candidate is kept only once it is realized. Realizing it places what it
holds: each placement takes a position at random in its region, on the
deployment grid its footprint needs, and a battle skill's release takes its
positions at random on the battlefield. The layout is then compiled and
written as a replay. A refusal draws the positions again, a bounded number
of times; a candidate still refused is dropped, and the pairs it held stay
uncovered. A side's units take their indices legacy first, then a delivered
squad, then the units that join, with no gap.

## Determinism invariants

A batch is a function of its seed and the binary that drew it:

- every random choice is drawn from one stream the seed starts, in the order
  this document states them;
- layout `i` of a batch is the same whatever count the batch was drawn with,
  since the layouts before it are the only ones that decide which pairs are
  still uncovered;
- each layout's own `seed` is drawn from the stream, and is positive, the
  seeds the opening that lays its constructions is dealt for.

The space is read from the build's tables, so the same seed draws another
batch on another build. A batch is named by its seed and the index of each
layout in it, which is enough to draw any one of its layouts again.

## Fidelity boundary

A generated layout is legal for the game, not reachable by play: it may hold
a level-9 unit in round 1, or two officers no deal hands one side together.
What it holds is what the Training Ground can stage, and so what a fight has
to get right.

The space leaves out what a factor would only reach through combinations that
pairs do not cover well: standing battle skills, more than two units of
interest a side, units on a flank by any path but the lead unit's, and the
officers of a match that change only its economy. A fight they bear on is
reached through a match or a fixture instead.

## Unresolved

- Whether a batch covers triples on request, which every two-factor
  interaction misses when a mechanism needs three decisions together to show.
