# Extra weapons

What an extra weapon technology adds to a unit: a skill of its own beside the
unit's main one, the rows of `TechnologyGroupData.extraWeaponTechnologies`.
Each row names the skill it adds in `skillID`, which `scripts/extract/extract-units.py`
writes under the unit's `extra_weapons` in `config/units/` where its shape is
one that file can state. The simulator fights Secondary Armament, the
Sabertooth's two guns, Anti-Air Missile, its missile at the air, Incendiary
Bomb, the Hound's and the Vulcan's, Scorching Charge, the Fire Badger's self-destruct, Homing
Missile, the Centurion's, Sticky Oil Bomb, the Phantom Ray's and the
Vulcan's, Whirlwind, the Rhino's, Energy Diffraction, the Melting Point's,
Spider Mine, the Tarantula's, Matrix Bombardment, the Wraith's, and
Anti-Air Barrage, the Fortress's, Air Defense Mark, the Typhoon's,
Disintegration, the Abyss's, Naval Gun, the Overlord's, Gun-launched
Missile, the Mountain's, Electromagnetic Barrage, the Melting Point's,
Dual Wield, the Centurion's, Fork, the Raiden's, Smoke Bomb, the Mountain's,
Swarm Missiles, the Abyss's, and Rocket Punch, the Fortress's, and refuses every other member by name: the members' skills differ
in kind, a projectile, an explosion, a laser, a summon, a sweep around the
unit, and many leave a terrain or write a buff, so each joins once a recording
of it agrees.

## A skill beside the main one

`ExtraWeaponTech` is an `IExtraSkill`, and its provider,
`ExtraSkillProvider.AddEffect`, hands it to `ExtraSkillSystem.AddMech`, which
adds the row's skill through `SkillManager.AddSkill`. The first skill a
`SkillManager` is given is its main skill; every later one joins its extra
skills. `AddSkill` adds each `FightSkill` the row's skill makes, so a
standalone row adds one skill for each `weaponCountPerSkill` of its weapons:
Secondary Armament adds two, of one gun each, and Naval Gun one, of its two
guns. A grouped row adds one for each `weaponCountPerSkill` of its weapons
too, and `FightSkillFactory.PrepareGroupedSkill` puts them in a `SkillGroup`
whose every skill has the main skill for its `ParentSkill`: Energy
Diffraction adds four beams. A grouped row of one skill whose unit's main
skill holds no group makes none, and is a skill like any other, with no
parent: Gun-launched Missile's two launchers are one skill, which draws one
first interval, reaches its own 180 metres and takes the motion as any extra
skill does. Any other row adds one skill that fires all its weapons:
Incendiary Bomb adds one, with the Hound's two launchers.

- **Its slots follow the main skill's.** A recording names a skill by its
  index in `FightMech.GetSkills()`: the main skill, or each skill of its group,
  comes first, and the extra skills after it, in their order, each skill of
  an extra skill's group its own.
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
  updates, never the tick's prepared snapshot; over 51 candidates or more it
  scores on worker threads (`select_job`), the same scores. It scores from its
  own weapon's rotation, and a weapon held to an arc passes over what lies
  outside the arc widened by the skill's attack angle either side, the arc
  about the weapon's rest as the unit stands then, whichever way the weapon
  points.
- **Its attack angle is its row's.** `Init` takes a skill's attack angle from
  its own row where the row sets one, and otherwise gives an extra skill the
  whole circle and a main skill its owner's.
- **It hands its owner no lock.** `FightSkill.ChangeLockTarget` hands the
  owner a lock only from its main searcher (`FightSkillBase.IsMainSearcher`),
  so the unit's lock is the main skill's alone.
- **It takes the motion while the main skill holds no lock.** Each
  `FightSkill.SearchLockTarget` ends by handing the motion its attacker
  (`ISkillOwner.SetAttacker`). An extra skill takes it when the unit's main
  searcher holds no lock, unless it is a skill of a `SkillGroup`, which never
  takes it, and the main skill takes it back on its next search,
  whatever the extra skill holds, since an extra skill is no main target
  provider. While an extra skill holds it, `AutoMoveBehaviour` asks that skill,
  from the state the motion was in as the unit's skills began to update: the
  motion idles when the skill is idle or its lock is dead, attacks while what
  the skill fires at is in the skill's range, and otherwise moves after that
  lock. Entering its attack it stops (`MotionAttackState.Enter` calls
  `RVOControllerFixed.StopMove`), so a unit walking on the lock submits no
  speed on that very update and stands at the next solve's boundary.
  Attacking, it turns to what the skill fires at (`AttackRotate`), a
  block in the way before the lock behind it: a unit without a body its root,
  one with a body its body, whose chassis still turns to where it moves, as
  under the main skill: a Centurion walking on while its missile skill holds
  the motion. A unit whose main skill is a batch of standalone guns turns its
  turret to what the extra skill fires at: the Mountain's turret onto
  Gun-launched Missile's lock while its guns cool without one. Moving, it turns the skill's weapons to what the skill fires
  at, or to where the unit moves when it fires at nothing
  (`MotionMoveState.NormalRotate`, `CalculateTargetDirection`); with a body
  they are the turret every weapon shares, so a Centurion whose main skill
  has just lost its lock turns its turret to its missile skill's lock a tick
  before the main skill locks it. A Sabertooth whose main
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
  takes up its target from the next. A projectile it fires at a building climbs
  as one at a unit does, and is recorded under its own skill.
- **The fight's end lets its target go**, as it does the main skill's.

## Its weapon

An extra skill's weapon turns on a transform of its own,
`RotationLimitFightTransform`, mounted on what the row's `weaponMountNode`
names: the chassis for Secondary Armament. It enters the fight at its rest,
its mount's rotation plus its default angle, as a child that turned with the
chassis before the fight, and turns towards its skill's lock at the row's
`extraWeaponRotateSpeed` after the skill's state has updated, held within its
arc about its rest as the chassis pointed before the motion turned it.

**A weapon held at the edge of its arc searches again.** A target in range
but out of the attack angle fails an attack's check, unless the weapon stands
at an end of its arc (`SkillAttackAngleChecker.IsWeaponInAttackAngle`'s
`isMissing`, `CalculateRotationRange`) and the skill's search timer has run
out (`SearchTargetController.CanStartSearch`): then the check searches again
(`CheckWhenLoseTarget`) and goes on if what it finds is in the area. The
timer counts down in the attack state as in the idle one
(`SkillAttackState.Update`), so a Secondary Armament gun pinned at its arc's
edge by a Crawler walking past it takes the next Crawler within the arc.

A standalone row's weapon has a transform of its own whatever its arc, and
one with no arc turns freely: Air Defense Mark's marker turns onto its lock
at the unit's rotate speed, and stays where it points while it holds none.
One whose arc is no wider than its rest is held there: Disintegration's
emitter points as the Abyss pointed before the motion turned it, on every
update its skill holds a lock. A skill of two such weapons turns both towards
its lock (`FightSkill.RotateWeaponTo`), scores its searches and asks its
attack angle from the first (`FightSkill.GetMainTransform`), and fires a burst
from its own weapons in turn: Naval Gun's one shell a blow leaves its first
gun, and its 20 degrees either side are measured from where that gun points.
A side arm's weapon (`WeaponMode.SideArm`) has a transform of its own as a
standalone row's has, and one whose arc is no wider than its rest turns
freely (`RotateType.Free`, where any other such weapon is `Fixed`): Dual
Wield's gun turns onto its lock at the unit's rotate speed.
Any other weapon whose row gives it no arc has no transform of its own and
points where its mount points: the Hound's bomb launchers point as the unit does, and
the Centurion's missile launcher, mounted by default, as its turret does.
Its skill scores its searches from the unit's rotation as it stands then, the
same rotation the main skill scores from, and asks its attack angle of the
unit; it does not turn towards its lock.

## Its numbers

- **Range.** `FightSkill.GetAttackRange` of an extra skill whose row sets
  `useMainSkillRange` is the main skill's range with the row's own added: a
  correction of the main skill's range reaches it that way. So is that of a
  skill whose `ParentSkill` is the main skill, its own `SkillDataFloat`
  range added: every beam of Energy Diffraction reaches 95 metres, the
  Melting Point's 85 and its row's 10. Any other extra skill reaches its
  row's range.
- **Damage.** A skill whose row's `damageRate` is zero deals its own damage,
  the row's `damage` entry for the unit's level, the last entry for a level
  beyond the list (`SkillData.GetDamage`), and none where the row has no
  entries: Homing Missile's one entry, 2000, is every level's, and Incendiary
  Bomb's shell harms nothing by itself, and is no hit
  at all, so its Hound does not share a kill for having struck the target
  ([unit_experience.md](unit_experience.md#what-a-kill-hands-out)).
- **Splash and interval** are the row's.
- **Its minimum range is its own.** When an attacking check finds its target
  out of its area, `CheckWhenLoseTarget` searches again at once only for a
  target standing nearer than the skill's minimum range (`IsInAttackRange`'s
  `isMissing`), and that range is the extra skill's: an Incendiary Bomb skill
  whose target walks inside its 40 metres takes another on that update.
- **It asks for a block or a shield in the way with its own reach.**
  `FightSkill.SearchAttackTarget` asks whether an enemy construction stands in
  the line of fire, within the skill's reach, and with none whether a shield
  covers the lock, as the main skill does with its own: a Centurion's Homing
  Missile, reaching 160 metres, fires at a block in the way that its 110 metre
  main gun passes by.
- **A skill that deals nothing takes no tower, and no block in the way.**
  `FightSkill.IsTowerAttackable` lets an extra skill take a tower only where
  it deals damage of its own or a share of its unit's: Incendiary Bomb's skill
  searches the units alone, passing over a tower its main skill takes, and
  Scorching Charge's, of damage rate one, charges a tower.
  `WallConstructionTargetChecker.CheckWallConstruction` asks the same of the
  block it finds in the line of fire, so a Sticky Oil Bomb fires past a
  Rapid-Fire Turret at the unit behind it.

## A group of beams

Energy Diffraction's four beams are lasers, a group of the four skills its row
makes, each the main skill's child. The group behaves as a grouped main
skill's does ([combat.md](combat.md#a-grouped-slot-searches-around-its-siblings-locks)),
its first skill in the core's place:

- **The first starts on its own, the others as the group attacks.** The first
  beam starts once its target is in its 95 metres; the other three only while
  a skill of the group attacks (`GroupedSkillAttackBehaviour.CanStartAttackCheck`,
  `SkillGroup.IsAttacking`), each then searching around what the others hold,
  so that they share a formation out.
- **They search from their mount.** A beam has no transform of its own, and
  scores its search from the turret it is mounted on, as the first does.
- **The group's first shares a lock as any other does.** A beam that shares
  its lock gives it up to the first of the others that has struck fewer
  blows (`SkillAttackableChecker.TrySearchGroupSkillLockTarget`); the one it
  passes over is the owner's main skill's first, which the group does not
  hold, so the group's first counts among the others, and gives its own lock
  up as they do, its search timer counting down through its attack
  (`SkillAttackState.Update`) as theirs does.
- **They take no motion.** A beam that searches while the main beam holds
  no lock does not take the motion (`FightSkill.SearchLockTarget`), so the
  Melting Point stays idle while its first beam prepares, until its main
  beam locks again.
- **Each beam ramps on its own blows.** A beam's damage is its row's rate of
  the unit's base damage at its level times its ramp's multiplier for the
  blow, truncated, then corrected as the main skill's is, which a skill with
  a damage rate holds.
- **The technology lowers every skill's damage.** Its
  `allWeaponReduceDamageRate`, −0.83, is the rate `ExtraSkillProvider.EnableEffect`
  writes on the main skill (`IExtraSkill.GetReduceDamageRateBase`) as its
  `DamageReduceRateBase`, which the beams hold too. `DamageProperty.CalculateDamage`
  multiplies the factor the damage rates meet in by it last: every beam, the
  main one's among them, deals 0.17 of its ramp,
  trunc(trunc(168 × m) × 0.17). `DamageCalculator.GetNormalDamage`, what a
  recording reads as the unit's damage, leaves it out.

## A group that joins the main skill's

`FightSkillFactory.PrepareGroupedSkill` gives a grouped row's skills the main
skill's `SkillGroup` where the main skill holds one, and makes a group of
their own only where it does not, as for Energy Diffraction. Matrix
Bombardment's four guns join the Wraith's four: one group of eight, the core
the main skill's first, the row's skills slots 4 to 7 after the main row's.
Fork's two bolts join the Raiden's three, slots 3 and 4, so a blow strikes
five targets.

- **They are the group's slots.** They search around what the others hold
  and start as the group attacks, as the main row's slots do, and they deal
  the unit's base damage at their row's rate of one.
- **Each reaches its own range beyond its parent's.** A slot of the row
  reaches the main skill's 60 metres and the row's own 5 beyond them, 65,
  where a slot of the main row reaches 10 beyond (`FightSkill.GetAttackRange`).
  Fork's row reaches 10 beyond, as the main row's slots do, past the main
  skill's 100 metres, the technology's −10 on the Raiden's 110.
- **Each takes its row's attack angle.** `FightSkill.Init` gives an extra
  skill its row's attack angle, and the whole circle where the row sets none:
  Fork's bolts fire at what the Raiden's 60 degrees either side leave out.
- **Each is its row's skill, not the main skill's.** It draws its own first
  interval, as each skill of a grouped row does. It is no main searcher
  (`FightSkillBase.IsMainSearcher` asks `isMainSkill`): a lock it takes or
  drops does not reach the unit, whose lock is the last the main row's slots
  took. Its projectile climbs first, as every extra skill's does, and leaves
  on the tick after its release.

The simulator runs the row's slots on the main skill's numbers, so a row
joins only where its own are the same but for its range, its attack angle
and its weapons, and it refuses any other by name.

## A fire where it lands

A row whose `rangeItemType` is fire leaves one where each of its shots lands.
`ExtraSkillProvider.PerformHitEffect` hands `RangeItemSystem.AddItem` the
unit's fire (`GroundFireController.GetFireMech`) at the point the hit struck,
its height kept, under the unit's side. The fire is the unit's own numbers:
`ExtraSkillProvider.AddEffect` writes the skill's splash and the row's first
`fireLifeTime` (`ExtraWeaponTechnologyData.GetFireLifeTime`) onto the unit's
`DataSet` as the fire's range and life time (`MechDataModifer.AddData`) when
that life time is above zero, an oil's row's too, and `GetFireMech` reads them
back. A recording
keeps them among the unit's modifiers, `gf_range_value` and
`gf_life_time_value`, from the first tick. The fire burns as any fire does
([terrain.md](terrain.md)), at the shared fire's damage and period. Incendiary
Bomb's fire reaches 12 metres from where it lands and burns for 10 seconds;
the Vulcan's, of its own row, 18 metres for 12 seconds, ten shells a volley
scattered within 70 metres of the target.

## An oil where it lands, and a buff on what it strikes

A row with a `buffID` writes that buff on every unit its hit struck, from the
unit that fired, before anything else its hit does. A row whose
`rangeItemType` is oil then leaves an oil where the hit lands, at the height
it landed at, under the unit's side. The oil is the technology's own: it
reaches as far as the skill splashes, stands for no set time and until the
fight ends, and writes the row's buff on every unit that stands in it, as a
battle skill's oil does ([terrain.md](terrain.md)). A fire reaching it turns
it to a fire as wide that burns the row's first `fireLifeTime`, and a bomb
landing on a fire leaves that fire at once.

Sticky Oil Bomb's buff is the battle skill's oil's: a second of -0.55 of move
speed. Its bomb deals nothing, so a Rhino it lands beside is struck, slowed,
and slowed again by the oil every 19 ticks while it stands in it. Each time
the buff is written again it keeps the unit that first wrote it as its
source, so the oil's renewals name the Phantom Ray whose bomb struck first.

## A fog where it lands

A row whose `rangeItemType` is a fog leaves one where each of its shots
lands, as an oil's row leaves an oil: `ExtraSkillProvider.PerformHitEffect`
adds the range item with the technology as its provider, at the point the
hit struck, its height kept, under the unit's side. `ExtraWeaponTech` answers
`IRangeItemProvider` for it as for an oil, as wide as its skill splashes, for
no set time, and one round, and `IFogProvider.GetAttackRangeChangeRate` with
its row's `fogAttackRangeChangeRate`. The fog then does what a battle skill's
does ([terrain.md](terrain.md)): it rates the range of every ranged skill of a
ground unit standing in it, of either side, and goes as the fight ends.

Smoke Bomb's eight shells leave eight fogs of 18 metres, each taking 35% off
the range of what stands in it: a Marksman's 140 metres become 91.

## A wave from its unit

Disintegration's skill strikes (`skillDatas`), splashes about its own unit
(`useSelfSplash`) and diffuses (`isDiffusion`). Its blow, three seconds after
it starts, deals nothing where it lands; `DamagePerformer.Perform` hands a
diffusing splash to `PerformDiffusionRangeEffect`, which measures it from
where the Abyss stands as the blow lands and starts a `GRTimer` of the
performer's own.

- **The wave grows a step every half second.** The timer fires every
  `diffusionInteval`, 10 ticks, from the update after the blow, ahead of the
  tick's modules. Its `n`th firing reaches `n` times `diffusionSpeed`, 30
  metres, and never past the skill's 280 metres of splash. A Rhino whose edge
  stands 85 metres off is reached at the third firing, a second and a half
  after the blow.
- **Each firing strikes what it reaches and no firing struck before**, of
  the other side and the domains the skill attacks, the ground alone
  (`PrepareRangeTargetsInDiffusion`, `SkillDamageProvider.GetTargetType` of a
  skill that diffuses). What it was aimed at is not struck unless the wave
  reaches it. Its splash takes 10 steps, 9 of 30 metres and the 10 left; the
  timer fires once more, striking nothing (`DiffusionCompleteCallBack`).
- **A firing while the Abyss is dead strikes nothing**, and the wave goes on
  growing.
- **A battlefield shield stands in each firing's way** as in any splash's
  (`ProcessAdvancedEnergyShieldEffect`): what an enemy shield covers that
  does not hold where the wave started is taken out of the firing, and is not
  counted struck, so every later firing reaches it again. Each firing then
  strikes the shield covering what the blow was aimed at, and every other the
  whole 280 metres reach, not only that firing's; Disintegration's strikes
  it for nothing, which no recording reads.
- **Every unit struck takes the row's buff**, a slow of -0.4 of move speed
  for 5 seconds, which first takes 0.2 of its life now:
  `IBEC_ChangeCurrentLife.Perform` takes the whole part of the unit's life
  times `currentLifeDisposableChangeRate`, and deals it as a hit of no unit
  under the buff's side that the unit's damage taken raises. A Rhino of 19297
  loses 3860, the whole part of −3859.4. The hit is recorded before the
  buff applied. A buff the unit already runs is renewed, and takes its share
  again (`Buff.ReEnableDisposableEffect` before `Buff.Reset`): two Abysses'
  waves reaching a Crawler on one tick take 50 of its 250 and then 40.
- **The skill names its target through its wind-up.** A skill that splashes
  about itself checks no target as it winds up, so it neither finishes nor
  searches anew when its target dies: Disintegration goes on naming a Fang
  that died a second into its wind-up until its blow.
- **No equipment writes onto it** (`ignoreEquipmentEffect`,
  `SkillDataModifier.AvaliableCheck`).

## Shells that strike shields

Electromagnetic Barrage's skill fires sixteen shells a blow from the Melting
Point's two launchers in turn, one every two ticks, scattered 55 metres about
its target as any burst of two weapons is: the skill's own two weapons, not
the main beam's one. They deal nothing, and each writes the row's buff on
what its 7.5 metres of splash reach: -0.4 of move speed for 8 seconds, which
turns the unit's technologies off while it runs (`disableTechnology`), so
`status_mask` reads `technology_disabled` though the unit carries none.

- **A shield takes the row's `energyShieldDamage`.** The row is the skill's
  damage modifier, and where its `energyShieldDamage` is not negative
  (`IsChangeHitEnergyShieldDamage`) a hit deals that to a shield in place of
  its damage (`DamagePerformer.CalculateHitEnergyShieldDamage`): a
  battlefield shield it strikes, and a unit whose own shield has energy left,
  before what the hit deals is asked. Each shell red's shield absorbs takes
  6000 off it, the last as much as it has left.
- **A burst names its target until its last shell is out.** The rest of a
  burst whose target died is fired where it was aimed, and the skill names
  the dead target until then rather than finishing its attack: the Melting
  Point's beam kills the Crawler the barrage aims at, and the barrage fires
  its last two shells at it and names it until they are out.

## A preemptive strike about its unit

An around skill (`FightAroundSkill`, an `aroundSkillData` row) is a
preemptive one that is not permanent. It starts only where three things
hold, checked by `AroundSkillStartAttackChecker` as the skill's idle state
would start its attack:

- **No preemptive skill runs, and the main skill rests.** The main skill is
  idle with its target not yet in its attack area, or between two blows of
  an attack that has struck: its backswing has run out and its next blow has
  not begun.
- **The skill's own target is in its attack area.**
- **Enough enemies stand near.** Of the enemy units the side's unit quadtree
  answers for a square of the select radius about the unit, a node at a
  time, as many as `targetNumCondition` are alive, of a domain the skill
  takes, and have their edge within the radius, the three-dimensional
  distance less their radius.

Leaving its idle state the skill takes the main skill's place: the main skill
locks, letting its lock go, and the skill searches, taking the motion. It
strikes once, about its own unit rather than its target, out to its splash,
for its rate of the unit's base damage at its level, truncated. Nothing
checks its target until it has struck: a skill that splashes about itself
waits for its attack point, and strikes, with its checks off
(`SkillAttackController`'s waits and `NormalAttackPerformer.IsEnableCheckTarget`
answer `!IsSelfSplash`), so a lock another unit kills while it winds up is
not let go, its motion idles on the dead lock, and it strikes at its attack
point all the same. Its attack check fails once it has struck, so it returns
to its idle state with its targets cleared and hands the main skill back, idle
with its search due at once.

Whirlwind starts with two enemies within 25 metres and strikes out to 35 for
1.4 times its Rhino's damage, a 3560 blow becoming 4983. A Rhino against one
enemy never starts it.

## A punch when its unit is hurt

Rocket Punch adds a rocket punch skill (`FightRocketPunchSkill`), a
projectile skill that is preemptive and not permanent: leaving its idle state
it takes the main skill's place, and returning hands it back, as an around
skill does. It starts only where `RocketPunchAttackChecker.Check` lets it:

- **The preemptive checker's.** No preemptive skill runs, the main skill
  rests, and the skill's own target is in its attack area.
- **A punch is left.** It has thrown fewer than `triggerCount`, two
  (`curAttackCount`, which `OnAttack` counts as the blow starts and
  `ExitFight` sets back to none).
- **Its unit is hurt enough for the punch it is at.** The unit's life over its
  maximum, as `FPoint`s, is at or below `lifePercentCondition0`, 0.85, for the
  first punch, and `lifePercentCondition1`, 0.55, for the second.

The fist is one projectile from the first of its two launchers, dealing the
row's damage for the unit's level, 12000 at level one, out to its 25 metre
splash.

## A permanent preemptive explosion

Scorching Charge adds an explosion skill (`FightExplosionSkill`) that is
permanent and preemptive (`isPreemptivePermanent`): it waits locked
(`SkillLockState`) until its condition holds and then takes the main skill's
place for the rest of the fight. The row also raises the unit's life by 80%.

- **It activates once the unit's life is half its maximum or less.**
  `SkillManager.Update` ends with `PreemptiveSkillController.Update`, after
  every skill of the unit and before its motion. Its condition,
  `PermanentPreemptiveActiveConditionLifeController.CheckCanActive`, divides
  the life by the maximum as `FPoint`s and holds at the skill's share (0.5) or
  below. A Fire Badger hit below half on one update activates on the next.
- **Activating, the main skill locks and the skill takes over.**
  `SkillManager.ActivePermanentPreemptiveSkill` locks the main skill, which
  lets its lock and target go and updates no more. The skill writes its buff
  on the unit, from the unit (`FightSkill.OnPermanentPreemptiveSkillActive`),
  takes the motion (`FightExplosionSkill.OnPermanentPreemptiveSkillActive`
  gives the unit an `AutoMoveBehaviour` that asks it), and, a life condition
  having no transition to wait out, leaves its lock for its idle state with
  its search timer restarted while the motion stops idle. It searches and
  locks on the next update, and from then on it is the unit's main searcher
  (`FightSkillBase.IsMainSearcher`): its lock is the unit's.
- **Its buff makes the unit fast and invincible.** Buff 6020 adds 60 metres a
  second to the unit's speed, which a recording keeps as the buff's
  `move_speed_value`, and makes it invincible for as long as the fight lasts.
  Invincibility does not keep a fire from burning it.
- **Its blow is the unit's own death.** The skill is a melee one of range 1:
  the unit charges what it locks, unit or building, and its blow
  (`SuicideEffect`) takes the whole of its life from itself. Who last hurt the
  unit keeps the credit for its death. Its agent still moves on that update,
  as it was moving, and the unit dies where that leaves it.
- **Its death explodes, however it dies.** `FightExplosionSkill.EnterFight`
  hands the unit to `DeadEffectSystem` with the skill as its dead effect, or,
  for a unit still travelling in from a rear deployment, once it arrives
  (`OnTravelFinished`): one killed on its way neither explodes nor burns.
  When that module updates, before it calls the dead units' `OnDead`,
  `DeadExplosiveController.PerformDeadEffect` strikes everything within the
  unit's radius and the skill's splash of where it fell, its own side too
  (`enableFriendlyFire`), buildings included, for the side the unit was
  deployed on whether a beam has turned it or not
  (`DeadExplosiveDamageProvider.GetTeamController` answers the unit's
  `originTeamController`), with the life the unit had
  before it took its own (`explosiveDamageCondition` 2) times the skill's
  multiplier. A unit some other hit killed had no such life and deals
  nothing. A unit the explosion kills explodes in turn on the same update, and
  what it kills reads after what the tick's shots killed.
- **Every such death leaves a fire.** The explosion leaves a fire of the
  skill's own, reaching 40 metres for 7 seconds, where the unit fell, under
  the side it stands on as it dies, a beam's if one turned it
  ([control.md](control.md)), the death of a side's last unit too. A fight decided on the tick
  the fire is left ends on that tick, and the fire goes with it before any
  snapshot holds it.
- **Switched off, it waits.** While a disabling buff holds the unit's
  technologies off, the condition holds nothing and a death sets off neither
  explosion nor fire: [technology_effects.md](technology_effects.md#switched-off).

## A support skill and the units it makes

Spider Mine adds a support skill (`FightSupportSkill`, a `supportSkillDatas`
row): a preemptive skill that is not permanent, whose blow does nothing, and
which owns a production line (`SupportUnitCreator`) that makes the row's unit.
The skill is the line's gate.

- **It starts when a batch is due and the main skill rests.**
  `SupportSkillStartAttackChecker` replaces the default start check, which
  asks for the target in the attack area, with its own. No preemptive skill
  runs, the line's next update makes a batch
  (`SupportUnitCreator.PreCalculate`), and the main skill rests, as an
  around skill asks it to. A batch due while the main skill is busy locks the
  line instead (`SupportUnitCreator.Lock`), which then counts nothing until
  the skill starts and unlocks it. The skill needs a lock to start but not
  its range: a Tarantula starts it at t1 at a Rhino 201 metres off, its range
  80.
- **It holds the main skill while it winds up.** Leaving its idle state, it
  takes the main skill's place, as an around skill does: the main skill locks
  and lets its lock go. Its search there is `FightSkill.SearchLockTarget`
  alone, which changes the lock and leaves the attack target its idle state
  took: a skill that locks a new enemy fires at, and turns the turret to,
  the one it held. The skill prepares its 1.5 seconds and attacks. Its
  attack check is only that its lock lives
  (`SkillAttackableChecker.Check` tests for a `FightSupportSkill`). It
  performs once, does nothing, and returns to idle, handing the main skill
  back: 32 ticks of the Tarantula's main skill locked for each batch.
- **The line makes its batch on the update it is due.** `SupportUnitSystem`
  updates after every unit. Spider Mine's line makes two mines every 15
  seconds, the first on the first tick, with no bound on batches and 99 alive
  at most. Each stands at its offset, 20 metres right or left and 40
  forward, turned by the turret (`SupportUnitPositionSpace.ParentBody`). Each
  is at its owner's level (`DynamicMechLevel.Parent`). Each takes the row's
  `productTime`, 2 seconds, to appear (`appearType` 8), where an item's line
  takes `APPEAR_DURATION`'s second.
- **The line dies with its unit.** As `DeadEffectSystem` calls the unit's
  `OnDead`, after `SupportUnitSystem` has updated on that tick,
  `FightEffectSystem.DeactiveEffect` takes its effects off and its support
  skill's line leaves its side's creators
  (`SupportUnitSystem.RemoveSkillOwner`): a dead Tarantula makes no more
  mines, and the ones it made stay. A batch due on the tick the unit dies is
  not measured.
- **A Spider Mine is an explosion.** Its main skill is a suicide that explodes
  as it dies (`DeadExplosiveController`). It deals the skill's attack damage
  (`explosiveDamageCondition` 0), 2500 at level one, to everything within its
  radius and its 12 metres of splash, its own side too: a mine's blast
  reaches the mine beside it.

## A side arm

Dual Wield adds a side arm: a skill whose weapons are `WeaponMode.SideArm`
(`FightSkill.IsSideArmSkill`), here the main gun's own projectile from a
second gun, dealing the unit's damage and reaching as far as the main skill
(`useMainSkillRange`). `FightSkill.EnterFight` hands it to the main skill
(`SetSideArmSkill`). Its row is a magazine whose reload takes no time
(`isLoadingType`, `reloadingTime` 0), which `FightSkill.CanAutoReload` never
reloads and nothing else reads.

- **The two fire in turn.** The main skill beginning a blow
  (`SkillAttackController.PerformAttack`, `SetFireTurnsMark`) makes the turn
  the side arm's, to wait `sideArmFireDelay` over the tick, the fraction
  dropped: 0.2 seconds, four ticks. The main skill's `FightSkill.Update`
  counts the wait down before its state updates. The side arm beginning a
  blow gives the turn back. Neither begins one out of turn
  (`CanFireByTakeTurns`, which `SkillIdleState.Update` asks before it starts
  an attack and `SkillAttackState.TryPerformAttack` before each blow): the
  main skill waits for the side arm, and the side arm for its turn and its
  wait.
- **A side arm that cannot take its turn gives it back.** Its idle and its
  cooling states ask as they update (`TrySideArmResetFireMark`): it keeps the
  turn while it is enabled, not cooling, reloading or locked, its lock lives,
  and it still waits or what it fires at is in its attack area, or it fires
  at nothing yet while its lock is. A failed check in its prepare state
  gives the turn back (`ForceSideArmEndFireTurn`), and so does its attack
  ending, or its blow coming due out of its angle, in its turn with no blow
  under way.
- **It searches about the main skill's lock**
  (`SideArmSearchTargetController.PerformNormalSkillSearch`), or about the
  lock the main skill last changed from while it holds none
  (`prevLockTargetForSideArm`). It keeps a live lock of its own within its
  `sideArmSearchRange`, 40 metres, of that anchor and in its attack area;
  otherwise it takes, of the other sides' units of the anchor's domain within
  that range of it less their radius (`RangeTargetCalculator.CalculateRangeTargets`,
  buildings left out), the anchor aside, those in its attack area as its
  selector scores them; with none, the anchor while it lives.
- **It searches only when its lock no longer suits**
  (`NeedRefreshSideArmTarget`), in its idle state in place of the search
  timer and in its attack state on every update before its check: when the
  main skill holds no live lock and it holds one, when its lock is dead or
  out of its attack area, and when its lock is not the main skill's and
  stands further from it than its search range. A side arm on the main
  skill's lock keeps it while it is in its area.
- **The main skill takes the side arm's lock.** A main skill with a side arm
  whose own lock is gone takes the side arm's live lock in place of a search
  (`FightSkill.SearchLockTarget`, `TrySetSideArmTargetAsMainTarget` inlined
  there).

## What reaches an extra skill

`SkillDataModifier.AvaliableCheck` decides, skill by skill, whether a source
writes onto it. An extra skill takes a source that names its skill
(`GetTargetSkillID`), one its own technology lets (`IExtraSkill.AvaliableCheck`),
one that answers `IsExtraSkillEffect`, and, where its row's `damageRate` is
above zero, one that answers `IsMainSkillEffect`. No officer answers
`IsExtraSkillEffect`, and an Energy Tower skill always does; an equipment and
a technology answer from their rows. Every officer answers
`IsMainSkillEffect`, and so does every technology of a unit.

So a skill with a damage rate holds the main skill's damage corrections, an
officer's, a blueprint's and a technology's, and its damage composes them as
the main skill's does: its rate of the unit's base damage, truncated, then
raised and impaired by them and the buffs'. Whirlwind's 4983 becomes 5580
under Attack Enhancement's +0.12. Scorching Charge's explosion skill has a
damage rate of 1 too, so it holds an officer's damage rate on its slot,
though what its explosion deals is its unit's life.

A skill whose range is the main skill's with its own added, of a row that
uses the main skill's range or a grouped row's, never reads its own range, so
a range that reaches it changes nothing: Energy Diffraction's own −30 metres
lands on its beams as on the main beam, and leaves them reaching 95.

A skill without a damage rate holds what reaches it alone, an equipment's
through its `extraSkillEffect` and an Energy Tower skill's, and its damage,
its row's entry for the unit's level, composes them and the buffs' the same
way: Secondary Fire Control System's +0.25 makes a Homing Missile's 2000 a
2500. A recording holds each skill's corrections on its own slot.

Its range of its own composes what it holds the same way, as the main skill's
does (`AttackRangeProperty.GetAttackRange`), and no buff's: a buff's
`attackRangeChangeValue` is the main skill's, and an extra skill would read
`extraAttackRangeChangeValue`, which no buff here writes. Enhanced Range, the
Energy Tower skill that adds 15 metres to every ranged unit, takes a Homing
Missile's 160 metres to 175: a Centurion's first missile leaves 172 metres
from its Rhino's edge. A melee skill reads no correction of its range, so
Scorching Charge's charge still strikes at its 1 metre, though the skill's
slot records the main skill's 15 metres with its other numbers. Any other
number than damage and range a source corrects on an extra skill is not read,
nor a range on a skill with a damage rate whose range is its own and that is
not melee, so the simulator refuses it.

## Evidence

### Recorded

- A dead Tarantula's line makes no more mines, its living partner's makes
  on: `tests/corpus/fights/67160345-r2.yaml`, ticks 191 and 301.
- An Energy Tower skill's range reaches an extra skill of its own range
  without a damage rate, and a melee one's slot records it without its range
  moving: `tests/extra_weapon/fights/enhanced-range-homing-missile.yaml`,
  `tests/extra_weapon/fights/enhanced-range-scorching-charge.yaml`.
- A Secondary Armament gun at the edge of its arc whose Crawler walks out of
  it takes the next Crawler within the arc on its check:
  `tests/corpus/fights/201340110-r5.yaml`, blue's Sabertooth 179, tick 143.
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
- A technology's and an officer's range, written on the main skill alone,
  reach both of a Sabertooth's extra skills through the main skill's range:
  Range Enhancement's +40 metres lets the missile and the extra guns fire
  from 145 and 152 metres, centre to centre, at once,
  `tests/extra_weapon/fights/sabertooth-range-enhancement.yaml`, beside its
  control `tests/extra_weapon/fights/sabertooth-range-enhancement-control.yaml`;
  Advanced Targeting System's +10 metres lets the missile fire from 122
  metres nine ticks earlier,
  `tests/extra_weapon/fights/anti-air-missile-advanced-targeting.yaml`, beside
  its control `tests/extra_weapon/fights/anti-air-missile.yaml`.
- An extra gun takes the motion while the main gun holds no lock, and the
  motion moves after the gun's target out of its reach:
  `tests/extra_weapon/fights/secondary-armament-takes-motion.yaml`, beside its
  control `tests/extra_weapon/fights/secondary-armament-takes-motion-control.yaml`.
- A Fire Badger brought to half its life locks its main skill, writes its
  buff, charges, and takes its own life against what it reached; its death
  explodes within its radius and the skill's splash with the life it had,
  felling its own side's Badgers and the enemies about it, and the fire it
  leaves goes with the fight it ends on that tick:
  `tests/extra_weapon/fights/scorching-charge.yaml`,
  beside its control `tests/extra_weapon/fights/scorching-charge-control.yaml`.
- Each Fire Badger's death leaves a fire where it fell, one an ally's explosion
  felled as well as the one that exploded, and the fire burns an invincible
  Badger; a Badger charges a tower when no unit is left to it, and its
  explosion strikes the tower: `tests/extra_weapon/fights/scorching-charge-survivor.yaml`.
- A side's last Fire Badger, killed by a blow, leaves its fire, and the fight
  ends on the next tick with it standing (t1678):
  `tests/corpus/fights/67160729-r4.yaml`.
- A Fire Badger killed while travelling in leaves no fire, and what an
  explosion kills reads after the units a turret's shots killed on that tick:
  `tests/corpus/fights/134259672-r3.yaml`.
- A Centurion's Homing Missile fires its two missiles at its own interval,
  with no offset, the second leaving for where its target stood the tick
  before: `tests/extra_weapon/fights/homing-missile.yaml`,
  `tests/extra_weapon/fights/enhanced-range-homing-missile.yaml`, beside its
  control
  `tests/extra_weapon/fights/homing-missile-control.yaml`.
- A Centurion whose missile skill holds its motion turns its turret to the
  missile skill's lock as it moves, a tick before its main skill locks it:
  `tests/corpus/fights/134265566-r4.yaml`.
- A Hound's bomb skill whose target walks inside its 40 metre minimum range
  takes another on that update, and its search passes towers over:
  `tests/corpus/fights/201371791-r4.yaml`, `tests/corpus/fights/201371791-r5.yaml`.
- A Hound's main skill releases before its bombs on the update both release:
  `tests/extra_weapon/fights/incendiary-bomb-with-main.yaml`.
- A Hound's bomb skill scores every search from the unit's rotation as it
  stands then: recorded with `target_search,target_candidate`,
  `tests/extra_weapon/fights/incendiary-bomb-with-main.yaml` reads each bomb
  search's source rotation equal to the body rotation the update began with.
  That fight's result does not turn on it; a corpus round does, where the
  bomb's search over 125 candidates scored each exactly as the main skill's.
- Incendiary Bomb is one skill beside the Hound's main one, its shell deals
  nothing, and each lands a fire of its splash and its row's life time at the
  height it landed at, which burns the Marksman in it; the unit carries the
  fire's range and life time; the bombs' skill takes the motion when the main
  skill's target burns to death, and the Hound turns its root:
  `tests/extra_weapon/fights/incendiary-bomb.yaml`, beside its control
  `tests/extra_weapon/fights/incendiary-bomb-control.yaml`.

- Anti-Air Missile is one skill beside the Sabertooth's main gun, reaching
  as far as it and taking the air alone, a missile every 3 seconds; with no
  ground target the missile skill takes the motion:
  `tests/extra_weapon/fights/anti-air-missile.yaml`, beside its control
  `tests/extra_weapon/fights/anti-air-missile-control.yaml`; with one, the two
  skills fire side by side:
  `tests/extra_weapon/fights/anti-air-missile-mixed.yaml` and
  `tests/corpus/fights/201340110-r3.yaml`.
- Whirlwind starts once two Crawlers' edges are within 25 metres as the
  Rhino's unit quadtree answers them, locks the main skill and strikes about
  the Rhino out to 35 metres, felling all 24 Crawlers:
  `tests/extra_weapon/fights/whirlwind.yaml`, beside its control
  `tests/rhino/fights/m3-crawler-4242.yaml`. It starts as the main skill's
  first backswing runs out, strikes two Rhinos for 1.4 times the Rhino's
  damage truncated, returns to idle once it has struck and hands the main
  skill back, which attacks on the next tick:
  `tests/extra_weapon/fights/whirlwind-rhinos.yaml`. Against one enemy it
  never starts: `tests/extra_weapon/fights/whirlwind-one-enemy.yaml`.
- Energy Diffraction's beams reach 95 metres, the main beam's 85 and their
  own 10: the first prepares a tick after the Rhino comes within 95 and the
  main beam a tick after it comes within 85; the other three start as the
  first attacks, and every beam, the main one's among them, deals 0.17 of its
  ramp, the recorded damage leaving the rate out:
  `tests/extra_weapon/fights/energy-diffraction-rhino.yaml`, beside its
  control `tests/melting_point/fights/m2-rhino-4242.yaml`. The beams share a
  formation out, searching from the turret:
  `tests/extra_weapon/fights/energy-diffraction-formations.yaml`, beside its
  control `tests/melting_point/fights/m6-formations-4242.yaml`. A group's
  first beam gives up a lock it shares, and the beams take no motion while
  the main beam holds no lock: `tests/corpus/fights/201370830-r6.yaml` and
  `tests/corpus/fights/201370830-r7.yaml`.
- A blueprint's and an officer's damage rates on the main skill reach
  Whirlwind, the recording holding them on its skill too, and compose on its
  damage as on the main skill's:
  `tests/extra_weapon/fights/whirlwind-attack-enhancement.yaml` and
  `tests/extra_weapon/fights/whirlwind-cost-control.yaml`.
- An item's damage rate reaching extra skills through its `extraSkillEffect`
  composes on a Homing Missile's own damage and on Whirlwind's, and the
  recording holds it on every slot it reaches:
  `tests/extra_weapon/fights/homing-missile-secondary-fire-control.yaml`,
  `tests/extra_weapon/fights/whirlwind-haste-module.yaml` and
  `tests/extra_weapon/fights/sabertooth-amplifying-core.yaml`.
- Sticky Oil Bomb's bomb writes its buff on the Rhino it strikes, from the
  Phantom Ray, and leaves an oil of its splash that renews the buff every 19
  ticks, keeping the Phantom Ray as its source, and stands to the fight's
  end; the unit carries the oil's fire's range and life time:
  `tests/extra_weapon/fights/sticky-oil-bomb.yaml`, beside its control
  `tests/extra_weapon/fights/sticky-oil-bomb-control.yaml`.
- A Sticky Oil Bomb landing on a Hound's fire leaves a fire of its splash
  that burns the row's 7 seconds:
  `tests/extra_weapon/fights/sticky-oil-bomb-fire.yaml`.
- A Vulcan's Sticky Oil Bomb, which does not lock its target, leaves its
  oils: `tests/extra_weapon/fights/sticky-oil-bomb-vulcan.yaml`.
- The Vulcan's Incendiary Bomb fires ten shells a volley from one skill of
  both its launchers, each leaving a fire of 18 metres that burns 12
  seconds, and a Marksman burns in them:
  `tests/extra_weapon/fights/incendiary-bomb-vulcan.yaml`.
- A Phantom Ray's Sticky Oil Bomb fires past a Rapid-Fire Turret standing in
  its line of fire, at the unit behind it:
  `tests/corpus/fights/268447927-r2.yaml`.
- Smoke Bomb's eight shells leave eight fogs of 18 metres, a Marksman
  standing in one ranges 91 metres for its 140 and 140 again once it leaves,
  and every fog goes as the fight ends; with fogs half as wide the events
  part on the tick the first lands:
  `tests/extra_weapon/fights/smoke-bomb-marksmen.yaml`.
- Spider Mine's support skill starts at t1 at a Rhino 201 metres off. It
  locks the main skill until t33, prepares to t31 and returns to idle at t33.
  Its line makes two mines at t1, which appear at t41 and explode at t109 for
  2500 each, one blast reaching the other mine for its 750:
  `tests/extra_weapon/fights/spider-mine-rhino.yaml`. Against Crawlers:
  `tests/extra_weapon/fights/spider-mine-crawlers.yaml`.
- The line makes a batch every 15 seconds (t1, t301, t601), the skill
  starting each time. At t601 it starts while the main skill is attacking
  between blows: `tests/extra_weapon/fights/spider-mine-sledgehammers.yaml`.

- Air Defense Mark's marker strikes for nothing and writes its mark on
  every aircraft within 100 metres of where it lands: 0.3 more damage taken
  and 20 metres off the main skill's range, a Wraith's 60 to 40, the buffs'
  `attack_range_reduce_value` reading -20:
  `tests/extra_weapon/fights/air-defense-mark-wraiths.yaml`. It marks the
  Wasps and not the Rhino beside them, and once no aircraft is left it holds
  no lock, an extra skill being no main searcher that
  `TrySearchAliveTarget` would answer:
  `tests/extra_weapon/fights/air-defense-mark-wasps.yaml`.
- Anti-Air Barrage releases its sixteen projectiles every ten seconds, its
  two weapons in turn, one every two ticks, scattered 55 metres about its
  target and climbing first, and strikes aircraft alone, 900 a hit:
  `tests/extra_weapon/fights/anti-air-barrage-wasps.yaml`, beside its
  control `tests/fortress/fights/m4-wasp-4242.yaml`, and
  `tests/extra_weapon/fights/anti-air-barrage-mixed.yaml`.
- Matrix Bombardment's four guns join the Wraith's group as slots 4 to 7,
  prepare with the main row's as the core attacks and deal 381 each; their
  projectiles leave the tick after their release:
  `tests/extra_weapon/fights/matrix-bombardment-rhino.yaml`, beside its
  control `tests/wraith/fights/m2-rhino-4242.yaml`. The eight slots share
  seven Crawlers out, and the Wraith's lock is the last the main row's slots
  took: `tests/extra_weapon/fights/matrix-bombardment-crawlers.yaml`. Over
  two formations: `tests/extra_weapon/fights/matrix-bombardment-formations.yaml`.
- Swarm Missiles' 46 missiles are shared out evenly among the units within
  the skill's reach and its extra search range
  ([combat.md](combat.md#ordinary-projectiles)): two units 23 each,
  `tests/extra_weapon/fights/swarm-missiles-mixed.yaml`; a Crawler swarm over
  three volleys, `tests/extra_weapon/fights/swarm-missiles-crawlers.yaml`.
- Rocket Punch throws at 83.8% of the Fortress's life and again at 51.4%,
  each fist striking what its splash reaches for 12000; with the first
  condition for both punches the second is thrown from tick 377:
  `tests/extra_weapon/fights/rocket-punch-rhinos.yaml`.
- Fork's two bolts join the Raiden's group as slots 3 and 4 and strike with
  the main row's from the first blow, five Crawlers a blow; with the Raiden's
  60 degrees in place of the whole circle on the row's slots the Raidens'
  locks part from tick 186: `tests/extra_weapon/fights/fork-crawlers.yaml`,
  beside its control `tests/raiden/fights/m3-crawler-4242.yaml`. The core
  reaches 100 metres and every other slot 110, and no two bolts share a
  target: `tests/extra_weapon/fights/fork-mixed.yaml`.

- Disintegration's wave strikes the ground about the Abyss 30 metres further
  every half second after its blow, each unit once, the Rhinos and Crawlers
  each losing 0.2 of their life as the slow is written:
  `tests/extra_weapon/fights/disintegration-rhinos.yaml`. Two Abysses' waves
  renew each other's buff and take their share again, the buff keeping its
  first source, and leave the Wasp among them alone:
  `tests/extra_weapon/fights/disintegration-two-abysses.yaml`. A shield keeps
  the Marksman it covers out of every firing that reaches it, until the
  Abyss's sweep breaks it, and a shield struck for nothing records nothing:
  `tests/extra_weapon/fights/disintegration-shield.yaml`.

- Naval Gun's two guns are one skill, slot 1 after the Overlord's main
  skill, that fires one shell a blow from its first gun, every three seconds
  give or take 0.3, for 7000:
  `tests/extra_weapon/fights/naval-gun-rhinos.yaml`. Two Overlords whose guns
  turn onto targets spread wide, each searching from where its first gun
  points: `tests/extra_weapon/fights/naval-gun-spread.yaml`.

- Gun-launched Missile's two launchers are one skill, slot 4 after the
  Mountain's guns, firing one missile a blow from its first launcher at its
  own 180 metres; it takes the motion as the guns cool without a lock and
  the turret turns onto its lock:
  `tests/extra_weapon/fights/gun-launched-missile-rhinos.yaml`.

- Electromagnetic Barrage's sixteen shells a blow write the slow that turns
  technologies off on what they reach, the Rhino and the Crawlers carrying
  none, and its skill names the Crawler the beam killed through the rest of
  its burst: `tests/extra_weapon/fights/electromagnetic-barrage-rhinos.yaml`.
  Red's shield takes its shells for 6000 each, the seventh for the 4000 it has
  left, which breaks it: `tests/extra_weapon/fights/electromagnetic-barrage-shield.yaml`.

- Dual Wield's side arm begins its blow four ticks after the main gun
  begins one, and takes the main gun's lock where nothing else is in its
  reach within 40 metres of it: `tests/extra_weapon/fights/dual-wield-close.yaml`.
  Attacking, it waits for its turn however long its interval is over, and
  searches about the main gun's last lock while it holds none:
  `tests/extra_weapon/fights/dual-wield-spread.yaml`. The main gun out of its
  cooling takes the side arm's lock in place of a search:
  `tests/extra_weapon/fights/dual-wield-near.yaml`. Among Marksmen and Wasps
  it fires at a Wasp beside the main gun's:
  `tests/extra_weapon/fights/dual-wield-mixed.yaml`.

### Replayed

- A Tarantula's Spider Mine skill that locks a new enemy as it leaves its
  idle state keeps its attack target, and its turret turns to the enemy it
  held: replay 2324_20260925--134259672 round 5, ticks 301 to 332, fought by
  the game with `scripts/corpus/match-replays.py`.
- An officer's damage rate reaches a Fire Badger's Scorching Charge slot:
  replay 2324_20260925--134259672 round 5, tick 1, fought by the game with
  `scripts/corpus/match-replays.py`.
- A Spider Mine a Hacker turned explodes for the side it was made on:
  replay 2324_20260925--134259672 round 5, tick 156, fought by the game with
  `scripts/corpus/match-replays.py`.
- A Centurion's Homing Missile takes a block in the way that its main gun is
  too short for: replay 2324_20260925--67159970 round 6, fought by the game
  with `scripts/corpus/match-replays.py`.
- A Homing Missile fired at a block in the way names its missile skill and
  climbs first: replay 2324_20260925--67159970 round 6, fought by the game with
  `scripts/corpus/match-replays.py`.
- A Centurion's missile skill scores its search from its turret's rotation,
  and takes the target its main gun takes: replay 2324_20260925--67159970
  round 6, fought by the game with `scripts/corpus/match-replays.py`.
- Disintegration goes on naming the Fang it winds up on after the Fang dies,
  until its blow: replay 2324_20260925--67161951 round 4, ticks 21 to 62,
  fought by the game with `scripts/corpus/match-replays.py`.
- A Centurion whose missile skill holds its motion turns its turret to the
  block its missiles fire at, not to the lock behind it: replay
  2324_20260925--67159970 round 6, fought by the game with
  `scripts/corpus/match-replays.py`.

### Read

- A dead unit's line: `FightMech.OnDead`, `FightEffectSystem.DeactiveEffect`,
  `FightSupportSkill.Destroy`, `SupportUnitSystem.RemoveSkillOwner`,
  `TeamSupportUnitManager.RemoveCreator`.
- An extra skill's range: `FightSkill.GetAttackRange`,
  `AttackRangeProperty.RegisterDataChangeEvent`, `AttackRangeProperty.Refresh`,
  `SkillDataModifier.AddData`.
- A weapon held at the edge of its arc searches again once its timer has run
  out, and the timer counts down while it attacks:
  `SkillAttackableChecker.Check`, `SkillAttackableChecker.CheckWhenLoseTarget`,
  `SkillAttackAngleChecker.IsActorInAttackAngle`,
  `SkillAttackAngleChecker.IsWeaponInAttackAngle`,
  `SearchTargetController.CanStartSearch`, `SkillAttackState.Update`.
- A self splash's blow checks no target: `SkillAttackController..ctor`,
  `NormalAttackPerformer.IsEnableCheckTarget`,
  `NormalAttackPerformer.IsInterruptedByInvalidTarget`.
- An extra weapon technology adds its skill through its provider:
  `ExtraWeaponTech`, `ExtraSkillProvider.AddEffect`,
  `ExtraSkillSystem.AddMech`, `SkillManager.AddSkill`, `FightSkill.EnableUseMainSkillRange`.
- The manager's skills update in ascending skill ID: `SkillManager.Update`,
  `SkillManager.SortSkills`.
- A buff's range: `AttackRangeProperty.GetAttackRange`,
  `BuffManager.GetAttackRangeAddValue`, `BuffManager.GetAttackRangeReduceValue`,
  `BuffManager.GetExtraAttackRangeAddValue`; an extra skill's fallback search,
  `SkillSearchTargetController.TrySearchAliveTarget`.
- An extra skill's differences: `FightSkill.Init`,
  `SkillSearchTargetController.PrepareSearch`,
  `MainSkillSearchTargetController.PrepareSearch`,
  `SkillSearchTargetController.PerformNormalSkillSearch`,
  `FightSkill.ChangeLockTarget`, `FightSkillBase.IsMainSearcher`,
  `SkillIdleState.Update`, `ProjectileSystem.Create`, `FightProjectile.IsFlying`.
- Its damage: `SkillData.GetDamage`.
- Its range: `FightSkill.GetAttackRange`; the attacking check's minimum range,
  `SkillAttackableChecker.CheckWhenLoseTarget`; the towers it may take,
  `FightSkill.IsTowerAttackable`, which
  `WallConstructionTargetChecker.CheckWallConstruction` asks of the block in
  the way it found.
- Its fire: `ExtraSkillProvider.PerformHitEffect`, `ExtraSkillProvider.AddEffect`,
  `ExtraWeaponTechnologyData.GetFireLifeTime`, `MechDataModifer.AddData`,
  `GroundFireController.GetFireMech`, `RangeItemSystem.AddItem`,
  `RangeItemSystem.DoAddItem`, `RangeItemSystem.GetRepeatItem`.
- Its fog: `ExtraWeaponTech`'s `IFogProvider.GetAttackRangeChangeRate`,
  which reads its row's `fogAttackRangeChangeRate`, and its
  `IRangeItemProvider` as for an oil.
- Its buff and oil: `ExtraSkillProvider.PerformHitEffect` writes
  `IBuffDataSource.GetBuffData` through `BuffSystem.AddBuff` from the skill's
  owner, then adds the range item of `IExtraSkill.GetRangeItemType` with the
  technology as its provider; `ExtraWeaponTech` answers
  `IRangeItemProvider.GetLifeTime` zero, `GetRangeItemRange` its skill's
  `splashRange`, `GetRoundDuration` one and `IFireProvider.GetFireLifeTime`
  its row's first `fireLifeTime`. `BuffItemController.PerformItemEffect` adds
  the buff with no source, and `Buff.Reset` keeps a buff's source unless its
  data summons (`IBuffData.IsSummoning`).
- Its taking the motion: `FightSkill.SearchLockTarget`,
  `FightSkillBase.IsMainTargetProvider`,
  `FightSkillBase.IsMainTargetProvider`, `FightMech.SetAttacker`,
  `FightMech.SetMotionAttackerAfterSkill`, `AutoMoveBehaviour.IsIdle`,
  `AutoMoveBehaviour.IsActive`, `FightSkillBase.IsLockTargetAvaliable`,
  `MotionAttackState.Update`, `MotionAttackState.AttackRotate`,
  `MotionIdleState.Update`, `MotionMoveState.Update`,
  `MotionMoveState.NormalRotate`, `MotionController.CalculateTargetDirection`,
  `MotionController.RotateWeaponTo`, `FightSkill.RotateWeaponTo`.
- A grouped extra row: `FightSkillFactory.Create`,
  `FightSkillFactory.PrepareGroupedSkill`, `FightSkill.GetAttackRange`,
  `FightSkillBatch.GetAttackRange`, `SkillAttackableChecker.TrySearchGroupSkillLockTarget`;
  its technology's damage rate: `ExtraSkillProvider.EnableEffect`,
  `IExtraSkill.GetReduceDamageRateBase`, `ExtraWeaponTech.GetReduceDamageRateBase`,
  `DamageProperty.Refresh`, `DamageProperty.RegisterDataChangeEvent`,
  `DamageProperty.CalculateDamage`, `DamageCalculator.GetNormalDamage`,
  `LaserDamageCalculator.GetNormalDamage`, `LaserDamageCalculator.GetAttackDamage`.
- An around skill: `FightAroundSkill.CreateStartAttackChecker`,
  `AroundSkillStartAttackChecker.Check`, `PreemptiveSkillStartAttackChecker.Check`,
  `PreemptiveSkillStartAttackChecker.IsMainSkillIdleState`,
  `FightUtility.CalculateDistance3D`, ``FightQuadtree`1.Query``,
  `PreemptiveSkillExitIdleBehaviour.Execute`, `PreemptiveSkillEnterIdleBehaviour.Execute`,
  `PreemptiveSkillController.SetPreemptiveSkill`, `PreemptiveSkillController.RemovePreemptiveSkill`,
  `SkillLockState.Exit`, `SkillAttackState.CheckAttackable`,
  `SkillIdleState.TryPerform`, `SkillManager.SortSkills`,
  `SkillDamageProvider.CalculateDamagePosition` (its `IsSelfSplash`),
  `DamageProperty.RefreshBaseDamage`, `FightAroundSkill.StartForObject`,
  `AroundSkillExitAttack.Execute`.
- A permanent preemptive skill: `PreemptiveSkillController.Update`,
  `PreemptiveSkillController.GetActiveTransitionDuration`,
  `PermanentPreemptiveActiveConditionLifeController.CheckCanActive`,
  `SkillManager.ActivePermanentPreemptiveSkill`, `SkillLockState.Enter`,
  `SkillLockState.Exit`, `FightSkill.OnPermanentPreemptiveSkillActive`,
  `FightExplosionSkill.OnPermanentPreemptiveSkillActive`,
  `FightSkillBase.IsMainSearcher`, `FightMech.GetMainSearcherSkill`.
- An explosion: `FightExplosionSkill.EnterFight`, `FightExplosionSkill.OnTravelFinished`,
  `FightExplosionSkill.GetAttackEffect`, `SuicideEffect.Perform`,
  `DeadEffectSystem.Update`, `DeadExplosiveController.PerformDeadEffect`,
  `DeadExplosiveDamageProvider.GetDamage`,
  `DeadExplosiveDamageProvider.GetSplashRange`,
  `DeadExplosiveDamageProvider.GetTeamController`,
  `DeadExplosiveDamageProvider.GetEffectTargetType`,
  `DeadExplosiveDamageProvider.GetMainTarget`.
- A support skill: `FightSupportSkill.CreateStartAttackChecker`,
  `SupportSkillStartAttackChecker.Check`, `SkillStartAttackChecker.Check`,
  `FightSkill.CanStartAttack`, `SkillIdleState.TryStartAttack`,
  `FightSkill.CheckAttackable`, `SkillPrepareState.Update`,
  `SupportUnitCreator.PreCalculate`, `SupportUnitCreator.Update`,
  `SupportUnitCreator.CreateMech`, `FightSupportSkill.Init`,
  `FightSupportSkill.Enable`, `SupportSkillData.PreProcess`.
- A wave: `DamagePerformer.Perform`, `DamagePerformer.PerformDiffusionRangeEffect`,
  `DamagePerformer.DiffusionIntevalCallBack`, `DamagePerformer.DiffusionCompleteCallBack`,
  `DamagePerformer.PrepareRangeTargetsInDiffusion`,
  `DamagePerformer.ProcessAdvancedEnergyShieldEffect`,
  `DamagePerformer.PerformHitAdvancedEndergyShieldEffect`, `GRTimerManager.Update`,
  `GRTimer.Init`, `GRTimer.Update`, `SkillDamageProvider.GetTargetType`; its
  buff's share of life, `BuffManager.AddBuff`, `Buff.ReEnableDisposableEffect`,
  `Buff.Reset`, `IBEC_ChangeCurrentLife.Enter`,
  `IBEC_ChangeCurrentLife.ReEnableDisposableEffect`,
  `IBEC_ChangeCurrentLife.Perform`; its weapon, `FightWeapon..ctor`.
- A shield's damage: `DamagePerformer.CalculateHitEnergyShieldDamage`,
  `DamagePerformer.PerformHitTargetEffect`,
  `DamagePerformer.PerformHitAdvancedEndergyShieldEffect`,
  `ExtraWeaponTech.IsChangeHitEnergyShieldDamage`,
  `ExtraWeaponTech.ChangeHitEnergyShieldDamage`.
- What reaches it: `SkillDataModifier.AvaliableCheck`, `OfficerData.IsMainSkillEffect`,
  `TechnologyData.IsMainSkillEffect`, `DamageProperty.CalculateDamage`,
  `OfficerData.IsExtraSkillEffect`, `TechnologyData.IsExtraSkillEffect`,
  `EquipmentData.IsExtraSkillEffect`, `EnergyTowerSkillData.IsExtraSkillEffect`.

### Not established

- **The motion an extra skill leads beyond one update.** Every recording
  shows the main skill taking the motion back on the update after; how a
  moving or idle unit walks after an extra skill's lock is read, not recorded,
  and so is how a unit with a body whose main skill is no batch turns while
  an extra skill holds its attack. `AttackRotate` also turns the extra skill's own weapons
  (`FightSkill.RotateWeaponTo`) unless they are standalone; no recording reads
  that turn.
- **When a batch's first gun starts after an extra skill held the motion.**
  The build starts an idle skill whose target is in its attack area in its
  own update (`SkillIdleState.TryPerform`, `SkillStartAttackChecker.Check`),
  holding fire only under a move ability (`SkillManager.IsHoldFire`), and the
  simulator starts a batch's first gun from the motion. A Mountain whose
  Gun-launched Missile held the motion and whose motion is idle starts its
  first gun in the game as soon as its target turns into its 10 degrees, four
  ticks before the simulator: `layouts/gun-launched-missile-spread.yaml`,
  t532, not pinned.
- **What Electromagnetic Barrage's buff switches off** beyond a plain
  technology's numbers and an extra skill
  ([technology_effects.md](technology_effects.md)): a unit that carries any
  other is refused, and so is a unit's own shield taking a shell, which only a
  technology gives it.
- **A wave that deals damage.** Disintegration's deals none, and the
  simulator strikes with a wave's damage as a splash does, the shields too.
  Nor is a shield covering what the blow was aimed at recorded, nor a wave's
  timer ordered against a summon's that is due on the same tick: the
  simulator runs the summons first.
- **A correction other than damage composing on an extra skill**, an Energy
  Tower skill's range among them. Refused.
- **What invincibility keeps off.** A fire burns an invincible Fire Badger;
  whether a shot or a blow does is not recorded, and the simulator lets every
  hit through.
- **An around skill's `preemptiveInterval`**, which no fought member sets
  and the extraction refuses, and its self splash at a target that is not
  visible (`SkillAttackRangeChecker.IsAttackTargetInAttackRange`), which no
  recording reaches.
- **A correction other than damage on a skill with a damage rate**, a range
  on a skill that reads its own range or an interval reaching it through the
  main skill. Refused.
- **The other preemptive skills and conditions.** A transition to wait out
  (condition type 2), an ammunition condition, an extra weapon buff and an
  incompatible skill are read in part and refused by the extraction.
- **A support skill's line locked.** A batch due while the main skill is
  busy locks the line until the skill starts. This is read, and no recording
  reaches it: the main skill rests between its blows, and that is when each
  batch fell due.
- **Every other member of the list.** Refused by name.
