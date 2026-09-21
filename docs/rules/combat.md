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

## The current interval carries a per-cycle stagger

`RefreshAttackInterval` writes a skill's `attackInterval` as
`attackIntervalProperty.Value` divided by the logical step and truncated, and
that integer is what MCFR's `derived.attack_interval` carries. **It is not the
description's interval.** It differs from it by a stagger drawn per unit from
the unit's `attackDurationRandomValue`, which
[`config/units/`](../../config/units) calls `interval_offset`:

| Unit | Interval | Offset | Description in ticks | At tick one |
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
- **Every cycle draws again.** Three Marksmen read `55, 65, 56` at tick one,
  `62, 70, 52` once their first shot has gone, then `72, 57, 64`. The number
  is the interval the cycle in progress was scheduled with, so a unit's
  cadence jitters from cycle to cycle rather than being staggered once at
  deployment.

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
`tests/interval/stagger-singles.yaml` puts four single-member units in
four rows, declared in the opposite order to the one the stream takes them in,
and all four readings come out of the seed in `z` order:
`62−7, 18+5, 62−6, 32+0`. `stagger-mustang-then-marksman.yaml` puts a twelve
member Mustang formation in front of one Marksman, so a draw per member makes
the Marksman the thirteenth unit and a draw per formation makes it the second:
those two places in the stream hold different numbers, 72 and 55, and the game
stored **72**.

### The stagger cannot be separated from the interval it rides on

`GetCurrentAttackInterval` is an exact concept and the name is the build's
own: it is the interval the unit is on **now**, and it is right for it to
move, because a correction can arrive or leave mid-fight. Electromagnetic
Explosion disables the target's technologies on hit, and a technology that
corrects an attack interval stops correcting it for as long as that lasts —
the current interval is where a reader would see that happen.

What a reading cannot do is tell the two apart. One number carries both the
composed interval and that cycle's stagger, and no single reading separates
them.

**Except where the offset is zero.** A unit whose `interval_offset` is zero
draws nothing, so its current interval is the composed interval exactly, with
nothing added. The Rhino, the Stormcaller, the Crawler and the Steel Ball are
that kind, and they are what a measurement of an interval correction should be
built on. Where a measurement must use a unit that does draw — the
Sledgehammer is the only one that can hold an interval value and an interval
rate at once — the stagger has to be cancelled rather than avoided, which is
what `tests/modifier/interval-order.mcscript` does with three
calibration fixtures.

**A disabled technology takes its correction with it, and the current interval
says so.** A Rhino carrying Mechanical Rage, an `attack_interval_value` of
`−0.3` seconds, reads 12 ticks of its description's 18. At the tick an
Electromagnetic Shot lands its `IsTechnologyDisabled` turns true and the same
reading turns 18: the correction is not there while the technology is off.
`tests/modifier/disable.mcscript` records it, and the Rhino is the
target for two reasons found the hard way — it survives the hit, where a
Stormcaller and a Marksman were shot at first and never read anything, and its
offset is zero, so nothing else rides on the number.

**Not covered.** What else consumes from the same stream once the fight is
running, and how long a disable lasts: the Raiden in that recording fires
every 92 ticks and the Rhino is never undisabled before it dies.

This simulator schedules its own stagger — `sample_actor_attack_interval` adds
a draw from the team stream to the next attack step — and reproduces the
game's fights tick for tick, so the two agree about *when* a unit fires while
storing different numbers for *what its interval is*. That is why
[`mcfr.md`](../spec/mcfr/mcfr.md) calls this the one field the two backends
knowingly answer differently.

## Where a moving unit is sent

A unit moving on its target is sent to the point along the line to it where
its reach begins: the target's centre distance, less the target's radius,
plus the unit's attack range and its own radius. It is sent to the target's
centre instead when it is no farther than that already.

"No farther" is asked of the **squared** centre distance, which is exact,
against the square of that stopping distance, which comes from the fast
fixed-point square root. They disagree in the last bits exactly where a unit's
reach cancels the target's radius, as a Crawler's range 6 and radius 2 cancel a
Marksman's 8. Of 24 Crawlers charging one Marksman, the game sends the eight in
front to the Marksman's centre and the sixteen behind to a point a few
thousand raw units off it, the side each falls on decided by that comparison;
comparing the two roots instead sent all 24 to the centre and moved every
Crawler behind the front row by a few hundred raw units at the first RVO
publication. `tests/regression/crawlers-vs-marksman.yaml` is that fight.

## A kill the quick switch cannot follow

A unit that switches targets quickly — the Marksman does — takes a new target
the moment its own dies, as long as the new one can be attacked at once. When
its shot kills the target before the attack that fired it is over, within the
attack point, and the selector's answer lies outside its attack area, it does
not: it stays idle with no lock for its cooling, its weapon already on that
answer, clears the weapon on the tick after, and searches for a lock on the
tick after that. A Marksman's cooling is 0.2 seconds, so it is idle for five
ticks and locked on the sixth, on whatever the selector answers then — in
`crawlers-vs-marksman.yaml` a different Crawler from the one its weapon held,
because the Crawlers have moved. A shot that kills later in its flight, or a
replacement already in the attack area, is followed at once, and a unit whose
cooling is nothing — the Stormcaller's — never holds. Both fights recorded
with a Marksman killing while enemies remained show it, on every tick.

## Attack scheduling

`RefreshAttackInterval` converts by the logical step and floors at one tick.
After a unit first enters Attack, the next update is the earliest release.

**Not covered.** Every other branch of the skill state machine.

## Base facing

There is no `aim_tolerance` turn deadzone. A value like 20 is not a threshold the
build applies.

**Not covered.** `Normalize -> Angle -> RawAcos` over all directions, and
boundary rounding.

## The body follows the lock, the weapons follow the attack target

A unit has two targets and the build keeps them apart: the mech's lock
(`FightMech.lockTarget`, recorded as `mech_lock_target`) and each skill's attack
target (`GetAttackTarget()`, recorded per weapon channel in `weapon_aims`).
[mcfr.md](../spec/mcfr/mcfr.md#what-a-unit-is-directed-at) defines the fields;
this is what they were measured to do. They coincide in an ordinary fight and
part company when an enemy construction stands in the line of fire, which is
where every reading below was taken.

**The body travels toward the lock.** While `moving`, the angle between a
unit's velocity and the direction to its lock has a median under one degree for
every unit type read: Marksman 0.7° with all 77 samples inside 5°, Wraith 0.0°
over 108, Crawler 0.8° over 9814, Fang 0.8° over 215. The Crawlers and Fangs
that stray further are formations being pushed by their neighbours.

**Whether it travels is the attack target's.** `attacking` is entered when an
attack target is in reach, whatever the lock is: across the wall recordings,
1429 unit-ticks are `attacking` a construction with the lock out of reach and
the construction in it. `MotionAttackState.Enter` calls
`RVOControllerFixed.StopMove`, and only `MotionMoveState` calls
`MotionController.Move`, so no state both travels and fires. A single unit
`attacking` moves exactly 0 metres a tick, whether its attack target is a unit
or a construction. A velocity already published carries on until the next RVO
publish: a Wraith entering attack travelled five more ticks at its full 0.5
metres before stopping.

**A body faces the lock; a unit without one faces its attack target.** Attacking
a construction, a Marksman — `has_body: true` — keeps its body on the unit
behind it: 0.0° off the lock and 6.6° off the block it is shooting, nearer the
lock on 95 of 95 ticks. `MotionAttackState.AttackRotate` asks
`FightMech.IsHaveBody` before `RotateBodyTo`, and a unit with a body turns only
its weapons in attack. The units without a body — Fang, Crawler, Wraith — turn
the root toward the attack target: nearer it on 311 of 328, 850 of 1034 and 52
of 54 decidable ticks.

**A grouped skill's lock is its latest allocation, and a construction is never
allocated.** A Wraith's lock follows the unit most recently allocated to one of
its four slots. A construction in a slot's way replaces what that slot fires
at, not what it was allocated, so all four slots can be firing at a block while
the lock reads the Marksman behind it — which is how "the lock follows the
group's last attack target" and "a wall never reaches the lock" are both true.
The slots are dropped with the lock: the tick a block the Wraith was shooting
fell, and the tick its last enemy died, all four read empty, and the children
were allocated again only eight ticks after the core was next attacking.

**Not covered.** The reading of which interface slot `MotionMoveState.Update`
asks before it enters attack: the dispatch goes through slots the index does not
name, so "the attack target in reach" is what the recordings and the shape of
`IAttacker` — `IsAttackTargetInAttackRange` beside `GetLockTarget` — say, not a
method body read. A Marksman's weapon has no pose in a recording, so its aim
angle is not observed, only that its shot reaches the construction. Every
reading is the build's; `crates/simulation/src/kernel.rs` implements the split
as `lock_target` and `Actor::attack_target`.

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
