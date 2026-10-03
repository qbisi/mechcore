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
  that is a construction, and its lock otherwise. Its line is the one from the
  target back to the unit, turned a quarter turn where the skill's
  `isPerpendicular` is set, as the Abyss's is.
- **It runs one way, then the other.** An attack whose
  `SkillAttackController.totalAttackCount` is even runs the opposite way to
  one whose count is odd. The count is every blow the skill has started in
  the fight (`SkillAttackController.PerformAttack` counts it,
  `SkillAttackController.ResetTotalAttackCount` clears it as the fight
  starts); leaving the attack state does not reset it.
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
  than every radius there has none.

## While it lasts

- **Nothing interrupts it.** A target that dies or becomes invalid does not
  stop the sweep (`SweepAttackPerformer.IsInterruptedByInvalidTarget`), and
  the skill's checks do not run until it is over
  (`SweepAttackPerformer.IsEnableCheckTarget`). The skill goes on naming its
  dead target, and its motion, the lock dead, stands idle.
- **Its end is met on the next update.** On the update after the last strike
  the checks run again, and a skill whose target died lets it go.

## The weapon

The Abyss's weapon rests straight ahead with an arc no wider than its rest
(`FightWeapon.rotateType`): it has no transform of its own, and points as the
unit does. Its configuration writes no arc for it.

## Evidence

### Recorded

- The first sweep of a fight lays its strip out on the update its blow starts
  and strikes on every second update after: the Rhino is struck on ticks 77,
  79 and 81 by a sweep that began on tick 62,
  `tests/abyss/fights/m2-rhino-4242.yaml`.
- A unit wider than every radius in the list is struck on every strike that
  reaches it on the stretch then struck: the Rhino three times a sweep,
  `tests/abyss/fights/m2-rhino-4242.yaml`; another Abyss seven to ten times,
  `tests/abyss/fights/m1-mirror-4242.yaml`. A Crawler, narrower than the first
  radius, is struck once a sweep: `tests/abyss/fights/m3-crawler-4242.yaml`.
- A sweep that kills its target runs on, the skill naming the dead Crawler
  and the motion idle, until its last strike; the update after, the skill
  lets it go and locks another on the next:
  `tests/abyss/fights/m3-crawler-4242.yaml`, ticks 168 to 192.
- A sweep's direction follows the count of blows its Abyss has started in
  the fight, not in its attack state: counted the other way, the later
  sweeps of `tests/abyss/fights/m3-crawler-4242.yaml` and
  `tests/abyss/fights/m6-formations-4242.yaml` strike other units.
- The Abyss fights each of the six standard layouts on both seeds:
  `tests/abyss/fights/`.

### Read

- The strip: `FightSweepSkill.CreateAttackArea`, `LineRange.Overlaps`,
  `FightUtility.CalculateIsDistanceFromPointToLineBiggerThanValue2D`.
- The strikes: `SweepAttackPerformer.Start`,
  `SweepAttackPerformer.TryPerformEffect`, `SweepAttackPerformer.FirstPrepare`,
  `SweepAttackPerformer.Perform`, `SweepSkillData.GetMaxDamageTimesByRadius`,
  `DamageEffect.PerformInRange`,
  `RangeTargetCalculator.CalculateRangeActorsInFightRangeInternal`.
- The count and the phases: `SkillAttackController.PerformAttack`,
  `SkillAttackController.ChangeToNextPhase`,
  `SkillAttackController.ResetTotalAttackCount`,
  `SkillAttackController.totalAttackCount`.
- What it ignores: `SweepAttackPerformer.IsInterruptedByInvalidTarget`,
  `SweepAttackPerformer.IsEnableCheckTarget`.
- The weapon: `FightWeapon.rotateType`.

### Not established

- **Shields.** A sweep whose lock stands in a battlefield shield centres on
  the shield where that is nearer, and counts the shields it strikes as it
  counts units (`FightSweepSkill.CheckIsLockTargetInEnergyShield`); no
  recording has a shield, and the simulator does not take them.
- **Buildings.** Whether a strike reaches a tower or a construction standing
  in the strip is not recorded; the simulator strikes it.
- **A delay or a technology.** No sweep with a `damage_delay`, and no
  technology that changes the strip (`SweepSkillIntensifyTech`, which can
  reverse it or keep it one way), is recorded.
- **Order within a strike.** Units a strike reaches are struck in the order
  the target trees hold them, as a splash's are; a strike that kills two
  units is recorded, and their order agrees, but the order is not read.
