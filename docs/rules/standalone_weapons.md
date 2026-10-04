# Standalone weapons

How a unit whose weapons are each a skill of their own fights: the four guns
of the Mountain and of the War Factory. Its `config/units/` file marks them
`weapons.mode: standalone`, gives each weapon's arc under `weapons.arcs` and
what it turns about under `weapons.mount`, and sets `default_rotation_search`;
the War Factory's also gives its guns' own `weapons.rotation_speed` and sets
`mech_search`. `scripts/extract/extract-units.py` writes them all from the
unit's `MechData` and its main skill's weapons.

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

Each gun has a resting angle from what it is mounted on,
`WeaponData.defaultAngle`. A weapon whose `rotateAngleLeft` and
`rotateAngleRight` are both set turns within that arc of its rest, which need
not be as wide on one side as on the other; one with either side negative
turns freely (`FightWeapon.rotateType`). The arc's weapon is a
`RotationLimitFightTransform` whose parent the skill's `weaponMountNode`
names: the mech body, the turret (`FightMech.mechBody`), for `MechBody`, which
the Mountain's guns are, and the unit's root, its chassis, for `Default`,
which the War Factory's are.

- **It turns towards its own skill's lock**, in `FightSkill.Update` after the
  skill's state has updated (`RotationLimitFightTransform.RotateTo`), at the
  skill's own weapon rotation speed where it has one
  (`SkillData.extraWeaponRotateSpeed`, the War Factory's) and the unit's
  otherwise. A weapon whose skill holds no lock, cooling or idle, stays where
  it points.
- **It is then held within its arc**, `FightUtility.ClmapRotation` between
  the rest less the left side and the rest plus the right, the rest being
  the parent's rotation plus the default angle
  (`RotationLimitFightTransform.CalculateRotationRange`). The parent turns
  after the skills, so the arc is the one it made before it turned on that
  tick, and a weapon at its arc's edge is carried with it.
- **A check sees the rotation of the tick before**: the state updates first,
  so whether a weapon faces its target is asked of where the weapon pointed
  before it turned on that tick.
- **The guns enter the fight with the unit's facing.** The fight's first
  search (`PresearchTargetController.SearchTarget`) asks the batch to search,
  which its first gun does, and turns the unit to that gun's lock
  (`FightMech.UpdateRotation`); every gun takes that rotation and is held
  within its arc on its first update.

## Searching from the rest

A skill whose data sets `useDefaultRotationSearchTarget` scores its search
candidates from where its weapon rests rather than from where it points
(`FightSkillBase.IsUseDefaultRotationSearchTarget`,
`ScoreRatingTargetSelector.Selector.CalculateRotationData`). A weapon held
to an arc passes over a candidate outside its arc widened by the attack angle
on its side, its left side to the left and its right to the right, as a
candidate out of range is passed over; a weapon that turns freely has no such
window but is still scored from its rest. So the Mountain's first gun,
resting ten degrees off the turret, takes a target to that side of one
straight ahead.

## The turret and the motion

The turret turns only in the motion: `MotionAttackState.AttackRotate` and the
move state's turn call `FightMech.RotateBodyTo` towards
`MotionController.CalculateTargetDirection`. What the motion follows depends
on whether the unit searches for itself.

### A unit that searches for itself

A unit whose `MechData.isEnableMechSearchTarget` is set, the War Factory, runs
a `MechSearchTargetController` of its own, and `FightMech.SetMotionAttackerAfterSkill`
makes the unit itself its motion's attacker:

- **Its own search.** `MechSearchTargetController.Update` runs first in the
  unit's update, before its skills. While its lock lives it counts its timer
  down, and searches once the lock is gone or the timer has run out
  (`SearchTargetController.CanStartSearch`), then waits ten updates: a search
  every eleventh update. It scores from where the unit stands and its root
  points as it updates, over the whole turn (the unit's `GetMainTransform`
  and `IsUseDefaultRotationSearchTarget`), and falls back on any live enemy,
  a tower among them (`MechSearchTargetController.SearchLockTarget`).
- **No skill hands it a lock.** `FightSkill.ChangeLockTarget` hands the owner
  a main skill's lock only when the owner does not search for itself
  (`ISkillOwner.IsMechSearchTargetEnabled`). The unit's lock is its search's
  alone, whatever its guns lock.
- **The motion goes after the unit's lock.** It is idle without one
  (`FightMech`'s `IsLockTargetAvaliable`), walks on it, and turns the turret
  to it. It attacks once the lock is nearer than the main skill's range less
  a metre (`FightMech`'s `IsActorInAttackRange`), edge to edge where the units
  stand as it updates. An attacking motion whose lock is out of range changes
  to its move state, and neither turns nor walks until the update after.
- **Each gun starts and fires in its own update**, the first among them: none
  waits for the motion to attack.
- **A won fight goes on after it.** When the other side has no unit left, the
  skills drop their locks but not the unit's, and its motion walks on to the
  tower its search took until the fight's last tick, which clears it.

### A unit that does not search for itself

What the Mountain's motion follows is, as recorded:

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
- The War Factory's guns are held about its chassis: the chassis turns on
  tick 9 and two guns at the edges of their arcs follow it on tick 10 while
  the turret stays: `tests/war_factory/fights/m1-mirror-4242.yaml`.
- The War Factory enters the fight facing its first gun's lock, scored from
  that gun's rest well off its nose; its guns start at that facing, two of
  them set at once on the edges of their arcs, and one turns four and a half
  degrees a tick: `tests/war_factory/fights/m3-crawler-4242.yaml`, ticks 1
  to 3.
- The War Factory's own lock stays on its first search's while its guns lock
  others, and changes on its next search, eleven updates on:
  `tests/war_factory/fights/m6-formations-4242.yaml`, ticks 1 and 12.
- Its motion keeps moving and turns its turret to a lock that is in its
  guns' range but not nearer than a metre less, and attacks on the next;
  its first gun starts its attack while the motion moves:
  `tests/war_factory/fights/m6-formations-4242.yaml`, ticks 12 and 13.
- Its attacking motion, its new lock out of range, changes to moving and
  turns only from the next update:
  `tests/war_factory/fights/m6-formations-4242.yaml`, ticks 137 and 138.
- With the Rhino dead, its motion walks on to the tower its search took, and
  the fight's last tick clears the lock:
  `tests/war_factory/fights/m2-rhino-4242.yaml`, ticks 104 and 105.
- The War Factory fights each of the six standard layouts on both seeds:
  `tests/war_factory/fights/`.

### Read

- The batch: `FightSkillFactory.Create`, `FightSkillBatch.Init`,
  `FightSkillBatch.GetLockTarget`, `FightSkillBatch.IsAttackTargetInAttackRange`,
  `FightSkillBatch.RotateWeaponTo`, `FightSkill.GetAttackRange`.
- The arcs: `FightWeapon.rotateType`, `SkillData.weaponMountNode`,
  `SkillData.extraWeaponRotateSpeed`, `RotationLimitFightTransform.RotateTo`,
  `RotationLimitFightTransform.CalculateDefaultRotation`,
  `RotationLimitFightTransform.CalculateRotationRange`,
  `FightUtility.ClmapRotation`, `FightSkill.Update`.
- The search: `FightSkillBase.IsUseDefaultRotationSearchTarget`,
  `ScoreRatingTargetSelector.Selector.CalculateRotationData`,
  `FightCoreSystem.PreCalculate`.
- The turret: `MotionAttackState.AttackRotate`, `FightMech.RotateBodyTo`,
  `MotionController.CalculateTargetDirection`.
- The unit's own search: `MechSearchTargetController.Update`,
  `MechSearchTargetController.SearchLockTarget`,
  `SearchTargetController.CanStartSearch`, `MechData.isEnableMechSearchTarget`,
  `PresearchTargetController.SearchTarget`, `FightMech.UpdateRotation`,
  `FightMech.ChangeLockTarget`.
- The motion's attacker: `FightMech.SetMotionAttackerAfterSkill`,
  `MotionMoveState.Update`, `MotionAttackState.Update`,
  `MotionController.ChangeToAutoMove`, `AutoMoveBehaviour.IsActive`,
  `AutoMoveBehaviour.IsIdle`, `FightSkill.ChangeLockTarget`.

### Not established

- **Which attacker the Mountain's motion asks.**
  `FightMech.SetMotionAttackerAfterSkill` hands the motion of a unit that
  does not search for itself the batch's first skill, which alone would leave
  the motion idle whenever the first gun cools; the recordings show it going
  after the mech's lock and the turret after another gun's. The rules under
  "A unit that does not search for itself" are what the recordings show.
  `FightSkill.SearchLockTarget` hands the motion to a skill that is not the
  main searcher while the main searcher holds no lock (`extra_weapons.md`);
  whether that is what makes the Mountain's is not checked.
- **Whether the unit's own search is prepared.** The War Factory's is scored
  where the units stand as it updates; no recording has told that from where
  they stood as the tick opened.
- **Technology and equipment.** No technology of the Mountain's or the War
  Factory's is read, the War Factory's production among them, nor an
  equipment that changes a weapon's arc or the batch.
