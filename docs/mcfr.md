# MCFR deterministic recording contract

[简体中文](mcfr.zh.md)

## Status

This document defines the baseline logical contents of `S` and `E` shared by
native capture, simulation, comparison, and playback. Schema version 1 has a
reference Rust implementation in `mechcore-mcfr`. Event-specific typed payload
schemas and the final columnar projection of world objects remain open.

## Purpose

MCFR records one continuous combat round in a unified world model. The same
logical format must be produced from two paths:

- the game adapter captures an authoritative native battle;
- the Rust kernel deterministically simulates a battle from the same initial
  state and random state.

A Godot or Three.js player must be able to restore the visible battle timeline
from the resulting file. Native and simulated results must also be comparable
without source-specific commands or specialized comparison formats.

MCFR does not describe deployment economy, supply, reactor-core state,
recruitment, settlement, or other cross-round state.

## Deterministic model

The logical transition contract is:

```text
D + S(0) -> S(1..n) + E(0..n-1)
```

where:

- `D` is the durable deterministic context;
- `S(t)` is the authoritative world snapshot at one logic boundary;
- `E(t)` is the ordered event log for the transition from `S(t)` to
  `S(t+1)`;
- `I(t)` is optional, temporary research instrumentation and is not part of
  the formal deterministic contract.

`S(0)` must be captured at the fighting-entry boundary before the first combat
logic update. A finalized recording must identify the single frame where the
battle changes from fighting to over; phase is not repeated in every frame.

Snapshots are the contiguous sequence `S(0)..S(n)`. The sequence ordinal is
the zero-based logic step, so a separate `frame_index`, `logic_tick`, or
per-frame `time_seconds` field is not stored. `D` defines the fixed logic-step
duration. The fighting-to-over boundary is stored once as top-level
`terminal_step`. A source-native clock may be retained as non-canonical
evidence, but it does not enter `S` or formal hashes.

## Durable deterministic context

`D` contains every input required to reproduce the battle but not naturally
expressed as a per-frame world snapshot. At minimum, it binds:

- the MCFR schema version and its canonicalization rules;
- the game build;
- logic-step timing and numeric conventions;
- the fighting-entry random seed bundle or equivalent RNG states;
- stable identity and update-order contracts;
- any external command that is allowed after fighting begins.

A displayed round seed is insufficient when random streams have already been
consumed before `S(0)`. Reproduction must begin from the same effective RNG
state.

If combat is intended to be closed after `S(0)`, no unrecorded external action
may affect it. Any permitted action becomes durable context rather than
temporary research input.

## Consumer levels

The format supports three progressively stronger consumers:

| Consumer | Required logical tracks | Requirement |
| --- | --- | --- |
| Godot or Three.js playback | `S` | Restore the visible world at every logic boundary. |
| Native/simulation alignment | `S + E` | Compare both visible evolution and normalized discrete mechanisms. |
| Mechanism research | `S + E + I` | Diagnose hidden inputs and intermediate native behavior. |

All consumers also read ordinary container metadata. Alignment and research
bind their tracks to `D` through `scenario_hash`; the table lists only the
timeline tracks each consumer must interpret.

A minimal player may ignore `E`. A richer player may consume events for
transient attack, impact, status, and lifecycle effects that do not survive in
a tick-end snapshot.

## Unified world snapshots

`S` is source-neutral. Native pointers, runtime object addresses, adapter
bookkeeping, and simulator-private types do not enter it.

The current candidate world boundary contains:

- units;
- projectiles;
- buildings, including constructions and contraptions;
- independent area shields;
- dynamic terrain or persistent spatial regions;
- persistent statuses.

Team and formation are ownership and identity attributes on concrete world
objects, not per-frame container entities. A unit is directly associated with
its team and formation. Personal shields remain unit state; an area shield is
independent because it has its own spatial boundary, energy, and lifecycle.

Status is the shared representation for persistent buffs and debuffs,
including technology-disable effects. Specialized `buffs` and
`technology_disabled` fields are not maintained in parallel.

The baseline snapshot admits a field only when it satisfies at least one of
these rules:

1. it is persistent state needed to advance the next logic step;
2. it is an authoritative result produced by the kernel;
3. it is required to restore the visible battle;
4. it identifies a world object or a stable relationship between objects.

Physical storage may deduplicate immutable values, but the logical snapshot
seen by readers must remain complete.

### Required baseline state

Immutable identity, type, ownership, and geometry may be stored once in entity
metadata instead of repeated physically at every logic step. They remain part
of the logical snapshot.

| World kind | Required logical state |
| --- | --- |
| Unit | `unit_id`, `team_id`, `formation_id`, `unit_type_id`, optional `parent_unit_id`, domain; position, body rotation, independently changing aim pose, velocity and motion state; collision radius; life, maximum life, alive, active, targetable and visibility state; personal-shield active/enabled state and current/maximum energy when present. |
| Projectile | `projectile_id`, team, owner/source and projectile type; position and independently changing orientation or velocity; target object reference; cached target position and radius; active state and current/maximum projectile life when applicable. Immutable movement, targeting and damage rules belong to `D` or entity metadata. |
| Building | `building_id`, team and formation when applicable, building type; position, rotation and collision boundary; life, maximum life, alive, active/available, targetable and collision-enabled state. |
| Area shield | `shield_id`, team and owner/source; position, radius and height when applicable; current/maximum energy and active state. |
| Dynamic terrain | `terrain_id`, team and source, terrain type; position and region shape or grid; active state and elapsed/remaining lifetime. Immutable effect rules belong to `D` or entity metadata. |
| Status | `status_id`, status type, source and target; stack count; elapsed, remaining and maximum duration; active state; a periodic-effect clock when the status owns one. Immutable status rules belong to `D` or status metadata. |

A component field is present only for world kinds that actually implement that
component. Absence means the object does not have the component; `unknown` and
`unsupported` must use distinct representations.

Target-search caches, controller clocks, solver buffers, and other disputed
state do not enter this baseline merely because the old simulator stores them.
They are recorded in `I` until native evidence proves that they persist across
logic boundaries and are required to produce the next accepted `S/E` result.

## Normalized event log

`E` is a typed, ordered battle log. It records a discrete fact only when its
intra-step occurrence, cause, or order cannot be recovered reliably from
adjacent snapshots. Each event belongs to transition `E(t)` and has a
zero-based `event_seq` within that transition. The transition index and event
sequence form its canonical order; redundant timestamps are not stored.

The required baseline event families are:

| Family | Required events and payload |
| --- | --- |
| Object lifecycle | Creation and removal of Unit, Projectile, Building, AreaShield and DynamicTerrain objects, including identity, type, ownership/source and removal reason. This preserves objects that are created and removed inside one logic step. |
| Action | Attack or skill start and release, with acting object, action/skill identity and target when one is committed. Internal controller sub-phases are not baseline events. |
| Projectile | Release, impact/interception and removal, with projectile, owner/source, target, impact position and outcome as applicable. |
| Shield | Personal- or area-shield hit and deactivation, with source, shield/owner, requested and applied amount, and energy before/after. Mere containment tests are not baseline events. |
| Status | Apply, refresh/extend and removal/expiry, with status identity/type, source, target, stack/duration change and reason. |
| Dynamic terrain | Creation, region change, lifetime reset and removal, plus a typed terrain effect when it causes damage, healing or Status change. Internal overlap selection is not a baseline event. |
| Combat result | Damage, healing and death, with provider/source, target, applied amount, and relevant life/shield before/after state. |

Events must use the same source-neutral object identities as snapshots. Exact
enum encoding and typed payload layout remain open, but producers may not add
source-specific event kinds to the formal track.

`S` remains authoritative. `E` explains the transition but does not provide a
second conflicting source of persistent state.

## Temporary research instrumentation

`I` exists only to close an unresolved mechanism. It may contain temporary
state samples, events, inputs, or native observations. Baseline examples are:

- target candidate order, `nearest_actor`, `motion_target`, `attack_target`,
  and lock-target caches until their cross-step necessity is established;
- attack/skill controller sub-phases, timers, cooldowns, cycles, and other
  controller counters;
- RVO desired targets and velocities, next-target points, dynamic avoidance
  radii, neighbours, solver buffers, gradients, and iterations;
- random-call sites and consumed samples;
- projectile source-pose caches and area-shield containment sets;
- dynamic-terrain overlap candidates, selected/sticky membership, and
  controller pulse counters;
- observed update order, native hook order, native pointers, and runtime
  addresses.

Research instrumentation must not expand the permanent MCFR schema by
default. It should be emitted as a separate, explicitly profiled sidecar and
excluded from formal result hashes.

The reference API exposes `InstrumentationSink`. Adapter capture and simulation
may submit a `step`, `channel`, `content_type`, and arbitrary byte payload to
the same interface. `InstrumentationWriter::record_json` provides canonical
JSON as a convenience; `NoInstrumentation` disables collection without
changing producer control flow. A sidecar binds to its formal recording by
`scenario_hash`, and records its `profile` and `producer`. It has no formal
state, event, or result hash.

When research ends:

- an intermediate or diagnostic value remains outside MCFR and its capture
  implementation may be removed;
- a value proven necessary for deterministic reproduction must be promoted to
  a source-neutral field in `D`, `S`, or `E` before the temporary probe is
  removed.

## Common production path

Native capture and simulation must not maintain independent MCFR
serialization implementations. Both paths should submit the same normalized
records to one Rust canonicalizer and writer:

```text
game adapter capture ----\
                          +--> Rust canonicalizer/writer --> .mcfr
Rust simulation kernel --/
```

The adapter owns native observation. The Rust writer owns identity
normalization, ordering, validation, hashing, and physical serialization. This
keeps HDF5 and comparison policy outside the injected adapter.

## HDF5 container version 1

The `.mcfr` file is HDF5. The writer creates a temporary sibling and publishes
the final path without overwriting an existing file only after all datasets,
metadata, and hashes have been finalized.

| Path | HDF5 type | Meaning |
| --- | --- | --- |
| `/context/data` | contiguous `u8` | One canonical `D` record. |
| `/states/data` | chunked, deflate-compressed `u8` | Concatenated canonical `S(0)..S(n)` records. |
| `/states/offsets` | append-only `u64` | Record boundaries, including initial zero. |
| `/events/data` | chunked, deflate-compressed `u8` | Concatenated canonical `E(0)..E(n-1)` records. |
| `/events/offsets` | append-only `u64` | Transition boundaries, including initial zero. |

Root attributes contain the format and container/schema versions, state and
transition counts, `terminal_step`, and the four canonical hashes. Container
version 1 requires `state_count = transition_count + 1` and
`terminal_step = transition_count`.

Each logical record is a schema-validated Rust value encoded as canonical
UTF-8 JSON bytes. Records are concatenated in numeric datasets rather than
stored as HDF5 variable-length JSON strings. This first layout supports
streaming writes and random record reads while event payload schemas are still
being closed. A later typed columnar projection requires a container-version
change but must preserve the same logical hashes.

An instrumentation sidecar uses the HDF5 format marker
`mechcore.mcfr.instrumentation`. It stores steps, channels, content types,
payload bytes and payload offsets under `/records`, plus `scenario_hash`,
`profile`, `producer`, and `record_count` attributes.

## Canonical hashes

Correctness is defined over canonical logical content, not raw HDF5 file
bytes. HDF5 library versions, metadata order, chunk layout, compression, and
source provenance may change physical bytes without changing battle content.

Schema version 1 uses BLAKE3 with domain separation and a little-endian `u64`
length before every canonical record. The formal hash model is:

```text
scenario_hash = BLAKE3("scenario-v1", canonical D, canonical S(0))
state_hash    = BLAKE3("state-v1", canonical S(0)..canonical S(n))
event_hash    = BLAKE3("event-v1", canonical E(0)..canonical E(n-1))
result_hash   = BLAKE3("result-v1", scenario_hash, state_hash, event_hash)
```

`D` contains the schema version, RNG state, and durable commands, so the
implemented `scenario_hash` is equivalent to the expanded requirements
expression.

Two recordings are comparable only when their schema versions and
`scenario_hash` values match.

- equal `state_hash` means the authoritative baseline state evolution is
  equal;
- equal `state_hash` but unequal `event_hash` means the same snapshots were
  reached through a different recorded mechanism sequence;
- equal `result_hash` means the two producers agree within the accepted `S/E`
  contract for that scenario.

A whole-file hash may additionally protect transport integrity, but it is not
the native/simulation correctness criterion. Source provenance is retained as
metadata and excluded from `result_hash`.

Canonical encoding recursively sorts object keys. World-object collections are
sorted by source-neutral identity, dynamic-terrain grid cells by coordinate,
and events by their required contiguous `event_seq`. Formal snapshot numbers
are schema-defined integers; non-finite floating-point values cannot enter the
canonical JSON representation.

## Validation boundary

A finalized MCFR must fail validation when any required deterministic input is
missing, frame coverage is discontinuous, object identity is ambiguous,
references are unresolved, event ordering is invalid, or the stored tracks do
not satisfy the declared schema version.

The following claims are intentionally separate:

- a file is structurally valid;
- a file is playable from `S`;
- two files have equal state evolution;
- two files have equal normalized events;
- a mechanism is understood without temporary `I` evidence.

## Open specification work

The next revisions must decide, in order:

1. typed payload schemas for each accepted event enum;
2. promotion or rejection of each disputed `I` field through native evidence;
3. source-neutral identity assignment for initial and dynamically created
   objects;
4. an explicit unknown/unsupported representation for fields that require it;
5. the typed columnar HDF5 projection and compression profile;
6. the player-facing read and interpolation contract.

These details must be derived jointly from native capture feasibility,
deterministic-kernel requirements, and playback requirements rather than from
one specialized research case.
