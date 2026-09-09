# Battle definition

## Status

This document is a design draft, and the field names below are proposals rather
than a contract. Sections marked **Unresolved** name what still has no evidence
behind it.

One half of it is implemented. `mechcore convert battle <replay.grbr>
<battle.yaml>` writes this document from a locally recorded replay, and the
[conversion section](#converting-a-replay) says what it rebuilds and what it
refuses. Nothing executes a battle yet.

A battle spans a whole match, so the four tracked ranked replays that carry most
of the [state](state.md) and [turn](turn.md) claims are too small a corpus for
it. Everything measured here is measured over the locally recorded ranked
replays of the Steam installation: 11 matches, 22 player slots, 202
player-rounds and 180 round transitions. `tests/grbr/README.md` explains which
replays are usable and why the downloaded ones are not, and
`work/research/battle_invariant_support.py` reproduces every count below.

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

The three documents nest rather than compete. A battle is turns plus what they
share, a turn is a state plus the decisions taken from it, and a layout is the
projection of a state onto what the fight simulates.

## What every round shares

`BattleInfo` is the match's own header, and it is almost entirely constant.
Across the 11 ranked matches only three of its 19 fields take more than one
value:

| Field | Distinct values | Where it goes |
| --- | ---: | --- |
| `SystemSeed` | 11 | `seed` |
| `MapID` | 5 | `map_id` |
| `BattleID` | 11 | excluded, it names a server record |

So a battle's own fields are the two a [state](state.md) already carries, and
hoisting them is the whole of what the root does. Both keep the meaning and the
optionality a layout gives them.

The rest of the header is one value in every ranked match:

| Field | Value |
| --- | --- |
| `StartTime`, `HostID`, `BlueprintIncreaseSupply` | `0` |
| `PrepareTime`, `DeployTime`, `FightTime` | `30`, `100`, `120` |
| `MaxRound` | `40` |
| `EnableAdvanceTeam`, `EnableReinforcement`, `EnableUnitReinforcement`, `EnableConstruction` | `true` |
| `GameMode`, `MatchMode`, `ScoreMode` | `Normal`, `VS_1_1`, `ReduceScore` |
| `SurviveModeDifficulty` | `VeryEasy` |
| `gameRules` | empty |

None of that is universal. It is the standard 1v1 rule set, and the two Training
Ground replays in `tests/grbr` show what a different mode does to the same
header: `PrepareTime` and `DeployTime` become `9999`, `MatchType` becomes
`Test`, the advance team, reinforcement and construction flags all become
`false`, and two fields appear that a ranked header does not have at all,
`PlayerCount` and `TeamCount`. A constant is therefore a property of the mode
this format describes, and the format states the mode once rather than restating
its consequences in every document.

`FightTime` is the only one of them a simulator reads. It is 120 seconds in all
11 matches, which is the value `crates/simulation/src/kernel.rs` already holds
as `FIGHT_TIME_SECONDS`.

### Game rules are the premise, so the battle carries them

`game_rules` is optional and its absence means none. It is the one constant
above that a battle states rather than drops, because the other two documents
are full of exclusions that rest on it being empty: the research queue is
unreachable without rule `999917`, equipment never wears out and so needs no
`durability` without rule `999903`, and the blueprint pool is `[1, 2, 3, 4, 5]`
only while no rule enables blueprint research. Each of those is a claim about
this field.

Every replay this machine holds records an empty `gameRules`, ranked and
Training Ground alike, so a battle that names a rule is outside what the format
has been measured against. A reader should refuse such a document rather than
read the states inside it under premises the rule breaks.

## The sides are positional

`blue` is `playerRecords[0]` and `red` is `playerRecords[1]`, which is the
convention `crates/document/src/grbr.rs` already converts coordinates by.

The record offers nothing better. `PlayerRecord.data.team` is `0` for both
players in all 22 slots, so it does not name a seat, and the record's `Seat`
names the client that did the recording rather than a side: it is `-1` in every
downloaded replay, which is one of the reasons `tests/grbr/README.md` rules that
class out. Player names and account IDs are not fields either. Nothing reads
them, and a side is identified by which side it is.

## The custom tech loadout

One per-side quantity is a property of the match rather than of a round, and it
is the only thing under `sides` here.

Each player chooses, before the match, which technologies each unit may
research. The record keeps that choice as `PlayerRecord.data.unitDatas`, one row
per unit, and the match reads it back: `PlayerAgent(PlayerData)` feeds each
row's `UnitData.GetTechnologies` into that unit's `UnitTechnologyManager`, and
`PAP_UpgradeTechnology.Check` resolves an action's `TechID` through
`TechnologyManager.GetTechnology(unitID, techID)`, which can only answer for a
technology the loadout put there. `PlayerAgent.GenerateUnitDatas` writes the
same rows back out, and the client fills them from
`LocalPlayerProxy.LoadCustomTechnologyData`, falling back to
`UnitUtility.GenerateDefaultTechnology`.

```yaml
    blue:
      tech_loadout:
        1: [1105, 10301, 10401, 10801]
        2: [702, 1802, 3202, 10202]
        17: [417, 3317, 10217, 12017, 12117, 12217]
        31: [631, 10231, 180931, 503101]
```

It is real state and not a catalogue. The 22 player slots hold 16 distinct
loadouts, and 32 of the 34 rows differ between at least two players. It is also
binding: in all 202 player-rounds every technology a side had researched comes
from that side's own loadout, without exception.

The row count is fixed at 34, in one order, for every player: units `1` through
`31`, then `2001` 丧钟, `2002` 泰山 and `4001` 试验级丧钟. Most rows hold four
technologies; `17` 战争工厂, `29` 深渊, `2001` and `2002` hold six, and `4001`
holds twenty. The rows for `2001` and `4001` are identical in all 22 slots, so
nobody in this corpus has customised them, which is a fact about the corpus and
not a rule.

The loadout keeps the per-unit grouping that a state's flat `techs.units` array
drops, because the decoding rule that lets a state flatten does not hold here.
Reading the owner off the end of the ID accounts for 2640 of the 2816 ordinary
rows and fails on the rest, `1106` belonging to unit `4` and `503101` to unit
`31`, and it says nothing at all about the three units above `2000`.

Ownership is still a function of the ID, since no technology appears under two
units in any of the 22 loadouts, and it is resolved the way the adapter already
resolves it: `technology_owner` asks the runtime catalogue which unit has the
technology, and the decoding rule survives only as a test helper. That weakens
the flattening argument in `docs/state.md`, where 18 of the 289 researched rows
break the same rule. The flat array survives, because the catalogue answers
where the digits do not.

## The turns are a sequence

`round` stays on each turn even though the list is ordered. It is the key the
record itself files a snapshot and an action list under, a list position is not,
and a battle that is sliced or that starts away from zero has to stay readable.

Today it is derivable: all 22 recorded round lists run contiguously from 0 and
agree with `matchDatas`. Keeping it is the same kind of redundancy as a turn
stating its round twice over, a fact the reader can check rather than a fact it
has to trust.

A battle repeats most of its state in every turn, and that is accepted rather
than encoded away. Each turn must stand alone as a document, so that one round
can be lifted out of a match and run on its own, and a delta encoding would make
every turn depend on all of its predecessors. The format already refused that
shape once: a state takes the reinforcement offer as an input precisely so that
it is not a machine that generates its own successors.

## Between two turns there is a fight

Consecutive states are not adjacent. Applying a turn's actions to its state
yields the position at the end of the deployment, projecting that position
yields the layout the fight starts from, and the fight is what produces the next
turn's roster and reactor core. A battle is therefore the only one of the three
documents that states an end-to-end simulator obligation, and the only one whose
checks can be cross-round.

It also means a battle records no outcome. The last recorded round is a
deployment like any other, with both reactor cores still positive in all 11
matches, and the fight that ends the match has no successor state to show its
result. That is the format's boundary rather than a hole in it: no fight result
is a field anywhere, and every earlier one is visible only as the difference
between two states.

## Cross-round checks

A turn checks itself within a round. A battle can check the seams between
rounds, and these are what the record supports. All 180 transitions satisfy
each:

| Check | Holds |
| --- | ---: |
| `next_index.unit` rises or holds | 180/180 |
| `next_index.contraption` rises or holds | 180/180 |
| researched technologies are kept | 180/180 |
| the skill panel is extended, never reordered | 180/180 |
| `shop.unlocked_units` are kept | 180/180 |
| `tower_strengthen_levels` rise or hold | 180/180 |
| `blueprints` are kept, or replaced by their own next level | 180/180 |
| `techs.officers` are kept, or replaced by their own next level | 180/180 |
| `reactor_core` falls or holds, except at the opening | 169/180 |

The two replacement rows are the same mechanism seen twice. `Active` goes
through `ReplaceBlueprint`, so a chain blueprint overwrites its predecessor
rather than joining it, and the product officer it grants follows: the 5
transitions that drop an officer all replace `20300` by `20301` or `20310` by
`20311`.

The reactor core rises 11 times, once per match, and every one of them is the
round 0 to round 1 transition, by 100 to 700. That is the advance team, whose
entry carries its own `reactorCore`, chosen by a round 0 action that the round 0
snapshot precedes. After the opening the core only falls.

Researched technologies stay inside the loadout, which is the one check that
crosses the two levels of the document: 202 of 202 player-rounds.

## The energy tower list is one flag seen at one instant

`playerData.energyTowerSkills` looks like an accumulating list and is not one.
The mechanism is here because it spans two rounds; what a state does about it is
in [`docs/state.md`](state.md).

The manager holds one entry per energy tower skill, created once by
`EnergyTowerSystem.Init` and never added to again, and each entry is an
`ActivableItem`. Activation sets a flag on it, and that flag is the whole of the
bookkeeping. Its life spans two rounds:

| When | What happens to a skill activated in round N |
| --- | --- |
| Round N, at the action | `ActiveSkill` sets the flag, and `AddSkillEffect` pays the immediate half: supply, shop buy count, shop unit level, unit buffs |
| Round N+1, `OnEnterDeploymentBefore` | `AddNewRoundEffects` arms the deferred half, registering `nextRoundSupplyChangeValue` through `Player.AddData`, where `AddRoundSupply` reads it as part of that round's income |
| Round N+1, `OnEnterDeploymentAfter` | `Refresh` removes the effect and calls `Deactive` |

`PlayerSnapshotController.TakeResearchCenterSanpshot` writes the flagged skills
that pass `EnergyTowerSkill.IsLongTermEffect`, which the disassembly shows to be
a test of the catalogue field at `0xBC`, `nextRoundSupplyChangeValue`. Only
skill `1` 快速补给 has one, at `-300`. The snapshot is taken between the last two
rows of that table, so it catches round N's flag during round N+1 and never
catches round N+1's own. The field reads as a record of the previous round
because it is one flag seen at one instant, not two quantities. It holds skill
`1` in the round after a round that activated it in 180 of 180 transitions,
against 15 activations.

### A state needs no field for the debt

The debt is the flag, and the flag is what `energy_tower_skills` already states.
Every energy tower skill is a 本回合 effect, including the `+200` half of skill
`1`, so the set a fight needs is the set activated this round, and that set
lives only in the action log. Skill `1` appearing in it is the debt: the `+200`
is already inside `supply`, and the `-300` is a consequence that lands in the
next state's `supply`, which is stated absolutely. The previous round's debt is
not a field either, since by the time a state exists its income has already
arrived net of the penalty.

It follows that this list is empty at every round start and fills as the round's
actions are applied, which is the other reason it can never equal the recorded
field.

Two places still have to know, and neither is a state field. A converter
rebuilding round N's `supply` needs round N's recorded `energyTowerSkills`,
because that is what says the `-300` applies, so the field belongs to the supply
reconstruction rather than to the energy tower. And an installer has to set the
flag without repeating the immediate half, which is what
`ApplyResearchCenterSnapshot` does when it calls `ActiveSkill` with its
`isSnapshot` argument, and what `AddSkillEffect` branches on. An installed state
whose flag is missing gives the next round 300 supply too many.

`docs/state.md` states the consequence in its own terms: the field means what
was activated this round, which is what the layout projection of skills `5` and
`6` already assumes, and it is rebuilt from the round's actions rather than
copied from the record.

## Converting a replay

```bash
mechcore convert battle <replay.grbr> <battle.yaml> [--force]
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

Two fields are always empty and refused rather than guessed when they could not
be. `airdrop_shields` has no recorded source, so a panel holding commander skill
`800001` is an error instead of a silent omission. `travelling` has none either,
and it is left absent rather than inferred from the ambush regions.

## Normal form

| Collection | Order |
| --- | --- |
| `turns` | ascending `round` |
| `formations`, `constructions`, `contraptions` | ascending `index` |
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
| `PlayerRecord.data.team`, `isLeader`, `type` | Constant in every ranked slot; a side is named by which side it is |
| `PlayerRecord.data` supply and core settings | The map's row in `matchSettings` gives them, keyed by `map_id` |
| `BattleRecord.reinforceItems` | Empty in every replay |
| `PlayerRecord.seed` | Constant through the match, and nothing draws from that stream, see the state document |
| `BattleRecord.Version`, `CreateTime` | Provenance, see below |
| The 1v1 header constants | Properties of the mode, listed above |

Provenance is the one thing a battle drops that is worth keeping somewhere. A
document's IDs are all resolved against one build's catalogue, and this
machine's replay directory holds three builds, so a converter should record
which replay and which build a battle came from beside the document rather than
inside it. The alternative, a `build` field that no rule reads, would state a
fact the reader cannot act on except by refusing the document.

**Unresolved.** Whether a build mismatch should be a refusal, and therefore
whether provenance belongs in the document after all, is not settled here. It is
the same question for all four kinds and should be answered once.

## Capabilities the adapter still lacks

A battle cannot be captured or executed today, for the reasons the other two
documents give: there is no complete state capture, and the player action space
a turn needs barely overlaps the Training Ground commands the adapter drives.

The tech loadout is the exception, and it is already reachable. The adapter
drives `MAD_ClearTechnology`, `MAD_AddTechnology` and `MAD_ActiveTechnology`
when it installs a layout's technologies, and `MAD_AddTechnology` is exactly the
command that puts a technology in a unit's manager without researching it. What
is missing is the distinction rather than the capability: layout application
adds only the technologies it is about to activate, so an installed match today
has a loadout equal to its researched set.
