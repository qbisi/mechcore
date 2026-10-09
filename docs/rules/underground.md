# Underground movement

How a unit whose `MechData.moveType` is `Underground`, the Sandworm, moves:
it burrows to travel and surfaces to attack, and while it is below it cannot be
attacked.

The four numbers it reads are its `config/units/` file's `underground` block,
which `scripts/extract/extract-units.py` writes from the unit's `MechData`:
how long burrowing takes (`enter`), how long surfacing takes (`exit`), how far
into surfacing it stays hidden (`exit_keep`), and how near its lock it
surfaces (`attack_range`, the row's `undergroundExitRange`).

## Burrowing and surfacing are transitions of the motion

`MoveAbility.Create` gives the unit an `UndergroundMoveAbility`, and its
`MotionController` hands the ability to its `MotionFSM` whenever it changes
state through it. `ChangeToMoveState` goes through the ability unless the unit
is already underground, `ChangeToAttackState` always, and
`MotionMoveState.ChangeToIdle` always; `ChangeToIdleState` and
`ChangeToStopState` do not. A change through the ability enters the
`TransitionState`, which records as `transitioning`:

- into `MotionMoveState`, the unit burrows: `EnterMoveBegin`;
- out of `MotionMoveState`, it surfaces: `ExitMoveBegin`;
- otherwise, from idle to attack, nothing happens, and the next state is
  entered on the transition's first update, one tick later.

The state a transition leads to is entered only as the transition ends, so
what its `Enter` asks of the agent waits until then: `MotionAttackState.Enter`
calls `RVOControllerFixed.StopMove`, which hands the agent its speed. A
summon that joins never handed one, and changes to attack through the
transition, goes into the solve its transition spans with no speed, and
stands however it is pushed.

While the transition lasts the motion does nothing else: `TransitionState`
updates only the ability, whose `Update` adds a tick to its time and answers
whether the time has reached what the transition lasts, by `FPoint`'s
comparison. Twenty ticks make a second exactly. The state it leads to is entered
on that tick, and updated from the next.

**Both ends stop every skill.** `SkillManager.Deactive` stops each skill's
attack (`FightSkill.StopAttack`), which drops its lock and keeps its attack
target, leaves it idle, and stops the manager updating it. The manager is
active again when the transition ends. A unit that burrows on its first tick
has searched and locked on that tick, and reads no lock until the tick after
it is below.

**An attack interval stands still while the manager is not active.** A skill
counts towards its next attack in its own update (`FightSkill.Update` adds to
`attackTime`), so the ticks a transition lasts are not counted: a Sandworm
that set off burrowing as its attack began, and surfaced on another unit,
waits through the ticks it updated below and after it surfaced, and none of
the transitions'.

**Both ends lock the agent.** `RVOControllerFixed.Lock(false, false)` puts the
agent on collider priority 11 at full priority, locked: it does not move,
`RVOAgentFixed.CalculateVelocity` gives it no speed, `Agent.BufferSwitch`
zeroes its velocity at the next boundary, and its neighbours carry all the
avoidance of it. The end of a normal transition gives the agent back its own
collider priority and priority.

**Below, the unit is hidden.** `EnterMoveEnd` sets its visibility to `Hide`,
holds its skills' fire (`SkillManager.EnterHoldFire`, which updates the main
skill alone), and moves its agent to the underground RVO layer, where it
avoids only what is also underground. Surfacing shows it again once
`exit_keep` has passed (`NormalExitMoveBehviour`), and its end puts the agent
back on the ground layer.

**Below, it surfaces at its exit range, not its attack range.** A unit moving
below does not ask whether its target is in its skill's range:
`MotionMoveState.Update` asks its ability alone
(`UndergroundMoveAbility.IsLockTargetInRange`) whether its lock is within the
exit range, `Distance2D` edge to edge, and if it is changes to attack through
the ability, which surfaces it; otherwise it walks on, though its target be
well within its skill's range. Nothing corrects the skill's range. No attack
starts on the tick it sets off surfacing, so a Sandworm that travelled below
with its target already in range strikes as soon as it has surfaced.

**A skill starts whatever the motion does.** `SkillIdleState.TryStartAttack`
starts the attack of a skill whose target is in range from the skill's own
update. A unit coming into range from idle starts its attack as its motion
enters the one-tick transition; one coming into range underground starts it
and has it stopped at once by the surfacing's `SkillManager.Deactive`. Its
blow does not wait for the motion either: `SkillAttackState.TryPerformAttack`
asks the attack angle alone, so a unit whose skills are active fires on the
tick its idle-to-attack transition ends, before the motion has entered the
attack state.

## Units made as it surfaces

A row of `moveAbilitySummonTechDatas` is a `MoveAbilitySummonTech`. Its
`MoveAbilitySummonProvider` registers it with its unit's move ability
(`MotionController.RegisterMoveAbilityChange`), and when the ability reaches
the row's `moveAbilityTimeType`, `OnExitMoveBegin` for every row of this
version, `MoveAbilitySummonSystem` hands the unit's side a
`SupportUnitCreator` of a `SpecialSupportUnitData` of it
(`TeamSupportUnitManager.AddTemporaryCreator`). The creator runs as a
production line does
([equipment_effects.md](equipment_effects.md#production-lines)):

- its offsets turn with the unit's root (`GetSupportUnitPositionSpace`
  answers `Parent`);
- its makes take the unit's level for `DynamicMechLevel.Parent`;
- a make of `appearType` 3 or 4 (`UndergroundStrike`,
  `UndergroundStrikeAnimator`) takes `APPEAR_DURATION`, as only the client
  tells them from a transition.

Its source's `GetLifeTime` answers -1, so `SupportUnitCreator.IsFinished`
ends it after its first update, which makes its first batch. Replicate makes
one Larva at the Sandworm's level, 35 metres ahead of it, each time the
Sandworm begins to surface: `tests/move_ability/fights/replicate.yaml`.

## A hidden unit as a target

A unit that is not visible is no valid target (`FightActor.IsValidTarget`,
recorded as `targetable`), but a lock already held on it stays: the units
locked on a Sandworm as it burrows go on walking towards it.

- **Scoring.** `ScoreRatingTargetSelector` scores a hidden candidate as if it
  stood `invisibleActorDistanceScoreOffset`, 40 metres, further off. Of the
  candidates it keeps the lowest score and the lowest-scored visible one
  besides it (`Selector.CheckResultTarget`), and it answers the visible one
  when the lowest is hidden and the visible one is in the attacker's range
  (`IsActorInAttackRange`). Otherwise the hidden one is taken.
- **Range.** `SkillAttackRangeChecker.IsActorInAttackRange` and
  `IsAttackTargetInAttackRange` answer that a hidden target is out of range,
  except to an attacker whose own `moveType` is `Underground`, which reaches
  one hidden the way it is, though not one in stealth. So no unit attacks a burrowed Sandworm, and two
  Sandworms surface on each other.
- **Shots.** A projectile that reaches a target that is not
  `IsValidTarget(Stealth)` does not strike it, as for a dead target: a Wasp's
  shot that arrives as its Sandworm finishes burrowing is spent.

## The end of the fight

`MotionController.ExitFight` changes the motion to idle directly and clears the
ability (`UndergroundMoveAbility.Clear`). A unit still underground surfaces
there and then: `DoExitMoveBegin(false)` locks its agent and
`DoExitMoveEnd(false)` shows the unit, puts its agent on the ground and takes
the range correction away, and does not let the agent go, which only a normal
exit does. The fight's last tick, the one after its towers are torn down,
leaves every mech before it updates, so a Sandworm that was travelling below
stands where it was on that tick while every other unit moves.

## Evidence

### Recorded

- A Sandworm's attack interval stands still through its transitions: unit 265
  of `tests/corpus/fights/201373545-r4.yaml` begins its next attack fifty
  updates after the last began, the twenty ticks of its burrow and the thirty
  of its surfacing not counted.
- A Sandworm burrows, its lock dropped and its agent still, reads hidden and
  untargetable below with no range correction, walks on below with its Rhino
  inside its attack range, surfaces once the Rhino is within its exit range
  (from tick 108), and strikes it as soon as it is up (tick 160):
  `tests/sandworm/fights/m2-rhino-4242.yaml`.
- A Larva Replicate makes joins on tick 128, changes to attack through its
  transition, fires on tick 129 as the transition ends, and stands through
  the solve of tick 128 with no speed while a Rhino charges through it:
  `tests/move_ability/fights/replicate.yaml`.
- Burrowing and surfacing, a lock kept on a burrowing Sandworm, Sandworms
  surfacing on each other, a Sandworm turning aside from an ally surfacing,
  a shot spent on a burrowed Sandworm, and a Sandworm below as the fight
  ends, each as above: `tests/sandworm/fights/`, both seeds of each layout.

### Read

- A skill counts towards its next attack in its own update, and its count is
  cleared only as an attack starts or the fight begins: `FightSkill.Update`,
  `FightSkill.ResetAttackData`, `SkillManager.Update`.
- The transitions and what each end does: `MoveAbility.Create`,
  `MotionController.ChangeToMoveState`, `MotionController.ChangeToAttackState`,
  `MotionMoveState.ChangeToIdle`, `MotionIdleState.Update`,
  `TransitionState.Enter`, `TransitionState.Update`, `TransitionState.Exit`,
  `MoveAbility.ExitMoveBegin`, `UndergroundMoveAbility.EnterMoveBegin`,
  `UndergroundMoveAbility.EnterMoveEnd`, `UndergroundMoveAbility.DoExitMoveBegin`,
  `UndergroundMoveAbility.DoExitMoveEnd`, `UndergroundMoveAbility.Update`.
- Surfacing at the exit range: `MotionMoveState.Update`,
  `UndergroundMoveAbility.IsLockTargetInRange`, `FightTransform.Distance2D`.
- The skills stopped and held: `SkillManager.Deactive`,
  `SkillManager.Update`, `FightSkill.StopAttack`,
  `SkillIdleState.TryStartAttack`.
- The units made as it surfaces: `MoveAbilitySummonSystem.AddMech`,
  `MoveAbilitySummonSystem.GetPerformAction`, `SupportUnitCreator.IsFinished`,
  `SupportUnitCreator.CreateMech`.
- The agent locked: `RVOControllerFixed.Lock`, `Agent.BufferSwitch`,
  `RVOAgentFixed.CalculateVelocity`.
- The hidden candidate: `ScoreRatingTargetSelector.Select`,
  `ScoreRatingTargetSelector.TrySelect`, `TeamScoreRatingTargetSelectJob.Execute`,
  `DistanceScoreCalculator.Calculate`,
  `SkillAttackRangeChecker.IsActorInAttackRange`, `FightActor.IsValidTarget`,
  `FightProjectile.Update`.
- The end of the fight: `MotionController.ExitFight`,
  `UndergroundMoveAbility.Clear`.

### Not established

- **Splash on a hidden unit.** Whether a splash strikes a burrowed unit
  standing in it is not recorded; the simulator strikes it.
- **A command.** A Sandworm a Mobile Beacon walks has not been recorded.
- **A skill not idle on the lock.** `IsLockTargetInRange` answers false while
  any of the unit's skills locked on the lock is not idle, which the simulator
  does not read: its skill below does not keep the state the build's does.
- **A new lock taken below.** In the corpus round 268447927 round 6 a
  Sandworm moving below takes a new lock on tick 236 and walks on, where the
  simulator leaves it idle without one; why is not read, and the round is not
  pinned.
- **A lock within an energy shield.** `IsLockTargetInRange` then measures to
  the shield's edge; the simulator refuses it by name.
- **Technology and equipment.** A technology or an equipment that changes the
  move ability (`MoveAbilityDynamicTech`, `BurrowTech`, the surfacing time's
  reduce rate) is not read.
- **Cloak.** `CloakMoveAbility`, which no unit's `MechData` makes, is refused
  by the extractor.
