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
- **Every member draws, in the order the fight updates them.** The stream
  walks the deployment by ascending world `z`, then `x`, and hands each
  **member** one draw in that member's own offset, whatever order the layout
  declares them in. That is the order [`mcfr.md`](../spec/mcfr/mcfr.md) numbers
  a recording's initial units in but for one difference: the fight compares
  `z` with `FPoint`'s tolerance (`FightUtility.PositionComparer`), so two
  members within 43 raw units of each other in `z` go by `x`, while their
  identities go by `z`. Every tick's updates follow the same order. Recorded in
  replays 201370830 and 67152171, round 1: two units 6 raw apart in `z` draw
  their first intervals, and act each tick, in `x` order, the later identity
  first.
- **The first searches are staggered in the same order.** The presearch
  (`PresearchTargetController.CreateMechDatas`) takes each side's mechs in
  that order and gives the n-th, counting from 0, a search timer of
  n / ⌈count / 10⌉ updates, so the units search for the first time in ten
  batches. Two units within 43 raw of each other in `z` fall in their `x`
  order here too: in replay 67152171, round 3, blue's Mustang 27, 6 raw
  behind Mustang 26 and left of it, is in the first batch and 26 in the
  second, and every search of 27's comes a tick sooner than its identity
  would put it.
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

**A cooling searches for nothing.** Its weapon names what the attack left it
for the whole cooling, the lock the failed check's search took even when that
search fell back on any live enemy and left the skill idle, and nothing when
the attack left it firing at a battlefield shield: a Phantom Ray whose lock dies, and whose next lock stands
inside the enemy's shield, names no unit while it cools, however long, and
locks again only once the cooling is over.

## A unit with nothing it can fire at walks on any enemy

A skill whose search finds nothing it can attack falls back on any live enemy,
of either domain, and is left idle. The fallback scores the candidates as the
search does, where they stand when it runs, and offers the enemy's blocking
constructions only when nothing else stands. It belongs to the skill that
searches for its unit, every slot of the main skill, so a construction's
skill falls back too. An idle skill fires at nothing: what it fires at is
cleared, and its lock is only where the unit goes. It searches again on its
timer, as one with a target does, or at once when its lock dies.

The unit walks on that lock, sent as it is to a target it would attack, until
the lock is in touch: the whole metres between the two, edge to edge, are no
more than twice the unit's radius. There it idles, and it sets off again
once the lock is out of touch. So a Vortex, which cannot fire at aircraft,
that fells the last tower with only aircraft left locks onto the nearest one
and follows it about. A unit attacking when its search leaves it idle stops
first: `MotionAttackState.Update` asks `IsIdle` before anything and goes idle
on that update, and it sets off only from the idle state, on the next.

## A free-moving unit keeps its speed whichever way it faces

A unit whose rotate speed is below 180° a second, and whose body is more than
90° from the way it drifts, moves slower than its full speed. A free-moving
unit never does: `MotionController.CalculateMoveSpeed` returns the full speed
first when `FightMech.isFreeMove` is set. The table's own column is false for
every unit; `MechData.PreProcess` sets the flag for an id of at most 54 whose
bit is set in the literal `0x40000000040010`: the Melting Point (4), the
Wraith (18) and the unit whose id is 54. `scripts/extract/extract-units.py` writes it
as `free_move`. A Wraith drifting 100° off its facing in its M3 with seed
1787720817 publishes its full 10 m/s at tick 172.

## Attack scheduling

`RefreshAttackInterval` converts by the logical step. The interval it
converts is never under one tick: `AttackIntervalProperty` holds a skill's
interval, its corrections in, to the logical step at least, so a Spider Mine,
whose explosion's row has an interval of 0, reads 1 to the fight's last tick.
A staggered interval is floored at one tick again after its draw; one with no
stagger is the property's.
After a unit first enters Attack, the next update is the earliest release.

**A blow is fitted into its interval.** A blow starts by drawing its interval
and then converts its attack point and its backswing to whole ticks, each
truncated on its own. When the two together are longer than the interval, the
attack point is scaled by the interval over their sum, an `FPoint` product
truncated to whole ticks, and the backswing is what is left of the interval.
A unit whose attack point and backswing fit its interval keeps both. The
Rhino's fit its own interval and no longer fit the one Mechanical Rage leaves
it, so the technology brings its blow forward as well as its next one.

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

## A unit beside a flanked tower faces the flank

**A unit enters the fight facing its formation's way, unless it stands beside
one of its towers and the other side holds the flank there.** As the fight
starts, `TerritoryManager.RefreshMechDiretion` turns each unit to
`PlayerTerritory.GetAttackFacing`, and the presearch then scores from that
facing. A side's main region has two defence areas: the left one runs from the
region's left edge to the left tower's right edge, and from its back edge to
the tower's front edge, which is 150 metres short of the region's front; the
right one mirrors it. A unit standing in one, by its own position floored to
whole metres, faces a quarter turn from its formation towards that side when
the region beside the area, the other side's flank there, holds a formation.
Otherwise it faces as its formation does. So members of one formation that
straddles the area's front edge face two ways, and a unit on the flank itself,
or in a fight with nobody on the other side's flanks, is not turned.

## The body follows the lock, the weapons follow the attack target

A unit has two targets and the build keeps them apart: the mech's lock
(`FightMech.lockTarget`, recorded as `mech_lock_target`) and each skill's attack
target (`GetAttackTarget()`, recorded per skill in `skills`).
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

**A unit stops once, where it goes idle.** `MotionIdleState.Enter` calls
`RVOControllerFixed.StopMove`, which makes the point the unit stands on its
target; `MotionIdleState.Update` does not, and a skill cooling after its attack
does not enter the motion's idle again. So a unit an RVO solve nudges while idle
keeps the point it stopped at, and the next solve steers it back there.

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
fire at. With no sibling holdings it uses the ordinary search, the slot's own:
its search controller's `PerformNormalSkillSearch`, scoring from the slot's
weapon and in the slot's reach, so a Raiden's second gun with no sibling
holding anything takes a Crawler in its own reach that the core's search
leaves out. Otherwise it
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
not the ordinary search's sibling-exclusion loop. It passes over the slot
itself and the owner's main skill's first skill, the core of a main skill's
group; an extra skill's group, which does not hold that skill, passes over
none of its own ([extra_weapons.md](extra_weapons.md#a-group-of-beams)). The fixtures
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
The hold does not reach a skill already in its attack state:
`SkillAttackState.TryPerformAttack` asks whether the interval is up and the
target in angle, and nothing of the motion. A Wasp whose motion moved during
its backswing, and came back into its attack out of angle, fires on the first
tick its target is in its angle
(`tests/regression/fights/barrier-fortress-vs-wasps.yaml`).

## Normal target scoring and pre-fight acquisition

Among currently visible, fully rotated, unsplit-quadtree ordinary ground targets
with a unique optimum, the build picks by Q32 edge distance, strict
minimum-range exclusion, an angle factor, an out-of-range penalty and a strict
minimum score.

**The angle is measured from the searcher's main transform.** That is its
first weapon's when the weapon has a transform, and the mech's root when it
has none, whether or not the weapons turn at a speed of their own: a
Wraith, whose four weapons turn at 90° a second but carry no transform,
scores from its root's facing, not from where its first weapon points.

**A search reads the tick's start only when its skill asked for that.** At a
tick's start `FightCoreSystem.PreCalculate` lets each skill state prepare a
search, scored on where everything stands at that moment, and
`ScoreRatingTargetSelector.TrySelect` answers a prepared skill from those
scores. A prepared answer that has died since, a tower that fell that tick
among them, or no prepared answer at all, sends the search to `Select` over
every enemy the skill attacks, with no square: a Crawler walking on a tower
that falls takes the best-scored Fang anywhere. A skill that was not prepared,
or whose prepared answer a beam has turned to its side, falls through to
`PerformSearch`, which scores every candidate where it stands when the search
runs, in the square below; when the square answers nothing it tries one 200 m
wider and then one 300 m wider again, and then every enemy. In the square a
candidate is scored only if the target tree
holds it in a node that a square around the searcher overlaps, a square twice
`max(range, 400)` wide: at the fight's first tick, a Crawler of replay 134266831
round 3 scores 13 candidates, and not the enemy 538 m straight ahead of it,
which a square 800 m wide leaves out. A prepared search gathers its
candidates as it is prepared, from the tree and about where the searcher stood
then; units that update before the searcher may have moved to other nodes by
its turn, and the candidates stay the ones prepared. An attacking skill is
prepared only while its lock is absent or dead, an idle one when it can start a
search as the tick opens, its search timer run out or its lock absent or dead,
and a preparing or cooling one never: an idle Crawler whose lock dies during a
tick its timer was not due searches with `PerformSearch`; the prepared scores are cleared every tick. So a
Stormcaller whose live lock walks inside its minimum range during a tick
searches past it and takes the next target that very tick. `PreCalculate`
prepares only the mechs of the fight as it runs, alive and not travelling, and
`FightingState.Update` runs it after the modules, before the next update's
timers join the summons that are due: a summon that joins is no attacker
`TrySelect` answers for, and its search on that tick is a `PerformSearch`.
Nor does it prepare a skill a `SkillGroup` holds, a Wraith's or a Raiden's:
their every search scores candidates where they stand as it runs, after the
units of the sides that update first have moved.

**A blow that loses its target inside the minimum range gives its interval
back.** That search is `SkillAttackableChecker.CheckWhenLoseTarget`, and on an
attacking check of the main searcher, while a blow is under way, it calls
`FightSkill.ResetAttackData`: the interval is drawn again, and when no blow of
the attack state has run its cycle out (`performCount` 0) and the blow is not
still winding up on a target it can attack, `attackTime` is set to the
interval, so the next blow may start as soon as a target is in the area. A
lock that died is searched for without it, and keeps the time its wind-up
spent. Recorded in replay 201372157 round 2: a Stormcaller whose target
walked inside its 70 m minimum range two ticks into the wind-up read
`attackTime` 3 before the check and 132, its interval, after it, and wound up
again at tick 558, 49 ticks later rather than the interval's 132; twelve ticks into an earlier wind-up
whose target died, the same check left `attackTime` at 12.

**A blow is counted as it hands back its last phase.**
`SkillAttackController.ChangeToIdle` adds it to `performCount`: a blow with no
backswing as its attacking phase ends, once its burst's last projectile is
released or its sweep is over, and a blow with a backswing as the backswing's
controller hands back on its last tick. A Farseer's two-projectile burst
released on tick 2 reads 0 until its last projectile goes out on tick 6, and
the Rhino it fires at, whose backswing ends on tick 107, reads 1 on that tick:
`tests/projectile/fights/farseer-rhino-closing.yaml`.

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

**A projectile with a splash strikes only what its splash reaches.**
`DamagePerformer.Perform` performs a hit with a splash as `PerformRangeEffect`,
over everything within the splash of where it lands, and only a hit without
one as `PerformSingleEffect` on what it was aimed at, wherever that stands. A
projectile that follows its target lands its offset from it, so one whose
offset passes its splash misses.

**A projectile leaves for where its burst aimed it.** Every projectile of a
burst is released toward the target's position when the burst began plus its
offset; one that follows its target takes the target's position up again from
its first update, which is why the rule above reads as the live position. A
burst with no offset draws no points as it begins, and each of its projectiles
leaves for where its target stands as it is released, a dead target where it
died: a Centurion's second Homing Missile, a quarter second after the first,
leaves for where its target stood the tick before
(`tests/extra_weapon/fights/enhanced-range-homing-missile.yaml`).

**Two weapons split a burst's offsets in three dimensions.** Each weapon takes
the half of the offsets on its side of the line from the weapon to the target,
ordered by the angle it sees, and the angle and the side are measured from the
weapon's height to the target's: an Overlord shooting down at a Crawler, and
two Overlords shooting at each other at the same height, order them so.

**A burst that allocates evenly shares its projectiles among units.** A skill
whose row sets `isEvenlyAllocated` aims its burst with
`EvenlyAllocatedAttackTargetPositionController`. On its first projectile
(`Prepare`) it takes the other sides' units within the skill's range and its
`extraSearchRange` of the unit, each measured less both radii, in the order
their trees answer a square twice that wide (`RangeTargetCalculator.CalculateRangeTargets`,
units only, fully visible), or the attack target alone when none is. It
shuffles them by the side's stream, each place swapped with one drawn from the
whole list (`IListExtensions.ShuffleSync`); each unit takes the burst's count
over theirs, and the rest go one each to units drawn one by one from those not
drawn yet (`GetRandomTargetsByRemain`, `RandomElementSync`). It draws the
offsets then, round by round over the units and then for the units drawn, each
`RandomInsideSphere` about where the attack target stands. Each projectile as
it leaves takes the first unit of the list that lives, dropping the dead, and
puts it last (`UpdateCurrentTarget`), leaves the next of the two weapons in
turn (`GetWeaponIndex`), aims at where its unit stands then
(`GetTargetPosition`), and takes that unit's next offset, none once they have
run out (`GetAndDeletePositionOffsets`). Its climb is measured to where its
unit stands. Swarm Missiles' 46 share two units 23 each.

**A projectile may climb before it flies.** A skill with a pre-flight height
sends its projectiles straight up at their speed, neither following the target
nor landing, until they stand at or above the height, the last step taken whole:
a Farseer's shot climbs 7 metres a tick to 63 for its 60. The height is the
pre-flight height times the distance to the target over the attack range, at
the range and beyond the whole of it (`ProjectileSystem.Create`). The distance
is measured for each projectile as it is created, from where its owner stands
then, to where the burst's target stood as the burst began: an Overlord pushed
aside between two releases of one burst climbs its later projectiles to
another height, and the first step of a climb, rounded through the reciprocal
of what is left to climb, lands a few raw units apart. A target of the side
that updates first has already moved that tick: a Farseer's burst at a Rhino
walking at it climbs both its shots to the height the Rhino's new position
gives, 30.92 m where the tick's start would give 31.31.

**A burst goes on after its target leaves reach.** A skill is not checked
between the projectiles of a burst, so a bodyless unit whose target walks out
of reach mid-burst moves after it and fires the rest: an Overlord follows its
Crawler and fires its fourth shot before it goes idle. Nor is it checked on the
update its last shot leaves: a Phantom Ray whose Rhino walks out of reach moves
after it on that update, keeping its lock, and goes idle on the next, when the
check fails.

**A single weapon lands its offsets last drawn first.** A Phantom Ray's first
projectile lands the second offset its burst drew, and its second the first.

**A projectile in simulated motion that lands on a dead target does nothing.**
A skill with `isSimulateMode=true` whose projectile arrives after its target
died deals no damage, splash included: a Fire Badger's or a Typhoon's shot at a
Crawler another shot killed while it flew leaves the Crawlers beside it
untouched, and so does a Fire Badger's shot at a wall block that fell while it
flew. Any other projectile still strikes where it lands, as an Arclight's
does.

**A shot that follows its target is spent on nothing out of its owner's
reach.** Each update, before it lands, a projectile of a skill with
`isLockTarget=true` asks whether it stands within its reach of its owner,
edge to edge in three dimensions from where the owner stands, dead or
alive. Its reach is the owner's radius, the skill's attack range and the
target's radius, all as the projectile was made; against a target flying at
another height than the owner, the hypotenuse of that and the 70 metres
between them. One out of reach is removed with no damage. A Mustang's shot,
135 metres of range, at a Crawler running away is spent 143.06 metres from
the Mustang, against the 3 + 135 + 2 + 3 it may stand at. A Stormcaller's
shells lock nothing and are never asked.

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

**A skill's own hit strikes only the domain of what it attacks.** A blow or a
beam asks the skill whether its attack target, or its lock when it has none,
flies, and its splash takes only units of that domain, as a projectile's
takes only its target's: a Melting Point's beam at a Phoenix in the air
splashes no Tarantula standing under it, whatever the beam could attack
otherwise.

**A query reads a tree a node at a time.** It takes a node's own before its
children's, the children in order, wherever a query reads a side's tree, as
a terrain finding who stands in it and a kill's shares of experience do.

**A side's tree takes its objects in the order they joined the side.** As
the fight starts it takes its towers, then its units, then its
constructions. A node holds twenty before it splits, the split hands each of
its own that fits a child down, the last first, and a node never joins its
children again: two units that stay in one node all fight are read in the
order the split left them.

**A splash reaches what its edge is within, as the fast square root measures
it.** An actor is struck when its distance from where the hit lands, less its
radius, is no more than the splash's radius, and the distance is the fixed-point
one the build takes with its fast square root, which errs by a few parts in a
hundred thousand. On the edge that decides it: a Crawler 6.99976 metres from
where a Tarantula's 5-metre splash lands, with its own 2, reads 7.00034 and is
spared.

**A splash passes over a unit underground.** The units a splash takes are
those valid as targets visible down to stealth, and a burrowed Sandworm is
hidden below that: a Shockwave landing beside one spares it.

**A skill's splash is its description's plus every splash value written onto
it.** An officer's, a technology's and an item's `splash_range_value` land in
the skill's `SplashRangeValue`, as Explosive Ammo's `splash_range` does, and
the splash a hit takes is the description's radius with their sum added, no
rate applying. A skill with no splash of its own that is given one splashes
as any other: a Marksman with Explosive Ammo strikes the Crawlers around the
one it shoots.

A Rhino's main skill, under those same baseline constraints and as a
single-target direct effect, reaches its description's damage through
`SkillDamageProvider -> FightSkill.GetDamage -> DamageProperty`. The killing
blow is clamped to the remaining life.

**A skill's damage reduce rate base comes last.** `DamageProperty.CalculateDamage`
multiplies the factor its damage rates meet in by the skill's
`DamageReduceRateBase` before it reaches the damage, and
`DamageCalculator.GetNormalDamage`, which a recording reads as the unit's
damage, leaves that rate out. An extra weapon technology's
`allWeaponReduceDamageRate` writes it
([extra_weapons.md](extra_weapons.md#a-group-of-beams)).

## Lifesteal

**A unit's skill hands back a share of the life each of its hits took.** After
a hit, the provider that dealt it hands the life it took from everything it
struck, summed, to the skill's hit effects, and a lifesteal source's effect
gives the skill's owner the whole part of that sum times the source's
multiplier. It gives nothing to an owner that is dead or at its maximum life,
and never past the maximum: what the owner was short of is what it gets. A
direct blow, a beam with or without a splash and a projectile all hand their
hit on, and so does a hit a shield took in part, for the life the rest took.
A turret's, a mine's and a battle skill's hits reach no unit's skill.

The multiplier is a fixed-point number, and the share is truncated: 0.8 is
stored a little short of itself, so 5 of damage gives back 3.

**A unit has one lifesteal source in force: the one of the highest
priority.** An item and a technology are both lifesteal sources, and the unit
holds one provider for them, which enables one. An item's priority is above a
technology's, so a unit with Absorption Module and Energy Absorption steals at
the item's multiplier alone; the two do not sum. A technology's source does
nothing while its owner's technologies are disabled, and an item's is never
disabled.

The rows are [`config/equipment_effects.yaml`](../../config/equipment_effects.yaml)'s
and [`config/technology_effects.yaml`](../../config/technology_effects.yaml)'s
`lifesteal_multiplier`; each source also writes its row's corrections, as any
item and technology does.

## Repair

**A unit with a repair source repairs itself while it is hurt, after a
second and every 0.1 second.** Each tick, after every unit has updated and
every projectile landed, a unit that is alive and short of its maximum life
advances two clocks by the tick: a start clock, which begins at −1 second,
and once that reaches the source's start time, a repair clock. When the
repair clock reaches the source's interval it is set back to zero and the
unit gains the whole part of its maximum life times the source's rate, as
far as its maximum. Nothing sets the start clock back during a fight: a unit
that is healed whole and hurt again repairs at once.

A fixed-point comparison counts 43 raw units as equal, and the tick is a few
raw units short of 0.05 second: twenty hurt ticks reach the second and two
reach the interval. With a start time of 0 and an interval of 0.1 second, as
every standard source has, a unit repairs from its twentieth hurt tick on,
every second tick.

Nano Repair Kit and Field Maintenance are repair sources, one per unit as
lifesteal's are, the item's priority above the technology's. Their numbers
are [`config/equipment_effects.yaml`](../../config/equipment_effects.yaml)'s
and [`config/technology_effects.yaml`](../../config/technology_effects.yaml)'s
`start_time`, `recovery_duration` and `recovery_life_rate`. A technology
whose `auto_recovery_state_type` is not 0 repairs only underground or
cloaked, which the simulator refuses.

## Personal shield

A unit with no shield still starts with its `EnergyShieldController` enabled.

**A unit with a shield source opens the fight with a shield of the whole part
of its maximum life times the source's rate, and a hit takes energy before
life.** While the shield holds energy, a hit of at least 1 takes as much as it
can of it and no life at all: what the shield held of it is what the hit
dealt, recorded and counted as such, and the rest of the hit is lost. Once
the shield is empty, hits take life. It does not refill.

Portable Shield is a source of its row's `shield_life_rate`, and an Energy
Shield technology a source of rate 1, whatever its row: the technology's
class answers the rate with a constant. A unit holds one source in force, an
item's over a technology's, as for lifesteal.

## Armour

**A unit with an armour technology loses a fixed amount less to each hit,
though never less than 1.** Once the rates on its damage taken have scaled a
hit, the unit's damage reduction comes off it, and a hit the reduction would
leave below 1 deals 1. The reduction is what its technologies write onto
its `reduce_damage_value`, summed: each its row's entry for the unit's level,
the last for a level beyond the list, as a skill reads its damage. A level-1
Rhino with Armor Enhancement takes 2269 of a Marksman's 2329, a level-3 one
2149, and a Mountain with Mountain Plating's 700 takes 1 of an Arclight's
365.

A summon's drop is the one hit the reduction does not take from, unless it
would leave less than 1. The reduction comes off before a shield takes the
hit, and a fire's, a buff's and an explosion's hits lose it too.

**A unit that travels in holds its armour once it arrives.** The reduction is
`ArmorStrengthenEffectProvider`'s, which `FightEffectSystem.ActiveEffect`
enables for a travelling unit only as it leaves its travel
(`SuperDeploymentController.ExitTravel`): a Rhino with Armor Enhancement that
travels in records no `reduce_damage_value` until the tick it arrives, while
its technologies' corrections are on it from the start.

The rows are [`config/technology_effects.yaml`](../../config/technology_effects.yaml)'s
`reduce_damage_value`; an armour technology also writes its row's
corrections, as any technology does.

## Aerial and ground targets

**A skill reaches and hurts a unit that flies by numbers of its own.** A
skill keeps two ranges, each its range with a value of its own added: the
ground one with its `AttackGroundRangeAddValue`, the air one with its
`AttackAirRangeAddValue`. While it locks a unit that flies it answers the air
one, and otherwise the ground one, for every check of reach: whether its
target is in range, whether its unit's motion stops for it, and the range past
which its search penalises a candidate. Its damage is two numbers the same
way, each its damage with a rate of its own added to the rates that correct
it: what it deals the target it attacks is the air one while that target
flies, and the damage a recording reads has neither rate in it. Aerial
Specialization writes 30 metres onto the air range and 0.9 onto the air rate:
a Marksman reaches an Overlord at 170 metres and deals it 4425 of its 2329.
Ground Specialization writes 2 onto a Wasp's ground rate, which deals an
Arclight 606 of its 202 and still reads 202, and Ground Targeting 60 metres
onto a Phantom Ray's ground range, 125 of its 65.

**A dead unit's shot lands by what it aimed at until the unit's `OnDead`.**
A projectile takes its owner's damage as it lands, the air one while the
owner's skill attacks a unit that flies. Only a living unit updates, and a
unit a hit kills leaves the fight when its `OnDead` comes, after every unit
and every projectile of the tick: its skill attacks what it attacked until
then. A Mustang's shot landing on a Phantom Ray on the tick the Mustang died
deals its air damage; one landing on a later tick, its skill having left the
fight, deals its ground damage.

**A search by distance counts an offset off a candidate's distance.** Aerial
Specialization writes its metres into the skill's `AttackRangeValueAir` as
well, and Ground Targeting into its `AttackRangeValueGround`, and each then
turns its unit's main skill's search from `Normal` to
`DistanceIntensify`, whose selector takes the skill's `AttackRangeValueAir`
and `AttackRangeValueGround` as it is made. Scoring a candidate, the selector
counts the offset for the candidate's domain off its distance, as it counts
the 40 metres it adds for one that is not visible. Only that term moves: the
distance added after it, and the range the out-of-range penalty compares,
are the candidate's own. So a Phantom Ray scores an Arclight 145 metres off
at 85 and takes it over Phoenixes 176 metres off, a Marksman takes an Overlord 11 metres further
off than a Mountain, and takes the Mountain over an Overlord scored nearer
but standing beyond its range. Its extra skills, the search a unit makes for
itself, and the fallback search over every live enemy keep the `Normal`
selector they were made with.

**A technology turns a skill onto or off aircraft.** A skill attacks
aircraft while its row's air flag, its `AirAttackValue` and its
`CanAttackAir` sum above zero, and the ground likewise. An air-attack
technology adds -1 to the main skill's `AirAttackValue` if its row attacks
aircraft and 1 if not: a Fang with Grenade Launcher passes an Overlord over,
and an Arclight, a Tarantula or a Sandworm with its technology attacks one.
Where the technology's row sets `extraSkillEffect`, each extra skill gains the
same value: a Tarantula's Spider Mine skill, whose row attacks neither domain,
then attacks aircraft only, and searches for them.

**A projectile's speed is its row's with the skill's value added.** Grenade
Launcher's -140 sends a Fang's shell 140 metres a second of its row's 280.

## A second damage around a hit

**A unit's second damage strikes around each of its main skill's hits, after
the hit.** Once a hit has struck what it strikes and handed its damage to the
skill's hit effects, every object of the other side within the second
damage's range of where it landed, of the domain the hit was aimed at, takes
the second damage, except what the hit itself struck unless the row says the
main target may be hit too. Shockwave's 75 reaches 30 metres: an Arclight's
shell that fells six Crawlers deals the eighteen others around them 75 each,
as `damage` events of the shell after its own.

**The second damage is the row's, raised by tower buffs and damage taken.**
Where the row says buffs reach it, the damage is the attacker's tower buffs'
rate on it, and then each struck unit's rate on the damage it takes; the
attacker's other damage rates never reach it, and the struck unit's rate
does not apply a second time as the hit is taken. An Arclight whose skill
deals 3.12 times its damage still deals 75, and a Sandworm under a 0.5 rate on
its damage taken takes 112.

**A second damage needs its unit alive.** A shell an Arclight fired before it
fell lands and deals its own damage, and no second damage.

## A melee skill's range

**A melee skill reaches its row's range, whatever corrects it.** Its range
property reads neither the skill's `AttackRangeValue` and rate nor a buff's.

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

**A main skill starts its attack in its own update.** `SkillIdleState.TryPerform`
searches first, when its timer is up or its lock is gone, and then starts the
attack if what the skill fires at is in its attack area (`CanStartAttack`),
whatever the motion does after it. A Melting Point that locks a Crawler
prepares on that update; when one of its own beams fells the Crawler before
its motion updates, the prepare's check fails on the next update and leaves
it idle. So an idle skill holds no lock in its attack area, and its periodic
search takes what the search answers: a Melting Point whose weapons are still
turning onto one Crawler takes the Crawler they already face and prepares
against it, and one whose turret has just come onto its lock as its timer runs
out takes the nearer unit the search answers. A grouped or standalone main
skill, and a unit that searches for itself, are still started from the
simulator's motion.

**A turret does not turn on the tick its unit sets off.** A weapon turns in
its skill's update only if it has a transform of its own
(`FightWeapon.CanRotate`), which a unit's ordinary weapon has not; its turret
is turned by the motion, `MotionAttackState.AttackRotate` and
`MotionMoveState.NormalRotate`. The tick the motion changes to
`MotionMoveState` the old state returns as it changes and the new one is not
updated, so nothing turns, whichever state the unit leaves: a Melting Point
whose Crawler an ally kills sets off for the next one with its turret still,
and so does a Sledgehammer whose lock walks out of its reach.

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
on the next, which is the fight's last. `DeadEffectSystem` writes them there,
after the projectiles that tick resolved and before what those killed: a
Wasp's shot at a tower torn down is removed before the tower falls.

**A tick that leaves neither side a unit takes both sides' towers down, when
no projectile is in flight.** A Missile Strike landing among both sides'
Crawlers, before any unit updates, leaves both teams' counts at zero, and all
four towers fall on that tick. Two sides that kill each other's last units
while shots are still in the air end the fight as the drain ends it, with
their towers standing. A side whose last unit dies on the tick a shot in the
air lands still loses its towers then, the fight ending on the next: a
Sandworm's blow killing the last Overlord as a tower's shot lands
(`tests/anti_air/fights/anti-aerial-sandworm.yaml`). Only a fight that waited
on its shots alone, every last unit gone on an earlier tick, ends with its
towers standing.

**On the tick a side loses its last unit, a winner's unit that updates after
that death takes one of the loser's towers.** It searches as it would on any
tick, and a tower is an ordinary candidate: nothing in target selection asks
whether an actor is a tower. With no enemy unit left, a tower is what the
search finds, and the unit turns toward it. A unit that updated before the
last death has already searched this tick and holds what it held.

**From the next tick the fight is off.** Once at most one side has a live
unit, every unit updates with the fight off, and no skill runs its state
machine. A skill that holds a lock leaves the fight: its attack stops, a
burst still firing included, it goes idle, and its lock is cleared. A skill
that holds none is not updated at all. A manager holding fire, a Sandworm's
below, is the exception (`SkillManager.Update` asks `isHoldFire` first): it
updates its main skill as in a fight, so a Sandworm below whose lock died
with the fight searches, takes the defeated side's tower and walks on to it
until the fight ends.

**When the fight stops, a unit's motion loses its target, and one that was
moving stops.** `MotionIdleState.Enter` calls `RVOControllerFixed.StopMove`,
which asks for no speed at the point the unit stands on, so the next solve
publishes none: an Overlord handed a tower on the tick the last enemy died,
and idle from the next, stays where it stood rather than walking on at the
next publish. A unit walking a Mobile Beacon is the exception while the won
fight runs on: its command stays active
([battle_skill.md](battle_skill.md#a-mobile-beacon)), so an attack changes to
moving rather than idle, and the unit goes on moving and turning to where it
moves. The fight's end idles every motion and stops every unit.

**A won fight runs on without ending its skills' coolings.** A skill that
was already cooling has no lock left, since finishing its attack cleared it,
so the fight's end does not reach it. It goes on cooling and goes on naming
what it named, the dead last enemy included: three Phantom Rays cooling on a
Crawler that died with the fight still name it for the five ticks after.
No skill updates once the fight is won, so the cooling does not run either:
a Phantom Ray that would have finished cooling three ticks after the
decision is still cooling when the fight ends. A slot of a grouped skill is a
skill of its own: a Raiden's second gun cooling as the fight is decided goes
on naming the Vortex it fired at, as its core lets its lock go. A skill that was attacking
still holds its lock, leaves the fight, and names nothing.

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

- A Sledgehammer whose lock walks out of its reach sets off with its turret
  still, and a Sandworm below when the fight is decided takes the defeated
  side's tower and walks on to it until the fight ends:
  `tests/corpus/fights/201373545-r4.yaml`, ticks 504 and 1028. A Farseer
  sets off as a Sledgehammer does: `tests/corpus/fights/134267654-r3.yaml`,
  tick 1739.
- A prepared search scores the candidates the tree held about the searcher as
  the tick opened: `tests/corpus/fights/67260372-r4.yaml`, tick 42, where
  Crawler 311 locks Centurion 159, which the tree by its turn no longer
  answers for its square.
- The presearch staggers each side's first searches in its update order, not
  its identities': `tests/corpus/fights/67152171-r3.yaml`, blue's Mustang 27,
  whose searches fall on ticks 1, 12, …, 133.
- A main skill starts its attack in its own update: a Melting Point prepares
  on the Crawler it locks, and goes idle when its own beam fells it first:
  `tests/extra_weapon/fights/energy-diffraction-crawler.yaml`, tick 283, and
  `tests/corpus/fights/67158166-r5.yaml`, tick 234. Its idle search takes
  what the search answers as its turret comes onto its lock:
  `tests/corpus/fights/67158166-r5.yaml`, tick 316.

- A Mustang's shot at a Crawler running away is spent on nothing out of its
  owner's reach: `tests/corpus/fights/67156074-r5.yaml`, tick 539; and
  `tests/corpus/fights/268477093-r3.yaml`.

- A Crawler on the very edge of a Tarantula's splash is spared:
  `tests/corpus/fights/67159970-r2.yaml`, tick 188.
- A Fire Badger's shot at a wall block that fell while it flew lands on
  nothing, splash included, and on the fight's last tick a Wasp's shot at a
  tower torn down is removed before the towers fall:
  `tests/corpus/fights/67158166-r2.yaml`, ticks 290 and 765.

- A Vortex left with only aircraft to fire at locks one, walks on it, idles
  in touch, searches every eleven ticks and sets off again when its lock
  changes: `tests/corpus/fights/201370830-r3.yaml`, ticks 1227 to 1347.
- A Crawler attacking a tower that another Crawler fells locks the Abyss,
  which it cannot fire at, stands idle on that update and walks on it from the
  next: `tests/sweep/fights/turret-1787720817.yaml`, ticks 1091 to 1093.

- A unit in a defence area beside a flanked tower enters the fight a quarter
  turn towards the flank, and one of the same formation outside it does not:
  `tests/corpus/fights/268487043-r4.yaml`; in replay 134270595 round 4 the
  back row of a Fang formation faced 270 degrees and its other rows 0, read
  from `PresearchTargetController.SearchTarget` with a temporary hook.
- An idle Marksman an RVO solve nudges while it cools goes back to the point
  it stopped at: `tests/corpus/fights/201373545-r2.yaml`, ticks 1112 to 1121.
- A tick that leaves neither side a unit, with no shot in the air, fells all
  four towers: `tests/battle_skill/fights/missile-strike-both-sides.yaml`.
- Every unit's current interval, its stagger and the three readings that
  complete it, on every tick of the standard unit fights, one directory per
  unit, `tests/marksman/fights/` among them.
- A skill whose row's interval is 0 reads one tick, on the fight's last tick
  too: Spider Mines that outlive the fight,
  `tests/extra_weapon/fights/spider-mine-outlives-the-fight.yaml`.
- A grouped skill's slots, their locks and their reach, and a grouped unit's
  core interval: `tests/wraith/fights/`.
- Each slot's own states, the core leaving its attack while its siblings go
  on, the unit's lock following the latest slot, a sibling giving up a unit
  it shares with the core, and a slot's interval outliving its attack,
  in the Wraith's M2, M3 and M6 fights: `tests/wraith/fights/`.
- A sibling that has struck keeping a unit it shares with siblings still
  preparing, and a free-moving Wraith at full speed off its facing, in the Wraith's M3 with
  seed 1787720817: `tests/wraith/fights/`.
- Where a charging Crawler is sent, and a Marksman's quick switch that cannot
  follow a kill: `tests/regression/fights/`.
- The body travelling toward the lock, attacking without moving, and the
  weapons on a construction while the body keeps the lock:
  `tests/construction/fights/`.
- Target scoring, ordinary projectiles, damage clamping, the first-attack
  delay, the backswing, dead-target retention and the endgame ordering, in the
  ordinary fights: `tests/regression/fights/`.
- A unit's personal shield enabled with no shield of its own, in every unit's
  standard fights, `tests/rhino/fights/` among them.
- A following projectile's offset, the order a single weapon lands its
  offsets, a simulated-motion shot at a dead unit, a weapon's index, and a
  splashing beam, in the standard fights of the Phantom Ray, Fire Badger,
  Typhoon, Hound, Sabertooth and Melting Point: `tests/phantom_ray/fights/`,
  `tests/fire_badger/fights/`, `tests/typhoon/fights/`, `tests/hound/fights/`,
  `tests/sabertooth/fights/`, `tests/melting_point/fights/`.
- A burst's aim, a climbing projectile, two weapons' offsets in three
  dimensions, a burst that goes on after its target leaves reach, a turret on a
  retarget, and a Vortex's single grouped weapon and its kill, in the standard
  fights of the Farseer, Overlord, Melting Point and Vortex:
  `tests/farseer/fights/`, `tests/overlord/fights/`,
  `tests/melting_point/fights/`, `tests/vortex/fights/`.
- The blow waiting a tick after the idle state is left, and an idle skill
  giving up a lock it cannot fire at, in the standard fights of the Fortress,
  Sledgehammer, Typhoon and Melting Point: `tests/fortress/fights/`,
  `tests/sledgehammer/fights/`, `tests/typhoon/fights/`,
  `tests/melting_point/fights/`.
- A projectile's climb measured as it is created, and a moving unit stopped
  when the fight stops, in the Overlord's standard fights:
  `tests/overlord/fights/`.
- A burst's climb measured to where its target stood after the target's own
  side moved that tick, for a Farseer's and an Overlord's bursts at a Rhino
  walking at them: `tests/projectile/fights/`.
- A Phantom Ray moving after a Rhino on the update its burst's last shot
  leaves, and going idle on the next:
  `tests/projectile/fights/phantom-ray-rhino-walks-off.yaml`.
- A cooling that goes on through a won fight, and an attack that goes idle,
  in the Phantom Ray's standard fights: `tests/phantom_ray/fights/`.
- A grouped slot whose siblings hold nothing searches from its own weapon and
  in its own reach: `tests/corpus/fights/134370978-r8.yaml`, tick 145, Raiden
  386's second gun.
- A cooling that does not run through a won fight, a Raiden's grouped slot's
  among them: `tests/corpus/fights/201477923-r5.yaml`, ticks 1618 to 1625,
  Phantom Rays 56 and 58 and Raiden 19's second gun.
- Wasps still on a Mobile Beacon when the fight is won going on moving and
  turning until the fight ends, and idle at its end:
  `tests/battle_skill/fights/beacon-wasps-won.yaml`,
  `tests/corpus/fights/134270595-r4.yaml`.
- A winner's unit that updates after the last death taking a tower, and
  letting it go the tick after, in the Stormcaller mirrors:
  `tests/regression/fights/`; a burst the fight's end stops, in the
  Phantom Ray's M2 fight: `tests/phantom_ray/fights/`.
- The Raiden's fusillade, its siblings' towers, a core taking a sibling's unit,
  siblings cooling and searching on their timers, and its weapons' poses, in
  the Raiden's standard fights: `tests/raiden/fights/`, and nine
  Raidens' blows landing on nine Fangs: `tests/raiden/fights/`.
- A presearched target a few raw units left of straight ahead faced at
  +0.245°, in the standard fights of the Steel Ball, Stormcaller, Hound, Fire
  Badger and Phantom Ray: `tests/steel_ball/fights/`,
  `tests/stormcaller/fights/`, `tests/hound/fights/`,
  `tests/fire_badger/fights/`, `tests/phantom_ray/fights/`.
- A Stormcaller whose live lock walks inside its minimum range taking blue's
  interceptor on that tick, and a second doing the same three ticks later:
  `tests/search/fights/lock-inside-min-range.yaml`.
- A red Wraith's search scoring blue's units where they stand after they
  moved, and taking another than the tick's start would give it:
  `tests/corpus/fights/201340110-r4.yaml`, tick 121.
- A Phantom Ray whose next lock stands inside the enemy's shield naming no
  unit for its whole cooling: `tests/corpus/fights/201370830-r5.yaml`, ticks
  113 to 130.
- A Melting Point's beam at a Phoenix splashing no Tarantula under it:
  `tests/corpus/fights/67159970-r6.yaml`, tick 371.
- Summons that join on one tick searching where everything stands then, and
  locking the enemy summons that joined with them:
  `tests/battle_skill/fights/underground-threat-both-sides.yaml`, tick 48.
- A Stormcaller's blow that lost its target inside the minimum range giving
  its interval back, and one whose target died keeping it:
  `tests/corpus/fights/201372157-r2.yaml`.
- A blow whose attack point and backswing outlast its interval is fitted into
  it: `tests/modifier/fights/technology-interval-value.yaml`.

- A hit hands back the whole part of its life times the multiplier, as far
  as the owner's maximum, through a blow, a projectile and a beam with no
  splash, and an item's source overrides a technology's:
  `tests/lifesteal/fights/absorption-module.yaml`,
  `tests/lifesteal/fights/technology-lifesteal.yaml`,
  `tests/lifesteal/fights/absorption-module-over-technology.yaml` and
  `tests/lifesteal/fights/technology-lifesteal-beam.yaml`.

- A splash value widens a skill's splash, from an item onto a skill with
  none and onto one with its own, from a technology and from an officer:
  `tests/splash/fights/explosive-ammo.yaml`,
  `tests/splash/fights/explosive-ammo-arclight.yaml`,
  `tests/splash/fights/assault-mode.yaml`,
  `tests/splash/fights/improved-overlord.yaml` and
  `tests/splash/fights/improved-tarantula.yaml`.
- A side's tree takes its towers, then its units, then its constructions,
  and a query takes a node's own before its children's, children in order:
  a splash at two Void Eyes in one node reads them in the order the units
  went in before their node split, tick 616, and a fire that Fangs of two
  children enter reads the first child's first, tick 83,
  `tests/corpus/fights/67156354-r3.yaml`.
- A unit repairs from its twentieth hurt tick on, every second tick, by
  the whole part of its maximum life times the rate, from an item and from a
  technology: `tests/repair/fights/nano-repair-kit.yaml` and
  `tests/repair/fights/field-maintenance.yaml`.

- A shield of the whole maximum life takes hits before life, its last hit
  only what it held: `tests/energy_shield/fights/portable-shield.yaml` and
  `tests/energy_shield/fights/energy-shield-technology.yaml`.
- An armour technology's reduction is its entry for the unit's level, taken
  off each hit and leaving at least 1: 60 and 180 off a Marksman's shot on a
  level-1 and a level-3 Rhino, `tests/armor/fights/rhino-armor-enhancement.yaml`
  and `tests/armor/fights/rhino-armor-enhancement-3.yaml`, and 1 left of an
  Arclight's hit on a Mountain, `tests/armor/fights/mountain-plating.yaml`;
  577 hits on six armoured Phantom Rays, 21 of them left at 1,
  `tests/corpus/fights/134258634-r4.yaml`.
- A skill locking a unit that flies reaches it by its air range and deals it
  its air damage: a Marksman with Aerial Specialization fires at an Overlord
  from 170 metres for 4425, `tests/anti_air/fights/aerial-specialization-overlord.yaml`,
  and a Wasp, a Mustang and a Farseer squad deal aircraft 1.9 times what they
  deal a Rhino, `tests/anti_air/fights/aerial-specialization-squads.yaml`.
- A dead unit's shot landing on the tick it died deals the air damage it was
  fired with, and one landing later the ground damage: a Mustang's on a
  Phantom Ray, `tests/corpus/fights/67154636-r6.yaml`.
- A search by distance counts the air offset off an aircraft's distance score
  and nothing else: the Marksman takes an Overlord further off than a
  Mountain, `tests/anti_air/fights/aerial-specialization-pick.yaml`, and the
  Mountain within its range over an Overlord beyond it,
  `tests/anti_air/fights/aerial-specialization-out-of-range.yaml`.
- The same against the ground: a Wasp's ground rate,
  `tests/ground_attack/fights/ground-specialization-wasp.yaml`, and a Phantom
  Ray's ground range and search offset,
  `tests/ground_attack/fights/ground-targeting-phantom-ray.yaml`.
- An air-attack technology turns a skill off aircraft,
  `tests/anti_air/fights/grenade-launcher-overlord.yaml`, or onto them,
  `tests/anti_air/fights/anti-aircraft-ammunition-arclight.yaml`,
  `tests/anti_air/fights/anti-aircraft-ammunition-tarantula.yaml` and
  `tests/anti_air/fights/anti-aerial-sandworm.yaml`, and an extra skill with
  it where its row says so,
  `tests/anti_air/fights/anti-aircraft-ammunition-spider-mine.yaml`.
- A projectile flies at its row's speed with the skill's value added:
  `tests/anti_air/fights/grenade-launcher.yaml`.
- A second damage strikes what lies around a hit and the hit did not strike:
  `tests/secondary_damage/fights/shockwave-crawlers.yaml` and
  `tests/secondary_damage/fights/shockwave-marksmen.yaml`.

### Read

- The presearch takes each side's mechs in their team's order, which
  `PrepareActors` sorts by `FightUtility.ActorComparer`, and staggers their
  first searches in ten batches:
  `PresearchTargetController.CreateMechDatas`,
  `PresearchTargetController.CalculateCountPerTime`,
  `PresearchTargetController.SearchTarget`, `FightTeam.PrepareActors`,
  `FightUtility.ActorComparer`.
- A weapon without a transform of its own turns only with the motion, and
  nothing turns the tick the motion changes to moving: `FightWeapon.CanRotate`,
  `FightSkill.Update`, `MotionAttackState.AttackRotate`,
  `MotionMoveState.NormalRotate`.
- A manager holding fire updates its main skill whatever the fight does:
  `SkillManager.Update`, `SkillManager.EnterHoldFire`.
- A projectile that locks its target is released with no damage out of its
  owner's reach: `FightProjectile.Update`, `FightProjectile.Init`,
  `FightProjectile.CalculateMaxMoveDistance`, `FightCalculator.IsInRange3D`,
  `FPoint.Sqrt`. The radius, the range and the reach were recorded by the
  `projectile_reach` channel.

- A splash is measured with the fast square root, edge to edge:
  `RangeTargetCalculator.CalculateRangeActorsInternal`,
  `FightCalculator.IsInRange2D`, `FightUtility.CalculateDistance2D`,
  `FVector2.Distance`, `FPoint.RawSqrt`, `FPCSMath.SqrtFastest`.

- A search that answers nothing falls back on any live enemy and leaves the
  skill idle, and an idle skill's lock search clears what it fires at:
  `SkillSearchTargetController.SearchLockTarget`,
  `SkillSearchTargetController.TrySearchAliveTarget`,
  `OpponentController.GetActors`, `FightConstruction.IsEnableBlock`,
  `SearchTargetController..ctor` (`aliveTargetSelector`, an
  `AliveTargetFilter`), `FightSkillBase.IsMainSearcher`,
  `FightSkill.SearchLockTarget`, `SkillIdleState.CanStartSearchTarget`.
- An idle skill's unit walks on its lock until it is in touch, and one
  attacking stops first: `MotionAttackState.Update`, `MotionIdleState.Update`,
  `AutoMoveBehaviour.IsActive`,
  `AutoMoveBehaviour.IsIdle`, `AutoMoveBehaviour.IsLockTargetInTouchRange`.

- A unit's facing as the fight starts is its territory's attack facing, a
  quarter turn in a defence area whose region holds something:
  `TerritoryManager.RefreshMechDiretion`, `PlayerTerritory.GetAttackFacing`,
  `PlayerTerritory.CreateLeftDefenseAreaLocal`,
  `PlayerTerritory.CreateRightDefenseAreaLocal`,
  `PlayerTerritory.DefenseArea.Contains`,
  `MapRect.Contains`, `MapRegion.IsDefenseRegionEmpty`,
  `TerritoryManager.PrepareDefenseRegion`.
- A blow draws its interval and is then fitted into it:
  `SkillAttackState.TryPerformAttack` calls `FightSkill.ResetAttackData`, which
  calls `FightSkill.RefreshAttackInterval`, before
  `SkillAttackController.PerformAttack`, which reads `FightSkill.GetAttackPoint`.
- An idle motion stops its unit when it is entered and not while it runs:
  `MotionIdleState.Enter` calls `RVOControllerFixed.StopMove` and
  `MotionIdleState.Update` does not.
- A skill's own hit takes the domain of its attack target, or its lock, for
  a skill that does not diffuse: `SkillDamageProvider.GetTargetType`,
  `ISkillData.IsDiffusion`, `DamagePerformer.PrepareRangeTargets`.
- A search's angle is measured from the skill's main transform, a weapon's
  or the root's: `FightSkill.GetMainTransform`.
- A search reads the tick's start only for a skill its state prepared, and
  the preparation is cleared every tick: `FightCoreSystem.PreCalculate`,
  `SkillAttackState.PreCalculate`, `SkillIdleState.PreCalculate`,
  `SkillState.PreCalculate`, `MainSkillSearchTargetController.PrepareSearch`,
  `ScoreRatingTargetSelector.TrySelect`,
  `SkillSearchTargetController.PerformNormalSkillSearch`,
  `ScoreRatingTargetSelector.ClearDatas`.
- A prepared answer that has died, or none, sends the search to `Select` over
  every enemy the skill attacks; a search not prepared tries the square, a
  square 200 wider and one 300 wider again, and then every enemy:
  `SkillSearchTargetController.PerformNormalSkillSearch`,
  `SkillSearchTargetController.PerformSearch`,
  `ScoreRatingTargetSelector.TrySelect`, `OpponentController.GetActors`.
- An idle skill prepares its search when its timer has run out or its lock
  is absent or dead: `SkillIdleState.PreCalculate`,
  `SkillIdleState.CanStartSearchTarget`, `FightSkill.CanStartSearchTarget`,
  `SearchTargetController.CanStartSearch`.
- A prepared search gathers its candidates as it is prepared:
  `MainSkillSearchTargetController.PrepareSearch`,
  `SkillSearchTargetController.PrepareAvailableTargets`,
  `ScoreRatingTargetSelector.Prepare`.
- A skill a `SkillGroup` holds is never prepared:
  `MainSkillSearchTargetController.PrepareSearch`.
- A search is prepared for the mechs alive and not travelling as the modules
  finish, and answered only for them: `FightingState.Update`,
  `FightCoreSystem.PreCalculate`, `SuperDeploymentSystem.IsTravelling`,
  `ScoreRatingTargetSelector.TrySelect`.
- A projectile's climb is scaled by the distance from where it leaves, as it
  is created: `ProjectileSystem.Create`, `IProjectileSkillData.GetPreFlyHeight`,
  `ProjectileFlyData.GetPosition`.
- Entering idle stops a moving unit: `MotionIdleState.Enter`,
  `RVOControllerFixed.StopMove`.
- A cooling only counts its time: `SkillCoolingState.Update`; what the
  weapons go on naming is what `FightSkill.SearchAttackTarget` left, the
  shield in place of the lock: `SkillSearchTargetController.SearchTargetShield`,
  `FightSkill.ChangeAttackTarget`.
- Leaving the fight ends a cooling: `FightSkill.ExitFight`,
  `SkillStateController.ChangeToIdleState`.
- The presearch faces a unit by the direction to its target, and a facing is
  mirrored only past `FPoint`'s tolerance: `PresearchTargetController.SearchTarget`,
  `FightUtility.ConvertToAngle`, `FightMech.UpdateRotation`,
  `FPoint.op_LessThan`, `FPoint.op_GreaterThan`.
- The team's interval stream is seeded by round and team:
  `FightTeam.RefreshRandomData`.
- The interval is converted by the logical step and truncated, and the offset
  is the skill's: `FightSkill.RefreshAttackInterval`,
  `FightSkill.GetCurrentAttackInterval`, `SkillData.attackDurationRandomValue`.
  A staggered interval below two ticks becomes one; with no offset the
  property's stands.
- A skill's interval is its row's with its corrections, held to one logical
  step: `AttackIntervalProperty.Refresh` ends in `FPoint.Max` with
  `FightUtility.DeltaTime`.
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
  `FightingState.Update`, `DeadEffectSystem.Update`. `DeadEffectSystem`
  updates after `FightCoreSystem` and `ProjectileSystem`:
  `FightController.AddModules`.
- A tower is an ordinary search candidate: `FightTeam.AddTower`,
  `FightTeam.AddActor`, `MechSearchTargetController.SearchLockTarget`.
- Once at most one side has a live unit the fight is off, and only a skill
  holding a lock is reached, to leave it: `FightCoreSystem.IsStepFinish`,
  `FightCoreSystem.TeamUpdate`, `FightMech.Update`, `SkillManager.Update`,
  `FightSkill.ExitFight`, `FightSkill.StopAttack`.

- Lifesteal: `DamagePerformer.PerformSingleEffect` and
  `DamagePerformer.PerformRangeEffect` hand the sum `FightActor.ReduceLife`
  returned to `IDamageProvider.DispatchHitDamageEvent`;
  `SkillDamageProvider` and `FightProjectile` pass it to
  `FightSkill.DispatchHitDamageEvent`, `SkillHitEffectController.PerformHitEffect`;
  `LifeStealEffectProvider.PerformHitEffect` checks the owner's life and
  `CanDisable`, multiplies by `ILifeSteal.GetLifestealMuliplier` and calls
  `FightMech.StealLife`, which `BuffManager.IsRecoverDisabled` stops and
  `FightActor.AddLife` caps. The provider's source is
  ``SingleEffectProvider`1.AddDataSource``'s first after sorting by
  `EffectProvider.IsOverrideEffect`, which compares `Equipment.GetPriority`
  and `Technology`'s; `Equipment`'s `CanDisable` is false and
  `Technology.CanDisable` reads `ignoreElectricEffect`.

- A splash value: `SplashEquipment.AddData` writes its row through
  `MechDataModifer.TryAddCommonData` and then ``Equipment`1.AddSkillData`` of
  `SkillDataChangeFloat.SplashRangeValue`; `FightSkill.GetSplashRange` adds the
  skill's data at that index to its row's splash.
- A query: `RangeItemController.UpdateAffectedActorChange` asks
  ``FightQuadtree`1.Query``, and ``FightQuadtreeNode`1.Query`` takes the
  node's elements and then queries each node of its list.
- A side's tree: `FightTeam.CreateQuadtree` inserts `activeActors` in
  order, which `FightTeam.AddTower`, `FightTeam.AddMech` and
  `FightTeam.AddConstruction` fill through `FightTeam.AddActor`;
  ``FightQuadtreeNode`1.Split`` and ``FightQuadtreeNode`1.RemoveElement``.
- Repair: `AutoRecoveryEffectProvider.DoActive` adds a unit to its side's
  `TeamAutoRecoveryManager`, whose `AutoRecoveryController` constructor
  calls `TeamAutoRecoveryManager+AutoRecoveryController.Reset`, setting the start clock to −1;
  `TeamAutoRecoveryManager.Update` advances the clocks by
  `FightUtility.DeltaTime`, compares them with `IAutoRecovery.GetStartTime`
  and `IAutoRecovery.GetRecoveryDuration` through
  `FPoint.op_GreaterThanOrEqual`, and calls
  `TeamAutoRecoveryManager.RecoveryMech`, which multiplies the maximum life by
  `IAutoRecovery.GetRecoveryLIfeRate` and calls `FightMech.RecoveryLife`.
  `FightController.AddModules` adds `AutoRecoverySystem` after
  `SuperDeploymentSystem`.

- A personal shield: `EnergyShieldProvider.AddEffect` sets the controller's
  `lifeRate` from `IEnergyShieldSource.GetLifeRate`, which
  `EnergyShieldEquipment.GetLifeRate` reads from its row and
  `EnergyShieldTech.GetLifeRate` answers as `FPoint.One`;
  `EnergyShieldController.Open` fills it to
  `EnergyShieldController.RefreshMaxEnergy`'s maximum life times that rate;
  `FightMech.OnHitted` takes `FPoint.Min` of the energy and the hit through
  `EnergyShieldController.ReduceEnergy` while `EnergyShieldBehaviour.IsAvaliable`,
  and calls `FightActor.ReduceLife` only otherwise.

- Armour: `ArmorStrengthenEffectProvider.EnableEffect` adds
  `IArmorStrengthen.GetReduceDamageValue` to a `FightMech`'s
  `MechDataChangeInt.ReduceDamageValue`, which
  `ArmorStrengthenTechnologyData.GetReduceDamageValue` reads from its list at
  `ISkillOwner.GetLevel`, clamped to its last entry, as
  `SkillData.GetDamage` reads a skill's. `FightCalculator.PerformHitTargetEffect`
  on a `FightMech`, after the amplification, sets a positive hit that
  `FightMech.GetDataInt` of it would leave below 1 to 1, and otherwise takes
  it off unless the hit's provider is a `SupportUnitDamageProvider`; every
  caller passes `isFirstHit` true, and `FightMech.OnHitted` takes the shield's
  energy afterwards.

- A dead unit keeps its skill until its `OnDead`: `FightCoreSystem.TeamUpdate`
  updates a `FightMech` only while `FightActor.IsAlive`; `FightActor.ReduceLife`
  hands an emptied actor to `DeadEffectSystem.OnActorDead`, and
  `DeadEffectSystem.Update` calls `FightMech.OnDead`, which calls
  `FightMech.ExitFight`, after `ProjectileSystem`.
- Aerial and ground targets: `FightSkill.GetAttackRange` answers
  `FightSkill.attackRangeAirProperty` while `FightActor.IsFly` of its lock and
  `FightSkill.attackRangeGroundProperty` otherwise;
  `AttackRangeAirProperty.GetAttackRange` adds `SkillDataChangeFloat.AttackAirRangeAddValue`
  to `AttackRangeProperty.GetAttackRange`. `DamageCalculator.GetAttackDamage`
  reads `DamageCalculator.airDamageProperty` while its skill's attack target
  flies, `DamageCalculator.GetNormalDamage` always `DamageCalculator.groundDamageProperty`,
  and `AirDamageProperty.GetExtraAddRate` answers `SkillDataChangeFloat.DamageChangeRateAir`.
  `SearchTargetSpecificProvider.DoEnable` adds `ISearchTargetSpecific.GetAirTargetOffset`
  to `SkillDataChangeInt.AttackRangeValueAir` and the skill's air range, then
  calls `FightSkill.ChangeSearchTargetType` with
  `SearchTargetSpecificTech.GetSearchTargetType`, `SkillSearchTargetType.DistanceIntensify`;
  `SearchTargetController.SetTargetSelector` hands that selector
  `SearchTargetController.GetAttackRangeValueAir` as its
  `ScoreRatingTargetSelector.airTargetDistanceScoreOffset`, which
  `ScoreRatingTargetSelector.Selector.Calculate` passes for a candidate that
  flies, less `ScoreRatingTargetSelector.invisibleActorDistanceScoreOffset`
  for one not visible, to `DistanceScoreCalculator.Calculate`, which takes it
  off the distance. `SearchTargetController.Change` replaces
  `SearchTargetController.targetSelector` alone. `SearchTargetSpecificTech.AddData`
  writes the row's `SearchTargetSpecificData.airDamageChangeRate`, and
  `DamageIntensifyTech.AddData` the rows' `DamageIntensifyTechnologyData.groundDamageChangeRate`
  and `DamageIntensifyTechnologyData.airDamageChangeRate` the same way.
- Turning onto or off aircraft: `FightSkill.IsAirAttack` sums the row's
  flag, `SkillDataChangeInt.AirAttackValue` and `SkillDataChangeInt.CanAttackAir`;
  `AirAttackEffectProvider.SwitchMechAirAttackEnabled` adds -1 to a
  `FightMech`'s main skill's `AirAttackValue` when `ISkillData.IsAirAttack` and 1
  otherwise, and, when `ISkillDataChangeDataSource.IsExtraSkillEffect`, the
  same to each of `ISkillOwner.GetExtralSkills` that does not hold it already.
- A second damage: `DamagePerformer.PerformRangeEffect` calls
  `DamagePerformer.PerformSecondaryEffect` after `IDamageProvider.DispatchHitDamageEvent`;
  `DamagePerformer.CheckSecondaryDamageApplied` refuses it while
  `ISkillOwner.IsTechnologyDisabled`; `DamagePerformer.PerformSecondaryRangeEffect`
  takes `DamagePerformer.PrepareRangeTargets` within `SecondaryDamageInfo.SplashRange`
  for the main target's `FightActor.IsFly`, less the hit's targets unless
  `SecondaryDamageInfo.CanMainTargetBeHit`;
  `DamagePerformer.CalculateSecondaryDamageByAttackerBuff` scales
  `SecondaryDamageInfo.Damage` by `BuffManager.GetTowerBuffDamageChangeAddRate`
  and `BuffManager.GetTowerBuffDamageChangeReduceRate`,
  `DamagePerformer.CalculateSecondaryDamageByTargetBuff` by the struck unit's
  `BuffManager.GetAmplifyDamageAddRate` and `BuffManager.GetAmplifyDamageReduceRate`,
  each when `SecondaryDamageInfo.CanBeAffectedByBuff`, and
  `DamagePerformer.PerformSecondaryHitTargetEffect` hands it to
  `FightCalculator.PerformHitTargetEffect` with `isAmplifyDamageAffected` false.
- A splash over a unit underground: `RangeTargetCalculator.CalculateRangeActors`
  asks `FightCalculator.IsValidTarget` with `ActorVisibility.Stealth`, and
  `FightActor.IsValidTarget` refuses a visibility above it.
- A melee skill's range: `AttackRangeProperty.GetAttackRange` adds
  `SkillDataChangeFloat.AttackRangeValue` only when not
  `ISkillData.IsMeleeAttack`, and `AttackRangeProperty.Refresh` applies the
  rates only then.

### Not established

- **A projectile's offset beyond its splash.** That one misses every unit
  follows from the read hit; no pinned fight of this version records it, the
  Homing Missile that showed it now drawing no offset.
- **A melee skill's range against a correction.** No pinned fight of this
  version records a melee skill with a range correction: Anti-Aerial, whose
  20 metres a Sandworm recorded and did not reach, now raises its damage.
- **A second damage raised by the struck unit's damage taken and not by the
  attacker's damage rate.** The corpus round 268447927 round 6 showed it on
  the previous version; on this one the simulator diverges from the round on
  tick 236, and it is not pinned.
- **Why a dead unit's shell deals no second damage.** The corpus round
  268447927 round 7 shows it at tick 197, and the simulator follows it; no
  read call names the check, and no pinned fight records it.
- **A second damage with battlefield shields, lifesteal, or a row that
  disables technologies or writes a buff** (Electromagnetic Cloud). The
  simulator refuses each.
- **Repair with its technologies disabled**, which stops the clocks
  (`AutoRecoveryEffectProvider.DisableEffect`) and which no recorded fight
  does; and which of an item and a technology a unit with both keeps, since
  every standard source repairs by the same numbers.
- **Lifesteal under a recovery-disabling buff or with its technologies
  disabled.** Only the Ignite buffs disable recovery, and the recorded fight
  that runs one stops a repair
  ([technology_effects.md](technology_effects.md#buff-technologies)), not a
  lifesteal; no recorded lifesteal unit had its technologies disabled. Which of two
  sources of one priority that answer differently a provider enables is not
  read, and a summon's lifesteal is not measured.

- **The interval stream.** The order in which one member's several skills
  consume it, and what else consumes from it once the fight is running.
- **A disabled technology's interval** beyond a plain technology's
  ([technology_effects.md](technology_effects.md)).
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
  order they are first named; a fusillade of weapons that do not strike, which the simulator refuses; which construction a
  striking slot takes, which the simulator refuses, since a Raiden recorded
  against a Defensive Wall (`layouts/raiden-wall-slots.yaml`) struck a block
  with its core alone where the simulator's siblings would each have; redistribution of
  wall blockers; and whether an idle slot that
  finds a unit beyond its reach keeps it as its lock, which no recorded
  sibling has done.
- **Touch range on another branch.** `IsLockTargetInTouchRange` scales the
  range differently in `MotionMoveState` when a flag the owner's data holds
  is set; which flag, and the scale, are not read. The recorded Vortex took
  the twice-its-radius branch. No recorded construction has fallen back, and
  no recorded grouped slot.
- **Target scoring**: a split quadtree, tied candidates, a building winning,
  moving candidates being reinserted, and other selector modes.
- **Where a search not prepared stands the searcher.** `PerformSearch` is read
  to score the candidates where they stand; every recorded searcher that
  searched that way had not moved during the tick, so whether its own position
  and facing are also read anew is not measured.
- **Projectiles**: why a projectile in simulated motion spares a dead
  target's neighbours, which is recorded and not read; interception; and every other
  projectile type.
- **A fixed weapon's transform**: that `RotationLimitFightTransform` refreshed
  for `RotateType.Fixed` copies its parent's rotation exactly, and which call
  keeps the core's weapon on the body's rotation, are measured, not read; the
  core's search condition that lets it take a sibling's unit is read as a
  fusillade's from where it stands, not from the flag it asks.
- **Damage**: building splash, the area boundary of a hit other than a splash,
  ordering, modifier chains,
  shields, and other providers or target domains.
- **Armour** under a shield, on a summon's drop, against a fire, a buff or an
  explosion, and with its unit's technologies disabled
  (`ArmorStrengthenEffectProvider.DisableEffect`), which no recorded fight
  does; an item's armour (`ArmorStrengthenEquipment`) is refused.
- **A personal shield's** refresh when its unit's maximum life changes
  (`EnergyShieldController.Refresh`), which no simulated buff does, and its
  disabling with its unit's technologies.
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
- **The unit whose id is 54** is free-moving by the same literal; no recorded
  fight fields it.
- **The endgame** with several members or groups, summons, respawns,
  constructions, shields, and mixed damage inside one tick.
