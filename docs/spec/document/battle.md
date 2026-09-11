# Battle definition

## Scope

A battle is one match. It holds what every round of that match shares, and the
rounds themselves in order.

```yaml
kind: battle
map_id: 1001
seed: 2038621361

sides:
  blue:
    tech_loadout: { ... }
  red:
    tech_loadout: { ... }

turns:
  - round: 0
    state: { ... }
    actions: { ... }
  - round: 1
    state: { ... }
    actions: { ... }
```

An entry of `turns` carries the [turn](turn.md) shape without `kind`, and
without the `map_id` and `seed` a standalone turn holds at its own root, since a
battle states them once for every round. It keeps its `round`.

The four documents nest rather than compete. A battle is turns plus what they
share, a turn is a state plus the [decisions](action.md) taken from it, a
[state](state.md) is one complete position, and a [layout](layout.md) is the
projection of a state onto what the fight simulates.

## What every round shares

Two fields are hoisted out of the rounds, and both keep the meaning and the
optionality a layout gives them.

| Field | Source |
| --- | --- |
| `seed` | `BattleInfo.SystemSeed` |
| `map_id` | `BattleInfo.MapID` |

The rest of the match header is a property of the standard 1v1 rule set rather
than of a match: the phase durations, the round cap, the advance team,
reinforcement and construction flags, the game and score modes. A battle states
the mode once by being this format, rather than restating its consequences in
every document. A mode that changes them is a different format.

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

## The sides are positional

`blue` is `playerRecords[0]` and `red` is `playerRecords[1]`.

Nothing else names a seat. The record's own `team` is the same value for both
players, and its `Seat` names the client that recorded the file rather than a
side. Player names and account IDs are not fields: nothing reads them, and a
side is identified by which side it is.

## The custom tech loadout

One per-side quantity is a property of the match rather than of a round, and it
is the only thing under `sides`.

Each player chooses, before the match, which technologies each unit may
research. A technology outside a unit's loadout cannot be researched in that
match, so the loadout bounds every `upgrade_technology` a turn can hold.

```yaml
    blue:
      tech_loadout:
        1: [1105, 10301, 10401, 10801]
        2: [702, 1802, 3202, 10202]
        17: [417, 3317, 10217, 12017, 12117, 12217]
        31: [631, 10231, 180931, 503101]
```

It is real state and not a catalogue: two players in one match hold different
loadouts. Rows cover units `1` through `31`, then `2001`, `2002` and `4001`.

The loadout keeps the per-unit grouping that a state's flat `techs.units` array
drops, because the rule that lets a state flatten does not hold here. A
technology's owner cannot be read off the end of its ID for every row, and says
nothing at all about the three units above `2000`. Ownership is still a function
of the ID, since no technology belongs to two units, and it is resolved against
the build's catalogue rather than by decoding digits.

## The turns are a sequence

`round` stays on each turn even though the list is ordered. It is the key the
record files a snapshot and an action list under, a list position is not, and a
battle that is sliced or that starts away from zero has to stay readable.

A battle repeats most of its state in every turn, and that is accepted rather
than encoded away. Each turn must stand alone as a document, so that one round
can be lifted out of a match and run on its own, and a delta encoding would make
every turn depend on all of its predecessors.

## Between two turns there is a fight

Consecutive states are not adjacent. Applying a turn's actions to its state
yields the position at the end of the deployment, projecting that position
yields the layout the fight starts from, and the fight is what produces the next
turn's roster and reactor core. A battle is therefore the only one of the four
documents that states an end-to-end simulator obligation, and the only one whose
checks can be cross-round.

It also means a battle records no outcome. The last recorded round is a
deployment like any other, and the fight that ends the match has no successor
state to show its result. No fight result is a field anywhere, and every earlier
one is visible only as the difference between two states.

## Cross-round invariants

A turn checks itself within a round. A battle checks the seams between rounds,
and these hold across every transition of a well-formed battle.

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
| `techs.officers` are kept, or replaced by their own next level |
| `reactor_core` falls or holds, except across the opening |

The two replacement rows are one mechanism seen twice. Activating a chain
blueprint replaces its predecessor rather than joining it, and the product
Officer it grants follows.

The reactor core rises only across the round 0 to round 1 transition, by the
amount the advance team the opening chose carries. After the opening it only
falls.

## The supply ledger

A battle is the level at which supply can be checked, because the identity
spans two rounds:

```text
supply(round + 1) = supply(round) - spent(round) + income(round + 1)
```

Income is the map's row plus what the side's officers and worn equipment add,
less what a Rapid Supply owes from the round before. Spending prices the turn's
decisions; [`action.md`](action.md) says what each costs, and the tables in
`config/` carry the amounts.

Two rounds cannot be decided by the identity alone. A side holding an officer
that pays a bounty for destroying a giant is paid by the fight in an amount no
document records, so such a round is counted apart rather than failed.

The check is reported, never enforced: a battle is well-formed whether or not
its ledger closes.

## Converting a replay

```bash
mechcore convert <replay.grbr> <battle.yaml> [--force]
```

The converter refuses rather than guesses. A replay from another build, a
downloaded one, a match mode other than `VS_1_1`, a `Test` match, a match
carrying game rules, rounds that are not the contiguous sequence both sides
share, an object this build's catalogues cannot name, and an action this format
has no representation for are each an error naming what was found.

Most fields are copied. Four are not, and each is argued in the document that
owns it:

| Field | Why it is rebuilt |
| --- | --- |
| `supply` | The snapshot precedes the round's income, which is added back from the map settings the record itself carries, less the energy tower debt |
| `shop.buys_remaining`, `unlocks_remaining` | The recorded counters state the previous round's remainder, so the allowance is read back from the next snapshot plus what this round spent |
| `energy_tower_skills` | The recorded list is a debt rather than an activation, so a round's start carries none |
| `equipment` | The recorded inventory includes fitted items, which the formations already name |

The last round has no next snapshot to read the shop allowance from, so it falls
back to the shipped constants, two and one.

The energy tower debt is the one quantity two readings produce, and the two have
to agree. A round's decisions say what it owes, and the recorded list says the
same thing a round later, because the activation flag survives into the round
after the one that set it. Only the skill with a deferred half can be
snapshotted, so a round whose recorded list names anything else, or names
nothing where the round before activated one, is refused rather than converted
under whichever reading happens to be consulted. An activation in the last round
is never compared: no snapshot follows it.

Two fields have no recorded source and are refused rather than guessed. A panel
holding commander skill `800001` is an error instead of a silently missing
`airdrop_shields`, and `travelling` is left absent rather than inferred from the
ambush regions.

## Normal form

| Collection | Order |
| --- | --- |
| `turns` | ascending `round` |
| `tech_loadout` | ascending unit ID, each row ascending technology ID |
| `game_rules` | ascending rule ID |

A turn's own collections, and a state's, keep the orders those documents define.

## Excluded fields

| Field | Why it is not in the battle |
| --- | --- |
| `BattleInfo.BattleID` | Names a server record; no rule reads it |
| `BattleRecord.Seat` | Which client recorded the file, not a property of the match |
| `PlayerRecord.name`, `id`, `ad` | Account identity; nothing reads it |
| `PlayerRecord.data.styleData` | Skins |
| `PlayerRecord.data.team`, `isLeader`, `type` | A side is named by which side it is |
| `PlayerRecord.data` supply and core settings | The map's row in `matchSettings` gives them, keyed by `map_id` |
| `BattleRecord.reinforceItems` | Carries no offer; the per-round array does |
| `PlayerRecord.seed` | Nothing draws from that stream, see the state document |
| `BattleRecord.Version`, `CreateTime` | Provenance, see below |
| The 1v1 header constants | Properties of the mode, above |

A document's IDs are all resolved against one build's catalogue, so a converter
records which replay and which build a battle came from beside the document
rather than inside it. A `build` field no rule reads would state a fact the
reader cannot act on except by refusing the document.

## Unresolved

**Whether provenance belongs in the document.** A build mismatch is currently
undetectable from a battle alone, because the build is recorded beside the file.
Making it detectable means a `build` field, and a field is only worth carrying
if a mismatch is a refusal. That is the same question for all four kinds and
should be answered once.

**Whether a battle records how the match ended.** Today it does not: the format
holds positions and decisions, and an outcome is neither. But giving up is a
decision a player takes that [`action.md`](action.md) has no representation for
precisely because it ends a match rather than moving a position. If that becomes
a battle-level field, the two questions are answered together.

**What a battle does with a match that carries game rules.** Refusing is the
current answer and it is a floor rather than a design. A rule changes premises
the other documents rest on, so admitting one means each of those documents
saying what it does under that rule, not just this one recording the rule's ID.
