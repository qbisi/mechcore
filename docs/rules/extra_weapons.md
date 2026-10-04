# Extra weapons

What an extra weapon technology adds to a unit: a skill of its own beside the
unit's main one, the rows of `TechnologyGroupData.extraWeaponTechnologies`.
Each row names the skill it adds in `skillID`, which `scripts/extract/extract-units.py`
writes under the unit's `extra_weapons` in `config/units/` where its shape is
one that file can state. The simulator fights Secondary Armament, the
Sabertooth's two guns, and Incendiary Bomb, the Hound's, and refuses every
other member by name: the members'
skills differ in kind, a projectile, an explosion, a laser, a summon, a sweep
around the unit, and many leave a terrain or write a buff, so each joins once a
recording of it agrees.

## A skill beside the main one

`ExtraWeaponTech` is an `IExtraSkill`, and its provider,
`ExtraSkillProvider.AddEffect`, hands it to `ExtraSkillSystem.AddMech`, which
adds the row's skill through `SkillManager.AddSkill`. The first skill a
`SkillManager` is given is its main skill; every later one joins its extra
skills. `AddSkill` adds each `FightSkill` the row's skill makes, so a
standalone row adds one skill for each of its weapons: Secondary Armament adds
two. Any other row adds one skill that fires all its weapons: Incendiary Bomb
adds one, with the Hound's two launchers.

- **Its slots follow the main skill's.** A recording names a skill by its
  index in `FightMech.GetSkills()`: the main skill, or each skill of its group,
  comes first, and the extra skills after it, in their order.
- **Every skill updates in ascending skill ID.** `SkillManager.Update` runs
  `allSkills`, which `SortSkills` keeps in ascending skill ID, and the motion
  updates after all of them. An extra skill whose ID is below the main
  skill's updates before it. Two skills of one row share an ID and keep the
  order the row's weapons come in.
- **Each draws its first attack interval in that order** as the fight starts.
- **A main skill already attacking performs its next blow in its own
  update** (`SkillAttackState.TryPerformAttack`): it draws the blow's
  interval, and releases one due at once, before an extra skill with a higher
  ID updates. A Hound's shot leaves before its bomb on the update both
  release.

## What sets an extra skill apart

`FightSkill.Init` makes an extra skill with `isMainSkill` false, and that is
the whole of the difference:

- **It searches as it updates.** Its search controller is a plain
  `SkillSearchTargetController`, whose `PrepareSearch` does nothing, so its
  search is a `Select` that scores every candidate where it stands as the skill
  updates, never the tick's prepared snapshot. It scores from its own weapon's
  rotation, and a weapon held to an arc passes over what lies outside the arc
  widened by the skill's attack angle either side, the arc about the weapon's
  rest as the unit stands then, whichever way the weapon points.
- **Its attack angle is its row's.** `Init` takes a skill's attack angle from
  its own row where the row sets one, and otherwise gives an extra skill the
  whole circle and a main skill its owner's.
- **It hands its owner no lock.** `FightSkill.ChangeLockTarget` hands the
  owner a lock only from its main searcher (`FightSkillBase.IsMainSearcher`),
  so the unit's lock is the main skill's alone.
- **It takes the motion while the main skill holds no lock.** Each
  `FightSkill.SearchLockTarget` ends by handing the motion its attacker
  (`ISkillOwner.SetAttacker`). An extra skill takes it when the unit's main
  searcher holds no lock, and the main skill takes it back on its next search,
  whatever the extra skill holds, since an extra skill is no main target
  provider. While an extra skill holds it, `AutoMoveBehaviour` asks that skill,
  from the state the motion was in as the unit's skills began to update: the
  motion idles when the skill is idle or its lock is dead, attacks while what
  the skill fires at is in the skill's range, and otherwise moves after that
  lock. Attacking, it turns to the skill's lock (`AttackRotate`): a unit
  without a body its root, one with a body its body. A Sabertooth whose main
  gun has just felled its target and found nothing reads moving on the next
  update when an extra gun took a target beyond its reach on it, and idle
  without the guns; a Hound whose main skill's target burnt to death stays
  attacking and turns its root to the bomb's target.
- **It starts and performs its own attack.** `SkillIdleState.Update` starts an
  attack once `CanStartAttack` lets it, unless the manager holds its fire, and
  nothing waits for the motion: the extra skill fires from where it stands, as
  a construction's skill does.
- **Its projectiles climb first.** `ProjectileSystem.Create` hands every
  projectile of a skill that is not the main one a flight to a height scaled
  as a main skill's pre-flight height is, whatever its row's height, and the
  projectile's first update is that climb (`FightProjectile.IsFlying`): a
  Secondary Armament shot stands where it left on the tick it is released, and
  takes up its target from the next.
- **The fight's end lets its target go**, as it does the main skill's.

## Its weapon

An extra skill's weapon turns on a transform of its own,
`RotationLimitFightTransform`, mounted on what the row's `weaponMountNode`
names: the chassis for Secondary Armament. It enters the fight at its rest,
its mount's rotation plus its default angle, as a child that turned with the
chassis before the fight, and turns towards its skill's lock at the row's
`extraWeaponRotateSpeed` after the skill's state has updated, held within its
arc about its rest as the chassis pointed before the motion turned it.

## Its numbers

- **Range.** `FightSkill.GetAttackRange` of an extra skill whose row sets
  `useMainSkillRange` is the main skill's range with the row's own added: a
  correction of the main skill's range reaches it that way. Any other extra
  skill reaches its row's range.
- **Damage.** A skill whose row's `damageRate` is zero deals its own damage,
  the row's `damage` entry for the unit's level, and none where the row has
  no entries: Incendiary Bomb's shell harms nothing by itself.
- **Splash and interval** are the row's.

## A fire where it lands

A row whose `rangeItemType` is fire leaves one where each of its shots lands.
`ExtraSkillProvider.PerformHitEffect` hands `RangeItemSystem.AddItem` the
unit's fire (`GroundFireController.GetFireMech`) at the point the hit struck,
its height kept, under the unit's side. The fire is the unit's own numbers:
`ExtraSkillProvider.AddEffect` writes the skill's splash and the row's first
`fireLifeTime` (`ExtraWeaponTechnologyData.GetFireLifeTime`) onto the unit's
`DataSet` as the fire's range and life time (`MechDataModifer.AddData`) when
that life time is above zero, and `GetFireMech` reads them back. A recording
keeps them among the unit's modifiers, `gf_range_value` and
`gf_life_time_value`, from the first tick. The fire burns as any fire does
([terrain.md](terrain.md)), at the shared fire's damage and period. Incendiary
Bomb's fire reaches 12 metres from where it lands and burns for 10 seconds.

## What reaches an extra skill

`SkillDataModifier.AvaliableCheck` decides, skill by skill, whether a source
writes onto it. An extra skill takes a source that names its skill
(`GetTargetSkillID`), one its own technology lets (`IExtraSkill.AvaliableCheck`),
one that answers `IsExtraSkillEffect`, and, where its row's `damageRate` is
above zero, one that answers `IsMainSkillEffect`. No officer answers
`IsExtraSkillEffect`, and an Energy Tower skill always does; an equipment and
a technology answer from their rows. How a correction composes on an extra
skill is not measured, so the simulator refuses a unit whose extra weapon an
equipment or an Energy Tower skill writes a skill correction onto.

## Evidence

### Recorded

- Secondary Armament's two guns are skills 1 and 2 beside the main gun, start
  at their rest, turn at their own speed, fire on their own once they face
  their target, deal their row's damage for the unit's level, and stand where
  they left on the tick they are released:
  `tests/extra_weapon/fights/secondary-armament.yaml`, beside its control
  `tests/extra_weapon/fights/secondary-armament-control.yaml`.
- Each gun searches from its own rotation within its arc, the two take
  different targets, they reach the main gun's corrected range plus their
  row's own, and a level 2 unit's deal the row's level 2 damage:
  `tests/extra_weapon/fights/secondary-armament-level-range.yaml`.
- An extra gun takes the motion while the main gun holds no lock, and the
  motion moves after the gun's target out of its reach:
  `tests/extra_weapon/fights/secondary-armament-takes-motion.yaml`, beside its
  control `tests/extra_weapon/fights/secondary-armament-takes-motion-control.yaml`.
- A Hound's main skill releases before its bombs on the update both release:
  `tests/extra_weapon/fights/incendiary-bomb-with-main.yaml`.
- Incendiary Bomb is one skill beside the Hound's main one, its shell deals
  nothing, and each lands a fire of its splash and its row's life time at the
  height it landed at, which burns the Marksman in it; the unit carries the
  fire's range and life time; the bombs' skill takes the motion when the main
  skill's target burns to death, and the Hound turns its root:
  `tests/extra_weapon/fights/incendiary-bomb.yaml`, beside its control
  `tests/extra_weapon/fights/incendiary-bomb-control.yaml`.

### Read

- An extra weapon technology adds its skill through its provider:
  `ExtraWeaponTech`, `ExtraSkillProvider.AddEffect`,
  `ExtraSkillSystem.AddMech`, `SkillManager.AddSkill`, `FightSkill.EnableUseMainSkillRange`.
- The manager's skills update in ascending skill ID: `SkillManager.Update`,
  `SkillManager.SortSkills`.
- An extra skill's differences: `FightSkill.Init`,
  `SkillSearchTargetController.PrepareSearch`,
  `MainSkillSearchTargetController.PrepareSearch`,
  `SkillSearchTargetController.PerformNormalSkillSearch`,
  `FightSkill.ChangeLockTarget`, `FightSkillBase.IsMainSearcher`,
  `SkillIdleState.Update`, `ProjectileSystem.Create`, `FightProjectile.IsFlying`.
- Its range: `FightSkill.GetAttackRange`.
- Its fire: `ExtraSkillProvider.PerformHitEffect`, `ExtraSkillProvider.AddEffect`,
  `ExtraWeaponTechnologyData.GetFireLifeTime`, `MechDataModifer.AddData`,
  `GroundFireController.GetFireMech`, `RangeItemSystem.AddItem`,
  `RangeItemSystem.DoAddItem`, `RangeItemSystem.GetRepeatItem`.
- Its taking the motion: `FightSkill.SearchLockTarget`,
  `FightSkillBase.IsMainTargetProvider`, `FightMech.SetAttacker`,
  `FightMech.SetMotionAttackerAfterSkill`, `AutoMoveBehaviour.IsIdle`,
  `AutoMoveBehaviour.IsActive`, `FightSkillBase.IsLockTargetAvaliable`,
  `MotionAttackState.Update`, `MotionAttackState.AttackRotate`,
  `MotionIdleState.Update`, `MotionMoveState.Update`.
- What reaches it: `SkillDataModifier.AvaliableCheck`,
  `OfficerData.IsExtraSkillEffect`, `TechnologyData.IsExtraSkillEffect`,
  `EquipmentData.IsExtraSkillEffect`, `EnergyTowerSkillData.IsExtraSkillEffect`.

### Not established

- **The motion an extra skill leads beyond one update.** Every recording
  shows the main skill taking the motion back on the update after; how a
  moving or idle unit walks after an extra skill's lock is read, not recorded,
  and so is how a unit with a body turns while an extra skill holds its
  attack. `AttackRotate` also turns the extra skill's own weapons
  (`FightSkill.RotateWeaponTo`) unless they are standalone; no recording reads
  that turn.
- **A correction composing on an extra skill**, an equipment's or an Energy
  Tower skill's. Refused.
- **Every other member of the list.** Refused by name.
