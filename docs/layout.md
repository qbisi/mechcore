# Training Ground layout definition

## Scope

`layout.yaml` describes the desired state of both players in a deterministic
Training Ground scene. It describes game state, not the actions used to create
that state.

The top-level `round` selects the deployment round in which the complete layout
becomes active. The document does not contain `reactor_core`, `supply`, game
startup parameters, capture settings, or exit behavior.

The same layout is also the input to the bounded deterministic simulator:

```text
mechcore sim layout.yaml [--seed <i32>] [--output battle.mcfr] [--config <directory>]
```

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

      - type: defensive_wall
        x: 140
        y: -105

      - type: interceptor
        x: 5
        y: -95

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

## Activation round

`round` is a required integer in `1..=15`. It names the Training Ground round
whose deployment phase is returned by a successful `apply_layout` call. Earlier
rounds are setup rounds owned by the adapter; callers do not submit or observe
separate partial layouts. These empty setup rounds are advanced quickly, so the
adapter's 55-second total layout timeout also covers round 15.

The native ambush zones become available from round 2. The layout rules are:

- round 1 cannot contain an ambush-zone unit;
- round 2 cannot contain an ambush-zone unit with `travelling: false`;
- round 3 and later accept both travelling and non-travelling ambush units.

These rules follow from native placement timing. An ambush unit first deployed
in the activation round has travelling state. A requested non-travelling ambush
unit must instead be deployed in the immediately preceding round and carried
through that round's battle. For activation round 2, the preceding round is
round 1, where ambush deployment is still locked; this is why such a layout is
illegal until activation round 3.

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
`battle_skills.positions`. A 180-degree transform does not change a formation's
`rotated` boolean. Authoritative native readback remains in world coordinates;
the adapter verifies that world state against the compiled position and returns
the layout-local position publicly.

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

`formations` is a union of units, constructions, and contraptions. Each entry
uses one semantic `type` instead of exposing a catalog category and numeric ID:

```yaml
- type: marksman
  x: 0
  y: -50
```

- `type` is the lower `snake_case` form of the unit, construction, shield,
  interceptor, or missile's English in-game name. It selects both the native
  catalog and the valid type-specific fields.
- `x` and `y` are required exact signed coordinates in the owning side's fixed
  local frame defined above, not native world or screen pixels.

A formation has no user-defined identifier. The adapter reports a failed
formation by its `type`, `x`, and `y`; a successful unit placement may return
the native runtime `unit_index`, and a successful construction placement may
return `construction_index`, but those transient values are not layout state.

The layout compiler resolves every unit, construction, and interceptor
deployment footprint and rejects positive-area overlap before any game
mutation. Exact edge contact is legal. In a main deployment region,
`rotated: true` exchanges a unit's footprint width and height. The left/right
ambush regions have a native quarter-turn orientation, which exchanges the
effective world footprint once more. Collision checks use this region-aware
footprint and compiled world positions, so units, constructions, and
interceptors share one collision space within a side and across `blue` and
`red` after the red-side 180-degree transform. Shields and missiles do not
participate in formation collision checks.

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
the formation footprint rules above. Neither has a modulo-10 requirement or a
collision footprint. A missile's center must lie in
`x=[-300,300], y=[-310,-10]`. A shield has a 70 m radius, and the native check
requires its complete edge to be strictly inside the same own-side region. For
integer layout coordinates, its center must therefore lie in
`x=[-229,229], y=[-239,-81]`.

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

Any unknown formation type remains rejected fail-closed; the compiler does not
infer a size from combat-member radius.

Tracked negative fixtures cover the spatial rejection cases:

- `crates/layout/tests/fixtures/invalid-footprint-boundary.yaml`: center
  inside, footprint edge outside;
- `crates/layout/tests/fixtures/invalid-unit-collision.yaml`: an unrotated
  `50 x 20` Sledgehammer overlaps a rotated `20 x 50` Crawler;
- `crates/layout/tests/fixtures/invalid-unit-construction-collision.yaml`: a
  `20 x 20` Marksman overlaps a `60 x 10` Defensive Wall.

Numeric formation IDs are adapter details and must not appear in a layout. The
following values form the closed public `type` vocabulary:

- Units: `abyss`, `arclight`, `crawler`, `fang`, `farseer`, `fire_badger`,
  `fortress`, `hacker`, `hound`, `marksman`, `melting_point`, `mountain`,
  `mustang`, `overlord`, `phantom_ray`, `phoenix`, `raiden`, `rhino`,
  `sabertooth`, `sandworm`, `scorpion`, `sledgehammer`, `steel_ball`,
  `stormcaller`, `tarantula`, `typhoon`, `void_eye`, `vortex`, `vulcan`,
  `war_factory`, `wasp`, and `wraith`.
- Constructions: `defensive_wall`, `anti_armor_turret`,
  `rapid_fire_turret`, and `magnetic_barrier`.
- Contraptions: `shield`, `interceptor`, and `missile`.

For example, a unit and a construction are unambiguous without a separate
category field:

```yaml
- type: fortress
  x: 0
  y: -50
- type: defensive_wall
  x: 140
  y: -105
```

An implementation must resolve each item as a discriminated union. It must
reject unknown fields and `type` values, missing required fields, and fields
that do not belong to the selected type.

#### Unit

```yaml
- type: marksman
  x: 0
  y: -50
  equipment: 13030001
  travelling: false
```

The adapter resolves `type` to a native `CardData.ID`; neither that ID nor the
runtime formation index is public layout state. `level` is the optional
displayed level, defaults to `1`, and must be in `1..=9`. `rotated` is an
optional boolean, defaults to `false`, and declares the native unit-orientation
flag; the owning map region's facing still contributes to its world footprint.
`equipment` is an optional positive native `EquipmentData.ID`. A unit has at
most one equipment slot, so this field is singular rather than an array.
`travelling` is an optional boolean and defaults to `false`. It has semantic
effect only for an ambush-zone unit: `true` requires that the unit be first
deployed during the activation round, while `false` requires deployment in the
immediately preceding round. `travelling: true` is invalid outside the ambush
zones. Constructions and contraptions must not declare `equipment` or
`travelling`.

The executor adds the unit, obtains its runtime unit index, moves it to the
declared position and orientation, and verifies type, level, position, and
rotation through authoritative unit readback. When `equipment` is present, it
then adds one copy from the runtime catalog to the current side's Training
Ground inventory through `MAD_AddEquipment`, uses the existing native
`PAD_UseEquipment` action on that unit, and requires authoritative equipment
ownership readback. Available IDs and effects are listed in the
[Equipment index](equipment.md) ([中文](equipment.zh.md)).

#### Construction

```yaml
- type: defensive_wall
  x: 140
  y: -105
```

A construction accepts no `level`, `rotated`, `equipment`, or `travelling`
field because the current native release action takes none of these values. The
executor resolves its English type to the native `ConstructionData`, performs
the placement check, releases it once, and verifies its type and exact position.

#### Contraption

```yaml
- type: interceptor
  x: 5
  y: -95
```

`shield`, `interceptor`, and `missile` each resolve directly to their native
contraption kind. A contraption accepts no `level`, `rotated`, `equipment`, or
`travelling` field, and none of these three types requires an extra position.
The executor performs the native placement check, releases the contraption once,
and verifies its type and exact position through authoritative recorder
readback.

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
   alignment, and applicable formation collisions are validated before
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
effects, tower strengthening, all supported formation placements, side
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
   formation, fixed tower, required blueprint, Energy Tower skill, and battle
   skill through each side's runtime catalog before mutation;
3. clear both sides in round 1 without placing combat formations;
4. start each earlier empty round and wait for the game to advance it naturally,
   polling authoritative status until the next deployment is stable;
5. in the round immediately before activation, deploy ambush units whose
   requested activation state is `travelling: false`; if that round enters
   battle, finish it immediately with the private Training Ground process-state
   action;
6. in the activation round, apply every remaining unit and construction,
   Officers, unit technologies, Research Center and Energy Tower state, shields,
   missiles, interceptors, and battle skills, then return while the game is
   still deploying.

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
sides. It rejects tower levels outside `0..=2`, unknown formation footprints,
formation collisions where applicable, and contraptions outside their target
regions. Unsupported state is never silently ignored.

The compiler owns the activation round and total formation count. A successful
`apply_layout` response includes them as `round` and `formation_count`, plus the
completed `stages` and `skipped_rounds`; callers do not recount the input or
returned arrays to establish completeness.

The clear phase invokes both `MAD_ClearOfficer` and `MAD_ClearTechnology` for
each side. Application also rejects a declared Officer or technology that is
already present immediately before its add action.

The live regression script reads YAML and sends the resulting object to this
operation:

```sh
scripts/smoke_training_ground.py tests/layouts/six-unit.yaml
scripts/smoke_training_ground.py tests/layouts/construction-battle.yaml
scripts/smoke_training_ground.py tests/layouts/interceptor-battle.yaml
scripts/smoke_training_ground.py tests/layouts/shield-missile-battle.yaml
scripts/smoke_training_ground.py tests/layouts/crawler-in-face.yaml
```

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
contraption centers deliberately avoid the formation modulo-10 grid. It is the
live regression sample for shield and missile target-region validation, their
exclusion from formation collisions, native recorder readback, battle
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

`start_test` takes no arguments. It always creates the Training
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
