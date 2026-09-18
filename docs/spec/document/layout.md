# Training Ground layout definition

## Scope

`layout.yaml` describes the desired state of both players in a deterministic
Training Ground scene. It describes game state, not the actions used to create
that state.

The top-level `round` selects the deployment round in which the complete layout
becomes active. The optional top-level `seed` selects the native or simulated
match seed; omitting it asks the executor to generate one. The document does
not contain `reactor_core`, `supply`, capture settings, or exit behavior.

The same layout is also the input to the bounded deterministic simulator:

```text
mechcore sim layout.yaml [--seed <i32>] [--output battle.mcfr] [--config <directory>]
mechcore verify layout.yaml
```

`mechcore sim` returns hashes, terminal battle structure, and generation profiling
without generating MCFR storage by default. `--output` additionally serializes,
validates, and publishes an MCFR at the requested path.

Unit combat values are resolved from the typed, one-file-per-unit
[unit configuration contract](../simulation/unit-rules.md), not stored in the
layout. The simulator's closure is narrower than native layout application, and
it rejects a formation or mechanism outside that closure before recording
begins rather than recording an approximation.

## Document shape

A layout contains exactly two player sides, `blue` and `red`. Persistent Officer
and unit-technology state is grouped under each side's `techs` object.

The required top-level `kind` names what the document is. It is a constant, not
a version: it takes the single value `layout` and never needs maintaining. Its
purpose is to separate this document from the other kinds this format will
define, such as a state, which carries everything a layout projects and so
overlaps this shape too much for structure alone to tell them apart. A reader
resolves `kind` before anything else, so a document of another kind is refused
as that kind rather than as a layout with an unexpected field. A document that
names no kind is refused as well, since a default would make the answer a guess.

`kind` marks a document root, not a subtree. A future document that embeds part
of a layout does not repeat it inside that part.

```yaml
kind: layout
round: 3

sides:
  blue:
    techs:
      officers: [10002, 20003, 30201]
      units: [10202, 10402]

    energy_tower_skills: [5, 6]
    tower_strengthen_levels: [1, 2]

    formations:
      - type: marksman
        index: 0
        position: {x: 0, y: -50}
        equipment: 13030001

      - type: arclight
        index: 1
        position: {x: -310, y: 20}
        travelling: true

    constructions:
      - type: defensive_wall
        index: 0
        position: {x: 140, y: -105}

    contraptions:
      - type: interceptor
        index: 0
        position: {x: 5, y: -95}

    airdrop_shields: []
    terrains: []

    battle_skills:
      - type: mobile_beacon
        positions:
          - {x: -100, y: -150}
          - {x: 0, y: -100}
          - {x: 100, y: -50}

  red:
    techs:
      officers: []
      units: []
    energy_tower_skills: []
    tower_strengthen_levels: []
    formations:
      - type: marksman
        index: 0
        position: {x: 0, y: -100}

    contraptions: []
    airdrop_shields: []
    terrains: []
    battle_skills: []
```

YAML field order has no semantic meaning. Examples and generated files should
nevertheless use the order above.

## Normal form

One state is one document. A layout is in normal form when syntax equivalent to
a public default is absent and every collection whose order carries no meaning is
in its defined order:

| Collection | Order |
| --- | --- |
| `formations`, `constructions`, `contraptions` | ascending `index` |
| `techs.officers`, `techs.units`, `energy_tower_skills` | ascending ID; `techs.officers` may repeat one |
| `airdrop_shields` | ascending `(x, y)` |
| `terrains` | ascending `type`, then control points |
| `battle_skills` | as written: release order is what it records |

`tower_strengthen_levels` is absent from that table because its order is its
meaning: it is keyed by tower position. An all-zero list is equivalent to
omitting it, and normal form omits it.

`airdrop_shields` can be reordered because the game sorts both of its shield
lists by position at fight start, so a retained shield's position in the list is
not observable during the fight. `terrains` can be reordered because under
current 1v1 rules the collection holds at most one entry, so there is no order
to observe; the sort is what keeps the rule total over the shape the schema
admits rather than over the states today's rules can reach.

`mechcore format` writes this form and `mechcore diff` compares it,
so two documents that denote the same state compare equal. Normalizing an
already-normal document changes nothing. A hand-written layout may still state a
default explicitly, which is valid and merely not normal.

A canonical document begins with `kind: layout`, since a reader has to know what
it is holding before any of it means anything.

Every coordinate pair in the schema is one `{x, y}` value rather than two
sibling fields. `formations`, `constructions` and `contraptions` carry it as
`position`; `airdrop_shields`, `terrains.control_points` and
`battle_skills.positions` are lists of the same value. The canonical writer
folds every one of them onto one line, a list item by item, since spending two
or three lines on a single value would bury whatever tells two of them apart.
An [action](action.md)'s `!area` target is written the same way.

The `mechcore-document` crate is the authoritative implementation of this public
shape, its static legality rules, and normalized execution plan. MCP uses its
`Layout` type for the `apply_layout` input schema and validates with the shared
compiler before contacting the game; the Adapter consumes the same plan, while
the Simulator adds only its narrower feature-support and configuration checks.
Runtime catalog availability and native readback remain Adapter-owned.

`mechcore verify layout.yaml` runs this shared static compiler without
starting the game or Simulator. It prints one JSON object per input, and a
layout's carries `kind: layout` beside the normalized seed, round, formation
count, construction count, contraption count, and airdrop shield count.

The command is not the layout's alone. It checks each file against the contract
that file names for itself, so a deployment recording is checked by replaying
its decisions through the transition instead;
[`action.md`](action.md#what-a-decision-reproduces) states that check and how a
batch is given. A layout is routed here by the `kind` at its
root, never by its extension.

`mechcore diff left.yaml right.yaml` normalizes both documents and reports
the fields that differ. Each difference carries a JSON pointer, except that
`formations`, `constructions` and `contraptions` are aligned by their entries'
`index` rather than by position, and their path segment reads `index=<value>`.
Aligning those by position would report an object inserted or removed in the
middle as a change to every later entry plus one addition or removal at the end;
keying by identity reports one removal, one change and one addition instead, so a
difference corresponds to a decision rather than to a shift in the list. A
collection whose entries lack a usable unique `index` falls back to positional
alignment. The report is `mechcore.layout-diff-result.v2`.

## Seed and map

`map_id` is an optional positive integer identifying the native `MatchSetting`
map (not the game mode). Native replay export records `BattleInfo.MapID`;
`apply_layout` selects that map before creating the Training Ground. Omission
uses map 1021 (训练基地), the Mechcore Training Ground baseline. Unknown IDs are
rejected by the native adapter.
Build 2259 examples: `1001` is 铁道小镇 (`MainSceneDesert`), and `1021` is
训练基地 (`MainSceneMilitaryBase`). Map-owned neutral crystals are retained:
the selected map, not a global deletion rule, determines the RVO environment.
This field does not imply simulator support for map-specific obstacles.
For standalone sessions use `start_test: {seed: 42, map_id: 1021}` in scripts,
or `start_test 42 --map-id 1021` in the shell.

`seed` is an optional signed 32-bit integer. When present it requests that exact
match seed. When absent it requests a generated seed, whose resolved `i32` value
is reported by the executor and persisted in MCFR `DurableContext.match_seed`.

The value `0` is rejected. It is the native `set_SystemSeed` request for a
system-generated seed, so a layout carrying it would denote every scenario at
once rather than one, and no recorded match seed can equal it. Absence already
expresses that request, and it expresses it without pretending to be a value.

A seed is therefore not a state field with a baseline. It is an argument of the
battle, and the layout only supplies a default for it. The call site holds the
real parameter, which is why one layout can be recorded under many seeds:

- `mechcore sim` resolves `--seed`, then `layout.seed`, then a generated seed,
  and reports which of the three it used; `--seed 0` is refused for the same
  reason the field is, and a generated seed never lands on `0`;
- `apply_layout` resolves its own optional `seed`, then `layout.seed`, and
  otherwise lets the game generate one;
- an MCFR always embeds a layout whose `seed` is the resolved value, and a
  recording whose embedded layout has no seed is refused.

## Round

`round` is a required integer of at least `1`. It is the match round this
layout describes, counted the way the game counts it and read back from
`Match.get_RoundCount()` during capture. It is game state, not a directive: it
says when this deployment happened, and the rules the game applies at that
point follow from it.

The round is what makes the rest of the document decidable: ambush availability
and the travelling rules below are both functions of it.

`apply_layout` additionally treats it as an activation round, because staging a
Training Ground to this state means advancing through every earlier round. That
is a property of that operation, not of the field. Earlier rounds are setup
rounds owned by the adapter; callers do not submit or observe separate partial
layouts.

The two readings pull apart at the upper end, so validation and staging are
separate checks:

- `mechcore verify` and every other schema consumer accept any positive
  round, since Training Ground and ranked matches both run past round 15;
- `apply_layout` refuses a round above `MAX_STAGED_ROUND`, which is `15`,
  because it advances through every earlier setup round inside the adapter's
  55-second total layout timeout.

`MAX_STAGED_ROUND` is that timeout budget, not a game rule. It lives in the
adapter protocol and constrains only the staging path. Replay decoding through
`record_replay_round` is not bounded by it either, since reading round `N` out
of a GRBR has nothing to do with advancing a live match to round `N`.

The native ambush zones become available from round 2. The layout rules are:

- round 1 cannot contain an ambush-zone unit;
- round 2 requires `travelling: true` on every ambush-zone unit;
- round 3 and later accept both travelling and non-travelling ambush units.

A unit enters native travelling state when a move takes it into a flank region
from another region, and the fight then takes it out again, so a unit settles
in the round after the one that deployed it. [`action.md`](action.md) states
that rule in full. Round 1 admits no ambush unit at all, so every round 2
ambush unit is necessarily a first flank deployment and cannot already have
settled. That makes the round 2 case decidable from one layout alone, so the
compiler rejects a round 2 ambush unit that is not travelling. From round 3 the
same question needs the previous round's state, which a layout does not carry,
so both values are accepted there.

`travelling: false` on a settled ambush unit is applied through a direct native
call rather than by replaying the arrival. The Adapter explicitly changes and
verifies that membership. A layout can therefore still describe a settled
ambush unit in a round where a real match could not have produced one, for
example a round 3 unit that no round 2 deployment placed. Reproducing a battle
does not require that its deployment be reachable by play, and the sandbox
deliberately keeps that freedom. A consumer that needs reachability rather than
reproducibility must check it against the preceding state, outside this schema.

All formations are deployed in declaration order after the activation round
begins.

## Coordinate system

Layout uses a self-contained two-dimensional `(x, y)` deployment coordinate
system. These names follow the game's native two-component deployment API;
layout `y` is not the Unity world height axis. Every position is expressed in
the owning side's fixed local frame, not in movable screen or camera
coordinates:

- the battlefield center is `(0, 0)`;
- `+y` always points from the owning side toward its opponent;
- `+x` always points to the owning side's right;
- the owning side's main deployment half therefore uses negative `y`.

The local main deployment rectangle is `x=[-300,300], y=[-310,-10]`. Ordinary
units may additionally use either local ambush rectangle:

- left: `x=[-360,-300], y=[10,310]`;
- right: `x=[300,360], y=[10,310]`.

The adapter compiles side-local positions to the native two-dimensional
deployment coordinates before invoking layout operations:

```text
blue: native(x, y) = local( x,  y)
red:  native(x, y) = local(-x, -y)
```

When a layout enters MCFR or the simulation kernel, its second coordinate is
normalized to the Unity world battlefield axis: `layout.x -> world.x` and
`layout.y -> world.z`; Unity/MCFR world `y` remains vertical height. This
mapping is outside the layout schema and does not rename its fields.

The same transform applies independently to every coordinate in
`battle_skills.positions` and every `terrains.control_points` entry. Terrain `grid_rows` are
also expressed in the owning side's local frame: rows advance along local `+y`
and low-order bits advance along local `+x`. The native terrain executor
therefore rotates both row order and bit order for the red side. A
180-degree transform does not change a formation's `rotated` boolean.
Authoritative native readback remains in world coordinates; the adapter verifies
that world state against the compiled position and returns the layout-local
position publicly.

## Side definition

`formations` is required for each side and must contain at least one valid
unit. Every other side field is optional and has its empty or baseline value
when omitted:

- Omitting `techs` is equivalent to setting both nested arrays to `[]`.
- `techs.officers` defaults to `[]`.
- `techs.units` defaults to `[]`.
- `energy_tower_skills` defaults to `[]`.
- `tower_strengthen_levels` defaults to `[]`, which puts every tower at level
  `0`.
- `constructions` defaults to `[]`.
- `contraptions` defaults to `[]`.
- `airdrop_shields` defaults to `[]`.
- `terrains` defaults to `[]`.
- `battle_skills` defaults to `[]`.
- A unit formation's `index` is required and has no default.
- A unit formation's `level` defaults to `1`.
- A unit formation's `rotated` defaults to `false`.
- A unit formation's `equipment` defaults to no equipment.
- A unit formation's `travelling` defaults to `false`.

Unknown fields must be rejected, and so is a document whose `kind` is absent or
names another kind. Numeric IDs outside formation definitions must
be positive integers unless the field explicitly defines `0` as a baseline
level.

### `techs`

```yaml
techs:
  officers: [10002, 20003, 30201]
  units: [10202, 10402]
```

`techs` groups two disjoint native ID spaces under one public layout concept.
The nested fields are retained because Officers and unit technologies use
different native catalogs and application operations. A bare combined ID array
would lose that distinction.

Build `1.11.1.2.2227` ID, localization, and configured-effect indexes:

- Officers and unit modifications:
  [English](../../rules/officers.md) / [简体中文](../../rules/officers.zh.md)
- Unit technologies:
  [English](../../rules/unit_techs.md) / [简体中文](../../rules/unit_techs.zh.md)

#### `techs.officers`

`techs.officers` is the multiset of native `OfficerData.ID` values whose
persistent effects belong to the side. It covers ordinary and opening Officers
regardless of how they were acquired.

Unit modifications are also native Officer entries. An Officer with a nonzero
`typeID` targets that unit type; it is not a separate technology kind and does
not require a `unit_modifications` field. For example, reference build 2227
uses Officer `30101` for Mass-Produced Fortress and Officer `30201` for
Range-Extended Marksman. Generic Officers use `typeID: 0` and remain in the
same array.

An ID may repeat. An Officer card whose `canRepeated` is set may be taken
again, and taking it twice stacks it rather than doing nothing: two copies of
Advanced Offensive Tactics are +60% damage and two of Advanced Targeting System
are +20 m of range, both of which a fight sees. One side in the local replay set
holds three copies of `20022`. The executor therefore adds one Officer per
entry and reads the count back, rather than reading a presence.

IDs carry no order semantics, so canonical layouts sort them ascending; that
sorted order is then the deterministic application order. Commander skills,
equipment, and extra formations granted by an Officer are not themselves
Officer modifiers. Their resulting state belongs to `battle_skills`, equipment,
or formation definitions.

The Officers the Research Center's two enhancement chains hand out, `20310`,
`20311`, `20300` and `20301`, belong here like any other Officer. A layout has
no separate attack or defense level, because the Officer is the whole of what
those levels do to a fight.

The deterministic implementation must add an Officer by ID through the native
Training Ground test action and verify the resulting `OfficerManager` count.
Choosing a random opening or reinforcement candidate by index is not an
implementation of this field.

#### `techs.units`

```yaml
techs:
  units: [10202, 10402, 10802, 10215]
```

`techs.units` is a flat array of native `TechnologyData.ID` values. Unit
technology IDs are globally unique in reference build 2227, and their final
two decimal digits encode the owning ordinary `unit_id`, so repeating that ID
in the layout would be redundant. The compiler resolves the owner from this
decompiled static contract, then confirms that the current runtime's unit and
technology catalogs still contain the same relationship. Catalog drift is
rejected before any layout mutation.

Every declared technology is both added and activated. The format does not
represent an acquired but inactive technology. IDs must be unique and carry no
order semantics, so canonical layouts sort them ascending; that sorted order is
then the deterministic order of native add and activate operations.

### `energy_tower_skills`

```yaml
energy_tower_skills: [5, 6]
```

`energy_tower_skills` lists the Energy Tower skills this round has activated,
by native ID, ascending. Two of them change what a fight does:

| ID | Effect |
| --- | --- |
| `5` | ranged-unit attack range +15 m |
| `6` | all-unit movement speed +3 m/s |

Only those two may appear. The tower's other skills buy supply or discount a
round's shopping, so they change a [state](state.md) and not a fight, and a
projection drops them rather than recording them here.

Unlike `tower_strengthen_levels`, these effects are cleared at the next
deployment and may be activated again in each round. The staged executor
activates each listed skill in the activation round, and refuses a layout whose
tower already holds a skill the layout does not list.

This field says the same thing the state field of the same name says, in the
same shape, so projecting a state onto a layout copies it rather than
translating it.

### `tower_strengthen_levels`

```yaml
tower_strengthen_levels: [1, 2]
```

`tower_strengthen_levels` holds one strengthening level per fixed tower, keyed
by the tower's position in `BuildingManager.buildings`. That is the key
`PAD_StrengthenTower.Index` uses and the key the state field of the same name
uses, so the two documents say this the same way.

The list is either empty, which puts every tower at level `0`, or exactly two
entries long. Each level is an integer in `0..=4`, the four levels
[`config/economy.yaml`](../../../config/economy.yaml) prices. The executor
strengthens a tower one level at a time and verifies the final level.

A position keys a tower; it does not name one. The two sides order their towers
oppositely: on map 1021 blue holds the Energy Tower at position `0` and red
holds it at position `1`, which [`docs/state.md`](state.md) explains. So `[1, 2]` does not say which building is at level `2` without
knowing the side and the map, and nothing needs to: a level is captured from a
position and applied to that same position.

The adapter checks on every apply and every capture that the side holds one
tower of each native kind, in whichever order, and refuses a scene that does
not. A side that lost a tower or grew one fails loudly rather than having its
levels written to the wrong building.

The Research Center's two persistent enhancements are not a field of their own.
They are Officers, and they live in `techs.officers` with every other Officer:

| Enhancement | Officer | Effect |
| --- | --- | --- |
| attack 1 | `20310` | attack +12% |
| attack 2 | `20311` | attack +36% |
| defense 1 | `20300` | life +15% |
| defense 2 | `20301` | life +45% |

A native match reaches them by researching blueprints `4`, `401`, `5` and
`501`, which is what a [state](state.md) records. A layout carries the Officer
the blueprint hands out, because that Officer is the whole of what a fight sees,
and a level of a chain is the second name for a thing `techs.officers` already
had a name for. Each chain contributes at most one Officer: its second level
replaces its first rather than joining it.

The executor installs these the way it installs any other Officer, without
researching a blueprint. `OfficerManager` keeps two lists and routes each
Officer by a property of its own, and these four are of the kind that lands in
the list `GetInvisibleOfficers` returns rather than `GetOfficers`. Both the
readback that verifies an apply and the capture that exports a side read both
lists, so one of these Officers is neither reported missing right after it was
installed nor dropped from a capture of the side that holds it.

### `formations`

`formations` contains the side's unit formations. Each entry uses the unit's
semantic `type` instead of exposing its native numeric ID:

```yaml
- type: marksman
  index: 0
  position: {x: 0, y: -50}
  exp: 12
```

- `type` is the lower `snake_case` form of the unit's English in-game name. It
  selects both the native catalog and the valid unit fields.
- `position` is the required `{x, y}` center in the owning side's fixed local
  frame defined above, not native world or screen pixels. Both are exact signed
  integers, and the pair is one field because it is one value.
- `index` is the required stable, non-negative native unit index. Indices must
  be strictly increasing in formation declaration order and may contain gaps.
- `exp` is the unit formation's non-negative integer experience within its
  current level. The default is `0` and canonical YAML omits that default.

A unit index is an identity, not a position in this array. It is allocated once
when the unit is bought and survives every later round the unit lives through,
so the same number names the same unit across a whole match, and selling a unit
retires its index rather than freeing it for reuse. That is why the field is
required: a document that leaves it to declaration order cannot say which unit
it is describing, and inserting or removing an entry would silently rename
every unit after it. Since indices must also increase in declaration order,
requiring them makes array order redundant: the index determines it.

The Adapter creates formations in declaration/index order, assigning each
requested index directly through `MAD_AddUnit.UIDX`. Missing indices remain
absent; no placeholder formation is created or removed. It writes experience through the
formation's native `MechTeam.SetExpInt` and verifies both index lookup and
`GetExpInt` readback before combat. Replay capture always exports native
`index`; it exports non-zero `exp` after canonical default elision.

The layout compiler resolves every unit, construction, and interceptor
deployment footprint and rejects positive-area overlap before any game
mutation. Exact edge contact is legal. In a main deployment region,
`rotated: true` exchanges a unit's footprint width and height. The left/right
ambush regions have a native quarter-turn orientation, which exchanges the
effective world footprint once more. `rotated` is therefore stated relative to
the region that holds the unit, not to the world: the base `width x height` is
transposed exactly when `rotated` and ambush membership disagree. An unrotated
ambush unit and a rotated main-region unit describe the same world shape, and
the same `rotated: true` describes two different world shapes depending on which
region the unit stands in. Collision checks use this region-aware
footprint and compiled world positions, so units, constructions, and
interceptors share one collision space within a side and across `blue` and
`red` after the red-side 180-degree transform. Shields and missiles do not
participate in deployment collision checks.

For units, constructions, and interceptors, all four footprint vertices must
lie on the native `10 x 10` deployment grid. The compiler enforces the
equivalent center congruence independently for each footprint dimension:

- `size ≡ 0 (mod 20)` requires `center ≡ 0 (mod 10)`;
- `size ≡ 10 (mod 20)` requires `center ≡ 5 (mod 10)`.

Consequently, in a main deployment region:

- a `20 x 20` footprint uses `x ≡ 0`, `y ≡ 0 (mod 10)`;
- a `30 x 30` footprint uses `x ≡ 5`, `y ≡ 5 (mod 10)`;
- an unrotated `50 x 20` footprint uses `x ≡ 5`, `y ≡ 0 (mod 10)`;
- a rotated `20 x 50` footprint uses `x ≡ 0`, `y ≡ 5 (mod 10)`.

For an ambush-zone unit these effective width/height examples are exchanged by
the region orientation. For example, a base `50 x 20` Crawler with
`rotated: true` uses an effective `50 x 20` ambush footprint and therefore
requires `x ≡ 5`, `y ≡ 0 (mod 10)`.

Every construction and interceptor footprint must fit inside the owning side's
local main deployment boundary `x=[-300,300], y=[-310,-10]`. An ordinary unit
whose center has `y < 10` follows the same rule. An ordinary unit whose center
has `y >= 10` must fit completely inside one of the two ambush rectangles
defined above. A center inside a legal rectangle is insufficient when any
footprint edge crosses it.

Shields and missiles use their native contraption target regions instead of
the deployment footprint rules above. Neither has a modulo-10 requirement or a
collision footprint. A missile's center must lie in
`x=[-300,300], y=[-310,-10]`. A shield has a 70 m radius, and the native check
allows its complete edge to touch the same own-side boundary. For integer
layout coordinates, its center must therefore lie in
`x=[-230,230], y=[-240,-80]`.

The footprint provider covers all 32 public ordinary units validated against
the build `1.11.1.3.2259` card catalog, the four ordinary opening constructions,
and the interceptor:

- `arclight`, `marksman`, and `vortex`: `20 x 20`;
- `farseer`, `hacker`, `rhino`, `sabertooth`, `scorpion`, `tarantula`, and
  `wraith`: `30 x 30`;
- `hound`, `phoenix`, `typhoon`, and `void_eye`: `40 x 20`;
- `fortress`, `melting_point`, `raiden`, `sandworm`, and `vulcan`: `40 x 40`;
- `crawler`, `fang`, `fire_badger`, `mustang`, `phantom_ray`, `sledgehammer`,
  `steel_ball`, `stormcaller`, and `wasp`: `50 x 20`;
- `overlord`: `50 x 50`;
- `abyss`, `mountain`, and `war_factory`: `70 x 70`;
- `defensive_wall`: `60 x 10`;
- `anti_armor_turret` and `rapid_fire_turret`: `20 x 20`;
- `magnetic_barrier`: `50 x 10`;
- `interceptor`: `30 x 30`.

Any unknown placement type remains rejected fail-closed; the compiler does not
infer a size from combat-member radius.

Tracked negative fixtures cover the spatial rejection cases:

- `crates/document/tests/fixtures/invalid-footprint-boundary.yaml`: center
  inside, footprint edge outside;
- `crates/document/tests/fixtures/invalid-unit-collision.yaml`: an unrotated
  `50 x 20` Sledgehammer overlaps a rotated `20 x 50` Crawler;
- `crates/document/tests/fixtures/invalid-unit-construction-collision.yaml`: a
  `20 x 20` Marksman overlaps a `60 x 10` Defensive Wall.

One further negative fixture covers the round 2 ambush rule rather than a
spatial rule:

- `crates/document/tests/fixtures/invalid-round-2-ambush-travelling.yaml`: a
  round 2 ambush Marksman that omits `travelling`.

Native catalog IDs are adapter details and do not appear in a layout. The
following values form the closed public `type` vocabulary for each field:

- Units: `abyss`, `arclight`, `crawler`, `fang`, `farseer`, `fire_badger`,
  `fortress`, `hacker`, `hound`, `marksman`, `melting_point`, `mountain`,
  `mustang`, `overlord`, `phantom_ray`, `phoenix`, `raiden`, `rhino`,
  `sabertooth`, `sandworm`, `scorpion`, `sledgehammer`, `steel_ball`,
  `stormcaller`, `tarantula`, `typhoon`, `void_eye`, `vortex`, `vulcan`,
  `war_factory`, `wasp`, and `wraith`.
- Constructions: `defensive_wall`, `anti_armor_turret`,
  `rapid_fire_turret`, and `magnetic_barrier`.
- Contraptions: `shield`, `interceptor`, and `missile`.

#### Unit

```yaml
- type: marksman
  index: 0
  position: {x: 0, y: -50}
  exp: 12
  equipment: 13030001
  travelling: false
```

The adapter resolves `type` to a native `CardData.ID`; that catalog ID is not
public layout state. `index` is the required stable native formation index.
`level` is the optional displayed level,
defaults to `1`, and must be in `1..=9`. `exp` is optional, defaults to `0`,
and records the formation's current-level experience. `rotated` is an optional
boolean, defaults to `false`, and declares the native unit-orientation flag. It
is region-relative rather than absolute, so the owning region's own orientation
still contributes to the world footprint; see the footprint rules above for the
exact transposition.
`equipment` is an optional positive native `EquipmentData.ID`. A unit has at
most one equipment slot, so this field is singular rather than an array.
`travelling` is an optional boolean and defaults to `false`. It has semantic
effect only for an ambush-zone unit. `travelling: true` is invalid outside the
ambush zones, and `travelling: false` is invalid for an ambush-zone unit in
round 2.

The executor adds the unit, obtains its runtime unit index, moves it to the
declared position and orientation, and verifies type, level, position, and
rotation through authoritative unit readback. When `equipment` is present, it
then adds one copy from the runtime catalog to the current side's Training
Ground inventory through `MAD_AddEquipment`, uses the existing native
`PAD_UseEquipment` action on that unit, and requires authoritative equipment
ownership readback. Available IDs and effects are listed in the
[Equipment index](../../rules/equipment.md) ([中文](../../rules/equipment.zh.md)).

### `constructions`

```yaml
constructions:
  - type: defensive_wall
    index: 0
    position: {x: 140, y: -105}
```

`constructions` is parallel to `formations` under one side and defaults to
`[]`. Each entry requires `type`, `index`, and `position`.

`index` is the construction's deployment identity, allocated in release order by
`ConstructionManager` and stable for as long as the object lives. It is never
reused, so an earlier round's sold or destroyed construction leaves a permanent
gap and the declared indices need not be contiguous. They must be non-negative
and strictly increasing in declaration order, which makes index order the only
normal form for this collection.

The index is comparable between a replay and the Training Ground. The 10000
offset visible in a Test match's `constructionIndex` snapshot field is written by
`ConstructionManager.TakeSnapshot` and stripped again by `ApplySnapshot`; the
live index that `GetConstructionIndex` reports is zero-based in both modes.

At prepare time, the executor first reconciles the same-seed Training Ground
opening constructions by `(type, position)`: exact matches are retained, while
entries absent from or different in the target layout are removed. A retained
construction must already carry its declared index, otherwise application fails
rather than proceeding with a different identity. At activation the executor
applies the missing constructions in declaration order, setting
`PAD_ReleaseConstruction.IDX` to the declared index instead of accepting the one
the release controller allocated, and rejecting the release if the action does
not keep it. Retention, creation, and removal are checked through
`ConstructionManager` lookup/count readback. Replay capture reads the index from
`ConstructionManager.GetConstructionIndex` and exports the collection in index
order.

Both replay and Training Ground MCFR capture derive construction Building IDs
from this same index plus `FightConstruction.GetConstructionChildIndex`, joined
through `ConstructionElement.GetFightConstructions`. A wall placement may own
several Building rows: layout `index` is the per-side deployment identity, not
the global MCFR `building_id`. MCFR building IDs are normalized independently at
the capture boundary.

### `contraptions`

```yaml
contraptions:
  - type: interceptor
    index: 0
    position: {x: 5, y: -95}
  - type: shield
    index: 3
    position: {x: 275, y: 20}
```

`shield`, `interceptor`, and `missile` each resolve directly to their native
contraption kind. `contraptions` is parallel to `formations` and
`constructions` under one side and defaults to `[]`. A contraption entry
requires `type`, `index`, and `position`, and none of these types requires an extra
position.

`index` is the contraption's deployment identity, allocated in release order by
`ContraptionManager`'s object recorder and stable for as long as the object
lives. Like a construction index it is never reused, so a sold missile or a
destroyed interceptor leaves a permanent gap; the example above is a side whose
indices 1 and 2 are gone. Indices must be non-negative and strictly increasing in
declaration order. Contraptions and constructions have separate allocators, so
the same index can appear once in each collection.

Replay export describes the contraptions still present at the deployment
boundary, including objects retained from earlier rounds; it is not a list of
this round's release operations. Export reads which objects exist from the full
shield collection, `TeamMineManager.GetLandMines()`, and live
`InterceptCtrGroup_Interceptor` sources, then reads each one's index from the
recorder's current and history records. Removed missiles and destroyed
interceptors are excluded; inactive reset-next-round shields are included. A live
contraption with no record fails the export rather than being given a synthetic
identity. Layout is captured before combat and is not reordered using S(1). MCFR
Shield IDs are normalized separately and are not inferred from layout entry
order.

Shield radius and maximum energy must match the selected native source's
defaults; otherwise export fails rather than silently losing state. Shields keep
their own-side radius-70 deployment bounds. Integer coordinates are converted
directly to Q32.32 without floating-point rounding.
For missile/interceptor objects, export projects native world X/Z onto the
layout plane; native object height is not a placement coordinate. Fractional
planar coordinates are rejected rather than rounded.

`PAD_ReleaseContraption` carries no index: a release takes whatever the
recorder's allocator holds. The executor therefore writes the declared index into
`FightObjectRecorder.NextObjectIndex` before each release, which is also how a
gap is reproduced. It performs the native placement check, releases the
contraption once, and verifies its type, exact position, and resulting index
through authoritative recorder readback.

### `airdrop_shields`

```yaml
airdrop_shields:
  - {x: -285, y: -22}
```

`airdrop_shields` records the Shield Airdrop objects an earlier round's release
left standing at the start of this fight. It defaults to `[]` and holds bare
side-local centers, because the object carries no other layout-visible state.

A Shield Airdrop is a commander-skill object, not a contraption. It has no
contraption index, it does not consume the contraption count, and it is created
through the commander-skill path rather than a contraption release. That is why
it is its own collection rather than a flag on a `contraptions` entry, even
though native export finds it in the same shield collection as contraption
shields.

An entry here always means a shield already present before this battle. A Shield
Airdrop released during the requested round is a `battle_skills` entry instead,
so one shield is never recorded in both places. The retained object uses
build-2259 `CS_EnergyShield` (ID 800001), which is not short-lived and resets to
maximum energy between rounds.

A replay records it, in the releasing skill's own `rangeItems` rather than in
any object list, and the entry survives as long as the shield does. Direct GRBR
decoding is exposed as `mechcore_document::retained_from_grbr_round`, which
reads the retained oil terrain from the same place.

Because a retained airdrop is an existing world object rather than a new
release, it only has to stand on the battlefield: its center must be inside
`x=[-400,400], y=[-350,350]` in side-local coordinates, and its radius-70 body
may extend outside the deployment area.

During the requested round's deployment, the executor constructs the data source
without adding commander inventory or a release record, then invokes
`AdvancedEnergyShieldSystem.Create(data, FVector3, teamController)` in layout
declaration order, after contraptions and before `terrains`. It verifies
full/active-list insertion, source, team, exact position, radius, energy, and
round policy. MCFR shield IDs remain normalized at S(1); no MCFR schema or hash
field changes.

### `terrains`

`terrains` records the active cross-round battlefield terrain owned by one side
at the start of this single fight. It defaults to `[]`. One entry represents one
original release, rather than one surviving circle of what it left. Under
current 1v1 rules a side holds at most one entry: the Sticky Oil Bomb is the
only release that survives its round, and it is unlocked once through the
research center. Entry order therefore carries nothing, and canonical layouts
sort it:

```yaml
terrains:
  - type: oil
    control_points:
      - {x: -24, y: 11}
      - {x: 80, y: 1}
    grid_rows:
      0: [240, 1020, 2046, 2046, 4095, 4095, 1023, 511, 254, 126, 60, 48]
      1: [240, 1020, 1022, 510, 255, 127, 63, 63, 30, 30, 12, 0]
      5: [16, 28, 30, 30, 63, 63, 127, 127, 254, 510, 1020, 240]
      6: [48, 124, 126, 254, 255, 511, 1023, 2047, 2046, 2046, 1020, 240]
```

- `type` names the substance, not the skill that made it. The names are the
  build's own: `fire`, `oil`, `fog`, `acid` and `recovery_zone`, which are its
  range-item types less the one a unit technology makes rather than a skill.
  Which skill produces which is a catalogue entry rather than a rule of this
  format, so a build that gave a second skill the same substance would need no
  new name here.

  A document may carry any of them; a plan can be built from `oil` alone. The
  geometry below is the producing skill's rather than the terrain's, and only
  the Sticky Oil Bomb's is measured: `docs/rules/battle_skill.md` carries every
  skill's point radius and no skill's point count. A layout naming any other
  substance is refused by name, which is a stated boundary rather than a
  silently wrong bound. That the refusal has never fired is a property of the
  rules rather than of the format: oil lasts two rounds and every other area
  lasts one, so every other area is gone before the round that would record it
  opens.
- `control_points` contains exactly two ordered integer points in the owning
  side's local frame. The first is the skill start point and the second fixes the
  release direction. It is named apart from `positions` because `grid_rows` is
  keyed over a different sequence: the seven generated centers, not these two.
  Build 2259's `CalculateAttackPositions` line branch expands them into seven oil
  centers. Only these two endpoints are integers. The step length divides the
  path magnitude by six through a fixed-point square root, so the five
  intermediate centers land on fractional Q32.32 values and cannot be written as
  layout coordinates. Reusing the same native `FixedMath` primitives restores
  them exactly instead.
- `grid_rows` is an optional map keyed by the native zero-based generated-point
  index, which for oil is `0..=6`. If the map is omitted or empty, all seven points are active as
  complete 30 m circles. If it is non-empty, its key set is the complete set of
  surviving points: an absent key means that point was intercepted or otherwise
  inactive.
- A mapped empty list means that point is active as a complete circle. A mapped
  non-empty list is the final shield-clipped `12 x 12` occupancy mask and must
  contain exactly 12 unsigned integer rows. Within each row the low 12 bits
  represent cells in increasing local x order, and rows appear in increasing
  local y order. Bits above bit 11 are rejected, and at least one cell must be
  active.

The control-point path, expanded by the 30 m radius, must overlap the battlefield
rectangle `x=[-400,400], y=[-350,350]`; edge contact is accepted. The compiler
validates this bound, the two-point arity, native index range and grid shape. It
does not accept `active_sub_effects` or `remaining_rounds`: the non-empty map's
keys already encode the active set, while remaining lifetime is not meaningful
inside a single-fight layout.

The Simulator rejects layouts with non-empty `terrains` before constructing a
simulated battle. Adapter execution reproduces the build-2259 line branch with
native `FVector3`/`FPoint` operations, adds only the declared active indexes through
`RangeItemSystem.AddItem`, then overwrites and reads back each optional
`GridBlockInt` mask. Direct GRBR decoding is exposed as
`mechcore_document::retained_from_grbr_round`; replay recording uses the
independent live `RangeItemSystem` enumeration path and groups items by provider.

### `battle_skills`

`battle_skills` describes the battle skills owned and released by one side in
the current round. “Battle skill” is the public layout term; native runtime
objects and operations may continue to use `CommanderSkillData` and
“commander skill”. The supported catalog is limited to position-targeted
skills that affect combat and can occur in standard 1v1 matches:
[English index](../../rules/battle_skill.md) / [简体中文索引](../../rules/battle_skill.zh.md).
Skills requiring a unit or construction target and configurations absent from
standard 1v1 are outside this layout contract.

```yaml
battle_skills:
  - type: missile_strike
    positions:
      - x: 0
        y: -100

  - type: mobile_beacon
    positions:
      - x: -100
        y: -150
      - x: 0
        y: -100
      - x: 100
        y: -50
```

Each entry has exactly two fields:

- `type` is the lower `snake_case` form of the skill's English in-game name;
  the adapter resolves it to the canonical native `CommanderSkillData` ID in
  the battle-skill index, so a native skill ID does not appear in the layout.
- `positions` is an ordered array of exact side-local coordinates. It remains
  an array when the skill requires only one coordinate. Fields such as
  `position_1`, `position_2`, or `position_3` are not part of the schema.

Each `x` and `y` is an `i32`; decimal values, including `10.0`, are rejected.
Battle-skill positions do not inherit formation grid-alignment or footprint
rules. One-position skills use a target position. Two-position skills use the
ordered pair `[start_position, end_position]`; the second coordinate is the
concrete skill endpoint, not a direction vector. `mobile_beacon` uses three
ordered positions: the first selects affected units and starts the path, the
second ends the intermediate segment, and the third is the final endpoint.
Position order must be preserved through the side-local to world-coordinate
transform and native release action.

The order of the entries themselves is also semantic, and a normalizing
consumer must never sort `battle_skills`. One side's skills draw from a single
`GRRandom` that `FightTeam` holds at field offset `0x68`, reached from the
fight-side `CommanderSkillManager` through `teamController.fightTeam.random`
and consumed by `CalculateAttackPositions`, so scattering skills take their
values in release order. Two properties bound the effect: the stream belongs to
one team, so blue's order and red's order are independent, and
`BattleSystem.OnEnterDeployment` resets it every deployment, so nothing carries
across rounds and a single-round layout still reproduces the battle. Formation
placement is the deliberate contrast: `MechPositionManager` builds a fresh
`GRRandom` per call, so declaration order does not affect where a formation
lands.

The order that matters is the order the skills are released, not the order the
player acquired them. Only the first is something a player varies within a
round, so the distinction decides whether this array is an action or a record
of fixed state. Every collection on the consuming path is built per release: a
release action reaches `ReleaseSkillAtFightPhase`, which appends one controller
to `releaseControllers` with `AddWithResize`, and `CSRC_Common.OnFightStart`
computes positions per controller in that list order. The owned-skill list,
which `CommanderSkillManager` keeps separately at field offset `0x28`, is never
iterated on that path; the by-skill lookup `TryGetReleaseCommanderSkillData` is
reached only from cancelling and clearing; and the deferred deployment handler
`OnPlayerReleaseSkillAtPreparePhase` is unimplemented, so a controller is
created when the player releases rather than replayed at fight start.

That last step is a negative result read from an indexed call graph rather than
a loop read directly, and no capture can currently separate the two orders,
because the executor provisions and releases in one pass over this array.
Distinguishing them by experiment would need an Adapter that can order
provisioning and release independently, which is not worth building for this
question alone. The consequence for the schema is that this array's order is
the release order, and that acquisition order is not layout state: a skill slot
or acquisition index would record something no outcome depends on.

`skill-order-orbital-first.yaml` and `skill-order-lightning-first.yaml` hold
the same pair of releases at the same two positions and differ only in which is
declared first, over a twelve-Crawler block that both circles cover.
`scripts/skill-release-order.mcscript` records three battles from them. Under
build `1.11.1.3.2259` and seed `20260907`, the same order recorded twice gave
byte-identical hashes at 415 ticks, while the swapped order diverged at tick 63
and ended at 416. They are not in the regression manifest, because its offline
reader simulates every case and the Simulator has no battle-skill feature slice
yet.

Before any mutation, the compiler applies the build-pinned geometry and map
rule from the battle-skill index against the `800 x 700` battlefield bound
`x=[-400,400], y=[-350,350]`. Circle and line footprints use their documented
effective range or full width; random-circle overlap includes both the outer
distribution radius and the sub-effect radius. In side-local coordinates the
enemy tower centers are always `(-140,170)` and `(140,170)`, so the same strict
tower-distance check applies to blue and red without an early coordinate
rotation. These static checks reject a known-invalid layout before mutation;
the later native check remains authoritative.

The compiler resolves the configured skill before mutation and checks the
build-pinned static position count. After provisioning the runtime skill, it
also requires `positions.len()` to equal that object's
`GetEffectPositionCount()`. The runtime value is authoritative because
historical game builds may expose a different position count for the same named
skill. A mismatch is a layout validation failure; the compiler must not
truncate, duplicate, or synthesize positions. It then validates every position
with the native `CanReleaseCommanderSkill` path, releases the skill once through
`PAD_ReleaseCommanderSkill`, and verifies the resulting skill state and exact
ordered positions through authoritative readback.

## What applying a layout must guarantee

These are obligations on any executor, not a description of one.
[adapter.md](../adapter/adapter.md) specifies the operation that meets them
here, and nothing about that operation changes what this document means.

Applying a layout is fail-closed:

1. The game must be in a fresh deterministic round-one Training Ground
   deployment, and `round` must satisfy the ambush/travelling rules above.
2. Both sides must exist and the adapter must be able to select each side
   explicitly.
3. Types, type-specific fields, side-local coordinates, static battle-skill
   position counts, known deployment footprints, applicable footprint grid
   alignment, and applicable deployment collisions are validated before
   mutation. After provisioning a battle skill, its runtime position count and
   every target position are checked natively before that skill is released.
4. Mutations are executed in document order within each array.
5. Every mutation is followed by authoritative native readback.
6. A rejected action, missing catalog entry, ambiguous tower, transport error,
   or readback mismatch stops the application. Formation failures identify the
   declared `type` and `position`; battle-skill failures identify the declared
   `type` and ordered `positions`. Mutations are never retried automatically.
7. Success means that the game is in the requested activation-round deployment
   and both sides match all state defined in this document; an accepted native
   action alone is insufficient.

The layout is a complete desired-state description, not a patch. Implementations
must start from a fresh match or prove that undeclared state is at its
baseline. They must not silently retain an extra Officer, technology, tower
level, or active Energy Tower effect from an earlier layout.

## Excluded fields

The following names are intentionally absent:

- `unit_modifications`: unit modifications are native Officer entries and live
  in `techs.officers`.
- `research_blueprints`: a blueprint's fight-visible product is an Officer, and
  it lives in `techs.officers`.
- `reactor_core` and `supply`: resource provisioning is an executor concern.
- `opening_techs` and `reinforcement_techs`: a layout states the state that
  holds, never the route taken to reach it, and Officer acquisition source does
  not change the resulting state in `techs.officers`. For the same reason
  `choose_opening` and `choose_reinforcement` are not a way to write
  `techs.officers`: they take transient candidate indices, and such an index
  does not denote the same thing twice.

## Unresolved

**Does the schema bound `round`?** An executor refuses a round it cannot stage
inside one budget, and [adapter.md](../adapter/adapter.md) is explicit that the
limit is its own rather than a schema rule. Nobody has decided whether a layout
at round 40 is a valid document no executor can currently apply, or not a
document at all.

**Can a layout describe a moment other than the end of deployment?** Every
field here settles at deployment, and the projection from a state produces that
moment. Whether a mid-fight position is the same document carrying more fields,
or a different document, is open.

**Do `seed` and `map_id` belong in a state document?** This document already
argues that a seed is an argument of the battle rather than a state field, and
that the layout only supplies a default the call site may override. The same
argument fits `map_id`, which is currently an ordinary field. Either both are
arguments that a layout may default, or the reasoning about the seed needs
narrowing.
