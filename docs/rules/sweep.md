# Sweep

How a sweep, the Abyss's main skill, strikes: a strip swept across its
target, one stretch of it at a time, each unit it crosses struck as often as
its radius allows. Its numbers are the `path` block of its `config/units/`
file, `type: sweep`, which `scripts/extract/extract-units.py` writes from the
skill's `sweepSkillDatas` row.

## The strip

A sweep's blow is a `FightSweepSkill` performed by a `SweepAttackPerformer`.
Its first update lays out the strip (`SweepAttackPerformer.FirstPrepare`,
`FightSweepSkill.CreateAttackArea`):

- **It lies across the target.** Its centre is what the skill fires at where
  that is a construction, and its lock otherwise; where the lock stands
  inside a shield of its side, and the sweep does not cross shields, the
  shield's centre if that is nearer the unit
  (`FightSweepSkill.CheckIsLockTargetInEnergyShield`). Its line is the one from the
  original target back to the unit, calculated before a nearer shield moves
  the centre; moving the centre does not turn this line. It is turned a
  quarter turn where the skill's
  `isPerpendicular` is set, as the Abyss's is.
- **Coincident positions use the unit's facing.** When the original target
  and the unit have exactly the same planar position, `CreateAttackArea`
  substitutes the negative of `FightTransform.GetLookDirection` for their
  difference. The perpendicular, reverse and even-attack transformations
  then apply normally; the strip does not collapse to a point.
- **It runs one way, then the other.** An attack whose
  `SkillAttackController.totalAttackCount` is even runs the opposite way to
  one whose count is odd. The count is every blow the skill has started in
  the fight (`SkillAttackController.PerformAttack` counts it).
  `FightSkill.EnterFight` and `FightSkill.ExitFight` call
  `SkillAttackController.ResetInFightAttackData`, which clears the total;
  leaving the attack state alone does not reset it. If the core fight
  temporarily stops while summons are pending, a skill that exits it starts
  its sweep count afresh when combat resumes.
- **It is cut into stretches.** The strip is `length` long and `width` wide,
  centred on the target, and is cut into `damage_times` stretches of equal
  length (`LineRange`), the last one ending where the strip does.

## The strikes

The performer starts as the blow's wait ends
(`SkillAttackController.ChangeToNextPhase`), and its first update runs on that
same update. After `damage_delay`, it strikes the next stretch every
`damage_interval` (`SweepAttackPerformer.TryPerformEffect`): every second
update for the Abyss, twenty stretches over forty updates. A strike
(`SweepAttackPerformer.Perform`):

- **covers the stretch and the one struck before it**, from the start of the
  one before to the end of this one.
- **queries the opponent's target tree with that area first.**
  `RangeTargetCalculator.CalculateRangeActorsInFightRangeInternal` passes the
  `LineRange` to `FightQuadtree.Query`; only the root's objects and the
  objects in nodes whose rectangles overlap the strip become candidates.
  `LineRange.Overlaps(RectRange)` tests contained corners and strip vertices,
  then its two long edges. `FightUtility.IsLinePassThroughRect` converts its
  line slope and intercept to integers before classifying the rectangle's
  corners. The unit-circle test alone does not admit a candidate whose node
  the strip did not visit.
- **strikes every enemy it reaches of the aimed target's domain**: air when
  what the skill aimed at as the sweep began flies, ground otherwise
  (`DamageEffect.PerformInRange`,
  `RangeTargetCalculator.CalculateRangeActorsInFightRangeInternal`). A unit is
  reached when its centre lies nearer the strip than its radius and half the
  width (`LineRange.Overlaps`,
  `FightUtility.CalculateIsDistanceFromPointToLineBiggerThanValue2D`). Each
  strike deals the skill's damage, as a blow does.
- **strikes a unit again only while it may.** A unit already struck is passed
  over unless it has been struck fewer times than its radius allows and it
  touches this stretch alone, not the one before
  (`SweepSkillData.GetMaxDamageTimesByRadius`). The cap is the one paired with
  the first radius in `hit_caps` at least as large as the unit's; a unit wider
  than every radius there has none. A unit passed over is counted with those
  struck (`DamageEffect.PerformInRange` adds what it strikes to the list of
  those it passed over, and `SweepAttackPerformer.Perform` counts the list).

## Shields

A sweep that does not cross shields, as the Abyss's does not:

- **reaches a shielded target where the shield's centre or the lock is in
  range**, whichever is nearer in space, from its edge
  (`SkillAttackRangeChecker.IsAttackTargetInAttackRange`), not where its line
  to the lock meets the shield. Once the shield is gone the lock is measured,
  as the sweep under way still names the shield.
- **strikes a shield in place of every unit** when the end, the middle or the
  start of a strike's area, at the height of the aimed domain, stands inside
  a shield of the other side that does not hold the unit
  (`FightCalculator.IsFightRangeInEnergyShield`,
  `FightCalculator.IsActorInEnergyShield`). The shield takes the skill's
  damage, and nothing else is struck.
- **uses the aerial strike plane for an aerial target**, the height from
  `IFightSetting.GetAirHeight` (`FightCalculator.IsFightRangeInEnergyShield`),
  rather than projecting its sample points onto the ground. A ground shield
  does not intercept points outside its sphere at that height even when their
  horizontal projections are inside it.
- **passes over a unit a shield covers** otherwise
  (`FightSkill.IsActorProtectedByEnergyShield`).

A Barrier is a shield as a battlefield shield is.

## While it lasts

- **Nothing interrupts it.** A target that dies or becomes invalid does not
  stop the sweep (`SweepAttackPerformer.IsInterruptedByInvalidTarget`), and
  the skill's checks do not run until it is over
  (`SweepAttackPerformer.IsEnableCheckTarget`). The skill goes on naming its
  dead target, and its motion, the lock dead, stands idle.
- **Its end is met on the next update.** On the update after the last strike
  the checks run again, and a skill whose target died lets it go.

## Technology

A sweep technology (`SweepSkillIntensifyTech`, the Abyss's Vertical Sweep)
hands the sweep its row's changes through `SweepSkillIntensifyEffectProvider`,
besides its numbers. `FightSweepSkill.ChangeLength` and `ChangeWidth` add
metres to the strip, a total below one making it nothing, and
`ChangePerpendicular`, `ChangeReverse` and `ChangeDisableDirectionChange` set
whether it lies across the line to the target, runs the other way, and keeps
one direction attack after attack. Vertical Sweep lays the strip along the
line, from the unit's side out past the target, the same way every attack,
and raises the damage by half: a strip along the line runs on over what
stands behind the target.

Switched off with its unit's technologies, the provider resets the skill to
its own length, width, perpendicular and reverse and its row's direction
change (`SweepSkillIntensifyEffectProvider.TryApply` with `apply` false,
`FightSweepSkill.ResetLength`, `ResetWidth`, `ResetPerpendicular`,
`ResetReverse`), and switched on applies the changes again. The skill holds
them, so a sweep under way reads them from its next update. Read from the
build; no recording holds it.

## The weapon

The Abyss's weapon rests straight ahead with an arc no wider than its rest
(`FightWeapon.rotateType`): it has no transform of its own, and points as the
unit does. Its configuration writes no arc for it.

## Evidence

### Recorded

- An Abyss's sweep passes over a Hound whose circle already overlaps the
  strike but whose target-tree node has not been reached. The next strike
  reaches its node and hits it: the game's complete fight of the collected
  match projection, `tests/corpus/991-r8.yaml`.
- A temporary exit while Shooting Squad's Fangs are pending resets the
  Abyss's sweep count. Its resumed sweep takes the same direction as its
  first, rather than retaining the parity of the interrupted fight:
  `tests/sweep/resumed-fight-resets-direction.yaml`.
- An Abyss sweeping a Sandworm at the same planar position uses its facing
  for a full strip, delaying the target's hit until the sweep reaches it:
  `tests/sweep/coincident-target-direction.yaml`.
- The first sweep of a fight lays its strip out on the update its blow starts
  and strikes on every second update after: the Rhino is struck on ticks 77,
  79 and 81 by a sweep that began on tick 62,
  `tests/abyss/m2-rhino-4242.yaml`.
- A unit wider than every radius in the list is struck on every strike that
  reaches it on the stretch then struck: the Rhino three times a sweep,
  `tests/abyss/m2-rhino-4242.yaml`; another Abyss seven to ten times,
  `tests/abyss/m1-mirror-4242.yaml`. A Crawler, narrower than the first
  radius, is struck once a sweep: `tests/abyss/m3-crawler-4242.yaml`.
- A sweep that kills its target runs on, the skill naming the dead Crawler
  and the motion idle, until its last strike; the update after, the skill
  lets it go and locks another on the next:
  `tests/abyss/m3-crawler-4242.yaml`, ticks 168 to 192.
- A sweep's direction follows the count of blows its Abyss has started in
  the fight, not in its attack state: counted the other way, the later
  sweeps of `tests/abyss/m3-crawler-4242.yaml` and
  `tests/abyss/m6-formations-4242.yaml` strike other units.
- The Abyss fights each of the six standard layouts on both seeds:
  `tests/abyss/`.
- A sweep at a Marksman inside red's battlefield shield stops when the
  Marksman, nearer than the shield's centre, is in range, strikes the shield
  every second update until it breaks and the Marksman with the next strike:
  `tests/sweep/shield-4242.yaml`, ticks 164 to 190;
  `tests/sweep/shield-1787720817.yaml`.
- A sweep locking a tower behind a nearer shield keeps the original
  tower-to-unit direction while centring its strip on the shield. After
  the shield breaks, it leaves the tower outside the remaining stretches:
  `tests/sweep/covered-tower-direction.yaml`, ticks 638 to 662.
- A sweep at a Fortress carrying a Barrier strikes the Barrier sixteen times,
  breaking it, and the Fortress from the next sweep:
  `tests/sweep/barrier-4242.yaml`, ticks 114 to 211;
  `tests/sweep/barrier-1787720817.yaml`.
- A sweep aimed at a Wasp strikes the Wasps it reaches and none of the
  Crawlers beside them, and one aimed at a Crawler no Wasp:
  `tests/sweep/domain-4242.yaml`, ticks 79 to 89 and 411 to 419;
  `tests/sweep/domain-1787720817.yaml`.
- An aerial Vertical Sweep kills an Overlord above a ground shield without
  damaging that shield: `tests/sweep/aerial-height-over-shield.yaml`.
- A sweep at red's Anti-Armor Turret strikes it, and a sweep at a tower the
  tower: `tests/sweep/turret-4242.yaml`, ticks 153, 339 and 869.
- With Vertical Sweep the strip runs along the line and one way: a Rhino is
  struck three times a sweep, `tests/sweep/vertical-rhino-4242.yaml`,
  `tests/sweep/vertical-rhino-1787720817.yaml`; a Marksman standing
  behind the one aimed at is struck by the same sweep,
  `tests/sweep/vertical-marksmen-4242.yaml`, ticks 102 and 108,
  `tests/sweep/vertical-marksmen-1787720817.yaml`; a Crawler swarm,
  `tests/sweep/vertical-4242.yaml`,
  `tests/sweep/vertical-1787720817.yaml`.
- A strip that runs on past the Marksman it killed strikes the tower and the
  wall blocks it crosses, which it was not aimed at:
  `tests/sweep/tower-vertical-4242.yaml`, ticks 111 and 119;
  `tests/sweep/wall-vertical-4242.yaml`, ticks 44, 48 and 129; and
  their second seeds.

### Read

- The strip: `FightSweepSkill.CreateAttackArea`, `LineRange.Overlaps`,
  `FightUtility.CalculateIsDistanceFromPointToLineBiggerThanValue2D`.
- The facing fallback at coincident positions:
  `FightSweepSkill.CreateAttackArea`, `FightTransform.GetLookDirection`,
  `FQuaternion.AngleAxis`, `FQuaternion.op_Multiply`.
- The strikes: `SweepAttackPerformer.Start`,
  `SweepAttackPerformer.TryPerformEffect`, `SweepAttackPerformer.FirstPrepare`,
  `SweepAttackPerformer.Perform`, `SweepSkillData.GetMaxDamageTimesByRadius`,
  `DamageEffect.PerformInRange`,
  `RangeTargetCalculator.CalculateRangeActorsInFightRangeInternal`.
- Candidate nodes: `LineRange.Overlaps`, `LineRange.Contains`,
  `FightUtility.IsLinePassThroughRect`, `FightUtility.CalculateIntersection`,
  `RectRange.Contains`.
- The count and the phases: `SkillAttackController.PerformAttack`,
  `SkillAttackController.ChangeToNextPhase`,
  `SkillAttackController.ResetTotalAttackCount`,
  `SkillAttackController.totalAttackCount`.
- Shields: `FightSweepSkill.CheckIsLockTargetInEnergyShield`,
  `SkillAttackRangeChecker.IsAttackTargetInAttackRange`,
  `FightCalculator.IsFightRangeInEnergyShield`,
  `IFightSetting.GetAirHeight`,
  `FightCalculator.IsActorInEnergyShield`,
  `FightSkill.IsActorProtectedByEnergyShield`.
- What it ignores: `SweepAttackPerformer.IsInterruptedByInvalidTarget`,
  `SweepAttackPerformer.IsEnableCheckTarget`.
- Technology: `SweepSkillIntensifyEffectProvider`,
  `FightSweepSkill.ChangeLength`, `FightSweepSkill.ChangeWidth`,
  `FightSweepSkill.ChangePerpendicular`, `FightSweepSkill.ChangeReverse`,
  `FightSweepSkill.ChangeDisableDirectionChange`.
- The weapon: `FightWeapon.rotateType`.

### Not established

- **Repeated shield hits capped by radius.** Whether a shield struck twice
  is capped by its radius, as a unit is, is not recorded.
- **A delay, a reversed strip, a changed size.** No sweep with a
  `damage_delay`, and no technology that reverses the strip or changes its
  width or length, is in the build's tables, so none is recorded.
- **Order within a strike.** Units a strike reaches are struck in the order
  the target trees hold them, as a splash's are; a strike that kills two
  units is recorded, and their order agrees, but the order is not read.
