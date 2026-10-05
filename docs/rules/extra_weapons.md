# Extra weapons

What an extra weapon technology adds to a unit: a skill of its own beside the
unit's main one, the rows of `TechnologyGroupData.extraWeaponTechnologies`.
Each row names the skill it adds in `skillID`, which `scripts/extract/extract-units.py`
writes under the unit's `extra_weapons` in `config/units/` where its shape is
one that file can state. The simulator fights Secondary Armament, the
Sabertooth's two guns, Anti-Air Missile, its missile at the air, Incendiary
Bomb, the Hound's, Scorching Charge, the Fire Badger's self-destruct, Homing
Missile, the Centurion's, Sticky Oil Bomb, the Phantom Ray's and the
Vulcan's, Whirlwind, the Rhino's, and Energy Diffraction, the Melting
Point's, and refuses every other member by name: the members' skills differ
in kind, a projectile, an explosion, a laser, a summon, a sweep around the
unit, and many leave a terrain or write a buff, so each joins once a recording
of it agrees.

## A skill beside the main one

`ExtraWeaponTech` is an `IExtraSkill`, and its provider,
`ExtraSkillProvider.AddEffect`, hands it to `ExtraSkillSystem.AddMech`, which
adds the row's skill through `SkillManager.AddSkill`. The first skill a
`SkillManager` is given is its main skill; every later one joins its extra
skills. `AddSkill` adds each `FightSkill` the row's skill makes, so a
standalone row adds one skill for each of its weapons: Secondary Armament adds
two. A grouped row adds one for each of its weapons too, and
`FightSkillFactory.PrepareGroupedSkill` puts them in a `SkillGroup` whose
every skill has the main skill for its `ParentSkill`: Energy Diffraction adds
four beams. Any other row adds one skill that fires all its weapons:
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
  the motion. Moving, it turns the skill's weapons to what the skill fires
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

A weapon whose row gives it no arc has no transform of its own and points
where its mount points: the Hound's bomb launchers point as the unit does, and
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
  entries: Homing Missile's one entry, 800, is every level's, and Incendiary
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
Bomb's fire reaches 12 metres from where it lands and burns for 10 seconds.

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
  (`enableFriendlyFire`), buildings included, with the life the unit had
  before it took its own (`explosiveDamageCondition` 2) times the skill's
  multiplier. A unit some other hit killed had no such life and deals
  nothing. A unit the explosion kills explodes in turn on the same update, and
  what it kills reads after what the tick's shots killed.
- **Every such death leaves a fire.** The explosion leaves a fire of the
  skill's own, reaching 40 metres for 7 seconds, where the unit fell, under
  its side, the death of a side's last unit too. A fight decided on the tick
  the fire is left ends on that tick, and the fire goes with it before any
  snapshot holds it.

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
under Attack Enhancement's +0.12.

A skill whose range is the main skill's with its own added, of a row that
uses the main skill's range or a grouped row's, never reads its own range, so
a range that reaches it changes nothing: Energy Diffraction's own −30 metres
lands on its beams as on the main beam, and leaves them reaching 95.

A skill without a damage rate holds what reaches it alone, an equipment's
through its `extraSkillEffect` and an Energy Tower skill's, and its damage,
its row's entry for the unit's level, composes them and the buffs' the same
way: Secondary Fire Control System's +0.25 makes a Homing Missile's 800 a
1000. A recording holds each skill's corrections on its own slot. Any other
number than damage a source corrects on an extra skill is not read, so the
simulator refuses it.

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
- A Centurion's Homing Missile fires its four missiles at its own interval,
  each landing its offset of up to 20 metres from its target, and a missile
  landing beyond its 7 metre splash of every unit strikes nothing:
  `tests/extra_weapon/fights/homing-missile.yaml`, beside its control
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
- A Phantom Ray's Sticky Oil Bomb fires past a Rapid-Fire Turret standing in
  its line of fire, at the unit behind it:
  `tests/corpus/fights/268447927-r2.yaml`.

### Replayed

- A Centurion's Homing Missile takes a block in the way that its main gun is
  too short for: replay 2324_20260925--67159970 round 6, fought by the game
  with `scripts/corpus/match-replays.py`.
- A Homing Missile fired at a block in the way names its missile skill and
  climbs first: replay 2324_20260925--67159970 round 6, fought by the game with
  `scripts/corpus/match-replays.py`.
- A Centurion's missile skill scores its search from its turret's rotation,
  and takes the target its main gun takes: replay 2324_20260925--67159970
  round 6, fought by the game with `scripts/corpus/match-replays.py`.
- A Centurion whose missile skill holds its motion turns its turret to the
  block its missiles fire at, not to the lock behind it: replay
  2324_20260925--67159970 round 6, fought by the game with
  `scripts/corpus/match-replays.py`.

### Read

- A self splash's blow checks no target: `SkillAttackController..ctor`,
  `NormalAttackPerformer.IsEnableCheckTarget`,
  `NormalAttackPerformer.IsInterruptedByInvalidTarget`.
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
  `DeadExplosiveDamageProvider.GetEffectTargetType`,
  `DeadExplosiveDamageProvider.GetMainTarget`.
- What reaches it: `SkillDataModifier.AvaliableCheck`, `OfficerData.IsMainSkillEffect`,
  `TechnologyData.IsMainSkillEffect`, `DamageProperty.CalculateDamage`,
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
- **Every other member of the list.** Refused by name.
