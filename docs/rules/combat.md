# Combat internals

What the fight core does inside a tick, on build `1.11.1.3.2259`.

Every entry is scoped, and the scope binds. What a rule does not cover is stated
with it, because a claim just outside a closed scope is unverified however
obvious an extension of it looks.

## Attack interval random stream

`FightTeam.RefreshRandomData` builds one `GRRandom` per team, seeded
`(round + teamIndex) * 4444`.

**Not covered.** The order in which several members, or one member's several
skills, consume that stream.

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
