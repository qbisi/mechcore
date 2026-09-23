# Unit value configuration

[简体中文](unit-rules.zh.md)

## Scope

`mechcore.unit` describes the baseline data of exactly one Formation Unit. Each
unit has one YAML file named `<type_name>.yaml`. The configuration does not
contain the simulation clock, numeric precision, RNG implementation, layout,
technologies, statuses, equipment, or research-only diagnostics.

The files under [`config/units`](../../../config/units) cover the 23 ordinary,
non-Huge Formation Units in the current P0 build. Unknown fields are rejected;
`type_name` and `unit_type_id` must each be unique within one configuration
root. Unit files carry neither a schema version nor a game-build field. The
top-level `config.yaml` carries `game_build`, without version matching.

## File shape

```yaml
schema: mechcore.unit
type_name: marksman
unit_type_id: 2

formation:
  members: 1
  slot_size: 20
  footprint: {width: 20, depth: 20}

domain: ground
max_life: 1622
collision_radius: 8
move_speed: 8
rotate_speed: 70
has_body: true
independent_aim: false

attack:
  base_damage: 2329
  min_range: 0
  range: 140
  attack_half_angle: 20
  targets: {ground: true, air: true}
  lock_target: true
  quick_switch_target: true
  timing:
    interval: 3.1
    interval_offset: 0.6
    initial_cooldown: 0
    prepare: 0.5
    attack_point: 0.1
    backswing: 0
    cooling: 0.2
  splash_radius: 0
  weapons:
    mode: normal
    count: 1
    per_skill: 1
  melee: false
  path:
    type: projectile
    count: 1
    release_interval: 0.2
    speed: 500
    target_offset_radius: 0
    evenly_allocate_targets: false
    extra_search_range: 0
    pre_flight_height: 0
    simulated_motion: false
    interceptible: false
    max_life: 0
```

## Conservative common fields

| Field | Meaning |
| --- | --- |
| `type_name`, `unit_type_id` | Layout name and stable MCFR unit type ID. |
| `formation.members` | Number of native members created for one Formation. |
| `formation.slot_size` | Native member-grid slot size used to derive row and column counts. |
| `formation.footprint` | Native card base width and depth used to generate member positions. |
| `domain` | Whether the unit is `ground` or `air`. |
| `max_life` | Unmodified level-1 maximum life per member. |
| `collision_radius` | Native member radius used by movement, range and hit tests. |
| `move_speed`, `rotate_speed` | Member movement and body rotation speed. |
| `has_body` | Whether native `MechData` exposes a separate mech body. |
| `independent_aim` | Whether the main weapon receives an aim direction generated independently of the mech body direction. In the current P0 catalog it is present exactly for units with a mech body; omission means not applicable, not `false`. |
| `base_damage` | Unmodified level-1 mech base damage before path-specific attack-count multipliers. |
| `min_range`, `range`, `attack_half_angle` | Native engagement distance bounds and effective main-skill half-angle. |
| `targets`, `lock_target` | Native ground/air target acceptance and projectile/skill target locking. |
| `quick_switch_target` | Whether a periodic search may replace a still-alive attack target without first leaving the active attack state. Dead or invalid target replacement follows a separate native branch. |
| `timing.*` | Native attack interval, random offset, initial cooldown, prepare, attack point, backswing and cooling phases. |
| `splash_radius` | Native base effect radius; zero means no area effect. |
| `melee` | The main skill's `SkillData.isMeleeAttack`. The fight's melee branches read it, and the Melee and Ranged targeting categories are answered from it. |

Formation rows and columns are derived from `members`, `slot_size`, and
`footprint`; jitter constants, member RNG, update order, and identity allocation
are kernel mechanisms. They are not duplicated in unit YAML. The Adapter assigns initial unit identities after
sorting by team, world `z`, then world `x`; native member creation order is not
the MCFR identity order.

`independent_aim` remains meaningful for units with a mech body. For bodyless
units there is no body direction to compare against, so the field must be
absent. In the current P0 data every applicable value is `false`; this does not
create a default for future units or weapon modes.

## Weapon topology and attack path

Weapon scheduling is orthogonal to the effect path:

- `weapons.mode` is `normal`, `group`, or `standalone`.
- `count` is the completed native weapon count and `per_skill` controls how
  many weapons build one `FightSkill`.
- `fusillade` and `allow_same_target` are required only for `group`; no default
  may substitute for missing native data.
- `rotation_speed` is present only when the native skill supplies a weapon
  rotation speed distinct from the member body; Wraith stores `90` while its
  body-level `rotate_speed` is `120`.

The `path` tagged union has four variants:

- `projectile`: count, release interval, movement and target-offset data,
  interception flag, and projectile life;
- `direct`: direct effect;
- `laser`: attack-count damage multipliers;
- `control_beam`: warmup attack count and warmup damage multiplier.

Single versus multi-projectile behavior comes from `path.count`; melee versus
non-melee behavior comes from `melee`, which every path has; Group/Fusillade
remains in weapon topology. Combination-shaped types such as `grouped_projectile` are not
part of the configuration schema.

## Units and numeric boundary

Configuration values use metres, seconds, and degrees without unit suffixes.
Spatial values must quantize to 1 mm, time values to 0.0005 s, and angle values
to 0.001 degree. Dimensionless laser/control multipliers must be positive and
finite. The native Q32 representation and operation order remain kernel-owned.

Arclight therefore stores `interval: 0.9`. The earlier `1799` value was a lossy
projection of the native Q32 value onto 2,000 internal time units per second;
it was not the design-level interval. Timing phases stay separate because the
native state machine quantizes and consumes them separately.

## The configuration a simulation reads

The configuration travels with the binary: `config.yaml`,
`training_ground.yaml` and one file per unit under `units/` are compiled in. A
simulation reads no configuration from disk, so a binary simulates the build it
carries and nothing else, wherever it runs.

The schema loads every P0 unit path, and the simulator rejects a unit whose
formation generation or native attack path it does not implement. A renamed
config with the same behaviour stays valid; an unimplemented unit, or any
change to a behaviour field, fails closed.

**Loading a config is not a claim of simulation parity.** The schema describes
what a unit is, and whether a kernel reproduces it is a separate question with
a separate answer.

## Normal form

A unit lives in exactly one file named `<type_name>.yaml`. Across the files a
binary carries, `type_name` and `unit_type_id` are each unique.

The format has no ordered collection, so there is nothing to canonicalise
inside a file: every value is a scalar or a fixed-key mapping, and two
configurations describing one unit differ only if a value differs.

The absence of ordering is itself the rule worth stating, because file order
carries no meaning anywhere downstream. Formation rows and columns derive from
`members`, `slot_size` and `footprint`. Native member creation order is not MCFR
identity order: the adapter assigns initial identities after sorting by team,
world `z`, then world `x`.

## Excluded fields

The configuration describes one unit's baseline and nothing about the machinery
that runs it.

- **The simulation clock, numeric precision and RNG implementation.** These are
  kernel-owned, and the native Q32 representation and operation order with them.
- **Layout, technologies, statuses and equipment.** All of these modify a unit
  at runtime. A baseline that already had them folded in could not be a
  baseline.
- **Jitter constants, member RNG, update order and identity allocation.** Kernel
  mechanisms, deliberately not duplicated per unit.
- **Research-only diagnostics.**
- **A schema version, and a per-unit game build.** `config.yaml` carries
  `game_build` for the root, and no version matching is performed against it.
- **Combination-shaped path types.** `grouped_projectile` and its relatives are
  not schema types. Single versus multi-projectile comes from `path.count`,
  melee versus non-melee from `melee`, and Group or Fusillade from weapon
  topology.

## Unresolved

**Should an absent field ever mean something other than its default?**
`independent_aim` is the only field where it does: omission means the unit has
no mech body to compare an aim direction against, which is not the same claim as
`false`. Every other absent field simply takes its default. Either this is a
pattern the schema endorses and should name, or `independent_aim` wants an
explicit third value.

**Should `game_build` be matched rather than recorded?** A root states the build
it was extracted from and nothing checks it, so a configuration from one build
loads silently against a kernel bound to another. Enforcing it would make the
mismatch loud at the cost of blocking deliberate cross-build experiments.

**Should a unit file carry a schema version?** It carries `schema:
mechcore.unit` as an identifier with no version. A field that gains a meaning
later cannot then be told apart from one written before the change.
