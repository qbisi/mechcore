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

The same non-fighting path runs for a construction's `SkillManager.Update`:
a skill still holding a lock calls `FightSkill.ExitFight`, whose
`RefreshAttackInterval` removes the stagger and readies its clock. This can
happen while a battle skill still has summons pending. If those summons
bring both sides back, the construction resumes from that ready clock.

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

Initial unit avoidance agents activate in the sorted team order used for
updates: when two z coordinates are within `FPoint`'s 43-raw-unit tolerance,
x orders them. Recording identities use strict coordinate order and can
therefore differ. This activation order also decides the neighbour tree's
insertion order, including which agents a split that grows its array loses.

Avoidance's sampled trace sets its iteration step from the unit's outer
radius and one fifth of its desired speed, using the build's tolerant
maximum. When the two differ by at most 43 raw Q32 units, the speed term
wins even when slightly smaller. Its solved movement is published at the
next four-tick avoidance boundary.

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

**A shield replacement can keep the attack through a changed lock.** When a
dead lock is replaced, the quick-switch guard compares the skill's actor
attack target before and after the search. Firing at a battlefield shield
leaves that actor target null. A replacement that also fires at a shield
therefore passes this comparison even without quick switching, provided it
is in the attack area; the changed lock alone does not end the attack.

**A cooling searches for nothing.** Its weapon names what the attack left it
for the whole cooling, the lock the failed check's search took even when that
search fell back on any live enemy and left the skill idle, and nothing when
the attack left it firing at a battlefield shield: a Phantom Ray whose lock dies, and whose next lock stands
inside the enemy's shield, names no unit while it cools, however long, and
locks again only once the cooling is over.
The same holds for an alive-only fallback inside a shield: the check's
`SearchAttackTarget` replaces the actor target with the shield even though
the fallback left the skill idle. Its cooling retains that empty actor target.

Its last cooling update enters idle immediately, though that idle state
first updates on the next tick. Later skills in the same manager update
already see idle: a Fortress's Rocket Punch can take the main skill's place
on the tick its main skill finishes cooling.

While a preemptive skill is running, a skill's motion-facing
`IAttacker.IsAttackTargetInAttackRange` asks that preemptive skill's attack
target and range. Another skill may own the motion after searching, and
continues to supply its direction and lock; its own range does not replace
the running preemptive skill's check. Ordinary skill attack checks still
use the skill's own target and range.

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
and follows it about. A unit with a move ability, the Sandworm's or the
Larva's burrow, is in touch while it stands idle within ten times its radius
(`IsLockTargetInTouchRange` asks the motion controller's `moveAbility` and
whether its state is a `MotionIdleState`): a Sandworm idle with only
aircraft left stays where it is with one 49 metres off, edge to edge, where
it would walk on one 27 metres off. A unit attacking when its search leaves it idle stops
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
Starting a blow requires an idle attack controller before that controller's
update. A projectile burst whose final shot leaves this update therefore
starts its next blow no earlier than the next update, even when its attack
interval has already elapsed.
On entry without an initial cooldown, `FightSkill.EnterFight` refreshes the
attack clock to its newly sampled interval. It is ready immediately,
regardless of property changes before entry; a waiting clock from before
entry does not survive that refresh.

`FightSkill.RefreshDatas` preserves elapsed attack time unless it exactly
equals the old interval, in which case it becomes the new interval. A clock
already past its old interval can therefore have to wait again when a buff
lengthens that interval: a Stormcaller whose interval grows from 82 to 132
ticks on tick 27 still waits with attack time 113 on tick 31
([`longer-interval-postpones-ready-clock.yaml`](../../tests/attack_timing/longer-interval-postpones-ready-clock.yaml)).

**A blow is fitted into its interval.** A blow starts by drawing its interval
and then converts its attack point and its backswing to whole ticks, each
truncated on its own. When the two together are longer than the interval, the
attack point is scaled by the interval over their sum, an `FPoint` product
truncated to whole ticks, and the backswing is what is left of the interval.
A unit whose attack point and backswing fit its interval keeps both. The
Rhino's fit its own interval and no longer fit the one Mechanical Rage leaves
it, so the technology brings its blow forward as well as its next one.
Both fitted wait-controller durations are stored when the blow begins
(`SkillAttackController.PerformAttack`). Refreshing the skill's current
interval during windup does not refit that blow's remaining phases.

## Base facing

`FightTransform.RotateTo` keeps its rotation when it compares equal to the
converted target angle under `FPoint`'s tolerance. Otherwise `RotateLeft` or
`RotateRight` takes `FPoint.Min(remaining angle, rotateDelta)`: values that
compare equal select the second operand, the full rotation step.

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

**A target standing on the unit's own position is straight ahead and turns
nothing.** `SkillAttackAngleChecker` measures a target's angle with
`FightUtility.CalculateAngle` of two transforms, which reads two equal
positions as angle 0 before it converts any direction, so such a target is in
every attack angle, a grouped slot's as well as the core's. `MotionAttackState.AttackRotate` turns towards
`CalculateTargetDirection`, and `FightTransform.RotateTo` returns on a zero
direction, so the unit keeps its facing and its lock and fires on. It happens
when a charging unit passes through the one aiming at it.
The same zero-direction return holds during movement: `MotionMoveState.NormalRotate`
passes the target direction to the mech body, so a unit moving onto its
burrowed lock keeps its turret's facing while its root can still turn to its
movement direction.
An extra skill passes this direction through the same motion interface: a
Centurion in Melee Mode whose Hound target occupies its own position also
keeps its root facing, rather than converting the zero vector into angle 0
([`extra_weapon/coincident-target-keeps-facing.yaml`](../../tests/extra_weapon/coincident-target-keeps-facing.yaml)).

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

**A retained shield changes its automatic movement distance.**
`AutoMoveBehaviour.CalculateTargetDistance` subtracts the lock's radius less
attack range from the centre distance. Outside its shield branch it also
subtracts the owner's radius from that stopping allowance. The shield branch
therefore submits a nearer movement point. `IAttacker.IsAttackingEnergyShield`
on a skill reads its cached `GetTargetEnergyShield`, even while its idle flag
suppresses the attack target; a mech that searches for itself answers false.

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
The non-fighting skill cleanup after a side is decided does not re-enter an
already idle motion either: letting its skill's lock go preserves that first
stop target while the fight drains.

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

`ChangeLockTarget` does nothing when the old and new locks are identical.
A sibling whose lock the core already took therefore does not clear the
owner's new lock again when it stops its attack. Its retained attack target
is still what its cooling names, even if that construction has fallen.

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

The batch asks its core skill whether the attack target is in range.
A sibling retaining a lock does not keep the unit in attack motion when
that answer is false, even if every slot is idle with a target retained.
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

A full-team selection reads the team's actor list, not spatial-tree
traversal. The list keeps shown towers first, then the ordered deployed
units and other actors; later joiners append. Equal scores retain the
first candidate. Opponent lists and their air/ground partitions refresh
after the tick's timers and before its modules, so summons whose timers
expire are already present. A unit changing team leaves its old opponents'
lists immediately and appends to its new opponents' lists on the same turn.
The same order is used by ordinary full-team fallback and alive searches.

The spatial tree starts from `FightTeam.activeActors` in registration order:
fixed towers, deployed units, crystal contraptions such as interceptors, then
constructions. An interceptor precedes defensive wall blocks in this list.
The tree retains the resulting node and element order when a range query
collects candidates; a chain's random selection uses that order.

When `CanAttackSameTarget` is true, a missing selection or one outside the
slot's range makes the selector consider the held targets as well. That list
keeps the other skills' order, including repeated locks; it is not a filter
of the opponent list (`SkillSearchTargetController.PerformGroupedSkillSearch`).
Equal scores keep the first of those held locks, so the main row's lock can
precede an equally scored lock of a row that joined its group
([`search/group-shared-lock-order.yaml`](../../tests/search/group-shared-lock-order.yaml)).
A successful held-target selection replaces the first answer; without one, the
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
This includes replacing an invisible best candidate with a visible one:
that replacement must be in this slot's range. A Mountain's second gun
keeps an underground Sandworm when the visible Mustang is beyond its
115 metres, even while the first gun could reach the Mustang.

`GroupedSkillAttackBehaviour.Update` and `OnStartAttack` are empty. The hooks
do not perform a separate allocation pass. The checker's
redistribution path, `TrySearchGroupSkillLockTarget`, is the rule above and
not the ordinary search's sibling-exclusion loop. It passes over the slot
itself and the owner's main skill's first skill, the core of a main skill's
group; an extra skill's group, which does not hold that skill, passes over
none of its own ([extra_weapons.md](extra_weapons.md#a-group-of-beams)). The fixtures
that separated the rest are [the Wraith fixtures](../../tests/wraith/).

Redistribution compares `FightSkill.GetAttackCount`, the number of blows
started, including a blow still winding up before its attack point. A sibling
that has started as many keeps the current slot from yielding its shared lock.
`tests/control/grouped-windup-count.yaml` records this for Multi Control.

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

A running fusillade's skills update before its owner's motion
(`FightMech.Update`, `SkillManager.Update`). If a sibling kills the lock of
an auto-moving unit, the motion enters idle on that update while the skill
retains its dead target until its next search.

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

This allocation tests the candidate actor's distance
(`FightSkill.IsActorInAttackRange`), even when the core's current attack
target is a shield. A shield surface in reach does not put the unit it
covers in reach for this decision; a core whose unit is still outside its
range can take its sibling's tower instead.

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
(`tests/shield/barrier-fortress-vs-wasps.yaml`).

A projectile burst already releasing suspends its skill's target checks.
Its motion still tests range and turns towards the target; it does not add
an independent attack-angle rejection. A bodyless unit keeps turning and
releasing the burst when a close target crosses its arc. The skill's checks
resume after the last projectile.

## Normal target scoring and pre-fight acquisition

An actor's death removes it from the spatial trees before its buff death
callbacks create summons. Those summons enter the trees when created;
assigning their recording IDs preserves their nodes and enumeration order.
Keeping the dead parent until after its summons can split a node at a
different time and reorder later splash hits
([`buff-summons-enter-after-parent-leaves.yaml`](../../tests/dead_summon/buff-summons-enter-after-parent-leaves.yaml)).

A standalone main gun that takes motion back from an extra skill replaces
the motion's attacker with itself. An extra skill is no main target
provider, so it cannot retain motion by pointing to another main gun's
lock. The unit then turns its turret towards the accepted main gun's
target
([`main-gun-retakes-motion.yaml`](../../tests/extra_weapon/main-gun-retakes-motion.yaml)).

The shared skill range check measures a retained dead target too: it tests
visibility and distance, while selection and attackability ask whether the
target lives. A standalone gun still releasing at a fallen construction
therefore keeps its unit's attack motion while that construction is in
range
([`standalone-fallen-target-range.yaml`](../../tests/construction/standalone-fallen-target-range.yaml)).

`SkillIdleState.TryPerform` returns after the lock search when `IsIdle` is
set: an alive-only fallback supplies a movement lock, and the skill does
not run the wall or shield attack-target search. A War Factory gun with no
normal candidate therefore retains its Raiden lock rather than replacing
it with a wall
([`idle-gun-keeps-alive-fallback-lock.yaml`](../../tests/search/idle-gun-keeps-alive-fallback-lock.yaml)).

An attack-angle check that finds a weapon held at an endpoint of its
limited rotation range can search again once that skill's search timer
allows it. This applies to standalone main guns as well as extra weapons.
If the replacement still cannot be attacked, cooling retains that newly
checked target rather than the old one
([`main-gun-arc-edge-refresh.yaml`](../../tests/search/main-gun-arc-edge-refresh.yaml)).

The unit-level selector asks the unit as `IAttacker`, while a weapon's
selector asks its skill. `FightMech` accepts a visible replacement only
strictly inside its main range minus one metre; `FightSkill` checks its
minimum and maximum range. Thus a War Factory of range 100 metres keeps
its hidden Sandworm over a visible Mustang 99.087 metres away
([`mech-search-range-margin.yaml`](../../tests/search/mech-search-range-margin.yaml)).

Among currently visible, fully rotated, unsplit-quadtree ordinary ground targets
with a unique optimum, the build picks by Q32 edge distance, strict
minimum-range exclusion, an angle factor, an out-of-range penalty and a strict
minimum score.

**Coincident positions have zero search angle.** `AngleCalculator.Calculate`
uses `FightUtility.CalculateAngle`, which first compares the two horizontal
positions with `FVector2` equality and returns angle zero when they match.
Each coordinate uses `FPoint`'s 43-raw-unit equality tolerance. Such a target
has no world bearing to compare with the searcher's facing
([`search/coincident-target-has-zero-angle.yaml`](../../tests/search/coincident-target-has-zero-angle.yaml)).

**Relative angles keep the native comparison tolerance.**
`FightUtility.CalculateAngle` passes bearing minus facing through
`FightUtility.ClampRotation`, then folds it only when `FPoint.op_GreaterThan`
puts it above a half-turn. A negative difference within 43 raw Q32.32 units
of zero remains negative, and a difference within 43 raw units above a
half-turn is not folded. The angle factor uses that signed value; taking an
absolute circular distance can turn two distinct candidate scores into a tie.

**The angle is measured from the searcher's main transform.** That is its
first weapon's when the weapon has a transform, and the mech's root when it
has none, whether or not the weapons turn at a speed of their own: a
Wraith, whose four weapons turn at 90° a second but carry no transform,
scores from its root's facing, not from where its first weapon points.

**The first tick has no prepared search job.** Preparation runs after a
tick's modules for the next tick. A search on the first tick therefore reads
the live state when it runs: a Phantom Ray that cloaks on that tick already
takes the invisible-target distance penalty.

**A search reads the tick's start only when its skill asked for that.** At a
tick's start `FightCoreSystem.PreCalculate` lets each skill state prepare a
search, scored on where everything stands at that moment and with the skill's
range and minimum range as they stand then (`PrepareSearch` reads
`GetAttackRange`), and
`ScoreRatingTargetSelector.TrySelect` answers a prepared skill from those
scores. Replacing a hidden winner with a visible alternative still asks the
live attacker's range. The range check does not test whether that alternative
has died since preparation: it is selected first, and the following life
check sends the search over all live enemies rather than retaining the hidden
winner ([`2344-r5.yaml`](../../tests/corpus/2344-r5.yaml)). A Sandstorm that
halves a Sledgehammer's range after preparation can leave its hidden tower selected even though the visible
Sandworm was in its prepared range
([`prepared-search-live-replacement-range.yaml`](../../tests/search/prepared-search-live-replacement-range.yaml)).
A registered job whose answer has died since, a tower that fell that tick
among them, or whose result selects nothing, sends the search to `Select` over
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
which a square 800 m wide leaves out. If preparation admits no candidates,
`ScoreRatingTargetSelector.Prepare` registers no job. The later search uses
those live squares: a Wraith that has landed since preparation can be found
immediately even while the cached ground-opponent list still omits it.

A prepared search gathers its
candidates as it is prepared, from the tree and about where the searcher stood
then; units that update before the searcher may have moved to other nodes by
its turn, and the candidates stay the ones prepared. An attacking skill is
prepared only while its lock is absent or dead, an idle one when it can start a
search as the tick opens, its search timer run out or its lock absent or dead,
and a preparing or cooling one never: an idle Crawler whose lock dies during a
tick its timer was not due searches with `PerformSearch`. Whether a search
was prepared is fixed as that tick begins: a later team change clearing a
skill's lock and timer cannot enqueue a job retroactively, and its next search
uses `Select` over live candidates. The prepared scores are cleared every
tick. So a
Stormcaller whose live lock walks inside its minimum range during a tick
searches past it and takes the next target that very tick. The ranges are
the ones it was prepared with: a Fang whose Range Enhancement an
Electromagnetic Impact switches off scores the search prepared before the
skill landed with its 130 metres, not the 90 it is left with, and takes the
Crawler within the 130 over one beyond it. The target's flight state is
prepared too (`TeamScoreRatingTargetSelectJob.TargetData.isFly`): a timer
restoring a Wraith to the air does not change the domain used by an already
queued ground-target search. Its domain-dependent score offsets use that
same prepared state; a later live fallback reads its current flight.
Presearch gathers flight after deployment effects, including Land Cruiser,
rather than from the unit row's default domain. `PreCalculate`
prepares only the mechs of the fight as it runs, alive and not travelling, and
`FightingState.Update` runs it after the modules, before the next update's
timers join the summons that are due: a summon that joins is no attacker
`TrySelect` answers for, and its search on that tick is a `PerformSearch`.
Nor does it prepare a skill a `SkillGroup` holds, a Wraith's or a Raiden's:
their every search scores candidates where they stand as it runs, after the
units of the sides that update first have moved.

**A replacement found after losing a target inside the minimum range can
give the interval back.** That search is
`SkillAttackableChecker.CheckWhenLoseTarget`. If it finds no lock, it clears
the attack target and returns without resetting the clock. After finding a
lock, on an attacking check of the main searcher while a blow is under way, it calls
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
The geometry for deciding whether to reset the clock reads the raw attack
target, including a lock found only by the alive-target fallback. That
fallback marks the skill idle, which makes the final check fail without
turning its successful geometry check into a clock reset.

**A blow is counted as it hands back its last phase.**
`SkillAttackController.ChangeToIdle` adds it to `performCount`: a blow with no
backswing as its attacking phase ends, once its burst's last projectile is
released or its sweep is over, and a blow with a backswing as the backswing's
controller hands back on its last tick. A Farseer's two-projectile burst
released on tick 2 reads 0 until its last projectile goes out on tick 6, and
the Rhino it fires at, whose backswing ends on tick 107, reads 1 on that tick:
`tests/projectile/farseer-rhino-closing.yaml`.

Ending the attack controller's backswing does not change the skill state
(`SkillAttackController.ChangeToIdle`). A skill waiting in `SkillIdleState`
stays idle; acquiring an attackable target still enters `SkillAttackState`
before that state starts a new blow on its following update
(`SkillIdleState.Update`). An elapsed backswing retained while no target was
available does not let the next acquisition skip that entry
([`attack_timing/idle-backswing-keeps-attack-entry.yaml`](../../tests/attack_timing/idle-backswing-keeps-attack-entry.yaml)).

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

**Tracking reads the skill's current setting.** After its climb,
`FightProjectile.Update` asks `IProjectileDataSource.IsLockTarget` on every
update; `FightSkill.IsLockTarget` reads its current `DataSet`, rather than a
setting copied into the projectile when it was made. A technology disabled
while a shell flies can therefore change whether it follows the target.
Disabling Scorpion Siege Mode turns its already flying shells from fixed
points to tracking their targets: `tests/technology_disable/in-flight-siege-tracking.yaml`.
`FightProjectile.Init` stores the launch offset even when tracking is off.
If a disabling buff later turns tracking on, the projectile adds that original
offset to its live target's position; it does not converge on the target's
centre by discarding the offset.

**A projectile keeps the target domain it was created with.**
`FightProjectile.Init` stores `targetType`, and `GetTargetType` returns that
stored value for range damage. With a target, an air-only projectile skill
keeps air; otherwise the target's current `IsFly` decides. With no
target it takes both domains. A target taking off while a ground shell
flies does not turn that shell's splash into an aerial hit. A single hit
without a splash instead names its target directly
(`DamagePerformer.PerformSingleEffect`); the target changing domain does not
remove that hit.

**A projectile with a splash strikes only what its splash reaches.**
`DamagePerformer.Perform` performs a hit with a splash as `PerformRangeEffect`,
over everything within the splash of where it lands, and only a hit without
one as `PerformSingleEffect` on what it was aimed at, wherever that stands. A
single hit checks the main actor's life, whether a unit or a building; it
does not recheck its distance from the impact point or depend on tracking.
Before that shared damage path, a projectile arriving without a splash
skips its main actor if it has joined the projectile controller's team.
Shield interception bypasses this actor-team check.
A projectile that follows its target lands its offset from it, so one whose
offset passes its splash misses.

**A projectile leaves for where its burst aimed it.** Every projectile of a
burst is released toward the target's position when the burst began plus its
offset; one that follows its target takes the target's position up again from
its first update, which is why the rule above reads as the live position. A
burst with no offset draws no points as it begins, and each of its projectiles
leaves for where its target stands as it is released, a dead target where it
died: a Centurion's second Homing Missile, a quarter second after the first,
leaves for where its target stood the tick before
(`tests/extra_weapon/enhanced-range-homing-missile.yaml`).
This recalculation applies to buildings too: even a stationary tower's
clamped aiming point changes as its attacker moves between releases
(`ProjectileMultiAttackPerformer.PerformEffect`,
[`projectile/building-aim-refreshes-on-release.yaml`](../../tests/projectile/building-aim-refreshes-on-release.yaml)).

**Where a burst aims is held within the skill's reach.** Every projectile
performer asks `ProjectileAttackPerformer.CalculateAttackPosition` where to
aim: at the burst's first projectile, at each projectile of a burst with no
offset, and at each unit of an evenly allocated burst. It holds the way from
the owner to the target to the skill's range plus both radii
(`FVector3.ClampMagnitude`). An evenly allocated burst adds its
`extraSearchRange`. When exactly one of owner and target flies, the reach is
the hypotenuse of that length and the air height (`FPoint.Pow`, `Sqrt`).
Ordinary bursts measure between the two positions in space. An evenly
allocated burst measures from the target's ground point: to the owner's
ground point when neither flies, and to its point in space otherwise. A skill
that attacks only the air aims at the air height. A target within reach is
aimed at where it stands. An Overlord firing at a Fortress's shield, with the
Fortress itself beyond reach, aims at the point of its reach on the way. The
projectile names that point until its first update, and one that follows its
target takes the target's position up again then, a construction as a unit
(`FightProjectile.Update`).

**Two weapons split a burst's offsets in three dimensions.** Each weapon takes
the half of the offsets on its side of the line from the weapon to the target,
ordered by the angle it sees, and the angle and the side are measured from the
weapon's height to the target's: an Overlord shooting down at a Crawler, and
two Overlords shooting at each other at the same height, order them so.

**A burst that allocates evenly shares its projectiles among actors.** A skill
whose row sets `isEvenlyAllocated` aims its burst with
`EvenlyAllocatedAttackTargetPositionController`. On its first projectile
(`Prepare`) it takes the other sides' units, towers and construction blocks
within the skill's range and its `extraSearchRange` of the unit, each measured
less both radii, in the
order their teams' trees answer a square twice that wide
(`RangeTargetCalculator.CalculateRangeTargets`, fully visible, without its
separate `BuildingSystem` search). A construction already in its team's tree
remains a candidate; the range query does not apply the ordinary target
selector's construction exclusion. It takes the attack target alone when
none is, a building as a unit. It shuffles them by the side's stream, each
place swapped with one drawn from the
whole list (`IListExtensions.ShuffleSync`); each unit takes the burst's count
over theirs, and the rest go one each to units drawn one by one from those not
drawn yet (`GetRandomTargetsByRemain`, `RandomElementSync`). It draws the
offsets then, round by round over the units and then for the units drawn, each
`RandomInsideSphere` about where the attack target stands. Each projectile as
it leaves takes the first unit of the list that lives, dropping the dead, and
puts it last (`UpdateCurrentTarget`), leaves the next of the two weapons in
turn (`GetWeaponIndex`), aims at where its unit stands then
(`GetTargetPosition`), and takes that unit's next offset, none once they have
run out (`GetAndDeletePositionOffsets`). With none of the list alive the
controller supplies no target and keeps the point the burst last aimed at,
`lastAttackPos`, with an offset drawn afresh from the side's stream about it.
`ProjectileSystem.Create` replaces a null target with the skill's attack
target or, absent that, its lock, without checking whether it lives. A burst
still naming a dead tower therefore keeps that tower's radius and projectile
target even after its allocation is exhausted. An Abyss shelling a tower takes
the tower into its list, and its shuffle draws once for it. Its climb is measured to where its
unit stands. Swarm Missiles' 46 share two units 23 each.

An evenly allocated burst measures its climb to the controller's current
aiming point, before the projectile's offset is added
(`ProjectileMultiAttackPerformer.PerformEffect`, `ProjectileSystem.Create`).
When every allocated target has died, the controller keeps `lastAttackPos`
(`EvenlyAllocatedAttackTargetPositionController.GetTargetPosition`); the
remaining missiles measure their climb to that last point too, even when
`Create` falls back to the skill's dead attack target for their target reference.
[`extra_weapon/swarm-missiles-exhausted-climb.yaml`](../../tests/extra_weapon/swarm-missiles-exhausted-climb.yaml)
holds a burst whose allocated targets die after its first release.

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

**A projectile that lands on a dead target does nothing, unless it splashes
outside simulated motion.** `FightProjectile.Update` releases a projectile that
arrives after its target is no longer valid (`IsValidTarget`) when it has no
splash, or when its skill has `isSimulateMode=true`: no damage, splash
included, and no shield asked. A Fire Badger's or a Typhoon's shot at a
Crawler another shot killed while it flew leaves the Crawlers beside it
untouched, a Fire Badger's shot at a wall block that fell while it flew does
too, and a Fang's shot at a Fire Badger that died inside a shield is taken by
no shield. A projectile that splashes, outside simulated motion, still strikes
where it lands, as an Arclight's does; and one that flew into a shield on its
way is taken by it, its target dead or not.

**A shot that follows its target is spent on nothing out of its owner's
reach.** Each update, before it lands, a projectile of a skill with
`isLockTarget=true` asks whether it stands within its reach of its owner,
edge to edge in three dimensions from where the owner stands, dead or
alive. Its reach is the owner's radius, the skill's attack range and the
target's radius, all as the projectile was made; when exactly one of owner
and target currently flies, the hypotenuse of that and the 70 metres between
them. The flight flags are read even after the target dies, while its cached
arrival point stops following it. Removing Land Cruiser on a Wraith's death
can therefore shorten a Wasp shot's reach without moving its destination.
One out of reach is removed with no damage. A Mustang's shot,
135 metres of range, at a Crawler running away is spent 143.06 metres from
the Mustang, against the 3 + 135 + 2 + 3 it may stand at. A Stormcaller's
shells lock nothing and are never asked.

**A weapon is named by its index.** A skill's weapons carry their own index in
the build, and a recording names a weapon aim and a projectile release by it: a
Hound's one weapon is index 2, a Sabertooth's two are 0 and 2.

## Damage and death

`FightCalculator.PerformHitTargetEffect` returns immediately when its target
is not alive. Overlapping ground fires therefore cannot kill a unit again
while it waits for `DeadEffectSystem` to process its death.

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
takes only its target domain stored at creation: a Melting Point's beam at a Phoenix in the air
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
struck, summed with the energy its hit took from battlefield shields, to the
skill's hit effects, and a lifesteal source's effect
gives the skill's owner the whole part of that sum times the source's
multiplier. It gives nothing to an owner that is dead or at its maximum life,
and never past the maximum: what the owner was short of is what it gets. A
direct blow, a beam with or without a splash and a projectile all hand their
hit on, including a hit absorbed wholly by a battlefield shield.
A turret's, a mine's and a battle skill's hits reach no unit's skill.

Hit effects run in registration order. Before registering its providers,
`FightEffectMananger.Sort` orders them by the `GetID()` of each provider's
first data source. With no lifesteal technology on the owner, Absorption
Module therefore heals after a lower-ID buff technology applies its buff.
The highest-priority source still supplies the healing; choosing it does
not replace the provider's full source list or its registration key.
This is the initial registration order. Disabling a technology unregisters
the hit effects its provider removes; restoring it appends them after those
that remained. `BuffEffectProvider.DisableEffect` and `EnableEffect` unregister
and register its buff controllers together, in source order. An equipment's
lifesteal can therefore precede a restored buff technology after the unit's
technologies were disabled, even though the buff registered first at entry.

The active source registers its hit effect on the skills its main and extra
skill flags admit. A main source also reaches its grouped extra weapons;
an independent extra weapon admits a main source only when it has a positive
damage rate. Energy Absorption therefore heals through Energy Diffraction,
but not through the independent, unrated EMP Barrage. Absorption Module
admits both main and extra skills.

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
far as its maximum. Nothing sets a registered controller's start clock back: a unit
that is healed whole and hurt again repairs at once.

Each side updates its repair controllers in registration order. Initially
active units register in their prepared update order; later summons and
arriving travellers append their controllers. Deactivating the effect removes
the controller, and activating it again registers a fresh one with reset clocks.
The manager is selected when `AutoRecoverySystem.AddMech` registers the unit;
control does not move that controller to the unit's new side. A controlled
unit therefore still repairs among its former side's controllers in its
registration position, even though it fights for the other side
([`control/recovery-keeps-registration-team.yaml`](../../tests/control/recovery-keeps-registration-team.yaml)).

A fixed-point comparison counts 43 raw units as equal, and the tick is a few
raw units short of 0.05 second: twenty hurt ticks reach the second and two
reach the interval. With a start time of 0 and an interval of 0.1 second, as
every standard source has, a unit repairs from its twentieth hurt tick on,
every second tick.

Nano Repair Kit and Field Maintenance are repair sources, one per unit as
lifesteal's are, the item's priority above the technology's. Their numbers
are [`config/equipment_effects.yaml`](../../config/equipment_effects.yaml)'s
and [`config/technology_effects.yaml`](../../config/technology_effects.yaml)'s
`start_time`, `recovery_duration` and `recovery_life_rate`.

**A repair of state 1 runs only while its unit is below.** A technology whose
`auto_recovery_state_type` is 1, `Underground`, has its controller out of
condition from the start
(`TeamAutoRecoveryManager+AutoRecoveryController.Reset`), and
`TeamAutoRecoveryManager.AddMech` registers it with its unit's
`UndergroundMoveAbility`: the end of a burrow (`OnEnterMoveEnd`) puts it in
condition, and the start of a surfacing (`OnExitMoveBegin`) takes it out.
`TeamAutoRecoveryManager.Update` passes over a controller out of condition,
so both its clocks stand still while the unit is up, and resume where they
stood. Burrow Maintenance's Sandworm, hurt while it is up, repairs nothing
until it has burrowed, and twenty ticks after it has, its start clock
reaches its second: `tests/move_ability/burrow_maintenance.yaml`. A
state of 2, `Cloak`, is refused.

**A repair is off while its technologies are.** A buff that disables a
unit's technologies has `AutoRecoveryEffectProvider.DisableEffect` clear its
controller's `isEnable` (`AutoRecoverySystem.DisableMech`), whichever of its
sources is in force, as long as one of its technologies that switches is a
repair source; `EnableEffect` sets it again. `Update` passes over a disabled
controller as over one out of condition (`AutoRecoveryController.IsPass`), so
both its clocks stand still while it is off. Read from the build.

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

**Switched off with its technologies, it lets hits through and keeps its
energy.** `EnergyShieldProvider.DisableEffect` disables the controller
(`EnergyShieldController.Disable`), which clears `isEnable` and records its
energy over its maximum, an `FPoint` quotient, rounded. A hit asks
`EnergyShieldBehaviour.IsAvaliable`, active, enabled and with energy left,
before the shield takes it, so a disabled shield takes nothing; a dead line
asks only that it is active with energy left, so a disabled shield still keeps
the line off. `EnableEffect` enables it (`EnergyShieldController.Enable`) with
the recorded share of its maximum, the product truncated. A recording reads
the shield disabled (`IsEnable`) from the tick an Electromagnetic Impact
switches a Fang's Portable Shield off.

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

Damage selects its aerial or ground property from the skill's attack target,
which can remain through cooling after its lock clears. An airborne target
retained then still selects aerial damage for a projectile landing during
cooling. The recording's normal damage remains the ground property.

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
damage's range of where it landed, of its main target's current domain, takes
the second damage, except what the hit itself struck unless the row says the
main target may be hit too. Shockwave's 75 reaches 30 metres: an Arclight's
shell that fells six Crawlers deals the eighteen others around them 75 each,
as `damage` events of the shell after its own.
A projectile's primary splash keeps the domain selected at creation, but
the secondary splash reads the main target's `IsFly` when it lands. If that
target has taken off in flight, Shockwave can strike it even when the
primary ground splash strikes nothing. Without a main target the secondary
splash uses its damage provider's target type.

**The second damage is the row's, scaled by tower buffs and target buffs.**
Where the row says buffs reach it, the damage is the attacker's tower buffs'
rate on it, and then each struck unit's buff rates on damage taken; the
unit's own numeric damage-taken modifiers do not apply. Subterranean Blitz
therefore leaves Shockwave's second damage whole. The attacker's other damage
rates never reach it, and the struck unit's buff rates do not apply a second
time as the hit is taken. An Arclight whose skill
deals 3.12 times its damage still deals 75, and a Sandworm under a 0.5 rate on
its damage taken takes 112.

**A second damage needs its provider enabled.** A lethal hit alone does not
disable it: a shell landing later on that tick still deals Shockwave before
the death module runs the owner's `OnDead`. That callback deactivates the
provider, so a shell landing afterward deals its primary damage alone.

**Battlefield shields stand in a second damage's way as in a splash's.**
Even a single-target hit absorbed by a shield performs its secondary effect
after the primary hit and its hit callbacks. A shield the primary hit empties
no longer excludes the units it covered from the secondary range effect.
`PerformSecondaryRangeEffect` prepares its targets as a splash does
(`PrepareRangeTargets`): what an enemy shield covers that does not hold the
point the hit landed at is taken out, unless the hit crosses shields, and
the shields its range reaches, and the one covering what the hit was aimed
at, take the second damage before any unit does
(`PerformSecondarySplashShieldDamage`, `PerformHitAdvancedEndergyShieldEffect`,
which asks `CalculateHitEnergyShieldDamage` as any hit on a shield does).
Read from the build; the corpus rounds that hold it are not pinned.

A second damage that names a buff writes it on what the hit struck and on
what the second damage reaches, a damage of nothing included:
[technology_effects.md](technology_effects.md#a-second-damage-that-writes-a-buff)
says how.

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

**Quick target switching still waits for the after-wait.** The after-wait
controller disables target checks, even when the attack interval has already
elapsed. A Mustang keeps a wall its projectile destroyed until that wait
finishes. Only the following update checks the dead target and finishes an
attack whose replacement cannot be attacked; the idle skill searches on its
next update. A new blow also requires the current attack controller to be idle.

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
At final exit, each team clears its live units' buffs and then its standing
constructions' buffs before the next team exits. Thus a defeated blue team's
surviving construction loses its tower debuff before red's surviving units
lose Photon Emission
([`final-buffs-clear-by-team.yaml`](../../tests/endgame/final-buffs-clear-by-team.yaml)).
The final tick's projectile victims still receive their `DeadEffectSystem`
callbacks before `FightingState.Update` changes to the over state. Even when
that tick publishes older torn-down buildings, the final victim's death and
buff clearing precede the survivors' whole-fight buff cleanup
([`final-drain-kill-before-buff-cleanup.yaml`](../../tests/endgame/final-drain-kill-before-buff-cleanup.yaml)).

Torn-down constructions follow their usual death cleanup: their buffs are
cleared immediately before `building_destroyed`, even while the fight waits
for other projectiles to drain.

**A tower torn down still stands in the RVO solve of the tick that tore it
down.** `FightingState.Update` runs the RVO after `TryDstroyTower`, and the
teardown only takes the tower's life (`DestroySelf`): the `OnDead` that
deactivates its agent runs in the next tick's `DeadEffectSystem`. When that
tick solves and the fight runs on, shots still in the air, the solve is built
with the fallen side's towers in it, and what it hands the units still walking
is published four ticks later. A unit that died that tick is gone from the
solve: its `OnDead` ran in that tick's `DeadEffectSystem`. A construction
the teardown takes goes the same way, through its own `DestroySelf`; that is
read, and no recording shows it.

**A tick that leaves neither side a unit takes both sides' towers down, when
no projectile is in flight.** A Missile Strike landing among both sides'
Crawlers, before any unit updates, leaves both teams' counts at zero, and all
four towers fall on that tick. Two sides that kill each other's last units
while shots are still in the air end the fight as the drain ends it, with
their towers standing. A side whose last unit dies on the tick a shot in the
air lands still loses its towers then, the fight ending on the next: a
Sandworm's blow killing the last Overlord as a tower's shot lands
(`tests/anti_air/anti-aerial-sandworm.yaml`). Only a fight that waited
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
leaving its attack stops the current attack controller, including a
backswing still waiting. If reinforcements resume the fight, that old wait
does not prevent the idle skill from searching.

A burst already releasing does not stop merely because the last opponent
has died or changed sides during the current tick. Its performer ignores an
invalid target after its first release
(`ProjectileMultiAttackPerformer.IsInterruptedByInvalidTarget`); the core's
non-fighting update stops it on the next tick.

A skill that holds none is not updated at all. An inactive manager returns
before this path: its clocks and targets stay as they were, and its motion's
transition continues. A manager holding fire, a Sandworm's below, is another
exception (`SkillManager.Update` asks `isActive` before `isHoldFire`): it
updates its main skill as in a fight, so a Sandworm below whose lock died
with the fight searches, takes the defeated side's tower and walks on to it
until the fight ends.

An appearing summon can keep the fight open while its side has no active
unit. Skills whose updates stop keep their elapsed attack time; when the
summon joins and fighting resumes, they wait out the remaining interval.
Time spent waiting for that summon does not bring their next blow closer.

A main skill locked by a permanent preemptive skill holds no lock target
(`SkillLockState.Enter`), so this path leaves it locked. Final
`SkillManager.ExitFight` visits every skill and returns it to idle, including
the locked main gun and incompatible side arm of Centurion's Melee Mode.

A transition that reactivates its manager does so after that tick's skill
update was skipped. Its queued motion state enters intact and updates on
the following tick, even when the last enemy has already died.

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
An already-idle skill with no lock also stops updating. This includes a
grouped slot: its core leaving does not clear the idle slot's retained
attack target ([`idle-grouped-target-after-last-kill.yaml`](../../tests/endgame/idle-grouped-target-after-last-kill.yaml)). If a controlled
unit's return to its side stopped the idle skill, it keeps the actor target
that `StopAttack` left while the won fight runs on. The final exit clears it.
When the fight ends, every skill leaves it, an extra skill's slots with it:
a Hacker whose Multi Control beams were cooling on the Rhino it had just
turned reads all five idle on the last tick,
`tests/hacker/multi-control-slots-leave-at-the-end.yaml`.

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

- Near-equal z coordinates put initial RVO agents in the team's x order
  rather than recording-ID order; a node-array expansion distinguishes the
  neighbour sets and movement: `tests/corpus/2477-r4.yaml`.

- A Sledgehammer substitutes its prepared visible backup for a hidden
  Sandworm; the backup's same-tick death makes it search again and lock a
  living Crawler: `tests/corpus/2344-r5.yaml`.

- Nearly symmetric Crawler targets retain their distinct signed search angles
  when a Mustang presearches, preserving its initial lock and final settlement:
  `tests/corpus/2259-r3.yaml`.

- A Marksman retains its Wraith attack target through cooling and deals its
  aerial damage when a previously released projectile lands then:
  `tests/anti_air/cooling-keeps-damage-target.yaml`.

- An Arclight killed before its shell lands on the same tick still deals
  Shockwave before its death callback:
  `tests/secondary_damage/shockwave-before-death-callback.yaml`.

- A Stormcaller waits until the update after its final burst shot before
  starting its next windup, with an already elapsed attack interval:
  `tests/attack_timing/burst-end-waits-next-update.yaml`.

- A Vortex's shield-breaking hit still applies Electromagnetic Cloud to the
  unit behind the depleted shield:
  `tests/secondary_damage/shield-break-keeps-cloud.yaml`.

- A War Factory's main gun counts its attacking search clock down and
  replaces a lock outside its arc without entering cooling:
  `tests/war_factory/main-attack-search-timer.yaml`.

- A Sandworm whose attack ended while the fight waited for reinforcements
  searches immediately when a Wasp Swarm joins, without waiting for the
  old backswing, and enters its movement transition:
  `tests/endgame/resumed-fight-discards-backswing.yaml`.

- A Raiden's grouped gun enters cooling with no actor attack target after
  its failed check finds an alive fallback covered by a shield:
  `tests/raiden/alive-fallback-clears-cooling-target.yaml`.

- An Abyss waiting for the opposing production-line Sledgehammers to join
  preserves its Dark Companion and Swarm Missiles clocks through the pause,
  then waits for their full remaining intervals:
  `tests/endgame/pending-summon-preserves-attack-wait.yaml`.

- An Anti-Armor Turret resumes with a ready clock after an empty side waits
  for Wasp Swarm, and releases its next shot on tick 50:
  `tests/turret/resumes-ready-after-core-pause.yaml`.

- Three Void Eyes enter attack with ready clocks, then begin their first
  wind-ups on the following update:
  `tests/attack_timing/entry-clears-modifier-deadline.yaml`.

- An Arclight shell fired at a grounded Wraith lands after it takes off:
  its primary splash misses, but Shockwave deals its secondary damage using
  the Wraith's current domain:
  `tests/secondary_damage/target-takes-off-before-shockwave.yaml`.

- An Overlord whose interval grows from 46 to 92 ticks still waits at
  attack time 89 on tick 43:
  `tests/attack_timing/overlord-waits-after-interval-growth.yaml`.

- A Stormcaller's interval grows to 132 on tick 27 while its clock retains
  its elapsed time; it has not started a blow at attack time 113 on tick 31:
  `tests/attack_timing/longer-interval-postpones-ready-clock.yaml`.

- A replicated group made when unit 27 dies preserves the native range-hit
  order, including units 31, 32, 33 and 39 at tick 352:
  `tests/dead_summon/buff-summons-enter-after-parent-leaves.yaml`.

- Mountain's main gun retakes motion from an extra skill and turns its
  turret towards tower 3 on tick 136:
  `tests/extra_weapon/main-gun-retakes-motion.yaml`.

- Mountain's second main gun keeps its attack motion on tick 205 after its
  retained construction target fell on tick 204:
  `tests/construction/standalone-fallen-target-range.yaml`.

- War Factory's idle guns retain Raiden 1 as their alive-only fallback
  during ticks 686 through 722 rather than replacing it with wall 5:
  `tests/search/idle-gun-keeps-alive-fallback-lock.yaml`.

- Final tick 271 clears blue construction 3's tower debuff before the red
  survivors' Photon Emission buffs:
  `tests/endgame/final-buffs-clear-by-team.yaml`.

- Mountain's second main gun refreshes its target from interceptor 6 to
  unit 14 on tick 403 before entering cooling, which retains unit 14:
  `tests/search/main-gun-arc-edge-refresh.yaml`.

- A War Factory's unit-level search keeps its hidden Sandworm on tick 465
  while the best visible replacement is within 100 metres but beyond
  99 metres:
  `tests/search/mech-search-range-margin.yaml`.

- A prepared Sledgehammer search scores with 95 metres of range, but after
  Sandstorm halves it to 47.5 metres on tick 321 it keeps the hidden tower
  rather than replacing it with a visible Sandworm outside the new range:
  `tests/search/prepared-search-live-replacement-range.yaml`.

- A fusillade sibling whose lock the core took retains its fallen wall
  while cooling; its already empty lock does not overwrite the new mech
  lock: `tests/raiden/core-takes-fallen-wall-lock.yaml`.

- A skill that exits during the drain clears its kill damage while grouped
  slots already cooling keep their targets:
  `tests/endgame/last-kill-damage-and-cooling.yaml`.

- Overlapping fires publish a unit's death once:
  `tests/terrain/fire-death-once.yaml`.

- An equipment's lifesteal callback follows the hit's buff application:
  `tests/lifesteal/generated-arclight-buff-order.yaml`,
  `tests/lifesteal/generated-marksman-buff-order.yaml` and
  `tests/lifesteal/generated-crawler-buff-order.yaml`.
- A Crawler's Absorption Module heals before its restored Replicate hit buff
  after Electromagnetic Interference and a change of side:
  `tests/lifesteal/restored-buff-follows-absorption.yaml`.
- Splash and Photon Coating target enumeration retain the prepared actor
  order when near-equal z coordinates give a different identity order:
  `tests/corpus/134520979-r2.yaml` and `tests/corpus/67409908-r6.yaml`.

- A Fortress follows Anti-Air Barrage's target while Rocket Punch's in-range
  target keeps the motion attacking through its recovery:
  `tests/extra_weapon/rocket-punch-motion-range-proxy.yaml`.

- A Fortress's main skill finishes cooling and Rocket Punch starts in the
  same tick, 112, locking the main skill:
  `tests/extra_weapon/rocket-punch-at-cooling-end.yaml`.

- An idle skill whose controlled target dies retains its actor target after
  losing its lock while projectiles drain, and clears it on final exit:
  `tests/control/idle-target-at-fight-end.yaml`.

- A Mustang projectile released and removed on the same tick strikes its
  living main tower outside the impact point's reach:
  `tests/construction/projectile-main-target-tower.yaml`.

- Steel Balls replace a dead lock while firing at a shield and keep their
  attack and its next hit:
  `tests/shield/lock-switch-keeps-shield-attack.yaml`.

- An idle Spider Mine retains a shield target after changing locks, submits
  the shield branch's movement point and publishes the resulting velocity:
  `tests/mech_search/idle-skill-keeps-shield-distance.yaml`.

- Scattered projectiles retain their launch offsets when a disabling buff
  enables tracking in flight:
  `tests/technology_disable/in-flight-scatter-offset.yaml`.

- A crystal interceptor precedes defensive wall constructions in the initial
  spatial tree, preserving which block a Raiden's chain randomly selects:
  `tests/chain/interceptor-before-wall.yaml`.

- Sandworm and Larva transitions continue after the last enemy dies,
  preserving inactive skill clocks and targets while holding-fire clocks
  still count: `tests/move_ability/transition-after-last-kill.yaml`.

- Summoned Wasps keep their facing for an equal-within-tolerance target
  angle and take a full turn when its remaining angle compares equal to
  the step: `tests/siege_mode/interceptor-keeps-trench.yaml`.

- An exhausted evenly allocated burst still names the fallen tower its
  skill retains, preserving the tower's radius and its projectile climb:
  `tests/extra_weapon/exhausted-missiles-disintegration.yaml`.

- A Fang's prepared search scores with the range Range Enhancement gave it
  as the search was prepared, on the tick an Electromagnetic Impact switches
  the technology off, and its Portable Shield reads disabled from that tick:
  `tests/corpus/201477923-r6.yaml`, tick 57.
- A Hound that a charging Fire Badger stands on keeps its lock and its facing
  and fires on: `tests/corpus/134367609-r4.yaml`, tick 300.
- A produced Sledgehammer moving onto its burrowed Sandworm lock keeps its
  turret facing once their horizontal positions coincide:
  `tests/turret/moving-tank-overlaps-burrowed-target.yaml`.
- A Sledgehammer whose lock walks out of its reach sets off with its turret
  still, and a Sandworm below when the fight is decided takes the defeated
  side's tower and walks on to it until the fight ends:
  `tests/corpus/201373545-r4.yaml`, ticks 504 and 1028. A Farseer
  sets off as a Sledgehammer does: `tests/corpus/134267654-r3.yaml`,
  tick 1739.
- A Fang's shot landing where the Fire Badger it was fired at died does
  nothing, though a shield holds the spot, and another flying into a shield on
  its way is taken by it: `tests/corpus/67252976-r7.yaml`, ticks 137
  and 84.
- A Crawler converted after preparation takes a live search when none was
  queued at the tick's start: `tests/control/turned-crawler-unprepared-search.yaml`.
- A prepared search scores the candidates the tree held about the searcher as
  the tick opened: `tests/corpus/67260372-r4.yaml`, tick 42, where
  Crawler 311 locks Centurion 159, which the tree by its turn no longer
  answers for its square.
- The presearch staggers each side's first searches in its update order, not
  its identities': `tests/corpus/67152171-r3.yaml`, blue's Mustang 27,
  whose searches fall on ticks 1, 12, …, 133.
- A main skill starts its attack in its own update: a Melting Point prepares
  on the Crawler it locks, and goes idle when its own beam fells it first:
  `tests/extra_weapon/energy-diffraction-crawler.yaml`, tick 283, and
  `tests/corpus/67158166-r5.yaml`, tick 234. Its idle search takes
  what the search answers as its turret comes onto its lock:
  `tests/corpus/67158166-r5.yaml`, tick 316.

- A Mustang's shot at a Crawler running away is spent on nothing out of its
  owner's reach: `tests/corpus/67156074-r5.yaml`, tick 539; and
  `tests/corpus/268477093-r3.yaml`.
- A Wraith dying with Land Cruiser loses its grounded domain. Wasp shots
  420 and 421 then lose the cross-domain reach allowance and are removed at
  tick 1073; the complete fight ends at tick 1088:
  `tests/projectile/dead-target-restores-flight-reach.yaml`.

- A Crawler on the very edge of a Tarantula's splash is spared:
  `tests/corpus/67159970-r2.yaml`, tick 188.
- A Fire Badger's shot at a wall block that fell while it flew lands on
  nothing, splash included, and on the fight's last tick a Wasp's shot at a
  tower torn down is removed before the towers fall:
  `tests/corpus/67158166-r2.yaml`, ticks 290 and 765.
- A torn-down construction clears its buffs immediately before its death
  event, while projectiles remain in flight:
  `tests/endgame/torn-down-construction-clears-buff.yaml`.
- The fallen side's towers stand in the solve of the tick they are torn down:
  red's Fortress dies to a Rhino on a solve tick with Wasp shots still in the
  air, and the two Typhoons walking on are published, four ticks later, the
  speed a solve with red's towers in it gives:
  `tests/endgame/towers-torn-down-stand-in-the-solve.yaml`, ticks 380
  and 384.

- A Stormcaller whose burst goes on at a dead target stops idle once, at the
  point it stands on, and an RVO solve that nudges it off that point steers
  it back to it rather than to where it was nudged:
  `tests/corpus/134370978-r7.yaml`, ticks 183 to 193.
- A Raiden's slot that strikes a building counts the blow as one at a unit
  does, reading `performCount` 1 that tick: `tests/corpus/134370978-r7.yaml`,
  tick 359.
- A Sandworm left with only aircraft to fire at locks the nearest, 49 metres
  off edge to edge, and stays idle where it stands: `tests/corpus/134370228-r7.yaml`,
  tick 856.
- A Vortex left with only aircraft to fire at locks one, walks on it, idles
  in touch, searches every eleven ticks and sets off again when its lock
  changes: `tests/corpus/201370830-r3.yaml`, ticks 1227 to 1347.
- A Crawler attacking a tower that another Crawler fells locks the Abyss,
  which it cannot fire at, stands idle on that update and walks on it from the
  next: `tests/sweep/turret-1787720817.yaml`, ticks 1091 to 1093.

- A unit in a defence area beside a flanked tower enters the fight a quarter
  turn towards the flank, and one of the same formation outside it does not:
  `tests/corpus/268487043-r4.yaml`; in replay 134270595 round 4 the
  back row of a Fang formation faced 270 degrees and its other rows 0, read
  from `PresearchTargetController.SearchTarget` with a temporary hook.
- A Phantom Ray keeps its close Steel Ball target through both projectiles
  while its root turns, then resumes the skill check and cools:
  `tests/projectile/bodyless-burst-target-crosses-arc.yaml`.
- A Steel Ball's avoidance trace selects the slightly smaller speed term
  within tolerance and publishes the resulting velocity four ticks later:
  `tests/avoidance/trace-step-tolerance.yaml`.
- An idle Marksman an RVO solve nudges while it cools goes back to the point
  it stopped at: `tests/corpus/201373545-r2.yaml`, ticks 1112 to 1121.
- A tick that leaves neither side a unit, with no shot in the air, fells all
  four towers: `tests/battle_skill/missile-strike-both-sides.yaml`.
- Every unit's current interval, its stagger and the three readings that
  complete it, on every tick of the standard unit fights, one directory per
  unit, `tests/marksman/` among them.
- A skill whose row's interval is 0 reads one tick, on the fight's last tick
  too: Spider Mines that outlive the fight,
  `tests/extra_weapon/spider-mine-outlives-the-fight.yaml`.
- A grouped skill's slots, their locks and their reach, and a grouped unit's
  core interval: `tests/wraith/`.
- Each slot's own states, the core leaving its attack while its siblings go
  on, the unit's lock following the latest slot, a sibling giving up a unit
  it shares with the core, and a slot's interval outliving its attack,
  in the Wraith's M2, M3 and M6 fights: `tests/wraith/`.
- A sibling that has struck keeping a unit it shares with siblings still
  preparing, and a free-moving Wraith at full speed off its facing, in the Wraith's M3 with
  seed 1787720817: `tests/wraith/`.
- Where a charging Crawler is sent, and a Marksman's quick switch that cannot
  follow a kill: `tests/crawler/vs-marksman.yaml`, `tests/marksman/vs-arclight.yaml`.
- The body travelling toward the lock, attacking without moving, and the
  weapons on a construction while the body keeps the lock:
  `tests/construction/`.
- Target scoring, ordinary projectiles, damage clamping, the first-attack
  delay, the backswing, dead-target retention and the endgame ordering, in the
  unit pairings, each unit's `vs-*.yaml`: `tests/rhino/vs-arclight.yaml`,
  `tests/crawler/vs-crawlers-60.yaml` and `tests/fang/vs-fangs.yaml` among them.
- A unit's personal shield enabled with no shield of its own, in every unit's
  standard fights, `tests/rhino/` among them.
- A following projectile's offset, the order a single weapon lands its
  offsets, a simulated-motion shot at a dead unit, a weapon's index, and a
  splashing beam, in the standard fights of the Phantom Ray, Fire Badger,
  Typhoon, Hound, Sabertooth and Melting Point: `tests/phantom_ray/`,
  `tests/fire_badger/`, `tests/typhoon/`, `tests/hound/`,
  `tests/sabertooth/`, `tests/melting_point/`.
- A burst's aim, a climbing projectile, two weapons' offsets in three
  dimensions, a burst that goes on after its target leaves reach, a turret on a
  retarget, and a Vortex's single grouped weapon and its kill, in the standard
  fights of the Farseer, Overlord, Melting Point and Vortex:
  `tests/farseer/`, `tests/overlord/`,
  `tests/melting_point/`, `tests/vortex/`.
- The blow waiting a tick after the idle state is left, and an idle skill
  giving up a lock it cannot fire at, in the standard fights of the Fortress,
  Sledgehammer, Typhoon and Melting Point: `tests/fortress/`,
  `tests/sledgehammer/`, `tests/typhoon/`,
  `tests/melting_point/`.
- An Overlord retaining its burst's attacking phase on the tick the last
  enemy is controlled, then leaving it on the next non-fighting update:
  `tests/projectile/burst-waits-for-core-stop.yaml`.
- A projectile's climb measured as it is created, and a moving unit stopped
  when the fight stops, in the Overlord's standard fights:
  `tests/overlord/`.
- A burst's climb measured to where its target stood after the target's own
  side moved that tick, for a Farseer's and an Overlord's bursts at a Rhino
  walking at them: `tests/projectile/`.
- A Phantom Ray moving after a Rhino on the update its burst's last shot
  leaves, and going idle on the next:
  `tests/projectile/phantom-ray-rhino-walks-off.yaml`.
- A cooling that goes on through a won fight, and an attack that goes idle,
  in the Phantom Ray's standard fights: `tests/phantom_ray/`.
- A grouped slot whose siblings hold nothing searches from its own weapon and
  in its own reach: `tests/corpus/134370978-r8.yaml`, tick 145, Raiden
  386's second gun.
- Raiden retains the first Crawler of its opponent's actor list when four
  overlapping Crawlers score equally, while its Fork skills hold their own
  targets: `tests/search/grouped-score-tie-team-order.yaml`, tick 293.
- A Mountain's second gun keeps the hidden Sandworm when the visible
  replacement is outside its own range:
  `tests/search/grouped-visible-fallback-range.yaml`.
- A cooling that does not run through a won fight, a Raiden's grouped slot's
  among them: `tests/corpus/201477923-r5.yaml`, ticks 1618 to 1625,
  Phantom Rays 56 and 58 and Raiden 19's second gun.
- An extra skill holding no lock as the fight is won stands as it was, a
  Sticky Oil Bomb that began cooling on the deciding tick still cooling and
  a Scorching Charge still locking: `tests/corpus/67263060-r6.yaml`,
  ticks 994 to 1003, Phantom Rays 25 and 100 and Fire Badger 75.
- Wasps still on a Mobile Beacon when the fight is won going on moving and
  turning until the fight ends, and idle at its end:
  `tests/battle_skill/beacon-wasps-won.yaml`,
  `tests/corpus/134270595-r4.yaml`.
- A winner's unit that updates after the last death taking a tower, and
  letting it go the tick after, in the Stormcaller mirrors:
  `tests/stormcaller/vs-stormcaller.yaml`; a burst the fight's end stops, in the
  Phantom Ray's M2 fight: `tests/phantom_ray/`.
- The Raiden's fusillade, its siblings' towers, a core taking a sibling's unit,
  siblings cooling and searching on their timers, and its weapons' poses, in
  the Raiden's standard fights: `tests/raiden/`, and nine
  Raidens' blows landing on nine Fangs: `tests/raiden/`.
- A Raiden core taking its sibling's tower when the Fortress itself is out
  of reach although its shield is within reach:
  `tests/raiden/shielded-lock-outside-actor-range.yaml`.
- A child whose siblings hold no lock using its own longer range to replace
  a hidden best candidate with a visible one in its normal fallback search:
  `tests/sweep/coincident-target-direction.yaml`.
- A presearched target a few raw units left of straight ahead faced at
  +0.245°, in the standard fights of the Steel Ball, Stormcaller, Hound, Fire
  Badger and Phantom Ray: `tests/steel_ball/`,
  `tests/stormcaller/`, `tests/hound/`,
  `tests/fire_badger/`, `tests/phantom_ray/`.
- A Tarantula whose prepared search has no eligible opponent finding a
  Wraith immediately when it lands, through live squares rather than the
  cached opponent list: `tests/search/landing-without-prepared-candidates.yaml`.
- A Stormcaller whose live lock walks inside its minimum range taking blue's
  interceptor on that tick, and a second doing the same three ticks later:
  `tests/search/lock-inside-min-range.yaml`.
- A red Wraith's search scoring blue's units where they stand after they
  moved, and taking another than the tick's start would give it:
  `tests/corpus/201340110-r4.yaml`, tick 121.
- A Phantom Ray whose next lock stands inside the enemy's shield naming no
  unit for its whole cooling: `tests/corpus/201370830-r5.yaml`, ticks
  113 to 130.
- A Melting Point's beam at a Phoenix splashing no Tarantula under it:
  `tests/corpus/67159970-r6.yaml`, tick 371.
- An idle core in a batch of standalone weapons uses its prepared search
  even while its siblings hold locks; a new summon on that tick is not among
  those prepared inputs: `tests/search/standalone-core-prepared-idle.yaml`.
- A Raiden with all slots idle changes to movement at tick 481 when its
  core's target leaves range, despite a sibling retaining another lock;
  its root does not rotate on the entry tick. The same fight's repairs on
  tick 614 follow registration order, Rhino 7, summoned Rhino 8, then the
  later arriving Rhino 5:
  `tests/raiden/idle-sibling-lock-does-not-hold-motion.yaml`.
- The first tick's live searches see Phantom Rays that have just cloaked:
  Typhoons 1 and 2 choose tower 4 instead, and the complete fight lasts 575
  ticks: `tests/search/first-tick-sees-cloak.yaml`.
- Each independent Mountain weapon consumes its own prepared search when
  queued; summons joining on that tick are absent from all four jobs:
  `tests/search/standalone-sibling-prepared-summon.yaml`.
- Summons that join on one tick searching where everything stands then, and
  locking the enemy summons that joined with them:
  `tests/battle_skill/underground-threat-both-sides.yaml`, tick 48.
- A Stormcaller's blow that lost its target inside the minimum range giving
  its interval back, and one whose target died keeping it:
  `tests/corpus/201372157-r2.yaml`.
- A Stormcaller whose minimum-range search falls back to a geometrically
  attackable building while idle keeps the three ticks spent in its wind-up:
  `tests/attack_timing/idle-fallback-keeps-windup-clock.yaml`.
- A blow whose attack point and backswing outlast its interval is fitted into
  it: `tests/modifier/technology-interval-value.yaml`.
- EMP changing a Sandworm's interval during windup leaves the already fitted
  backswing duration intact: `tests/technology_disable/emp-retains-fitted-backswing.yaml`.
- A Raiden's sibling killing its auto-movement lock idles the motion on that
  update, then the next search turns it to a living lock:
  `tests/technology_disable/emp-retains-fitted-backswing.yaml`.

- A hit hands back the whole part of its life times the multiplier, as far
  as the owner's maximum, through a blow, a projectile and a beam with no
  splash, and an item's source overrides a technology's:
  `tests/lifesteal/absorption-module.yaml`,
  `tests/lifesteal/technology-lifesteal.yaml`,
  `tests/lifesteal/absorption-module-over-technology.yaml` and
  `tests/lifesteal/technology-lifesteal-beam.yaml`. Battlefield shield
  absorption reaches the ranged hit effect in
  `tests/lifesteal/range-hit-on-battlefield-shield.yaml`, while EMP Barrage
  does not register Energy Absorption in
  `tests/lifesteal/energy-absorption-extra-skill-scope.yaml`.

- A splash value widens a skill's splash, from an item onto a skill with
  none and onto one with its own, from a technology and from an officer:
  `tests/splash/explosive-ammo.yaml`,
  `tests/splash/explosive-ammo-arclight.yaml`,
  `tests/splash/assault-mode.yaml`,
  `tests/splash/improved-overlord.yaml` and
  `tests/splash/improved-tarantula.yaml`.
- A side's tree takes its towers, then its units, then its constructions,
  and a query takes a node's own before its children's, children in order:
  a splash at two Void Eyes in one node reads them in the order the units
  went in before their node split, tick 616, and a fire that Fangs of two
  children enter reads the first child's first, tick 83,
  `tests/corpus/67156354-r3.yaml`.
- A unit repairs from its twentieth hurt tick on, every second tick, by
  the whole part of its maximum life times the rate, from an item and from a
  technology: `tests/repair/nano-repair-kit.yaml` and
  `tests/repair/field-maintenance.yaml`.
- A repair of state 1 runs only below, its clocks standing still while its
  unit is up: Burrow Maintenance's Sandworm, hurt from tick 147, repairs
  first on tick 283, twenty ticks after its burrow ended,
  `tests/move_ability/burrow_maintenance.yaml`.

- A shield of the whole maximum life takes hits before life, its last hit
  only what it held: `tests/energy_shield/portable-shield.yaml` and
  `tests/energy_shield/energy-shield-technology.yaml`.
- An armour technology's reduction is its entry for the unit's level, taken
  off each hit and leaving at least 1: 60 and 180 off a Marksman's shot on a
  level-1 and a level-3 Rhino, `tests/armor/rhino-armor-enhancement.yaml`
  and `tests/armor/rhino-armor-enhancement-3.yaml`, and 1 left of an
  Arclight's hit on a Mountain, `tests/armor/mountain-plating.yaml`;
  577 hits on six armoured Phantom Rays, 21 of them left at 1,
  `tests/corpus/134258634-r4.yaml`.
- A skill locking a unit that flies reaches it by its air range and deals it
  its air damage: a Marksman with Aerial Specialization fires at an Overlord
  from 170 metres for 4425, `tests/anti_air/aerial-specialization-overlord.yaml`,
  and a Wasp, a Mustang and a Farseer squad deal aircraft 1.9 times what they
  deal a Rhino, `tests/anti_air/aerial-specialization-squads.yaml`.
- A dead unit's shot landing on the tick it died deals the air damage it was
  fired with, and one landing later the ground damage: a Mustang's on a
  Phantom Ray, `tests/corpus/67154636-r6.yaml`.
- A search by distance counts the air offset off an aircraft's distance score
  and nothing else: the Marksman takes an Overlord further off than a
  Mountain, `tests/anti_air/aerial-specialization-pick.yaml`, and the
  Mountain within its range over an Overlord beyond it,
  `tests/anti_air/aerial-specialization-out-of-range.yaml`.
- The same against the ground: a Wasp's ground rate,
  `tests/ground_attack/ground-specialization-wasp.yaml`, and a Phantom
  Ray's ground range and search offset,
  `tests/ground_attack/ground-targeting-phantom-ray.yaml`.
- An air-attack technology turns a skill off aircraft,
  `tests/anti_air/grenade-launcher-overlord.yaml`, or onto them,
  `tests/anti_air/anti-aircraft-ammunition-arclight.yaml`,
  `tests/anti_air/anti-aircraft-ammunition-tarantula.yaml` and
  `tests/anti_air/anti-aerial-sandworm.yaml`, and an extra skill with
  it where its row says so,
  `tests/anti_air/anti-aircraft-ammunition-spider-mine.yaml`.
- A projectile flies at its row's speed with the skill's value added:
  `tests/anti_air/grenade-launcher.yaml`.
- A second damage strikes what lies around a hit and the hit did not strike:
  `tests/secondary_damage/shockwave-crawlers.yaml` and
  `tests/secondary_damage/shockwave-marksmen.yaml`.
  Subterranean Blitz's numeric damage reduction leaves Shockwave's secondary
  damage unchanged:
  `tests/corpus/201475625-r9.yaml`.

- A Stormcaller shell aimed at a ground Void Eye remains on the ground as
  the target enters Aerial Mode at tick 81. Its tick-82 impact creates fire
  without damaging the aerial units or their personal shields:
  `tests/projectile/keeps-domain-when-target-takes-off.yaml`.

- A Mustang retaining its destroyed wall through an after-wait that outlasts
  its interval: `tests/construction/quick-switch-keeps-after-wait.yaml`.

### Read

- Initial avoidance agents follow sorted team activation, and tree insertion
  preserves that list: `FightTeam.PrepareActors`, `FightTeam.OnFightStart`,
  `FightMech.OnFightStart`, `MotionController.Active`,
  `RVOControllerFixed.Active`, `Simulator.DoAddAgents`,
  `RVOQuadtree.Insert`, `RVOQuadtree.GetNodeIndex`,
  `RVOQuadtree.Node.Distribute`.

- Relative target angles preserve `FPoint` tolerance when wrapping and folding:
  `FightUtility.CalculateAngle`, `FightUtility.ClampRotation`,
  `FPoint.op_GreaterThan`, `AngleCalculator.Calculate`,
  `ScoreRatingTargetSelector.CalculateScore`.

- Secondary damage reads the skill's provider flag rather than its owner's
  current life, and death deactivates that provider:
  `DamagePerformer.CheckSecondaryDamageApplied`,
  `SecondaryDamageIntensifyEffectProvider.DoActive`,
  `SecondaryDamageIntensifyEffectProvider.DoDeactive`, `FightMech.OnDead`.

- A new blow requires the controller idle before its update:
  `SkillAttackState.Update`, `SkillAttackState.TryPerformAttack`,
  `SkillAttackController.IsIdle`, `SkillAttackController.Update`.

- A shield's single-target hit dispatches its hit callback before performing
  its secondary effect: `DamagePerformer.PerformSingleEffect`,
  `DamagePerformer.PerformSecondaryEffect`,
  `DamagePerformer.PerformSecondaryRangeEffect`.

- Every attacking skill counts its search clock down after the target check
  and attack attempt, including main skills and constructions:
  `SkillAttackState.Update`, `FightSkill.RefreshSearchTargetTime`,
  `SearchTargetController.RefreshSearchTargetTime`,
  `SearchTargetController.CanStartSearch`.

- Leaving a stopped fight forces the skill idle and stops its active attack
  controller: `FightSkill.ExitFight`, `SkillAttackState.Exit`,
  `SkillAttackController.Exit`.

- A failed check searches its fallback's attack target before returning;
  a shield replaces the actor target: `SkillAttackableChecker.Check`,
  `FightSkill.SearchAttackTarget`, `FightSkill.ChangeAttackTarget`.

- A skill's clock advances only when it updates, and a blow waits for that
  clock to reach its interval: `SkillManager.Update`, `FightSkill.Update`,
  `SkillAttackState.TryPerformAttack`, `FightSkill.CanPerformAttack`.

- Entry with no initial cooldown refreshes the sampled attack interval and
  sets attack time to it: `FightSkill.EnterFight`,
  `FightSkill.RefreshAttackInterval`.

- Data refresh preserves elapsed time except for a clock exactly equal to
  its old interval: `FightSkill.RefreshDatas`.

- Spatial destruction precedes buff death summons, which join immediately:
  `FightActor.OnDead`, `FightMech.OnDead`, `BuffManager.OnMechDead`,
  `SummonSystem.DoCreateMech`.

- A main searcher replaces an attacker that is no main target provider:
  `FightSkill.SearchLockTarget`, `FightSkillBase.IsMainTargetProvider`,
  `FightMech.SetAttacker`.

- A skill's range check tests visibility and distance without an alive or
  targetable predicate: `SkillAttackRangeChecker.IsActorInAttackRange`.

- An idle skill skips attack-target search when its lock search sets
  `IsIdle`: `SkillIdleState.TryPerform`.

- Final exit walks groups and teams, each team's live mechs before its
  constructions: `GroupManager.OnExitFight`, `FightTeam.ExitFight`.

- A failed angle check reports a missing target at a limited rotation
  endpoint, using fixed-point equality; the checker may then search again:
  `SkillAttackAngleChecker.IsWeaponInAttackAngle`,
  `SkillAttackableChecker.CheckWhenLoseTarget`.

- The unit-level replacement range test is strictly less than main range
  minus one metre: `FightMech.GameRiver.Fight.IAttacker.IsActorInAttackRange`.

- Hit callback order: `EffectProvider.Sort` orders data sources by `GetID`;
  `FightEffectMananger.Sort` compares the first source of each provider.
  `FightEffectMananger.RegisterMechEvent` registers those providers, and
  `SkillHitEffectController.PerformHitEffect` walks its registered list.
  `BuffEffectProvider.DisableEffect` calls `UnregisterEffectEvent` and
  `EnableEffect` calls `RegisterEffectEvent`, each visiting its controllers;
  `BuffCycleController.UnregisterMechEvent` and `RegisterMechEvent` remove
  and add their hit effects through the skill manager.

- A skill's motion range check delegates to the running preemptive skill:
  `FightSkillBase.GameRiver.Fight.IAttacker.IsAttackTargetInAttackRange`,
  `SkillManager.HasPreemptiveSkill`, `SkillManager.GetRunningPreemptiveSkill`.

- Cooling enters idle on its last update, and a preemptive checker sees the
  main skill's current state: `SkillCoolingState.Update`,
  `PreemptiveSkillStartAttackChecker.IsMainSkillIdleState`.

- The dead-lock quick-switch guard compares the actor attack target, which a
  shield replacement leaves null: `SkillAttackableChecker.Check`,
  `FightSkill.SearchAttackTarget`, `FightSkill.ChangeAttackTarget`.

- Automatic pursuit's shield and owner-radius branches:
  `AutoMoveBehaviour.CalculateTargetDistance`, `MotionMoveState.MoveUpdate`,
  `FightSkillBase.GameRiver.Fight.IAttacker.IsAttackingEnergyShield`,
  `FightSkill.GetTargetEnergyShield`,
  `FightMech.GameRiver.Fight.IAttacker.IsAttackingEnergyShield`.

- A projectile stores its launch offset independently of tracking and uses it
  when current tracking becomes enabled: `FightProjectile.Init`,
  `FightProjectile.Update`.

- Turning keeps equal-within-tolerance angles and selects the full step on
  a remaining-angle tie: `FightTransform.RotateTo`,
  `FightTransform.RotateLeft`, `FightTransform.RotateRight`, `FPoint.Min`,
  `FPoint.op_Equality`, `FPoint.op_LessThan`.

- An after-wait disables target checks, and the next blow requires an idle
  attack controller: `SkillAttackController..ctor`,
  `SkillAttackController.IsEnableCheckTarget`,
  `SkillAttackController.AttackWaitController.IsEnableCheckTarget`,
  `SkillAttackState.Update`, `SkillAttackState.TryPerformAttack`.

- A burst suspends skill target checks, while motion only tests range and
  turns: `ProjectileMultiAttackPerformer.IsEnableCheckTarget`,
  `SkillAttackState.Update`, `MotionAttackState.Update`,
  `MotionAttackState.AttackRotate`.

- Avoidance trace step-size ties use the speed term, and the next boundary
  publishes the solve: `GRPF.RVO.Sampled.Agent.Trace`, `FPoint.Max`,
  `FPoint.op_GreaterThan`, `GRPF.RVO.Sampled.Agent.BufferSwitch`.

- A target on the unit's own position is at angle 0 and is not turned to:
  `SkillAttackAngleChecker.IsAttackTargetInAttackAngle`,
  `SkillAttackAngleChecker.IsWeaponInAttackAngle`,
  `FightUtility.CalculateAngle`, `MotionAttackState.AttackRotate`,
  `MotionMoveState.NormalRotate`, `FightMechBody.RotateTo`,
  `FightTransform.RotateTo`.
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
- An inactive manager skips its non-fighting path, while an active manager
  holding fire updates its main skill whatever the fight does:
  `SkillManager.Update`, `SkillManager.EnterHoldFire`.
- A projectile preserves its creation-time target domain for range damage;
  a single hit names the target directly: `FightProjectile.Init`,
  `FightProjectile.GetTargetType`, `FightSkillBase.GetAttackTargetType`,
  `FightProjectile.GameRiver.Fight.IDamageProvider.GetMainTarget`,
  `DamagePerformer.Perform`, `DamagePerformer.PerformSingleEffect`,
  `FightProjectile.PerformDamage`.
- A projectile that locks its target is released with no damage out of its
  owner's reach: `FightProjectile.Update`, `FightProjectile.Init`,
  `FightProjectile.CalculateMaxMoveDistance`, `FightCalculator.IsInRange3D`,
  `FPoint.Sqrt`. The radius, the range and the reach were recorded by the
  `projectile_reach` channel.
- Reach uses the owner's and target's current flight flags even after the
  target dies; its cached arrival point follows only a living target:
  `FightProjectile.CalculateMaxMoveDistance`, `FightProjectile.Update`,
  `FightMech.IsFly`, `FlyTechEffectProvider.DoDeactive`.

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
- An idle skill's unit walks on its lock until it is in touch, ten times its
  radius for one standing idle with a move ability and twice it otherwise,
  and one attacking stops first: `MotionAttackState.Update`,
  `MotionIdleState.Update`, `AutoMoveBehaviour.IsActive`,
  `AutoMoveBehaviour.IsIdle`, `AutoMoveBehaviour.IsLockTargetInTouchRange`,
  `MotionController.moveAbility`.

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
- A grouped batch answers motion range through its core skill; a child's
  retained lock adds no exception: `FightSkillBatch.IsAttackTargetInAttackRange`,
  `FightSkill.IsAttackTargetInAttackRange`,
  `SkillAttackRangeChecker.IsAttackTargetInAttackRange`, `MotionAttackState.Update`.
- A search is prepared for the mechs alive and not travelling as the modules
  finish, and answered only for them: `FightingState.Update`,
  `FightCoreSystem.PreCalculate`, `SuperDeploymentSystem.IsTravelling`,
  `ScoreRatingTargetSelector.TrySelect`.
- The first preparation follows the first tick's modules, so its searches
  have no queued job: `FightingState.Update`, `FightCoreSystem.PreCalculate`,
  `ScoreRatingTargetSelector.TrySelect`.
- A projectile arriving at a target no longer valid is released with nothing
  done when it has no splash or is in simulated motion:
  `FightProjectile.Update`, `FightActor.IsValidTarget`,
  `IProjectileDataSource.GetSplashRange`, `IProjectileDataSource.IsSimulateMode`.
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
- Full-team candidate lists and their domain partitions keep actor order:
  `OpponentController.Refresh`, `OpponentController.GetActors`,
  `OpponentController.Remove`, `OpponentController.Add`, `FightController.Update`,
  `FightTeam.GetActors`,
  `FightTeam.PrepareActors`.
- A grouped slot searches around its siblings' locks:
  `SkillSearchTargetController.PerformGroupedSkillSearch`,
  `SkillGroup.CanAttackSameTarget`,
  `SkillAttackableChecker.TrySearchGroupSkillLockTarget`; visible fallback
  uses the passed attacker: `ScoreRatingTargetSelector.Select`,
  `SkillAttackRangeChecker.IsActorInAttackRange`.
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
- Grouped allocation tests the candidate actor rather than its current
  shield target: `SkillSearchTargetController.PerformGroupedSkillSearch`,
  `FightSkill.IsActorInAttackRange`,
  `SkillAttackRangeChecker.IsActorInAttackRange`.
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
- A tower torn down keeps its agent until its `OnDead`: the RVO runs after
  the teardown, `FightingState.Update`, `RVOSimulatorFixed.DoFixedUpdate`;
  the teardown takes life only, `FightCoreSystem.TryDstroyTower`,
  `FightActor.DestroySelf`, `FightConstruction.DestroySelf`,
  `FightActor.ReduceLife`, `DeadEffectSystem.OnActorDead`; the agent goes at
  `OnDead`, `FightCrystal.OnDead`, `FightTeamController.OnTowerDestoryed`,
  `FightCrystal.Deactive`, `RVOControllerFixed.Deactive`, and a unit's at
  its own, `FightMech.OnDead`, `MotionController.ExitFight`.
- A tower is an ordinary search candidate: `FightTeam.AddTower`,
  `FightTeam.AddActor`, `MechSearchTargetController.SearchLockTarget`.
- Once at most one side has a live unit the fight is off, and only a skill
  holding a lock is reached, to leave it: `FightCoreSystem.IsStepFinish`,
  `FightCoreSystem.TeamUpdate`, `FightMech.Update`, `SkillManager.Update`,
  `FightSkill.ExitFight`, `FightSkill.StopAttack`.

- Lifesteal: `DamagePerformer.PerformSingleEffect` and
  `DamagePerformer.PerformRangeEffect` hand the sum `FightActor.ReduceLife`
  returned, plus the energy taken from battlefield shields, to
  `IDamageProvider.DispatchHitDamageEvent`;
  `SkillDamageProvider` and `FightProjectile` pass it to
  `FightSkill.DispatchHitDamageEvent`, `SkillHitEffectController.PerformHitEffect`;
  `LifeStealEffectProvider.PerformHitEffect` checks the owner's life and
  `CanDisable`, multiplies by `ILifeSteal.GetLifestealMuliplier` and calls
  `FightMech.StealLife`, which `BuffManager.IsRecoverDisabled` stops and
  `FightActor.AddLife` caps. The provider's source is
  ``SingleEffectProvider`1.AddDataSource``'s first after sorting by
  `EffectProvider.IsOverrideEffect`, which compares `Equipment.GetPriority`
  and `Technology`'s; `Equipment`'s `CanDisable` is false and
  `Technology.CanDisable` reads `ignoreElectricEffect`. Its admitted skills
  follow `SkillManager.AddHitEffect` and `SkillDataModifier.AvaliableCheck`
  through the source's `mainSkillEffect`, `extraSkillEffect` and the extra
  weapon's `GetDamageRate`.

- A splash value: `SplashEquipment.AddData` writes its row through
  `MechDataModifer.TryAddCommonData` and then ``Equipment`1.AddSkillData`` of
  `SkillDataChangeFloat.SplashRangeValue`; `FightSkill.GetSplashRange` adds the
  skill's data at that index to its row's splash.
- A query: `RangeItemController.UpdateAffectedActorChange` asks
  ``FightQuadtree`1.Query``, and ``FightQuadtreeNode`1.Query`` takes the
  node's elements and then queries each node of its list.
- A side's tree: `FightPrepareState.Enter` calls `GroupManager.OnFightStart`
  before `FightTeam.CreateQuadtree`. `FightTeam.OnFightStart` calls
  `PrepareActors`, sorting units, crystals and constructions by
  `FightUtility.ActorComparer`, with towers first in their loader order.
  `CreateQuadtree` inserts that prepared `activeActors` order, independently
  of the recording's identities;
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
- Repair controllers enumerate their SyncDictionary in registration order;
  deactivation removes a controller and reactivation appends a new one:
  `FightEffectSystem.OnEnterFight`, `AutoRecoveryEffectProvider.DoActive`,
  `AutoRecoveryEffectProvider.DoDeactive`, `TeamAutoRecoveryManager.AddMech`,
  `TeamAutoRecoveryManager.RemoveMech`, `TeamAutoRecoveryManager.Update`.
- A repair's state: `TeamAutoRecoveryManager+AutoRecoveryController.Reset`
  sets `isCondition` for `AutoRecoveryStateType.Normal` alone; `TeamAutoRecoveryManager.AddMech`
  registers `MechEnterMoveEnd` and `MechExitMoveBegin` on the
  `UndergroundMoveAbility`'s `OnEnterMoveEnd` and `OnExitMoveBegin`, which
  set and clear it; `TeamAutoRecoveryManager.Update` passes over a
  controller out of condition; `MoveAbility`'s `OnTransitionEnd` invokes
  `OnEnterMoveEnd` whether the unit is alive or not.

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

- **An evenly allocated burst whose list has no unit left alive.** Its
  missile leaving at no target is read from the build and not recorded. The
  game aimed an Abyss's last Swarm Missile at t639 at a Fortress the same
  tick's sweep had struck for all its 3748 life before the release, and
  recorded the strike first; how a strike's life lands within the tick so
  that the Fortress still lives for the release is not read, and the
  simulator finds the list gone there.
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
- **A second damage's hit effects.** `DamagePerformer.ApplySecondaryDamageToActors`
  hands what it struck and the life it took, summed over
  `PerformSecondaryHitTargetEffect`, to `FightSkill.DispatchSecondaryDamageEvent`,
  which calls the skill's `DispatchHitDamageEvent` with `isSecondary` set. A
  lifesteal (`LifeStealEffectProvider`), a wreckage recovery and a kill
  explosion do not ask it and take the hit as any other. A buff source
  writes on it only when it is a second damage's own
  (`BuffCycleController.PerformHitEffect` and
  `ISecondaryDamageIntensifyEffectDataSource`), and the main skill's fire
  leaves none (`FireIntensifyEffectProvider`). No `PerformMainSkillHitted`
  follows, which a projectile's own dispatch raises. An Arclight with
  Shockwave and an Absorption Module takes back a share of each, a second
  heal after its shell's. Read from the build; no pinned fight records it.
- **A row that disables technologies or writes a buff** (Electromagnetic
  Cloud). The simulator refuses it.
- **Repair with its technologies disabled**, which stops the clocks
  (`AutoRecoveryEffectProvider.DisableEffect`) and which no recorded fight
  does, an item's repair beside a technology's among them; and which of an
  item and a technology a unit with both keeps, since
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
  order they are first named; a fusillade of weapons that do not strike, which the simulator refuses;
  a fusillade sibling cleared of its lock while its core's lock stands in a
  shield beyond the block, whose shield target the simulator keeps under the
  core's lock; redistribution of
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
- **Projectiles**: interception, and every other projectile type.
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
- **A personal shield's** refresh when its unit's maximum life changes.
  `FightMech.RefreshLifeData` runs `EnergyShieldController.Refresh` when
  asked (`needRefreshShield`), which `RefreshData`, `AddData`, `RemoveData`,
  `ChangeLevel` and `RefreshLifeByKillCount` do and a stacking buff's own
  step (`IBEC_ChangeMaxLife.DoAdditiveEffect`) does not. An active shield
  with a maximum takes its new one from `RefreshMaxEnergy`, full where it was
  full and otherwise its share of the old, an `FPoint` quotient times the
  new. A Rhino with Combat Evolvement and a Portable Shield grows its shield
  with its life. Read from the build; no recording holds it, nor the shield's
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
