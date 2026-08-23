# Unit value configuration

[简体中文](unit-rules.zh.md)

## Scope

`mechcore.unit` describes the baseline values of exactly one unit. Each unit
has one YAML file named `<type_name>.yaml`. A unit file does not contain the
simulation clock, coordinate precision, RNG contract, layout, technologies,
statuses, equipment, or research-specific diagnostics.

The current files are:

- [`marksman.yaml`](../config/units/marksman.yaml)
- [`arclight.yaml`](../config/units/arclight.yaml)

## File shape

```yaml
schema: mechcore.unit
type_name: marksman
unit_type_id: 2
domain: ground
max_life: 1622
collision_radius: 8
move_speed: 8
rotate_speed: 70
independent_aim: false

attack:
  attack_type: direct_projectile
  target_domain: both
  damage: 2329
  range: 140
  interval: 3.1
  interval_offset: 0.6
  release_delay: 0.6
  projectile_speed: 500
  effect_radius: 0
```

Unknown fields are rejected. `type_name` and `unit_type_id` must each be unique
within a directory. Unit files carry neither a schema version nor a game-build
version, and loading performs no version-match check.

## Conservative common fields

| Field | Meaning |
| --- | --- |
| `type_name`, `unit_type_id` | Layout name and stable MCFR type ID. |
| `domain` | The unit is `ground` or `air`. |
| `max_life` | Baseline maximum life. |
| `collision_radius` | Movement, range and projectile-impact boundary in m. |
| `move_speed`, `rotate_speed` | Movement and body rotation speed in m/s and deg/s. |
| `independent_aim` | Whether weapon aim is independent of body orientation. |
| `attack_type` | Currently `direct_projectile` or `area_projectile`. |
| `target_domain` | The attack accepts `ground`, `air` or `both`. |
| `damage`, `range` | Baseline damage and attack range; range uses m. |
| `interval`, `interval_offset` | Attack interval and deterministic random offset in s. |
| `release_delay` | Total action-start to projectile-release delay in s. |
| `projectile_speed`, `effect_radius` | Projectile speed and effect radius in m/s and m; direct projectiles require zero radius. |

The schema does not pre-abstract card dimensions, slots, flat damage reduction,
or separate backswing/cooling phases that this scene does not consume. A field
is added only after a mechanism is shown to participate in a state transition.

Configuration boundaries use metres, seconds and degrees. Loading requires
spatial values to quantize exactly to 1 mm, time values to 0.0005 s, and
rotation speed to 0.001 deg/s. A value outside that precision is rejected.

Arclight therefore stores `interval: 0.9`. The old kernel's `1799` was the
lossy projection of a Q32 fixed-point `0.8999999999... s` value onto a
2,000-units-per-second clock, not the unit's design-level interval. Nearest
rounding to the 0.05 s logic step maps both representations to 18 steps.

## Loading and determinism

The embedded unit files are used by default. A directory may be selected
explicitly:

```text
mechcore sim layout.yaml --config config
```

MCFR `rules_fingerprint` hashes only unit files referenced by the current
layout, canonically ordered by `type_name`; unrelated files in the directory do
not change the scenario hash. The same layout, unit configs and seed must
produce identical S/E semantic hashes.

Simulation timing, coordinate units, RNG algorithm, reference game build and
update order are kernel metadata recorded in MCFR durable context. They are not
unit configuration and do not reject a unit file.

## Current kernel boundary

`mechcore sim` currently accepts one level-one, single-member projectile unit
per side. Technologies, equipment, tower modifiers, battle skills and rotated
formations are rejected. The schema describes common unit values; it does not
claim native parity for arbitrary units or field combinations.
