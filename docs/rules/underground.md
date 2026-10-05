# Underground movement

How a unit whose `MechData.moveType` is `Underground`, the Sandworm, moves:
it burrows to travel and surfaces to attack, and while it is below it cannot be
attacked.

The four numbers it reads are its `config/units/` file's `underground` block,
which `scripts/extract/extract-units.py` writes from the unit's `MechData`:
how long burrowing takes (`enter`), how long surfacing takes (`exit`), how far
into surfacing it stays hidden (`exit_keep`), and the attack range it is given
below (`attack_range`).

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

**The underground range is written and not read.** `EnterMoveEnd` corrects the
main skill's `AttackRangeValue` by the underground range less the skill's
range, and surfacing takes the correction away. `FightSkill.AddData` marks no
property dirty, so the range every check reads is the one read before: the
unit surfaces when its target comes into its ordinary range. A recording
carries the correction among the unit's modifiers.

**A skill starts whatever the motion does.** `SkillIdleState.TryStartAttack`
starts the attack of a skill whose target is in range from the skill's own
update. A unit coming into range from idle starts its attack as its motion
enters the one-tick transition; one coming into range underground starts it
and has it stopped at once by the surfacing's `SkillManager.Deactive`.

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
  one hidden the way it is. So no unit attacks a burrowed Sandworm, and two
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

- A Sandworm burrows for twenty ticks from its first, its lock dropped and its
  agent still; it reads hidden, untargetable and corrected by the underground
  range from the tick it is below, keeps locking from the next, surfaces for
  thirty ticks once its target is in its ordinary range, shows itself twenty
  ticks in, and locks and strikes once its attack state is entered:
  `tests/sandworm/fights/m2-rhino-4242.yaml`, ticks 1 to 142.
- A Rhino keeps its lock on a Sandworm that burrows and walks on towards it:
  `tests/sandworm/fights/m2-rhino-4242.yaml`, ticks 21 to 156.
- Two Sandworms, both below, surface on each other at their ordinary range:
  `tests/sandworm/fights/m1-mirror-4242.yaml`, tick 101.
- A Sandworm coming into range from idle enters the one-tick transition and
  starts its attack as it does, its attack point counted from that tick:
  `tests/sandworm/fights/m6-formations-4242.yaml`, ticks 145 to 165.
- A Sandworm moving below turns aside from an ally surfacing in front of it,
  whose agent is locked: `tests/sandworm/fights/m6-formations-4242.yaml`,
  ticks 64 to 68.
- A Marksman whose Sandworm burrows locks it again while the other Sandworm is
  out of its range, and takes the other as soon as it is in range:
  `tests/sandworm/fights/m6-formations-4242.yaml`, ticks 228 and 239.
- A Wasp's shot that arrives as its Sandworm finishes burrowing is spent:
  `tests/sandworm/fights/m4-wasp-4242.yaml`, tick 410.
- A Sandworm still below when the fight ends stands still on its last tick and
  reads visible, idle and uncorrected:
  `tests/sandworm/fights/m6-formations-4242.yaml`, tick 335;
  `tests/sandworm/fights/m6-formations-1787720817.yaml`, tick 386.

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
- The skills stopped and held: `SkillManager.Deactive`,
  `SkillManager.Update`, `FightSkill.StopAttack`,
  `SkillIdleState.TryStartAttack`.
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
- **The underground range made live.** Something that marks the main skill's
  range property dirty while the Sandworm is below would make its correction
  read; no recording does.
- **Technology and equipment.** A technology or an equipment that changes the
  move ability (`MoveAbilityDynamicTech`, `BurrowTech`, the surfacing time's
  reduce rate) is not read.
- **Cloak.** `CloakMoveAbility`, which no unit's `MechData` makes, is refused
  by the extractor.
