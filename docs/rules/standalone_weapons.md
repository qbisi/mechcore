# Standalone weapons

How a unit whose weapons are each a skill of their own fights: the Mountain's
four guns. Its `config/units/` file marks them `weapons.mode: standalone`,
gives each weapon's arc under `weapons.arcs`, and sets
`default_rotation_search`; `scripts/extract/extract-units.py` writes all three
from the unit's `MechData` and its main skill's weapons.

## A batch of main skills

A unit whose `MechData.weaponMode` is `Standalone` has its main skill made by
`FightSkillFactory.Create` as a `FightSkillBatch` of one `FightSkill` per
weapon, and `FightSkillBatch.Init` makes every one of them a main skill. No
`SkillGroup` holds them together, so none of a group's rules reach them:

- **Each weapon searches, locks and fires on its own.** Two weapons may hold
  two targets, or one; nothing keeps one off what another holds, and none
  waits for another to attack before it starts.
- **Each weapon has its own range.** `FightSkill.GetAttackRange` adds the ten
  metres a grouped skill gets from its parent only to a skill a group
  parents; a standalone weapon's skill reaches as far as the unit's range.
- **Each weapon fires its own shots**, from its own weapon, at a point its own
  target offset draw gives, and measures its attack angle from its own
  weapon's rotation.
- **Each weapon's search is a main skill's**: it is prepared as the tick opens
  (`FightCoreSystem.PreCalculate`), on where every unit stood then.

What the batch answers for the unit as a whole is the first of its weapons
that answers: its lock is the first weapon's that holds one
(`FightSkillBatch.GetLockTarget`), and its target is in range when any
weapon's is (`FightSkillBatch.IsAttackTargetInAttackRange`).
`FightSkillBatch.RotateWeaponTo` turns nothing: the motion turns no weapon of
a batch.

## Weapons that turn within an arc

The Mountain's guns sit on its body, the turret (`FightMech.mechBody`), and
each has a resting angle from it, `WeaponData.defaultAngle`. A weapon whose
`rotateAngleLeft` and `rotateAngleRight` are both set turns within that arc
of its rest; one with either side negative turns freely
(`FightWeapon.rotateType`). The arc's weapon is a
`RotationLimitFightTransform` parented to the turret:

- **It turns towards its own skill's lock**, in `FightSkill.Update` after the
  skill's state has updated, at the unit's rotate speed
  (`RotationLimitFightTransform.RotateTo`). A weapon whose skill holds no
  lock, cooling or idle, stays where it points.
- **It is then held within its arc**, `FightUtility.ClmapRotation` between
  the rest less the left side and the rest plus the right, the rest being
  the turret's rotation plus the default angle
  (`RotationLimitFightTransform.CalculateRotationRange`). The turret turns
  after the skills, so the arc is the one the turret made before it turned
  on that tick, and a weapon at its arc's edge is carried with the turret.
- **A check sees the rotation of the tick before**: the state updates first,
  so whether a weapon faces its target is asked of where the weapon pointed
  before it turned on that tick.

## Searching from the rest

A skill whose data sets `useDefaultRotationSearchTarget` scores its search
candidates from where its weapon rests rather than from where it points
(`FightSkillBase.IsUseDefaultRotationSearchTarget`,
`ScoreRatingTargetSelector.Selector.CalculateRotationData`). A weapon held
to an arc passes over a candidate outside its arc widened by the attack angle
either side, as a candidate out of range is passed over; a weapon that turns
freely has no such window but is still scored from its rest. So the
Mountain's first gun, resting ten degrees off the turret, takes a target to
that side of one straight ahead.

## The turret and the motion

The turret turns only in the motion: `MotionAttackState.AttackRotate` and the
move state's turn call `FightMech.RotateBodyTo` towards
`MotionController.CalculateTargetDirection`. What a batch's motion follows
is, as recorded:

- **The motion goes after the batch's lock**, the attack target of the first
  weapon holding one, but only while the mech holds a lock of its own. The
  mech's lock, `FightMech.lockTarget`, is the latest lock any weapon took or
  dropped (`FightSkill.ChangeLockTarget` hands the owner each). Without one
  the motion goes idle, even with weapons locked; a weapon that drops its
  lock as it cools, updating after the others, leaves the mech none.
- **An attacking motion holds while a weapon attacks**, mech lock or not, and
  stays in its attack while any weapon's attack target is in that weapon's
  range. A weapon going into its cooling does not end it.
- **The first weapon starts from the motion.** As the motion attacks, the
  first weapon's skill starts its attack on its own target once that target
  is in its range and its weapon's angle; the others start in their own
  updates.
- **The turret turns to a weapon's lock.** It turns to the lock of the first
  weapon that holds one; while the motion attacks, a weapon in its attack
  state comes before one that holds a lock idle, so a weapon taking a fresh
  lock does not draw the turret off one firing.

## Evidence

### Recorded

- Four weapons hold three targets at once, each turning to its own:
  `tests/mountain/fights/m3-crawler-1787720817.yaml`, ticks 72 to 75.
- A weapon at the edge of its arc is set on it in one tick, further than it
  turns: the second gun, on tick 122 of
  `tests/mountain/fights/m6-formations-1787720817.yaml`.
- The turret stays on the target two weapons fire at while the two others
  turn idle to a fresh lock: `tests/mountain/fights/m6-formations-1787720817.yaml`,
  ticks 122 to 124.
- The turret turns to the lock of the only weapon holding one while the
  others cool: `tests/mountain/fights/m6-formations-4242.yaml`, ticks 105 to
  110.
- While the unit moves, the turret keeps to the first weapon's lock as the
  others take theirs, and turns when the first takes another:
  `tests/mountain/fights/m6-formations-4242.yaml`, ticks 61 to 63.
- The motion stays in its attack with no mech lock while three weapons
  attack and the fourth cools: `tests/mountain/fights/m6-formations-4242.yaml`,
  ticks 94 to 100.
- The motion goes idle as the last weapon to update drops its lock, though
  three hold a lock out of range, and stays idle:
  `tests/mountain/fights/m6-formations-4242.yaml`, ticks 113 to 123.
- The Mountain fights each of the six standard layouts on both seeds:
  `tests/mountain/fights/`.

### Read

- The batch: `FightSkillFactory.Create`, `FightSkillBatch.Init`,
  `FightSkillBatch.GetLockTarget`, `FightSkillBatch.IsAttackTargetInAttackRange`,
  `FightSkillBatch.RotateWeaponTo`, `FightSkill.GetAttackRange`.
- The arcs: `FightWeapon.rotateType`, `RotationLimitFightTransform.RotateTo`,
  `RotationLimitFightTransform.CalculateDefaultRotation`,
  `RotationLimitFightTransform.CalculateRotationRange`,
  `FightUtility.ClmapRotation`, `FightSkill.Update`.
- The search: `FightSkillBase.IsUseDefaultRotationSearchTarget`,
  `ScoreRatingTargetSelector.Selector.CalculateRotationData`,
  `FightCoreSystem.PreCalculate`.
- The turret: `MotionAttackState.AttackRotate`, `FightMech.RotateBodyTo`,
  `MotionController.CalculateTargetDirection`.
- The motion's attacker: `FightMech.SetMotionAttackerAfterSkill`,
  `MotionController.ChangeToAutoMove`, `AutoMoveBehaviour.IsActive`,
  `AutoMoveBehaviour.IsIdle`, `FightSkill.ChangeLockTarget`.

### Not established

- **Which attacker the motion asks.** `FightMech.SetMotionAttackerAfterSkill`
  hands the motion the batch's first skill, which alone would leave the
  motion idle whenever the first gun cools; the recordings show it going
  after the mech's lock and the turret after another gun's. The rules under
  "The turret and the motion" are what the recordings show; the methods
  that make them are not read.
- **The War Factory's arcs**, which are not symmetric about the rest. The
  extractor writes them; the search window here is drawn about the rest.
- **Technology and equipment.** No technology of the Mountain's is read, nor
  an equipment that changes a weapon's arc or the batch.
