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

## A free-moving unit keeps its speed whichever way it faces

A unit whose rotate speed is below 180° a second, and whose body is more than
90° from the way it drifts, moves slower than its full speed. A free-moving
unit never does: `MotionController.CalculateMoveSpeed` returns the full speed
first when `FightMech.isFreeMove` is set. The table's own column is false for
every unit; `MechData.PreProcess` sets the flag for an id of at most 54 whose
bit is set in the literal `0x40000000040010`: the Melting Point (4), the
Wraith (18) and the unit whose id is 54. `scripts/extract-units.py` writes it
as `free_move`. A Wraith drifting 100° off its facing in its M3 with seed
1787720817 publishes its full 10 m/s at tick 172.

## Attack scheduling

`RefreshAttackInterval` converts by the logical step and floors at one tick.
After a unit first enters Attack, the next update is the earliest release.

## Base facing

There is no `aim_tolerance` turn deadzone. A value like 20 is not a threshold
the build applies.

**A direction is mirrored to the left half only when it is clearly left.** A
direction becomes a facing through `FightUtility.ConvertToAngle`: the angle
from forward, and `360` less that angle when the angle is above zero and the
direction's `x` is below zero. Both tests are `FPoint`'s, which count a
difference of up to 43 raw Q32.32 units as equal, so an `x` of −43 raw or more
is not below zero. `AcosFastest(1)` is not zero, so a target straight ahead is
faced at +0.245°, and so is one a few raw units to the left, where a
formation's jittered slots can put it, rather than at −0.245°.

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

**A grouped unit's lock is the latest any slot took or dropped, and a
construction is never allocated.** `FightSkill.ChangeLockTarget` hands the
owner every lock a slot of the group takes or drops, the core's and its
siblings' alike, in the order the slots update, the core first. So a unit
with several weapon slots reads the unit most recently allocated to one of
them, and reads nothing on the tick a sibling that updates after the core
drops its lock. A construction in a slot's way replaces what that slot fires
at, not what it was allocated, so every slot can be firing at a block while
the lock reads the unit behind it.

**Each slot of a group runs its own skill states.** A Wraith's four slots are
four `FightSkill`s, and each prepares, attacks and goes idle by itself:

- A sibling starts only while the group attacks:
  `GroupedSkillAttackBehaviour.CanStartAttackCheck` is `SkillGroup.IsAttacking`,
  any skill of the group in `SkillAttackState`. An idle sibling then searches
  around what the others hold and prepares for the unit it finds if it is in
  reach. The core's own attack is what first sets its siblings going: on the
  tick the core enters `SkillAttackState`, the siblings, updating after it,
  allocate and prepare.
- Preparing and attacking slots ask `SkillAttackableChecker.Check` every
  update, and a slot whose check fails goes idle and drops its lock, whatever
  its siblings do. The core leaving its attack, because its target is beyond
  its range, leaves its siblings attacking with their extra 10 metres, and
  the unit's motion follows the core: a Wraith whose core has lost reach walks
  on toward its lock while its other weapons keep firing.
- A slot that is already preparing goes on into its attack even when no
  sibling attacks any more; only starting waits for the group.
- A slot enters its attack when its prepare is over and fires on the update
  after, then at its own interval; the core does the same. The time its next
  blow is due is the skill's own and outlives the attack: a slot that leaves
  its attack and comes back to it fires when its interval is up, not on the
  update after its return.
- The core's own searches, in its check and in its idle search alike, are
  the group's: around what its siblings hold, and among it when nothing
  else answers.
- Leaving the fight leaves every slot idle.

**A sibling gives up a shared unit by its blows.** An attacking sibling's
check asks `SkillAttackableChecker.TrySearchGroupSkillLockTarget` while its
lock lives; the core's never does, nor a preparing sibling's. It groups the
locks of the unit's own skills, and answers to give the unit up only when:

- every skill of the group holds a lock, two of them the same one, and the
  sibling does not hold its unit alone;
- every other skill sharing a lock, the core aside, has struck more blows in
  its current attack (`SkillAttackController.attackCount`) than this one:
  the first with fewer, or else the last with as many, is the one to move,
  and this one keeps its unit;
- its search timer is up: leaving idle sets it to ten, and each attacking
  update counts it down after the check;
- and the search it then runs, which sets the timer to ten again, finds a
  unit no skill of the group holds.

The check then fails with the lock unchanged, the slot goes idle and drops
its lock, and on the next update it searches around the others and prepares
anew. So a sibling the core joins on a unit leaves it once its timer is up
and a free unit stands in reach, and one that has struck a blow keeps a unit
it shares with a sibling that is still preparing: in the Wraith's M3 with
seed 1787720817, at tick 178, the core took a Crawler three of its siblings
held, two of them still preparing, and the attacking one kept it.

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
do not perform a separate allocation pass. The checker's
redistribution path, `TrySearchGroupSkillLockTarget`, is the rule above and
not the ordinary search's sibling-exclusion loop. The fixtures
that separated the rest are [the Wraith fixtures](../../tests/wraith/README.md).

## A fusillade fires with its core

A skill whose data says `IsFusillade` gets a `GroupedSkillFusilladeBehaviour`
over its group in place of the ordinary one; the Raiden's three weapons are
the one recorded. It holds every sibling to the core:

- **A sibling starts only while the core prepares or attacks.**
  `CanStartAttackCheck` asks the core's state alone, not whether any skill of
  the group attacks. An idle sibling searches and starts once the core is in
  `SkillPrepareState` or `SkillAttackState`, and with no prepare it is
  attacking on the update it starts; while the core idles, the siblings hold
  nothing.
- **The core entering its attack hands every sibling its schedule.**
  `FusilladeStart`, which `OnStartAttack` runs for the core, calls
  `RefreshAttackData(core, isSync: true)` on each sibling: the interval and
  the time already counted are the core's.
- **A sibling fires only after the core has.** `CanPerformAttack` holds until
  the core, which updates first, has started a blow on the same update
  (`OnStartPerformAttack` puts it first in the list of skills attacking), so
  the three blows land on one tick, one round every 92 ticks.
- **After a round, every sibling is due again when the core is.**
  `FusilladeEnd`, which the group's update runs when a skill attacked,
  `RefreshAttackData(core)` without `isSync`: the core's interval, the count
  started over.

Where the simulator ends the core's update matters: a core with no prepare
enters its attack in `SkillIdleState.TryStartAttack`, which the simulator
runs with the unit's motion, so a fusillade's siblings update after the
motion, with the facing the body had before it turned.

**A fusillade's siblings never share, and are allocated towers.** A grouped
search leaves out what the other skills hold, and a fusillade does not fall
back on it, as a group that shares does. With no unit left, the answer is an
enemy tower: a Raiden alone against a Rhino has its core on the Rhino and its
two siblings each on a tower, idle while it is out of reach.

**A fusillade's core takes a unit from its siblings.** The core searches around
what its siblings hold like any slot; when that answer is missing or out of
reach, `PerformGroupedSkillSearch` asks the selector about the held units, as a
sharing group does, and each skill that holds what it finds drops its lock
(`ChangeLockTarget(null)`). The sibling's attack target stays until it searches
again; an attacking one searches on its next check. In the Raiden's M3 with
seed 4242, the core, idle after its cooling, takes the Crawler its first
sibling attacks at tick 177, and that sibling finds another one outside its
angle and cools.

**A sibling cools as the core does.** A sibling's check that fails in its attack
ends in `SkillAttackState.Finish`: its lock dropped, reaching the owner, and
its cooling time with its weapon on what the check turned it to; the last step
of the cooling reads idle with nothing named, and the slot starts as an idle
one on the update after.

**An idle sibling searches on its own timer.** `SkillIdleState.TrySearchLockTarget`
searches when the skill's search timer, ten updates, is up or what it holds is
gone, and keeps the lock in between; a sibling that found a unit outside its
area holds it idle until then.

**A cooling that has nothing to drop hands the owner nothing.** A grouped unit's
lock is the latest a slot wrote; a core cooling with no lock does not write one
each update, so a sibling's lock stays the unit's.

## A weapon fixed to the body

`FightWeapon`'s constructor gives each weapon of the unit whose data is 27, the
Raiden, a `RotationLimitFightTransform` of `RotateType.Fixed`, parented to the
unit's transform, where every other unit's weapon has none. A recording carries
such a weapon's pose:

- it stands where the unit stands;
- the core's weapon has the body's rotation on every tick;
- a sibling's weapon turns only when its own skill updates holding a lock:
  `FightSkill.Update` turns a skill's weapons toward its lock after the state
  has updated, and a fixed weapon ignores the direction and takes the parent's
  rotation, the body's before this update turned it. A sibling with no lock
  keeps the rotation it had, and every weapon enters the fight with the body's.
  On the update the fight stops no skill updates and no weapon turns.

`FightSkill.GetMainTransform` answers the weapon's transform where
`CanRotate` does, which a fixed weapon's does, so a Raiden sibling's search
scores, and its attack area is measured, from its own weapon's rotation: a
sibling whose weapon was left behind finds a unit outside its angle and does
not start on it.

**A bodyless unit entering its attack out of angle holds its fire while it
turns**, whatever its path: the Raiden, whose blows strike, as the units whose
weapons fire projectiles, lasers or melee blows. Without the hold its attack
state would find the target out of angle on the next update and give it up.

## Normal target scoring and pre-battle acquisition

Among currently visible, fully rotated, unsplit-quadtree ordinary ground targets
with a unique optimum, the build picks by Q32 edge distance, strict
minimum-range exclusion, an angle factor, an out-of-range penalty and a strict
minimum score.

**The angle is measured from the searcher's main transform.** That is its
first weapon's when the weapon has a transform, and the mech's root when it
has none, whether or not the weapons turn at a speed of their own: a
Wraith, whose four weapons turn at 90° a second but carry no transform,
scores from its root's facing, not from where its first weapon points.

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

**A projectile leaves for where its burst aimed it.** Every projectile of a
burst is released toward the target's position when the burst began plus its
offset; one that follows its target takes the target's position up again from
its first update, which is why the rule above reads as the live position.

**Two weapons split a burst's offsets in three dimensions.** Each weapon takes
the half of the offsets on its side of the line from the weapon to the target,
ordered by the angle it sees, and the angle and the side are measured from the
weapon's height to the target's: an Overlord shooting down at a Crawler, and
two Overlords shooting at each other at the same height, order them so.

**A projectile may climb before it flies.** A skill with a pre-flight height
sends its projectiles straight up at their speed, neither following the target
nor landing, until they stand at or above the height, the last step taken whole:
a Farseer's shot climbs 7 metres a tick to 63 for its 60. The height is the
pre-flight height times the distance to the target over the attack range, at
the range and beyond the whole of it (`ProjectileSystem.Create`). The distance
is measured for each projectile as it is created, from where its owner stands
then, to where the burst's target stood when the tick the burst began on
began: an Overlord pushed aside between two releases of one burst climbs its
later projectiles to another height, and the first step of a climb, rounded
through the reciprocal of what is left to climb, lands a few raw units apart.

**A burst goes on after its target leaves reach.** A skill is not checked
between the projectiles of a burst, so a bodyless unit whose target walks out
of reach mid-burst moves after it and fires the rest: an Overlord follows its
Crawler and fires its fourth shot before it goes idle.

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

**Leaving the idle state is entering one.** `SkillIdleState.TryStartAttack`
enters `SkillAttackState` on the tick the attack target comes into the attack
angle, and the state is not updated until the tick after, so the blow is
released then. A Fortress whose weapons turn onto a Crawler while its motion
already attacks reads `SkillAttackState` on the tick they come within the
angle and releases on the next; so does a Sledgehammer or Typhoon that locks a
new target after a kill. A skill leaving its cooling does not wait.

**An idle skill keeps its lock only while it can fire at it.** The idle
skill's periodic search answers a new lock when the one it holds is outside
its attack area, even while its motion attacks: a Melting Point whose weapons
are still turning onto one Crawler takes the Crawler they already face and
prepares against it.

**A turret does not turn on the tick its skill retargets after a kill.** When
the target a skill attacked dies during a tick and the skill's own search
answers a new one, the skill's update that tick tracked the dead target: a
Melting Point whose Crawler an ally kills sets off for the next one with its
turret still.

**A group of one weapon is fired as one weapon.** A Vortex's grouped skill has
a single direct weapon, so its fusillade is one blow; its target scoring reads
the root's rotation, because a grouped weapon turns on its own only at a speed
of its own, as a Wraith's does.

## Ordinary synchronous direct-attack backswing

A Rhino's ordinary direct attack enters its backswing on the effect tick. The
wait controller increments first each tick and completes on
`counter >= duration`, so the tick carrying the last wait update still cannot
re-initiate. `TryPerformAttack` is re-entered on the tick after.

## Dead-target retention after a synchronous direct kill

A Rhino that kills its current target with its own synchronous direct attack
keeps the dead target and stays Idle until that after-wait ends. It acquires a
new target and re-enters Move only afterwards.

**A direct kill holds its target through the tick even with no backswing.** A
Vortex reads idle on the tick its blow kills its target, still on the dead
unit, and attacks the next one the tick after.

## Main-skill aim command

In the ordinary main-skill attack turn branch with `isHaveBody=true`, a Marksman
or Arclight passes the same direction local, from a single
`CalculateTargetDirection`, to both the mech body and the main weapon. So
`independent_aim=false`, and the mech body is not an MCFR unit root: a
recording carries it as the unit's `turret_rotation`.

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

**When the fight stops, every unit's motion enters idle, and a unit that was
moving stops.** `MotionIdleState.Enter` calls `RVOControllerFixed.StopMove`,
which asks for no speed at the point the unit stands on, so the next solve
publishes none: an Overlord handed a tower on the tick the last enemy died,
and idle from the next, stays where it stood rather than walking on at the
next publish.

**A won fight runs on without ending its skills' coolings.** Between the
tick a side loses its last unit and the fight's end, a skill that was
already cooling goes on cooling and goes on naming what it named, the dead
last enemy included: three Phantom Rays cooling on a Crawler that died with
the fight still name it for the five ticks after. A skill that was attacking
goes idle and names nothing. `FightSkill.ExitFight`, which ends a cooling
through `SkillStateController.ChangeToIdleState`, is therefore not what the
won fight's first tick calls, and a cooling the simulator would begin on
that very tick is not one the game shows.

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
- Each slot's own states, the core leaving its attack while its siblings go
  on, the unit's lock following the latest slot, a sibling giving up a unit
  it shares with the core, and a slot's interval outliving its attack,
  in the Wraith's M2, M3 and M6 fights: `tests/units/regressions.mcscript`.
- A sibling that has struck keeping a unit it shares with siblings still
  preparing, and a free-moving Wraith at full speed off its facing, in the Wraith's M3 with
  seed 1787720817: `tests/units/regressions.mcscript`.
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
- A burst's aim, a climbing projectile, two weapons' offsets in three
  dimensions, a burst that goes on after its target leaves reach, a turret on a
  retarget, and a Vortex's single grouped weapon and its kill, in the standard
  fights of the Farseer, Overlord, Melting Point and Vortex:
  `tests/units/regressions.mcscript`.
- The blow waiting a tick after the idle state is left, and an idle skill
  giving up a lock it cannot fire at, in the standard fights of the Fortress,
  Sledgehammer, Typhoon and Melting Point: `tests/units/regressions.mcscript`.
- A projectile's climb measured as it is created, and a moving unit stopped
  when the fight stops, in the Overlord's standard fights:
  `tests/units/regressions.mcscript`.
- A cooling that goes on through a won fight, and an attack that goes idle,
  in the Phantom Ray's standard fights: `tests/units/regressions.mcscript`.
- The Raiden's fusillade, its siblings' towers, a core taking a sibling's unit,
  siblings cooling and searching on their timers, and its weapons' poses, in
  the Raiden's standard fights: `tests/units/regressions.mcscript`, and nine
  Raidens' blows landing on nine Fangs: `tests/raiden/regressions.mcscript`.
- A presearched target a few raw units left of straight ahead faced at
  +0.245°, in the standard fights of the Steel Ball, Stormcaller, Hound, Fire
  Badger and Phantom Ray: `tests/units/regressions.mcscript`.

### Read

- A search's angle is measured from the skill's main transform, a weapon's
  or the root's: `FightSkill.GetMainTransform`.
- A projectile's climb is scaled by the distance from where it leaves, as it
  is created: `ProjectileSystem.Create`, `IProjectileSkillData.GetPreFlyHeight`,
  `ProjectileFlyData.GetPosition`.
- Entering idle stops a moving unit: `MotionIdleState.Enter`,
  `RVOControllerFixed.StopMove`.
- Leaving the fight ends a cooling: `FightSkill.ExitFight`,
  `SkillStateController.ChangeToIdleState`.
- The presearch faces a unit by the direction to its target, and a facing is
  mirrored only past `FPoint`'s tolerance: `PresearchTargetController.SearchTarget`,
  `FightUtility.ConvertToAngle`, `FightMech.UpdateRotation`,
  `FPoint.op_LessThan`, `FPoint.op_GreaterThan`.
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
- A sibling starts only while the group attacks, and every slot's lock
  reaches the owner: `GroupedSkillAttackBehaviour.CanStartAttackCheck`,
  `SkillGroup.IsAttacking`, `SkillGroup.IsCoreSkill`,
  `FightSkill.ChangeLockTarget`, `FightSkillBase.IsMainSearcher`.
- A failed check ends a slot's attack: `SkillAttackState.CheckAttackable`,
  `SkillAttackState.Finish`.
- An attacking sibling's check asks whether to give its unit up:
  `SkillAttackableChecker.Check`,
  `SkillAttackableChecker.TrySearchGroupSkillLockTarget`.
- A free-moving unit is not slowed by its facing, and which units are free
  moving: `MotionController.CalculateMoveSpeed`, `FightMech.isFreeMove`,
  `MechData.PreProcess`, `FightMech.EnterFight`.
- A fusillade holds its siblings to the core: `SkillGroup..ctor`,
  `GroupedSkillFusilladeBehaviour.CanStartAttackCheck`,
  `GroupedSkillFusilladeBehaviour.CanPerformAttack`,
  `GroupedSkillFusilladeBehaviour.FusilladeStart`,
  `GroupedSkillFusilladeBehaviour.FusilladeEnd`,
  `GroupedSkillFusilladeNormal.OnStartPerformAttack`,
  `FightSkill.RefreshAttackData`.
- A grouped search falls back to the normal one, and a core's fallback takes
  a unit from its siblings: `SkillSearchTargetController.SearchLockTarget`,
  `SkillSearchTargetController.PerformGroupedSkillSearch`,
  `FightSkill.ChangeLockTarget`.
- Only a child skill asks whether to give its unit up, within the unit's
  own group, by blows struck and behind a search timer:
  `SkillAttackableChecker.TrySearchGroupSkillLockTarget`,
  `SkillAttackController.attackCount`, `SkillAttackController.PerformAttack`,
  `SkillAttackController.Exit`, `SearchTargetController.CanStartSearch`,
  `SearchTargetController.ResetSearchTargetTime`, `SkillIdleState.Exit`,
  `SkillAttackState.Update`.
- The unit whose data is 27 has fixed weapons, which turn to their parent's
  rotation when their skill updates with a lock: `FightWeapon..ctor`,
  `FightWeapon.RotateTo`, `FightWeapon.CanRotate`, `FightSkill.Update`,
  `FightSkill.GetMainTransform`, `RotationLimitFightTransform`.
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
  needs the game, `tests/modifier/disable.mcscript`, and no gameless test pins
  it; how long a disable lasts is not recorded.
- **The skill state machine** beyond first entry into Attack.
- **Base facing's angle**: `FVector3.Angle` over all directions, and its
  rounding away from straight ahead. Only the mirroring test is read.
- **Which interface slot `MotionMoveState.Update` asks before it enters
  attack.** The dispatch goes through slots the dump does not name, so "the
  attack target in reach" is what the recordings and the shape of `IAttacker`,
  `IsAttackTargetInAttackRange` beside `GetLockTarget`, say. A Marksman's
  weapon has no pose in a recording, so its aim angle is not observed.
- **Grouped slots**: which skill `TrySearchGroupSkillLockTarget` removes
  from the sharers besides itself, read as the core from the list it takes
  it from; the order the build's dictionary walks the locks in, taken as the
  order they are first named; a fusillade of weapons that do not strike, which the simulator refuses; redistribution of
  wall blockers; and whether an idle slot that
  finds a unit beyond its reach keeps it as its lock, which no recorded
  sibling has done.
- **Target scoring**: a split quadtree, tied candidates, a building winning,
  moving candidates being reinserted, and other selector modes.
- **Projectiles**: why a projectile in simulated motion spares a dead unit's
  neighbours, which is recorded and not read; interception; and every other
  projectile type.
- **A fixed weapon's transform**: that `RotationLimitFightTransform` refreshed
  for `RotateType.Fixed` copies its parent's rotation exactly, and which call
  keeps the core's weapon on the body's rotation, are measured, not read; the
  core's search condition that lets it take a sibling's unit is read as a
  fusillade's from where it stands, not from the flag it asks.
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
- **When `FightSkill.ExitFight` runs** in a won fight, and why a cooling the
  simulator would begin on the won fight's first tick is not one the game
  shows; the recordings fix what is seen, not the call that makes it.
- **The unit whose id is 54** is free-moving by the same literal; no recorded
  fight fields it.
- **The endgame** with several members or groups, summons, respawns,
  constructions, shields, and mixed damage inside one tick; and which of the
  winner's units the build hands a tower.
