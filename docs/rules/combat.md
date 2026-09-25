# Combat internals

What the fight core does inside a tick.

Every entry is scoped, and the scope binds. What a rule does not cover is stated
under Not established, because a claim just outside a closed scope is
unverified however obvious an extension of it looks.

## Attack interval random stream

`FightTeam.RefreshRandomData` builds one `GRRandom` per team, seeded
`(round + teamIndex) * 4444`.

## The current interval carries a per-cycle stagger

`RefreshAttackInterval` writes a skill's `attackInterval` as
`attackIntervalProperty.Value` divided by the logical step and truncated, and
that integer is what MCFR's `derived.attack_interval` carries. **It is not the
description's interval.** It differs from it by a stagger drawn per member from
the skill's `attackDurationRandomValue`, which
[`config/units/`](../../config/units) calls `interval_offset`.

- **A unit whose offset is zero never deviates**, and takes nothing from the
  stream.
- **The deviation is a member's own**, not its type's: members of one formation
  read different intervals.
- **The draw is the team stream's, and it is signed.** The stream is the one
  above, seeded by the round and the team rather than by the match, so the same
  layout answers the same numbers under any seed. The projection is
  `next_in_range(offset)`, uniform over `[1 − offset, offset − 1]` in ticks by
  masked rejection, so a unit can read above its description as well as below.
- **Every member draws, in the order the recording numbers them.** The stream
  walks the deployment by ascending world `z`, then `x`, the order
  [`mcfr.md`](../spec/mcfr/mcfr.md) gives a recording's initial units, and
  hands each **member** one draw in that member's own offset, whatever order
  the layout declares them in.
- **Every cycle draws again.** The number is the interval the cycle in progress
  was scheduled with, so a unit's cadence jitters from cycle to cycle rather
  than being staggered once at deployment.

`GetCurrentAttackInterval` is an exact concept and the name is the build's own:
it is the interval the unit is on **now**, and it moves when a correction
arrives or leaves mid-fight. Electromagnetic Explosion disables the target's
technologies on hit, and a technology that corrects an attack interval stops
correcting it for as long as that lasts. One number carries both the composed
interval and that cycle's stagger, and no single reading separates them,
**except where the offset is zero**: such a unit's current interval is the
composed interval exactly. The Rhino, the Stormcaller, the Crawler and the
Steel Ball are that kind, and a measurement of an interval correction should be
built on one of them.

This simulator schedules the same stagger: `sample_actor_attack_interval` adds
a draw from the team stream to the next attack step and stores the interval
each cycle was scheduled with. Three readings complete it:

- a unit with a group of weapons reads its **core**'s interval, not whichever
  slot drew last;
- a unit with **no enemy left** reads its interval as composed, with no
  stagger, because there is no cycle in progress. A unit whose lock was alive
  reads it from the tick after its last enemy dies, which is when it loses the
  lock; a unit already idle and lockless when the last enemy dies keeps its
  drawn interval;
- every unit reads the composed interval on the fight's last tick.

## Where a moving unit is sent

A unit moving on its target is sent to the point along the line to it where
its reach begins: the target's centre distance, less the target's radius,
plus the unit's attack range and its own radius. It is sent to the target's
centre instead when it is no farther than that already.

"No farther" is asked of the **squared** centre distance, which is exact,
against the square of that stopping distance, which comes from the fast
fixed-point square root. They disagree in the last bits exactly where a unit's
reach cancels the target's radius, as a Crawler's range and radius cancel a
Marksman's radius: of Crawlers charging one Marksman, the ones in front are
sent to its centre and the ones behind to a point just off it, the side each
falls on decided by that comparison.

## A kill the quick switch cannot follow

A unit that switches targets quickly, as the Marksman does, takes a new target
the moment its own dies, as long as the new one can be attacked at once. When
its shot kills the target before the attack that fired it is over, within the
attack point, and the selector's answer lies outside its attack area, it does
not: it stays idle with no lock for its cooling, its weapon already on that
answer, clears the weapon on the tick after, and searches for a lock on the
tick after that, on whatever the selector answers then. A shot that kills later
in its flight, or a replacement already in the attack area, is followed at
once, and a unit whose cooling is nothing never holds.

## Attack scheduling

`RefreshAttackInterval` converts by the logical step and floors at one tick.
After a unit first enters Attack, the next update is the earliest release.

## Base facing

There is no `aim_tolerance` turn deadzone. A value like 20 is not a threshold
the build applies.

## The body follows the lock, the weapons follow the attack target

A unit has two targets and the build keeps them apart: the mech's lock
(`FightMech.lockTarget`, recorded as `mech_lock_target`) and each skill's attack
target (`GetAttackTarget()`, recorded per weapon channel in `weapon_aims`).
[mcfr.md](../spec/mcfr/mcfr.md#what-a-unit-is-directed-at) defines the fields.
They coincide in an ordinary fight and part company when an enemy construction
stands in the line of fire.

**The body travels toward the lock.** While `moving`, a unit's velocity points
at its lock, up to the push of its neighbours in a formation.

**Whether it travels is the attack target's.** `attacking` is entered when an
attack target is in reach, whatever the lock is. `MotionAttackState.Enter`
calls `RVOControllerFixed.StopMove`, and only `MotionMoveState` calls
`MotionController.Move`, so no state both travels and fires: a single unit
`attacking` does not move. A velocity already published carries on until the
next RVO publish.

**A body faces the lock; a unit without one faces its attack target.**
`MotionAttackState.AttackRotate` asks `FightMech.IsHaveBody` before
`RotateBodyTo`, and a unit with a body turns only its weapons in attack, its
body staying on its lock. A unit without a body, a Fang, a Crawler or a Wraith,
turns its root toward the attack target. A unit with a body loses it while its
data set holds a positive `MechDataChangeInt.DisableBody`.

**A grouped skill's lock is its latest allocation, and a construction is never
allocated.** A unit with several weapon slots locks the unit most recently
allocated to one of them. A construction in a slot's way replaces what that
slot fires at, not what it was allocated, so every slot can be firing at a
block while the lock reads the unit behind it. The slots are dropped with the
lock: the tick a block the unit was shooting falls, and the tick its last enemy
dies, every slot reads empty, and the children are allocated again only when
the group allocates them after the core is next attacking.

`crates/simulation/src/fight/mech.rs` implements the split as `lock_target`
and `Actor::attack_target`.

## A grouped slot searches around its siblings' locks

For a non-fusillade main skill group, each `FightSkill` keeps its own lock.
`SkillSearchTargetController.PerformGroupedSkillSearch` walks the group's other
skills and collects their non-null **lock targets**, not what their weapons
fire at. With no sibling holdings it uses the ordinary search. Otherwise it
scores alive opponents outside that list with the slot's selector, retaining
the first strict score minimum. It does not allocate a sorted table of targets
to all slots at once.

When `CanAttackSameTarget` is true, a missing selection or one outside the
slot's range makes the selector consider the held targets as well. A
successful held-target selection replaces the first answer; without one, the
first answer is retained. Sharing therefore does not promise equal numbers of
slots per target. A wall redirecting a slot's attack target does not change the
lock excluded by the other slots.

**A main child skill reaches 10 metres beyond its parent.**
`FightSkillFactory.PrepareGroupedSkill` assigns the parent to child skills;
`FightSkillBatch.Init` propagates `isMainSkill` to the members.
`FightSkill.GetAttackRange` reads `ParentSkill` and `isMainSkill`, and in that
branch adds the Q32 literal `0xA00000000` to the parent's range, which is
exactly 10 metres. The first skill has no parent and keeps its ordinary range.
Both the selector's range penalty and its fallback range test use the slot's
own range; the unit's recorded range remains its core's.

`GroupedSkillAttackBehaviour.Update` and `OnStartAttack` are empty. The hooks
do not perform a separate allocation pass. The checker also has a distinct
live-sharing redistribution path: `TrySearchGroupSkillLockTarget` groups
existing locks, detects whether the current skill is their sole holder, and
chooses among shared holders using attack counts before a throttled search.
That path is not the ordinary search's sibling-exclusion loop. The fixtures
that separated the rest are [the Wraith fixtures](../../tests/wraith/README.md).

## Normal target scoring and pre-battle acquisition

Among currently visible, fully rotated, unsplit-quadtree ordinary ground targets
with a unique optimum, the build picks by Q32 edge distance, strict
minimum-range exclusion, an angle factor, an out-of-range penalty and a strict
minimum score.

`FightPrepareState` completes the first acquisition and syncs initial facing
before the first persisted state S(1).

## Ordinary projectiles

`Init/Update/Move` move in Q32.32, hold `released=false` while active, carry the
default rotation, and remove at the actual transform position.

A Marksman or Arclight single ordinary projectile refreshes the target root Q32
position every tick where `isLockTarget=true`, the target is alive and moving,
and `randomTargetRange=0` with zero offset.

**A projectile that follows its target keeps its offset.** A burst draws every
projectile's offset within `randomTargetRange` when it begins. A projectile of
a skill with `isLockTarget=true` lands its offset from wherever the live target
stands: it is released at the target's position then plus its offset, and its
target point follows the target with the offset kept. The second of a Phantom
Ray's two projectiles, released 0.3 seconds after the first, lands its offset
from where a charging Rhino stands by then. A target already dead when the
projectile is released is not followed: the projectile goes to the point the
burst aimed it at when it began.

**A single weapon lands its offsets last drawn first.** A Phantom Ray's first
projectile lands the second offset its burst drew, and its second the first.

**A projectile in simulated motion that lands on a dead unit does nothing.** A
skill with `isSimulateMode=true` whose projectile arrives after its target died
deals no damage, splash included: a Fire Badger's or a Typhoon's shot at a
Crawler another shot killed while it flew leaves the Crawlers beside it
untouched. Any other projectile still strikes where it lands, as an Arclight's
does.

**A weapon is named by its index.** A skill's weapons carry their own index in
the build, and a recording names a weapon aim and a projectile release by it: a
Hound's one weapon is index 2, a Sabertooth's two are 0 and 2.

## Damage and death

`ReduceLife` clamps the life actually lost to `min(currentLife, incomingDamage)`.

With no technology, equipment, dynamic buff or shield involved, a level-1
Arclight centres target selection on the projectile transform, takes everything
inside the radius, and records the sum of life actually lost across those
targets as one Damage event on the primary target.

A beam with a splash strikes as any other hit does: a Melting Point's beam at
one Crawler takes the Crawlers around it too, in the order the target trees
hold them. A Steel Ball's beam has no splash and strikes its target alone.

A Rhino's main skill, under those same baseline constraints and as a
single-target direct effect, reaches its description's damage through
`SkillDamageProvider -> FightSkill.GetDamage -> DamageProperty`. The killing
blow is clamped to the remaining life.

## Personal shield baseline

A unit with no shield still starts with its `EnergyShieldController` enabled.

## Ordinary first-attack delay

The ordinary first-attack branches divide `prepareTime` and `attackPoint`
separately by the logical step, and truncate each: a Marksman comes out at its
prepare ticks plus its attack-point ticks, each truncated on its own. This is
not a formula that adds the two fields first, and must not be generalised into
one.

## Ordinary synchronous direct-attack backswing

A Rhino's ordinary direct attack enters its backswing on the effect tick. The
wait controller increments first each tick and completes on
`counter >= duration`, so the tick carrying the last wait update still cannot
re-initiate. `TryPerformAttack` is re-entered on the tick after.

## Dead-target retention after a synchronous direct kill

A Rhino that kills its current target with its own synchronous direct attack
keeps the dead target and stays Idle until that after-wait ends. It acquires a
new target and re-enters Move only afterwards.

## Main-skill aim command

In the ordinary main-skill attack turn branch with `isHaveBody=true`, a Marksman
or Arclight passes the same direction local, from a single
`CalculateTargetDirection`, to both the mech body and the main weapon. So
`independent_aim=false`, and the mech body is not an MCFR unit root.

## Endgame ordering of a 1v1 direct kill

`FightCoreSystem.TeamUpdate` caches the alive count before updating its own
team's units. An earlier team's synchronous direct kill can therefore be rebuilt
to zero by a later team within the same tick, after which `TryDstroyTower`
destroys the building.

Tower death lands later than that tick's `DeadEffectSystem.Update`, so one drain
tick is still required. Once the fight meets the finish gate, no further RVO
runs.

`FightingState.Update` calls `TryDstroyTower` once per tick, after every module,
and it does not ask what dealt the last blow. A laser kill takes the loser's
towers down the same way: gone on the kill's tick, their `building_destroyed`
on the next, which is the fight's last.

On the tick a side loses its last unit, the winner's units may be handed one
of the loser's towers for that tick, and turn toward it. What the loser's own
last units were attacking does not decide whether that happens. Which of the
winner's units are handed a tower is the simulator's `natural_finish_handoff`,
not read from the build.

## Reference unit fields

[`config/units/marksman.yaml`](../../config/units/marksman.yaml) and
[`config/units/arclight.yaml`](../../config/units/arclight.yaml) hold the two
units most of the rules above were measured on. Their fractional times are the
YAML rendering of the build's raw Q32.32 fields. Native logic converts each
field to ticks separately, so these are not interchangeable with tick counts.

Marksman targets ground and air, Arclight ground only. Both are ground units
with `lock_target=true`, `quick_switch_target=true` and
`target_offset_radius=0`, and both take the Normal-topology single-projectile
path. A zero or non-zero `splash_radius` describes effect topology only. It is
not the game's native attack-type enum.

## Evidence

### Recorded

- Every unit's current interval, its stagger and the three readings that
  complete it, on every tick of the standard unit fights:
  `tests/units/regressions.mcscript`.
- A grouped skill's slots, their locks and their reach, and a grouped unit's
  core interval: `tests/wraith/regressions.mcscript`.
- Where a charging Crawler is sent, and a Marksman's quick switch that cannot
  follow a kill: `tests/regression/simulate.mcscript`.
- The body travelling toward the lock, attacking without moving, and the
  weapons on a construction while the body keeps the lock:
  `tests/construction/regressions.mcscript`.
- Target scoring, ordinary projectiles, damage clamping, the first-attack
  delay, the backswing, dead-target retention and the endgame ordering, in the
  ordinary fights: `tests/regression/simulate.mcscript`.
- A unit's personal shield enabled with no shield of its own:
  `tests/units/regressions.mcscript`.
- A following projectile's offset, the order a single weapon lands its
  offsets, a simulated-motion shot at a dead unit, a weapon's index, and a
  splashing beam, in the standard fights of the Phantom Ray, Fire Badger,
  Typhoon, Hound, Sabertooth and Melting Point:
  `tests/units/regressions.mcscript`.

### Read

- The team's interval stream is seeded by round and team:
  `FightTeam.RefreshRandomData`.
- The interval is converted by the logical step, truncated and floored at a
  tick, and the offset is the skill's: `FightSkill.RefreshAttackInterval`,
  `FightSkill.GetCurrentAttackInterval`, `SkillData.attackDurationRandomValue`.
- A disabled technology is a unit's state: `FightMech.IsTechnologyDisabled`.
- Attacking stops the body, and only moving moves it:
  `MotionAttackState.Enter`, `RVOControllerFixed.StopMove`,
  `MotionMoveState.Update`, `MotionController.Move`.
- A unit with a body turns only its weapons in attack, and a positive
  `DisableBody` takes the body away: `MotionAttackState.AttackRotate`,
  `FightMech.IsHaveBody`, `MechDataChangeInt.DisableBody`,
  `FightMech.lockTarget`.
- A grouped slot searches around its siblings' locks:
  `SkillSearchTargetController.PerformGroupedSkillSearch`,
  `SkillGroup.CanAttackSameTarget`,
  `SkillAttackableChecker.TrySearchGroupSkillLockTarget`.
- A main child skill reaches 10 metres beyond its parent:
  `FightSkillFactory.PrepareGroupedSkill`, `FightSkillBatch.Init`,
  `FightSkill.GetAttackRange`.
- The group's attack hooks are empty: `GroupedSkillAttackBehaviour.Update`,
  `GroupedSkillAttackBehaviour.OnStartAttack`.
- The first acquisition happens before the first state: `FightPrepareState.Enter`.
- Life lost is clamped to life left: `FightActor.ReduceLife`,
  `FightSkill.GetDamage`.
- The alive count is cached per team, and towers fall once per tick after every
  module: `FightCoreSystem.TeamUpdate`, `FightCoreSystem.TryDstroyTower`,
  `FightingState.Update`, `DeadEffectSystem.Update`.

### Not established

- **The interval stream.** The order in which one member's several skills
  consume it, and what else consumes from it once the fight is running.
- **A disabled technology's interval.** That the current interval drops the
  correction while the technology is disabled was measured by a script that
  needs the game, `tests/modifier/disable.mcscript`, and no offline test pins
  it; how long a disable lasts is not recorded.
- **The skill state machine** beyond first entry into Attack.
- **Base facing's arithmetic**: `Normalize -> Angle -> RawAcos` over all
  directions, and boundary rounding.
- **Which interface slot `MotionMoveState.Update` asks before it enters
  attack.** The dispatch goes through slots the dump does not name, so "the
  attack target in reach" is what the recordings and the shape of `IAttacker`,
  `IsAttackTargetInAttackRange` beside `GetLockTarget`, say. A Marksman's
  weapon has no pose in a recording, so its aim angle is not observed.
- **Grouped slots**: live-sharing redistribution when an unheld target becomes
  available, a child leaving its attack area, grouped fusillade, redistribution
  of wall blockers, and the derivation of the group's prepare offset.
- **Target scoring**: a split quadtree, tied candidates, a building winning,
  moving candidates being reinserted, and other selector modes.
- **Projectiles**: why a projectile in simulated motion spares a dead unit's
  neighbours, which is recorded and not read; a projectile that climbs before
  it flies (`preFlyHeight`), which the simulator refuses; interception; and
  every other projectile type.
- **An attack begun from idle after a kill.** A Sledgehammer or Typhoon whose
  target is killed, which goes idle for a tick and locks a new one, fires a
  tick later in the game than in the simulator.
- **Damage**: building splash, area boundary and ordering, modifier chains,
  shields, and other providers or target domains.
- **A personal shield's** activation, absorption and destruction.
- **Attack timing**: repeat attacks, grouped and loading paths, phase
  adjustments other than the backswing; losing the target during windup, a
  third-party kill and quick target switching during a backswing; third-party
  kills and live-target cycling after a direct kill.
- **The aim command** for units with no mech body, other weapon modes, special
  skills, and independent direction sources.
- **What writes `DisableBody`.** No source of it is read, and no recorded unit
  loses its body.
- **A tower's own update.** `FightCoreSystem.TeamUpdate` now updates each live
  tower of the team; what that update does is not read, and the recorded fights
  match without modelling it.
- **The endgame** with several members or groups, summons, respawns,
  constructions, shields, and mixed damage inside one tick; and which of the
  winner's units the build hands a tower.
