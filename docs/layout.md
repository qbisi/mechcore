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
mechcore layout verify layout.yaml
```

`mechcore sim` returns hashes, terminal battle structure, and generation profiling
without generating MCFR storage by default. `--output` additionally serializes,
validates, and publishes an MCFR at the requested path.

Unit combat values are resolved from the typed, one-file-per-unit
[unit configuration contract](unit-rules.md), not stored in the layout. The current
simulator closure is intentionally narrower than native layout application;
unsupported formations or mechanisms are rejected before recording begins.

## Document shape

A layout contains exactly two player sides, `blue` and `red`. Persistent Officer
and unit-technology state is grouped under each side's `techs` object.

```yaml
round: 3

sides:
  blue:
    techs:
      officers: [10002, 20003, 30201]
      units: [10202, 10402]

    research_center:
      strength_level: 2
      attack_level: 2
      defense_level: 1

    energy_tower:
      strength_level: 1
      range_enhancement: true
      movement_enhancement: true

    formations:
      - type: marksman
        x: 0
        y: -50
        equipment: 13030001

      - type: arclight
        x: -310
        y: 20
        travelling: true

    constructions:
      - type: defensive_wall
        index: 0
        x: 140
        y: -105

    contraptions:
      - type: interceptor
        x: 5
        y: -95

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
    research_center:
      strength_level: 0
      attack_level: 0
      defense_level: 0
    energy_tower:
      strength_level: 0
      range_enhancement: false
      movement_enhancement: false
    formations:
      - type: marksman
        x: 0
        y: -100

    contraptions: []
    terrains: []
    battle_skills: []
```

YAML field order has no semantic meaning. Examples and generated files should
nevertheless use the order above.

The `mechcore-layout` crate is the authoritative implementation of this public
shape, its static legality rules, and normalized execution plan. MCP uses its
`Layout` type for the `apply_layout` input schema and validates with the shared
compiler before contacting the game; the Adapter consumes the same plan, while
the Simulator adds only its narrower feature-support and configuration checks.
Runtime catalog availability and native readback remain Adapter-owned.

`mechcore layout verify layout.yaml` runs this shared static compiler without
starting the game or Simulator. A successful JSON report includes the normalized
seed, round, formation count, construction count, and contraption
count.

## Seed

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

- `mechcore layout verify` and every other schema consumer accept any positive
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

A unit enters native travelling state when it is first moved to the flank, and
leaves that state in a later round. Round 1 admits no ambush unit at all, so
every round 2 ambush unit is necessarily a first flank deployment and cannot
already have settled. That makes the round 2 case decidable from one layout
alone, so the compiler rejects a round 2 ambush unit that is not travelling.
From round 3 the same question needs the previous round's state, which a layout
does not carry, so both values are accepted there.

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
`battle_skills.positions` and every `terrains.positions` control point. Terrain `grid_rows` are
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
- `research_center.strength_level` defaults to `0`.
- `research_center.attack_level` defaults to `0`.
- `research_center.defense_level` defaults to `0`.
- `energy_tower.strength_level` defaults to `0`.
- `energy_tower.range_enhancement` defaults to `false`.
- `energy_tower.movement_enhancement` defaults to `false`.
- `constructions` defaults to `[]`.
- `contraptions` defaults to `[]`.
- `terrains` defaults to `[]`.
- `battle_skills` defaults to `[]`.
- A unit formation's `level` defaults to `1`.
- A unit formation's `rotated` defaults to `false`.
- A unit formation's `equipment` defaults to no equipment.
- A unit formation's `travelling` defaults to `false`.

Unknown fields must be rejected. Numeric IDs outside formation definitions must
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
  [English](officers.md) / [简体中文](officers.zh.md)
- Unit technologies:
  [English](unit_techs.md) / [简体中文](unit_techs.zh.md)

#### `techs.officers`

`techs.officers` is the set of native `OfficerData.ID` values whose persistent
effects belong to the side. It covers ordinary and opening Officers regardless
of how they were acquired.

Unit modifications are also native Officer entries. An Officer with a nonzero
`typeID` targets that unit type; it is not a separate technology kind and does
not require a `unit_modifications` field. For example, reference build 2227
uses Officer `30101` for Mass-Produced Fortress and Officer `30201` for
Range-Extended Marksman. Generic Officers use `typeID: 0` and remain in the
same array.

IDs must be unique. Their order is the deterministic application order.
Commander skills, equipment, and extra formations granted by an Officer are
not themselves Officer modifiers. Their resulting state belongs to
`battle_skills`, equipment, or formation definitions. A compiler must use the
runtime Officer catalog to detect derived state and reject duplicate
declarations.

Research Center attack and defense levels own their native Officer products.
Those derived Officer IDs must not also appear in `techs.officers`.

The deterministic implementation must add an Officer by ID through the native
Training Ground test action and verify the resulting `OfficerManager` entry.
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
represent an acquired but inactive technology. IDs must be unique, and array
order is the deterministic order of native add and activate operations.

### `research_center`

```yaml
research_center:
  strength_level: 2
  attack_level: 2
  defense_level: 1
```

The Research Center contains its fixed building-strengthening level and two
persistent, side-wide combat enhancements:

| Field | Level 0 | Level 1 | Level 2 |
| --- | --- | --- | --- |
| `attack_level` | no enhancement | attack +12% | attack +36% |
| `defense_level` | no enhancement | life +15% | life +45% |

`strength_level` must be a non-negative integer supported by the runtime tower
catalog. The executor resolves the Research Center to exactly one native
building-manager entry, strengthens it one level at a time, and verifies the
final level. A transient building-manager index never appears in the layout.

`attack_level` and `defense_level` must be integers in `0..=2`. Native
blueprint IDs are adapter details and do not appear in the layout. For the
reference game data, the execution mapping is:

- attack level 1: activate blueprint `4`;
- attack level 2: activate blueprints `4`, then `401`;
- defense level 1: activate blueprint `5`;
- defense level 2: activate blueprints `5`, then `501`.

Research products that grant an Officer or commander skill are excluded from
this definition. Those objects can be added directly through Training Ground
capabilities and belong to `techs.officers` or `battle_skills`.

### `energy_tower`

```yaml
energy_tower:
  strength_level: 1
  range_enhancement: true
  movement_enhancement: true
```

The Energy Tower contains its fixed building-strengthening level and two
side-wide effects for the current round:

| Field | `false` | `true` |
| --- | --- | --- |
| `range_enhancement` | no effect | ranged-unit attack range +15 m |
| `movement_enhancement` | no effect | all-unit movement speed +3 m/s |

`strength_level` must be a non-negative integer supported by the runtime tower
catalog. The executor resolves the Energy Tower to exactly one native
building-manager entry, strengthens it one level at a time, and verifies the
final level. A transient building-manager index never appears in the layout.

For the reference game data, `range_enhancement: true` activates Energy Tower
skill `5`, while `movement_enhancement: true` activates skill `6`. The IDs are
adapter details. Unlike `strength_level`, these two effects are cleared at the
next deployment and may be activated again in each round. The staged executor
applies the complete Energy Tower state in the activation round.

### `formations`

`formations` contains the side's unit formations. Each entry uses the unit's
semantic `type` instead of exposing its native numeric ID:

```yaml
- type: marksman
  index: 0
  x: 0
  y: -50
  exp: 12
```

- `type` is the lower `snake_case` form of the unit's English in-game name. It
  selects both the native catalog and the valid unit fields.
- `x` and `y` are required exact signed coordinates in the owning side's fixed
  local frame defined above, not native world or screen pixels.
- `index` is the stable, non-negative native unit index. Indices must be
  strictly increasing in formation declaration order and may contain gaps.
  When omitted, the compiler assigns contiguous indices from zero for existing
  hand-written layouts.
- `exp` is the unit formation's non-negative integer experience within its
  current level. The default is `0` and canonical YAML omits that default.

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
effective world footprint once more. Collision checks use this region-aware
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

- `crates/layout/tests/fixtures/invalid-footprint-boundary.yaml`: center
  inside, footprint edge outside;
- `crates/layout/tests/fixtures/invalid-unit-collision.yaml`: an unrotated
  `50 x 20` Sledgehammer overlaps a rotated `20 x 50` Crawler;
- `crates/layout/tests/fixtures/invalid-unit-construction-collision.yaml`: a
  `20 x 20` Marksman overlaps a `60 x 10` Defensive Wall.

One further negative fixture covers the round 2 ambush rule rather than a
spatial rule:

- `crates/layout/tests/fixtures/invalid-round-2-ambush-travelling.yaml`: a
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
  x: 0
  y: -50
  exp: 12
  equipment: 13030001
  travelling: false
```

The adapter resolves `type` to a native `CardData.ID`; that catalog ID is not
public layout state. `index` is the optional stable native formation index and
defaults to declaration order. `level` is the optional displayed level,
defaults to `1`, and must be in `1..=9`. `exp` is optional, defaults to `0`,
and records the formation's current-level experience. `rotated` is an optional
boolean, defaults to `false`, and declares the native unit-orientation flag;
the owning map region's facing still contributes to its world footprint.
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
[Equipment index](equipment.md) ([中文](equipment.zh.md)).

### `constructions`

```yaml
constructions:
  - type: defensive_wall
    x: 140
    y: -105
```

`constructions` is parallel to `formations` under one side and defaults to
`[]`. Each entry contains only `type`, `x`, and `y`. Native construction indices
are not layout data; an `index` field is rejected. Formation `index` is unchanged.

At prepare time, the executor first reconciles the same-seed Training Ground
opening constructions by `(type, position)`: exact matches are retained,
while entries absent from or different in the target layout are removed. At
activation it applies only missing constructions in declaration order, letting
the native action allocate its own index. There are no index placeholders or
explicit-index recreation actions. Retention, creation, and removal are checked
through `ConstructionManager` lookup/count readback. Replay capture exports
constructions sorted by `(type, x, y)`, without native indices. MCFR building IDs
are normalized independently at the capture boundary.

Both replay and Training Ground MCFR capture derive construction Building IDs
from this same index plus `FightConstruction.GetConstructionChildIndex`, joined
through `ConstructionElement.GetFightConstructions`. A wall placement may own
several Building rows: layout `index` is the per-side deployment identity, not
the global MCFR `building_id`. Native `GetBuildingIndex()` is not used to order
those construction rows, because its allocation can differ between the modes.

### `contraptions`

```yaml
contraptions:
  - type: interceptor
    x: 5
    y: -95
  - type: shield
    x: 275
    y: 20
    isairdrop: true
```

`shield`, `interceptor`, and `missile` each resolve directly to their native
contraption kind. `contraptions` is parallel to `formations` and
`constructions` under one side and defaults to `[]`. A contraption entry
requires `type`, `x`, and `y`; shields additionally accept optional boolean
`isairdrop` (default false). Other contraptions and constructions reject this
field. None of these types requires an extra position. Replay export describes
the contraptions still present at the deployment boundary, including objects
retained from earlier rounds; it is not a list of this round's release
operations. Export reads ordinary and retained airdrop shields from the full
shield collection, `TeamMineManager.GetLandMines()`, and live
`InterceptCtrGroup_Interceptor` sources. Removed missiles and destroyed
interceptors are excluded; inactive reset-next-round shields are included.
Export orders categories as shield, missile, interceptor, preserving native
order within each category. Shields retain full-list order; layout is captured
before combat and is not reordered using S(1). MCFR Shield IDs are normalized
separately and are not inferred from layout entry order.
Shield radius and maximum energy must match the selected native source's
defaults; otherwise export fails rather than silently losing state. Ordinary
shields keep their own-side radius-70 deployment bounds. Retained airdrop shields
require only their center inside `x=[-400,400], y=[-350,350]`, in side-local
coordinates; they can extend outside the deployment area. Integer coordinates
are converted directly to Q32.32 without floating-point rounding.

`isairdrop: true` means a shield already present before this battle, not a new
`battle_skills: shield_airdrop` release. It uses build-2259 `CS_EnergyShield`
(ID 800001), which is not short-lived and resets to maximum energy between
rounds. During the requested round's deployment, the executor constructs this
data source without adding commander inventory or a release record, then invokes
`AdvancedEnergyShieldSystem.Create(data, FVector3, teamController)` in layout
declaration order. It verifies full/active-list insertion, source, team, exact
position, radius, energy, and round policy. MCFR shield IDs remain normalized at
S(1); no MCFR schema or hash field changes.
For missile/interceptor objects, export projects native world X/Z onto the
layout plane; native object height is not a placement coordinate. Fractional
planar coordinates are rejected rather than rounded.
For ordinary contraptions, the executor performs the native placement check,
releases the contraption once, and verifies its type and exact position through
authoritative recorder readback.

### `terrains`

`terrains` records the active cross-round battlefield terrain owned by one side
at the start of this single fight. It defaults to `[]`. One entry represents one
original Sticky Oil Bomb release, rather than one surviving oil circle:

```yaml
terrains:
  - type: oil
    positions:
      - {x: -24, y: 11}
      - {x: 80, y: 1}
    grid_rows:
      0: [240, 1020, 2046, 2046, 4095, 4095, 1023, 511, 254, 126, 60, 48]
      1: [240, 1020, 1022, 510, 255, 127, 63, 63, 30, 30, 12, 0]
      5: [16, 28, 30, 30, 63, 63, 127, 127, 254, 510, 1020, 240]
      6: [48, 124, 126, 254, 255, 511, 1023, 2047, 2046, 2046, 1020, 240]
```

- `type` currently accepts only `oil`, the native terrain type produced by the
  cross-round battlefield Sticky Oil Bomb. It does not use the producing battle
  skill name `sticky_oil_bomb`.
- `positions` contains exactly two ordered integer control points in the owning
  side's local frame. The first is the skill start point and the second fixes the
  release direction. Build 2259's `CalculateAttackPositions` line branch expands
  them into seven oil centers. Reusing the same native `FixedMath` primitives restores the
  five intermediate centers at their original Q32.32 values without storing
  fractional coordinates in YAML.
- `grid_rows` is an optional map keyed by the native zero-based generated-point
  index `0..=6`. If the map is omitted or empty, all seven points are active as
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
`mechcore_layout::terrains_from_grbr_round`; replay recording uses the
independent live `RangeItemSystem` enumeration path and groups items by provider.

### `battle_skills`

`battle_skills` describes the battle skills owned and released by one side in
the current round. “Battle skill” is the public layout term; native runtime
objects and operations may continue to use `CommanderSkillData` and
“commander skill”. The supported catalog is limited to position-targeted
skills that affect combat and can occur in standard 1v1 matches:
[English index](battle_skill.md) / [简体中文索引](battle_skill.zh.md).
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

## Validation and execution requirements

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
   declared `type`, `x`, and `y`; battle-skill failures identify the declared
   `type` and ordered `positions`. Mutations are never retried automatically.
7. Success means that the game is in the requested activation-round deployment
   and both sides match all state defined in this document; an accepted native
   action alone is insufficient.

The layout is a complete desired-state description, not a patch. Implementations
must start from a fresh match or prove that undeclared state is at its
baseline. They must not silently retain an extra Officer, technology, tower
level, or active Energy Tower effect from an earlier layout.

## Required adapter capabilities

The adapter now has the operations required for deterministic `techs`
application: stable Officer IDs are validated, added through `MAD_AddOfficer`,
and read back through `OfficerManager`; unit technology ownership is decoded
statically, checked against the runtime catalog, added, activated, and read
back through `TechnologyManager`. Research Center blueprints, Energy Tower
effects, tower strengthening, all supported formations, constructions, and
contraptions, side
switching, and field-state clearing use their corresponding native actions and
readbacks.

`choose_opening` and `choose_reinforcement` remain normal-game selection
operations. They accept transient candidate indices and must not be used to
materialize `techs.officers`.

## Current adapter compiler

The `apply_layout` operation accepts the layout object as its complete
`arguments` value. Its current implementation supports units with optional
equipment and travelling state, the four ordinary opening constructions, all
three contraptions, both fixed-tower strengthening levels, Research Center
attack/defense levels, Energy Tower range/movement enhancements, Officers,
active unit technologies, and every position-targeted `battle_skills` type in
the build-2227 index. The compiler applies this state as one fail-closed adapter
request:

1. require round-one Training Ground deployment;
2. compile the complete layout and resolve every Officer, unit technology,
   formation, construction, contraption, fixed tower, required blueprint, Energy Tower skill, and battle
   skill through each side's runtime catalog before mutation;
3. clear both sides in round 1 without placing combat formations;
4. start each earlier empty round and wait for the game to advance it naturally,
   polling authoritative status until the next deployment is stable;
5. in the activation round, apply every unit in formation declaration order and construction,
   Officers, unit technologies, Research Center and Energy Tower state, shields,
   missiles, interceptors, retained terrains, and battle skills; after each unit's final move,
   explicitly correct and verify any mismatching travelling state, then return
   while the game is still deploying.

The adapter keeps the selected side across layout stages instead of restoring
it after every catalog or mutation pass. A stage applies the currently selected
side, switches once for the other side, and the complete layout transaction
restores `blue` only after activation is finished. Red side-local positions are
still rotated 180 degrees into native world coordinates. Each battle skill is
provisioned, checked with its authoritative runtime position count and
target-region validator, released once, and read back with its exact ordered
world positions.

The compiler accepts omitted fields and explicit baseline values described in
this document, except that `formations` is mandatory and non-empty on both
sides. It rejects tower levels outside `0..=2`, unknown deployment footprints,
deployment collisions where applicable, and contraptions outside their target
regions. It accepts structurally valid `terrains`; the Adapter restores the
currently supported build-2259 oil form during activation. Unsupported terrain
types or native readback mismatches fail closed and are never silently ignored.

The compiler owns the round and the separate formation,
construction, and contraption counts. A successful `apply_layout` response
includes them as `round`, `formation_count`, `construction_count`, and
`contraption_count`, plus the completed `stages` and `skipped_rounds`; callers
do not recount the input or returned arrays to establish completeness.
`mechcore layout verify` additionally reports `terrain_count`.

The clear phase invokes both `MAD_ClearOfficer` and `MAD_ClearTechnology` for
each side. Application also rejects a declared Officer or technology that is
already present immediately before its add action.

A script reads the YAML and sends the resulting object to this operation. The
layout carries its own seed and round, so applying it is one step:

```yaml
game: launch
steps:
  - let:
      layout: read_yaml(tests/layouts/construction-battle.yaml)
  - apply_layout: $layout
```

See [mcscript.md](mcscript.md); `mechcore shell` applies the same layout
interactively with `apply_layout <layout.yaml>`.

`construction-battle.yaml` reproduces build 2227 opening construction group 28
with the `reverse_x` transform observed in replay R002. In blue's side-local
frame, the Defensive Wall is at `(-140, -55)`, Rapid-Fire Turret at
`(140, -100)`, and Anti-Armor Turret at `(-140, -100)`. Red uses the side-local
coordinates that compile back to R002's recorded world positions. Magnetic
Barrier is supported by the compiler but intentionally absent because it is not
part of that opening group.

`interceptor-battle.yaml` places one `30 x 30` interceptor and one `50 x 20`
Stormcaller for each side. It is the live regression sample for native
placement, recorder readback, shared footprint validation, round transition,
and shutdown.

`shield-missile-battle.yaml` gives each side a Stormcaller centered at
`(5,-40)`, a shield at `(1,-81)`, and a missile at `(1,-11)`. The Stormcaller's
complete `50 x 20` deployment footprint lies inside its own radius-70 shield.
After the red-side transform, each Stormcaller starts about 51.35 m from the
opponent's missile, inside the reference missile's 100 m trigger range. Both
contraption centers deliberately avoid the deployment modulo-10 grid. It is the
live regression sample for shield and missile target-region validation, their
exclusion from deployment collisions, native recorder readback, battle
interaction, round transition, and shutdown.

`crawler-in-face.yaml` places a `50 x 20` Crawler for each side at local
`y=-20`, with its front edge exactly on `y=-10`. The native placement succeeds
only when the room retains the standard 300 m-deep round-one main regions. A
successful `apply_layout` also proves that the initial native constructions
were cleared and their manager count read back as zero before placement.

`six-unit.yaml` also exercises persistent technology state and side-wide tower
state. Sledgehammer, Marksman, Fang, Wasp, and Arclight receive their respective
Range Enhancement technologies (`10213`, `10202`, `10209`, `10206`, and
`10215`), and red receives Improved Wasp Officer `30602`. Blue strengthens its
Research Center to level 2 and activates attack level 2 plus defense level 1;
red strengthens its Energy Tower to level 1 and activates both range and
movement enhancements. The public semantic levels are compiled to native
blueprint and skill IDs only inside the adapter. Blue releases `missile_strike`
at world position `(55,60)`, the center of red's local `(-55,-60)` front Fang.
Red releases `mobile_beacon` at local positions `(-55,-60)`, `(-105,-90)`, and
`(-105,20)`, which compile to world positions `(55,60)`, `(105,90)`, and
`(105,-20)`: the selected Fang first retreats briefly away from the adjacent
Wasp and strike point, then advances on the displaced line.

`start_test` accepts the effective layout seed. Zero or omission requests a
system-random native seed. It creates the Training
Ground used by `apply_layout`, with advanced teams, reinforcements, and unit
reinforcements disabled. Native constructions remain enabled while the room is
created so that round one uses the standard 300 m-deep main deployment regions;
`apply_layout` then clears every initial construction through
`MAD_ClearConstruction` and requires a zero-count manager readback before
placing the requested formations. `start_test` configures both
native `PlayerAgent` objects with `FirstRoundSupply=10000` and
`MaxRoundSupply=10000` before `CreateHost`, and requires exact getter readback.
`apply_layout` therefore performs no hidden economy mutation. Native layout
actions still run their normal affordability checks and deduct their exact
costs. Supply remains absent from the layout
because the schema does not define terminal economy values.

## Excluded fields

The following names are intentionally absent:

- `unit_modifications`: unit modifications are native Officer entries and live
  in `techs.officers`.
- `research_blueprints`: the public layout uses semantic attack and defense
  levels instead of native blueprint IDs.
- `tower_strengthening`: each fixed tower owns its `strength_level` directly.
- `reactor_core` and `supply`: resource provisioning is an executor concern.
- `opening_techs` and `reinforcement_techs`: Officer acquisition source does
  not change the resulting state in `techs.officers`.
