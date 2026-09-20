# Combat internals

[简体中文](combat.zh.md)

What the fight core does inside a tick, on build `1.11.1.3.2259`.

Every entry is scoped, and the scope binds. What a rule does not cover is stated
with it, because a claim just outside a closed scope is unverified however
obvious an extension of it looks.

## Attack interval random stream

`FightTeam.RefreshRandomData` builds one `GRRandom` per team, seeded
`(round + teamIndex) * 4444`.

**Not covered.** The order in which several members, or one member's several
skills, consume that stream.

## The stored interval carries a per-unit stagger

`RefreshAttackInterval` writes a skill's `attackInterval` as
`attackIntervalProperty.Value` divided by the logical step and truncated, and
that integer is what MCFR's `derived.attack_interval` carries. **It is not the
description's interval.** It differs from it by a stagger drawn per unit from
the unit's `attackDurationRandomValue`, which
[`config/units/`](../../config/units) calls `interval_offset`:

| Unit | Interval | Offset | Description in ticks | The build stored |
| --- | ---: | ---: | ---: | ---: |
| Stormcaller | 6.6 | 0 | 132 | 132 |
| Rhino | 0.9 | 0 | 18 | 18 |
| Arclight | 0.9 | 0.3 | 18 | 15 |
| Fang | 1.5 | 0.4 | 30 | 24 |
| Phoenix | 3.4 | 0.8 | 68 | 53 |
| Marksman | 3.1 | 0.6 | 62 | 55, 65, 56 |

Four claims, each measured:

- **A unit whose offset is zero never deviates.** Every such unit read exactly
  its description.
- **The deviation is a unit's own.** Three Marksmen in one fight read 55, 65
  and 56 — the same type, the same layout, three different numbers — so the
  draw is per unit and not per type, and it is signed rather than a
  subtraction.
- **It is the same draw every time.** The first Marksman of a layout read 55
  under three different match seeds and in four different fixtures, and the
  three above came out in spawn order. Whatever stream it comes from is not
  seeded by the match.
- **It does not move during the fight.** Read at ticks 1, 2, 3, 5, 10, 20 and
  40, every one of those numbers is the same, so the field is a stored
  interval rather than a countdown.

**The draw is the team stream's, and it is signed.** The stream is the
`GRRandom` this file's first rule describes, seeded `(round + teamIndex) *
4444` rather than by the match, which is why the same layout answers the same
numbers under any seed. The projection is `next_in_range(offset)`, uniform
over `[1 − offset, offset − 1]` by masked rejection, which is why a unit can
read *above* its description as well as below.

Three Marksmen in a round-one fight are the reading: their offset is twelve
ticks, and `GRRandom(4444)` answers `−7, +3, −6` for its first three draws in
that range, which is exactly `55, 65, 56` against a description of 62.
`crates/simulation/src/random.rs` pins those three numbers in a test.

**Every member draws, in the order the recording numbers them.** The stream
walks the deployment by ascending world `z`, then `x` — the order
[`mcfr.md`](../spec/mcfr/mcfr.md) already gives a recording's initial units —
and hands each **member** one draw in that member's own offset. A member whose
offset is zero takes nothing from the stream.

Two fixtures separate that from the alternatives.
`tests/layouts/interval/stagger-singles.yaml` puts four single-member units in
four rows, declared in the opposite order to the one the stream takes them in,
and all four readings come out of the seed in `z` order:
`62−7, 18+5, 62−6, 32+0`. `stagger-mustang-then-marksman.yaml` puts a twelve
member Mustang formation in front of one Marksman, so a draw per member makes
the Marksman the thirteenth unit and a draw per formation makes it the second:
those two places in the stream hold different numbers, 72 and 55, and the game
stored **72**.

**Not covered.** What else consumes from the same stream once the fight is
running. This is the stagger at deployment; the rest of the stream is nobody's
measurement yet.

This simulator schedules its own stagger — `sample_actor_attack_interval` adds
a draw from the team stream to the next attack step — and reproduces the
game's fights tick for tick, so the two agree about *when* a unit fires while
storing different numbers for *what its interval is*. That is why
[`mcfr.md`](../spec/mcfr/mcfr.md) calls this the one field the two backends
knowingly answer differently.

## Attack scheduling

`RefreshAttackInterval` converts by the logical step and floors at one tick.
After a unit first enters Attack, the next update is the earliest release.

**Not covered.** Every other branch of the skill state machine.

## Base facing

There is no `aim_tolerance` turn deadzone. A value like 20 is not a threshold the
build applies.

**Not covered.** `Normalize -> Angle -> RawAcos` over all directions, and
boundary rounding.

## Normal target scoring and pre-battle acquisition

Among currently visible, fully rotated, unsplit-quadtree ordinary ground targets
with a unique optimum, the build picks by Q32 edge distance, strict
minimum-range exclusion, an angle factor, an out-of-range penalty and a strict
minimum score.

`FightPrepareState` completes the first acquisition and syncs initial facing
before the first persisted state S(1).

**Not covered.** Split quadtree, tied candidates, a building winning, moving
candidates being reinserted, and other selector modes.

## Ordinary projectiles

`Init/Update/Move` move in Q32.32, hold `released=false` while active, carry the
default rotation, and remove at the actual transform position.

A Marksman or Arclight single ordinary projectile refreshes the target root Q32
position every tick where `isLockTarget=true`, the target is alive and moving,
and `randomTargetRange=0` with zero offset.

**Not covered.** Non-locking projectiles, a dead target, non-zero random offset,
interception, and every other projectile type.

## Damage and death

`ReduceLife` clamps the life actually lost to `min(currentLife, incomingDamage)`.

With no technology, equipment, dynamic buff or shield involved, a level-1
Arclight centres target selection on the projectile transform, takes everything
inside the radius, and records the sum of life actually lost across those
targets as one Damage event on the primary target.

Rhino main skill 5001, under those same baseline constraints and as a
single-target direct effect, reaches a nominal 3560 through
`SkillDamageProvider -> FightSkill.GetDamage -> DamageProperty`. The killing blow
is clamped to the remaining life.

**Not covered.** Building splash, area boundary and ordering, modifier chains,
shields, and other providers or target domains.

## Personal shield baseline

A unit with no shield still starts with `EnergyShieldController.enabled=true`.

**Not covered.** Actual activation, absorption and destruction.

## Ordinary first-attack delay

The ordinary first-attack branches divide `prepareTime` and `attackPoint`
separately by the logical step, and truncate each. Marksman comes out at 10+2
ticks, Arclight at 0+0.

**Not covered.** Repeat attacks, grouped and loading paths, backswing and other
phase adjustments. This is not a formula that adds the two fields, and must not
be generalised into one.

## Ordinary synchronous direct-attack backswing

A Rhino's ordinary direct attack enters a nine-tick backswing on the effect
tick. The wait controller increments first each tick and completes on
`counter >= duration`, so the tick carrying the ninth wait update still cannot
re-initiate. `TryPerformAttack` is re-entered on the tick after.

**Not covered.** Losing the target during windup, a third-party kill, quick
target switching, and other controllers.

## Dead-target retention after a synchronous direct kill

A Rhino that kills its current target with its own synchronous direct attack
keeps the dead target and stays Idle until that after-wait ends. It acquires a
new target and re-enters Move only afterwards.

**Not covered.** Third-party kills, live-target cycling, and other skill states.

## Main-skill aim command

In the ordinary main-skill attack turn branch with `isHaveBody=true`, a Marksman
or Arclight passes the same direction local, from a single
`CalculateTargetDirection`, to both the mech body and the main weapon. So
`independent_aim=false`, and the mech body is not an MCFR unit root.

**Not covered.** Units with no mech body, other weapon modes, special skills,
and independent direction sources.

## Endgame ordering of a 1v1 direct kill

`FightCoreSystem.TeamUpdate` caches the alive count before updating its own
team's units. An earlier team's synchronous direct kill can therefore be rebuilt
to zero by a later team within the same tick, after which `TryDstroyTower`
destroys the building.

Tower death lands later than that tick's `DeadEffectSystem.Update`, so one drain
tick is still required. Once the fight meets the finish gate, no further RVO
runs.

**Not covered.** Multi-member and multi-group fights, summons, respawns,
Construction, shields, and mixed damage inside one tick.

## Reference unit fields

Level-1, no technology, no equipment, no dynamic buff.

| Unit | life | radius | move | rotate | damage | range | interval | offset | prepare | attack point | backswing | cooling | projectile speed | splash |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Marksman | 1622 | 8 | 8 | 70 | 2329 | 140 | 3.1 | 0.6 | 0.5 | 0.1 | 0 | 0.2 | 500 | 0 |
| Arclight | 4813 | 9 | 7 | 80 | 365 | 95 | 0.9 | 0.3 | 0 | 0 | 0 | 0 | 300 | 7 |

The fractional times are the YAML rendering of the build's raw Q32.32 fields.
Native logic converts each field to ticks separately, so these are not
interchangeable with tick counts.

Marksman targets ground and air, Arclight ground only. Both are ground units
with `lock_target=true`, `quick_switch_target=true` and
`target_offset_radius=0`, and both take the Normal-topology single-projectile
path. A zero or non-zero `splash_radius` describes effect topology only. It is
not the game's native attack-type enum.
