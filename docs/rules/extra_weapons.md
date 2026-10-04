# Extra weapons

What an extra weapon technology adds to a unit: a skill of its own beside the
unit's main one, the rows of `TechnologyGroupData.extraWeaponTechnologies`.
Each row names the skill it adds in `skillID`, which `scripts/extract/extract-units.py`
writes under the unit's `extra_weapons` in `config/units/` where its shape is
one that file can state. The simulator fights Secondary Armament, the
Sabertooth's two guns, and refuses every other member by name: the members'
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
two.

- **Its slots follow the main skill's.** A recording names a skill by its
  index in `FightMech.GetSkills()`: the main skill, or each skill of its group,
  comes first, and the extra skills after it, in their order.
- **Every skill updates in ascending skill ID.** `SkillManager.Update` runs
  `allSkills`, which `SortSkills` keeps in ascending skill ID, and the motion
  updates after all of them. An extra skill whose ID is below the main
  skill's updates before it. Two skills of one row share an ID and keep the
  order the row's weapons come in.
- **Each draws its first attack interval in that order** as the fight starts.

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
  so the unit's body and motion follow the main skill alone.
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
  the row's `damage` entry for the unit's level.
- **Splash and interval** are the row's.

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
- What reaches it: `SkillDataModifier.AvaliableCheck`,
  `OfficerData.IsExtraSkillEffect`, `TechnologyData.IsExtraSkillEffect`,
  `EquipmentData.IsExtraSkillEffect`, `EnergyTowerSkillData.IsExtraSkillEffect`.

### Not established

- **The motion when the main skill loses its target while an extra skill
  attacks.** A Sabertooth whose main gun's target dies, with its extra guns
  attacking other units, reads moving on the next tick in the game where the
  simulator reads idle; without the guns both read idle. The main skill's
  checks and state are the same in both games, and `MotionAttackState.Update`
  as read would leave it idle.
- **A correction composing on an extra skill**, an equipment's or an Energy
  Tower skill's. Refused.
- **Every other member of the list.** Refused by name.
