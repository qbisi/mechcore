# MCFR deterministic recording contract

[简体中文](mcfr.zh.md)

## Status

This document defines the baseline logical contents of `S` and `E` shared by
native capture, simulation, comparison, and playback. Schema version 1 has a
reference Rust implementation in `mechcore-mcfr`. The final columnar
projection of world objects remains open.

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

## Native observability constraint

Every `S` field, every `E` event and payload value, and every future `I`
channel and value MUST have a documented adapter path that observes the same
fact directly from the native game at the applicable logic boundary or native
operation. Schema admission is limited by native adapter observability: a
simulator need or an offline analysis result does not by itself justify a
recorded field, event, or instrumentation value.

For `S`, direct observation means reading a native field, property, or method
result at that snapshot boundary. For `E`, a native callback, delegate, method
hook, argument, return value, or observation made inside the same native
operation MUST positively establish that the event occurred and supply its
semantics. For `I`, the profile MUST identify the corresponding native probe;
simulation may emit the same channel for comparison, but may not introduce a
simulation-only or post-processed value.

Neighbouring snapshots and recorded fields may be compared to validate
capture completeness, but their differences MUST NOT create a recorded state,
event, or instrumentation value, or supply a missing cause, reason, outcome,
classification, or payload value. A producer MUST NOT fill an unobserved value
with external rules, producer-private state, a constant, a default, a
heuristic, or an assumed lifecycle meaning.

Lossless representation normalization is allowed: checked numeric widening,
fixed unit conversion, exact native-enum mapping, source-neutral identity
mapping, canonical ordering, and physical deduplication. Such normalization
MUST NOT introduce a new game-semantic fact. Every admitted field, event kind,
and instrumentation channel must retain native-source evidence in the adapter
implementation or its capture audit. If a candidate fact has no direct native
source, it is removed from the applicable schema/profile. Capture failure is
not a substitute for a capturable schema, and inference is not a fallback.

## Durable deterministic context

`D` contains source-neutral battle inputs shared by capture and simulation but
not naturally expressed as a per-frame world snapshot. It binds:

- the MCFR schema version and its canonicalization rules;
- the game build;
- logic-step timing and integer numeric scales;
- the one-based combat round and match seed;
- the source-neutral identity contract.

Schema version 1 admits no external command that changes logical combat after fighting begins. A
Training Ground speed-up vote changes only wall-clock scheduling, not logic-step inputs or results;
it is therefore orchestration metadata and is excluded from `D`, `S`, and `E`. The adapter
must capture `S(0)` before the first combat update and before combat random
streams are consumed. A mechanism that cannot be reconstructed from the typed
round/seed fields and `S(0)` is outside this comparison contract until the
schema gains a directly capturable, source-neutral typed field. Producer-private
RNG JSON is not permitted in `D`.

Unit and mechanism rules are external inputs selected by the simulation and
are not embedded or fingerprinted in MCFR. Native capture therefore does not
require a unit-config directory. A wrong external rule set is detected by the
resulting `S/E` hash divergence rather than by a producer-specific context
field.

Combat is closed after `S(0)`: no external action may affect its logical evolution. Wall-clock-only
speed-up may be requested while recording because capture is attached to logic updates, not render
or wall-clock frames. A producer
that cannot guarantee this boundary must not finalize a schema-version-1
recording.

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

A minimal player may ignore `E`. A richer player may consume directly captured
projectile release/removal and damage events for transient effects that do not
survive in a tick-end snapshot.

## Unified world snapshots

`S` is source-neutral. Native pointers, runtime object addresses, adapter
bookkeeping, and simulator-private types do not enter it.

The schema-version-1 world boundary contains only:

- units;
- projectiles;
- buildings, including constructions and contraptions;
- persistent statuses.

Team and formation are ownership and identity attributes on concrete world
objects, not per-frame container entities. A unit is directly associated with
its team and formation. Personal shields remain unit state. Independent area
shields and dynamic terrain are not baseline entities until their complete
required state can be enumerated and captured through direct native paths.

Status is the shared representation for persistent buffs and debuffs,
including technology-disable effects. Specialized `buffs` and
`technology_disabled` fields are not maintained in parallel.

Schema version 1 uses `team_yx_sequential_v1` identities. Object IDs occupy
independent namespaces for Unit, Projectile, Building, and Status; each
namespace starts at one and has no gaps.
Formation IDs use another one-based, gapless namespace. For initial Units,
build 2227's evidenced order is team-controller order, then ascending unified
world `y`, then ascending unified world `x` within each team. Equal Unit
positions in one team are invalid; no synthetic tie-breaker is introduced.
This coordinate rule deliberately avoids translating world coordinates into
camera-relative labels such as “top-left”. Initial non-Unit objects retain
their game registration order, while dynamically created objects use
canonical event order. The public `IdentityAllocator` supplies the ordinals;
the writer rejects gaps and validates initial Unit order when starting a
recording.

Subject to the native observability constraint, the baseline snapshot admits
a field only when it satisfies at least one of these rules:

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
| Unit | `unit_id`, `team_id`, `formation_id`, `unit_type_id`, domain; position, body rotation, main-skill aim pose, current velocity and native MotionFSM state; collision radius; life, maximum life, alive, active, targetable and visibility; personal-shield active/enabled state and current/maximum energy. |
| Projectile | `projectile_id`, team and owner; position and orientation; target object reference; native cached target position and radius; released flag and current/maximum projectile life. Projectile class, velocity, active state, impact outcome and removal reason are not inferred. |
| Building | `building_id`, team and building type; position, rotation and native bounds width/height; life, maximum life, alive, destroyed, available, targetable and collision-enabled state. |
| Status | `status_id`, native buff type, source and target; additive stack; raw `duration_time`, `max_duration_time`, `step_time`, and `step_time_config`; finished and frozen flags. These values are preserved as native counters rather than reinterpreted as elapsed or remaining time. |

The adapter source map for this baseline is normative:

| World kind | Direct native source |
| --- | --- |
| Unit | Team ownership comes from `FightController.GetTeamControllers` and `FightTeamController.GetTeamIndex`; membership and formation come from `FightTeam.GetMeches` and `FightMech.GetMechTeam`; type/domain come from `GetMechID` and `IsFly`; body and aim transforms come from `GetFightTransform` and `GetMainSkill().GetMainTransform()`; velocity comes from `MotionController.GetCurrentVelocity`; motion comes from `MotionController.fsm.GetCurrentState`; radius, gauges and flags come from `GetRadius`, `GetLife`, `GetMaxLife`, `IsAlive`, `get_IsActive`, `IsValidTarget(0)`, and `GetVisibility`; personal-shield values come from `GetEnergyShieldController`. |
| Projectile | Enumeration and team come from `ProjectileSystem.projectileControllers` and `ProjectileController.GetTeamController`; owner, target, transform, cached target data, released flag and life come directly from `FightProjectile.GetOwner`, `GetTarget`, `GetFightTransform`, `GetTargetInfo`, `IsRelease`, `GetLife`, and `GetMaxLife`. |
| Building | Membership/team come from `FightTeam.GetTowers`; type/order, transform, bounds, life and flags come from `GetBuildingType`, `GetBuildingIndex`, `GetFightTransform`, `GetBoundsRect`, `GetLife`, `GetMaxLife`, `IsAlive`, `IsDestroyed`, `IsAvaliable`, `IsValidTarget(0)`, and `GetBuildingData().get_EnableCollision()`. |
| Status | Enumeration comes from `FightMech.GetBuffManager` and `BuffManager.buffs`; type/source, stack and flags come from `Buff.GetBuffID`, `GetSource`, `GetAdditiveStack`, `IsFinish`, and `IsFreeze`; the four clocks are the native `Buff` fields with the same names. Target is the owning `FightMech` whose manager contains the Buff. |

Runtime pointers are used only to map these native objects into source-neutral,
one-based IDs; pointer values are never persisted.

Visibility is the closed native-aligned enum `normal`, `disappear`, `stealth`,
or `hide`. Motion state is an exact mapping of the native MotionFSM state
classes `idle`, `move`, `attack`, and `stop`. A candidate field or enum case
that cannot be mapped directly is excluded from this schema rather than filled
with `unknown`, inferred, defaulted, or made a reason to reject an otherwise
capturable battle.

Target-search caches, controller clocks, solver buffers, and other disputed
state do not enter this baseline merely because the old simulator stores them.
They are recorded in `I` until native evidence proves that they persist across
logic boundaries and are required to produce the next accepted `S/E` result.

## Normalized event log

`E` is a typed, ordered battle log of facts reported directly by native
execution points. It is not a change log reconstructed from adjacent
snapshots. Each event belongs to transition `E(t)`. Its array position is its
canonical order within that transition; a redundant `event_seq` field and
timestamps are not stored.

The required baseline event families are:

| Family | Required events and payload |
| --- | --- |
| Projectile release | `ProjectileSystem.AddProjectile` supplies projectile identity, native owner and target. No projectile class is inferred from configuration or splash radius. |
| Projectile removal | `ProjectileSystem.Destroy` supplies projectile identity, owner, target, current position, and the native `intercepted` argument. The schema does not infer impact outcome or a richer removal reason. |
| Damage | The positive return value of `DamagePerformer.Perform` supplies `amount`; its native provider and target supply source and target identities. Life/shield deltas, healing and death are not synthesized as events. |

Events must use the same source-neutral object identities as snapshots. Exact
event kinds and payloads are the closed `EventPayload` enum implemented by
`mechcore-mcfr`; unknown fields and source-specific event kinds are rejected.
Array order preserves the order in which these hooked native operations occur.

`S` remains authoritative. `E` explains the transition but does not provide a
second conflicting source of persistent state.

## Temporary research instrumentation

`I` exists only to close an unresolved mechanism. It may contain temporary
native state samples, events, inputs, or intermediate observations that obey
the same native observability constraint as `S` and `E`. Baseline examples,
when backed by a direct native probe, are:

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
excluded from formal result hashes. Exclusion from formal hashes does not
relax the native-source requirement.

The reference API exposes `InstrumentationSink`. Adapter capture and simulation
may submit a `step`, `channel`, `content_type`, and arbitrary byte payload to
the same interface. `InstrumentationWriter::record_json` provides canonical
JSON as a convenience; `NoInstrumentation` disables collection without
changing producer control flow. A sidecar binds to its formal recording by
`scenario_hash`, and records its `profile` and `producer`. It has no formal
state, event, or result hash. The generic byte interface is a transport
mechanism, not permission for unproven or derived channels; each profile must
separately document its native probe and payload semantics.

When research ends:

- an intermediate or diagnostic value remains outside MCFR and its capture
  implementation may be removed;
- a value proven necessary for deterministic reproduction must be promoted to
  a source-neutral field in `D`, `S`, or `E` before the temporary probe is
  removed.

## Common production path

Native capture and simulation must not maintain independent MCFR
serialization implementations. Both paths use the public data model and
writer provided directly by the `mechcore-mcfr` crate:

```text
game adapter capture ----\
                          +--> mechcore-mcfr::McfrWriter --> .mcfr
Rust simulation kernel --/
```

`mechcore-mcfr` owns the logical record types, canonical ordering, validation,
hashing, HDF5 serialization, and corresponding reader. The adapter may call
the crate directly inside the game process; MCP may choose the output path and
orchestrate the recording lifecycle, but it is not a required serialization
intermediary.

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
streaming writes and random record reads. A later typed columnar projection
requires a container-version change but must preserve the same logical hashes.

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

`D` contains only typed fields: schema/build identity, timing and numeric
scales, combat round, match seed, and identity contract. Producer-private JSON,
configuration fingerprints, and source-specific commands do not enter the
formal scenario hash.

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
sorted by source-neutral identity, while event arrays retain native operation
order. Formal snapshot numbers
are schema-defined integers; non-finite floating-point values cannot enter the
canonical JSON representation.

## Validation boundary

MCFR validation covers container structure, canonical decoding, track lengths,
hashes, and the initial canonical identity contract. It does not decide whether
a gameplay state transition, reference, amount, gauge, or event sequence is
logically legal; that belongs to a game-specific analyzer or simulator test.

The following claims are intentionally separate:

- a file is structurally valid;
- a file is playable from `S`;
- two files have equal state evolution;
- two files have equal normalized events;
- a mechanism is understood without temporary `I` evidence.

## Open specification work

The next revisions must decide, in order:

1. promotion or rejection of each disputed `I` field through native evidence;
2. the typed columnar HDF5 projection and compression profile;
3. the player-facing read and interpolation contract.

These details must be derived jointly from native capture feasibility,
deterministic-kernel requirements, and playback requirements rather than from
one specialized research case.
