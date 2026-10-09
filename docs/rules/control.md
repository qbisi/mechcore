# Control beam

How a control beam, the Hacker's main skill, turns an enemy unit to its own
side, what it strikes instead when it may not, and what the turn does to the
skills around it. Its numbers are the `path` block of its `config/units/`
file: `warmup_attack_count` and `warmup_damage_multiplier`, beside the skill's
damage.

## What a hit does

`FightControllBeamSkill.GetAttackEffect` gives each hit one of two effects:

- **It turns** a unit that may be turned: a mech that no item keeps from it.
  An item keeps it off when it answers `IgnoreControllerBeam`, which every
  item of the buff-ignoring class does: the Anti-Interference Module.
- **It strikes** anything else with the skill's damage effect: a unit wearing
  such an item, a building, and a shield the skill fires at in place of its
  lock.

A hit's damage is `ControlBeamDamageCalculator.GetAttackDamage` of the skill's
attack count, counted from zero: up to `warmup_attack_count` it is the damage
times `warmup_damage_multiplier`, rounded down and at least 1, and full after.
The Hacker's three first hits deal 1, and every later one 600. A recording
reads the beam's damage as the calculator's at the default count, zero: the
Hacker reads 1 all fight, whatever it is on.

A hit that strikes deals `FightControllBeamSkill.GetDamage`: the calculator's
damage times `DAMAGE_MODIFIER`, three tenths in `FPoint`, rounded down. A full
hit deals 179, and a warmup hit nothing: a hit that deals nothing strikes
nothing and writes no damage. The effect is the main skill's damage effect, so
it splashes as the skill does: Explosive Ammo gives it 5 metres, and one hit
strikes every unit around the one it is on. A hit that turns strikes nothing,
and its splash nothing either; but after it adds its power, the unit it is on
raises `OnMechBeHit` with the beam's owner as the attacker
(`ControllEffect.Perform`), as a unit a hit reaches does. What listens to it
answers the Hacker: a Void Eye with Electromagnetic Armor switches the
Hacker's technologies off.

## Progress and the turn

`ControllEffect.Start`, as the beam's attack begins or changes target, adds the
skill to its target's entry in `TeamTranslationSystem.translatingDatas`, which
keeps its entries in the order they were added; `ControllEffect.Stop` takes it
away, and the entry goes with its last skill. Each skill is listed, so a unit
whose several beams hold one unit is listed once for each, in the order they
started; Multi Control's beams are each a skill of their own
([extra_weapons.md](extra_weapons.md#a-group-of-control-beams)). A beam's owner that dies stops
its beam on that tick (`FightMech.OnDead`, `SkillManager.OnOwnerDead`): the
Rhino a Hacker was turning holds no entry from the tick the Hacker dies. Each
hit that turns adds its
damage to the entry's progress (`TeamTranslationSystem.Translate`) and lists
the beam's owner among the unit's attackers, as a hit does.

`TeamTranslationSystem.Update` runs before `FightCoreSystem`. It takes the
entries' keys and asks each again as it comes: a unit still alive, still in
the dictionary, and whose life is no more than its progress, changes to the
side of its first beam's owner. Life alone is compared: a shield the unit
carries does not count. Turning one unit stops the beams locked on it and its
own, which may take another's entry away or move the side its owner stands on,
so two Hackers that turn each other on one tick leave both on the side of the
first one turned.

The turn (`TeamTranslationSystem.ChangeTeam`):

- **hands the unit out as a kill with no killer.** `ExpSystem.CalculateExp` is
  called with the side it goes to and no unit, while it still stands on its old
  side. With no killer the whole of its loot joins the assist pool, which is
  shared among its attackers and the units in range on the side it goes to.
  A unit a beam has turned takes no share, nor thins the others'
  (`ExpSystem.IsValidOwner`).
- **takes its buffs off** first (`BuffManager.RemoveBuffEffect`), last first,
  each recorded `team_changed`, but for the ones the common parameter
  `hacker_cannot_clear_bufflist` names: buff group 150002, Photon Emission's
  invincibility, Scorching Charge's charge, Combat Evolvement's and Kinetic
  Charge's stacks, and 8028. What each wrote goes with it, and the last that
  disabled the unit's technologies switches them on again: a Fire Badger an
  Electromagnetic Impact switched off has its 80% more life back as it turns.
- **moves it** to the other side's lists (`FightActor.ChangeTeam`), at
  their end (`FightTeamController.AddActor`): it updates after every unit
  already on that side, its summons among them, and a unit handed back on
  its death after every unit of the side it returns to. It leaves
  its formation for one of its own on the new side. `FightMech.GetMechTeam`
  answers none for a turned unit, so the recorder numbers a formation for
  the unit itself as it first records it, walking each side's units by where
  they stand, `z` before `x`: the units turned on one tick take theirs in that
  order as the tick ends. Its damage and its kills from then are recorded under a
  recorder of its own. A summon, a Spider Mine among them, has no `MechTeam`
  to leave: it is recorded under the formation it was made with, before the
  turn and after, and it counts under the recorder of its own it had, which
  moves to the new side with what it has counted.
- **leaves its shots in the air on the side they were fired from.** A
  projectile strikes for the side its controller was handed as it was
  released (`ProjectileSystem.Create`, `ProjectileController.Init`), so one
  the unit fired before the turn still strikes the side it now stands on;
  its removal names the side the unit stands on as it lands, or, the unit
  dead by then, the side the recording last saw it on.
- **stops every skill locked on it** (`FightSkill.OnChangeTeam`): `StopAttack`
  drops the lock and keeps the attack target. A skill idle already stays idle,
  its weapons naming what they named until it takes another; a cooling one
  goes on cooling. Any other enters its cooling, naming what it fired at, or
  without one enters `SkillIdleState` with its targets cleared, unless it is
  a main skill whose unit's permanent preemptive skill is active, which stays
  locked, and a permanent preemptive skill not yet active, which stays as it
  is. The motion is left alone. The turned unit's own skills, its main skill
  and each extra skill, end the same way: the beam's change runs before any
  unit updates, so a skill sent cooling is updated in it on the same tick, and
  a Fire Badger turned while it charges keeps its main skill locked and its
  charge's buff. A main skill a running preemptive skill has locked is no
  exception: a Rhino turned while it spins leaves its spin and its main skill
  idle on the tick it turns, the spin handing the main skill its place back
  (`PreemptiveSkillEnterIdleBehaviour`), and both search again.

A side whose every unit a beam has turned has none left, and loses as a
wiped-out side does: its towers are torn down.

## A turned unit's death

A hit that kills a turned unit, or its own blow, only queues it with
`DeadEffectSystem`, whose
update comes after every unit and projectile: what it took until then is
counted under its own recorder. Its `OnDead` then hands it back to the side it
was deployed on (`TeamTranslationSystem.OnMechDead`), and every skill still
locked on it stops its attack as above, an extra weapon's as a main skill's:
each is a `FightSkill` that hears its lock change side. Handing it back puts it in that
side's trees again (`FightTeam.AddMech`), after the dead have left them, and
nothing takes it out: it stays there dead for the rest of the fight, where a
kill's search for formations to share experience with
(`ExpSystem.AddRangeUnit`, which asks for no life) finds it and counts its
formation. A unit that updated after the blow
has met the death in its own update already. A summon stands in no side's
`FightTeam` (`TeamTranslationSystem.IsMechInFightGroup`), so it is not handed
back: it dies on the side that turned it, and the skills locked on it hold
their lock on it until they next update.

When the fight ends there, the weapons of the units left name nothing.

A turned Fire Badger that takes its own life explodes before it is handed
back, and its fire stands under the side that turned it: the fire is left
under the unit's side as it dies (`DeadExplosiveController.PerformDeadEffect`
hands `RangeItemSystem.AddItem` its `currentTeamController`), though its
blast strikes for the side it was deployed on.

## The skills around a turn

What a turn leaves behind is a skill idle while its motion still attacks,
which nothing else in a fight makes:

- A lock on the skill's own side is no valid target: the skill searches on its
  next update, as for a dead lock.
- `MotionAttackState.Update` turns the unit to its target
  (`MotionAttackState.AttackRotate`) and walks it on when the target is out of
  range. The motion's own exits, out of range or out of the attack angle,
  follow from an attacking skill and do not apply to an idle one.
- A bodyless skill that starts its attack from its own update fires its first
  blow from the tick after: a state is not updated on the tick it is entered.
- A search prepared as the tick opened (`FightCoreSystem.PreCalculate`) finds
  every unit on the side it stood on then. One made in the middle of the tick
  (`PerformSearch`) asks the side each stands on now, and takes a unit turned
  on that tick.
- A prepared search whose winner a beam has turned to the searcher's side
  since does not take it, nor the runner-up:
  `ScoreRatingTargetSelector.TrySelect` refuses the prepared result, and the
  search is a `Select` over where everything stands by then. A Crawler whose
  lock was turned before its side updated scores the enemies after they have
  moved this tick.

## Shields

The beam does not cross a shield. At a unit a battlefield shield or a Barrier
covers, the skill fires at the shield in place of its lock, and the shield
takes the damage effect, 179 a hit; nothing is turned. A unit's own shield,
from a Portable Shield, is never struck: a hit that turns deals no damage, and
the shield does not count against the turn.

## Evidence

### Recorded

- A Hacker turning a Void Eye with Electromagnetic Armor takes the armor's
  buff from tick 101, its Barrier going at 28568; without `OnMechBeHit` on a
  turning hit the simulator parts from the game at t101:
  `tests/control/fights/electromagnetic-armor.yaml`.
- The Hacker turns a Crawler whose progress reaches its life on tick 103, cools
  for a tick naming it, and searches again; the turned Crawler takes a new lock
  on that tick: `tests/hacker/fights/m3-crawler-4242.yaml`, ticks 101 to 105.
- The Rhino a Hacker was turning holds no entry from tick 203, when its blow
  kills the Hacker: `tests/hacker/fights/m2-rhino-4242.yaml`.
- A turn with one Hacker hands its formation the Crawler's 4 and the whole
  assist pool, 8 in all; with two, 4 to each formation in range:
  `tests/hacker/fights/m3-crawler-4242.yaml`, tick 103;
  `tests/hacker/fights/m6-formations-4242.yaml`, tick 97.
- A turned Crawler dies on its new side: the Crawlers that struck it in their
  attack state name nothing from that tick, those idle on it keep naming it,
  one that updated after the blow searches only on the next tick, and its last
  blow is counted under its own recorder:
  `tests/hacker/fights/m3-crawler-4242.yaml`, ticks 125 and 126.
- An extra weapon locked on a turned unit that dies drops its lock and keeps
  naming it until its next search: a Tarantula's Spider Mine skill on a turned
  Crawler that a projectile kills, in round 5 of replay 134259672, ticks 126
  and 127: `tests/corpus/fights/134259672-r5.yaml`.
- An idle turned Crawler whose lock is the Hacker searches at once:
  `tests/hacker/fights/m6-formations-4242.yaml`, tick 61. A turned Crawler
  caught in its backswing goes idle as it turns, turns to its new lock and
  starts its attack ten ticks on: `tests/hacker/fights/m3-crawler-4242.yaml`,
  ticks 263 to 273. A Crawler whose target is turned turns to the next while
  idle and walks on it once it is out of range:
  `tests/hacker/fights/m3-crawler-1787720817.yaml`, ticks 199 to 203, and
  `tests/hacker/fights/m3-crawler-4242.yaml`, tick 126. A bodyless start fires
  a tick later: `tests/hacker/fights/m6-formations-4242.yaml`, tick 156.
- Two Hackers turned on one tick take their formations in the recorder's
  order, which is their identity order there:
  `tests/hacker/fights/m6-formations-1787720817.yaml`, tick 189. Six Crawlers
  turned on one tick take theirs by where they stand, units 5, 4, 9, 10, 7
  and 14, not by identity: `tests/extra_weapon/fights/multi-control-crawlers.yaml`,
  tick 216.
- A skill that searches while the unit it would take was turned on that tick
  passes over it, a grouped skill as a skill of its own does:
  `tests/extra_weapon/fights/multi-control-crawlers.yaml`, tick 320.
- A skill of a group locked on a unit that turns hears it as a skill of its
  own (`FightSkill.OnChangeTeam`): `tests/extra_weapon/fights/multi-control-crawlers.yaml`.
- Two Hackers that turn each other on one tick end both on blue's side:
  `tests/hacker/fights/m1-mirror-1787720817.yaml`, tick 166.
- A Hacker turned with its side's last unit reads cooling at its target, and
  its side's towers fall: `tests/control/fights/anti-interference-mirror.yaml`,
  tick 168; `tests/hacker/fights/m1-mirror-4242.yaml`, tick 168.
- When the fight ends on a turned unit's death, the weapons left name nothing:
  `tests/hacker/fights/m3-crawler-1787720817.yaml`, tick 333.
- The Anti-Interference Module keeps its unit from being turned, and the beam
  strikes it for 179 a hit after three hits that write nothing:
  `tests/control/fights/anti-interference-rhino.yaml`,
  `tests/control/fights/anti-interference-crawlers.yaml`.
- Explosive Ammo splashes the strikes and nothing else:
  `tests/control/fights/explosive-ammo-anti-interference.yaml`,
  `tests/control/fights/explosive-ammo-crawlers.yaml`.
- A Portable Shield neither takes a hit nor slows the turn:
  `tests/control/fights/portable-shield-rhino.yaml`,
  `tests/control/fights/portable-shield-crawlers-4242.yaml`.
- A search in the middle of a tick takes a unit turned on it:
  `tests/control/fights/portable-shield-crawlers-1787720817.yaml`, tick 166.
- A battlefield shield and a Barrier take the beam's damage effect:
  `tests/control/fights/battlefield-shield.yaml`,
  `tests/control/fights/barrier.yaml`.
- A blue Fire Badger turned while it charges keeps its main skill locked and
  its charge's buff; its own blow hands it back on that tick, its fire under
  red's side; a blue Fire Badger an Electromagnetic Impact switched off has
  the Impact's buff taken off as it turns, and its technologies back on:
  `tests/corpus/fights/134259672-r4.yaml`, ticks 51, 76 and 142.

### Replayed

- A Hacker turns a Spider Mine, which stays in its formation, and its death
  is not handed back, so the Crawlers locked on it hold the lock for a tick:
  replay 2324_20260925--134259672 round 5, ticks 122 and 156, fought by the
  game with `scripts/corpus/match-replays.py`.
- A turned Spider Mine's statistics, the damage it has taken, move to red's
  side with it: replay 2324_20260925--134259672 round 5, tick 122, fought by
  the game with `scripts/corpus/match-replays.py`.
- A blue Crawler turned and killed on red's side stays in blue's tree where
  it died, and hands its formation a share of later kills near it: replay
  2324_20260925--134259672 round 5, ticks 385 and 494, fought by the game
  with `scripts/corpus/match-replays.py`; recorded in
  `tests/corpus/fights/134259672-r5.yaml`.
- A blue Crawler a Hacker turned updates after every red unit: replay
  2324_20260925--134259672 round 5, tick 161, fought by the game with
  `scripts/corpus/match-replays.py`.
- A red Crawler whose prepared winner, its own lock, was turned searches
  again with a `Select` and takes the nearer of two blue Crawlers as they
  stand after blue's update: replay 2324_20260925--134259672 round 5, tick
  210, fought by the game with `scripts/corpus/match-replays.py`.
- A blue Tarantula turned while its shot flies kills the red Mustang it
  fired at: replay 2324_20260925--134259672 round 5, tick 366, fought by the
  game with `scripts/corpus/match-replays.py`.

### Read

- The two effects and their damage: `FightControllBeamSkill.GetAttackEffect`,
  `FightControllBeamSkill.GetDamage`, `FightControllBeamSkill.DAMAGE_MODIFIER`,
  `ControlBeamDamageCalculator.GetAttackDamage`, `FightMech.IsSimulateMech`,
  `TeamTranslationSystem.IsIgnoredMech`, `IIgnoreBuffDataSouce.IgnoreControllerBeam`.
- Progress: `ControllEffect.Start`, `ControllEffect.Stop`,
  `ControllEffect.Perform` (which raises the target's `OnMechBeHit` after
  `Translate`), `TeamTranslationSystem.Add`,
  `TeamTranslationSystem.Remove`, `TeamTranslationSystem.Translate`,
  `TeamTranslationSystem.translatingDatas`.
- The turn: `ProjectileSystem.Create`, `ProjectileController.Init`,
  `ProjectileController.GetTeamController`, `TeamTranslationSystem.Update`,
  `TeamTranslationSystem.ChangeTeam`,
  `FightActor.ChangeTeam`, `FightTeamController.AddActor`, `ExpSystem.CalculateExp`, `ExpSystem.IsValidOwner`,
  `TeamTranslationSystem.IsTranslatedMech`, `FightMech.GetMechTeam`, which
  answers none for a turned unit.
- The buffs it takes off: `TeamTranslationSystem.ChangeTeam`,
  `BuffManager.RemoveBuffEffect`, `Config.GetTeamTranslationIgnoredBuffs`,
  `IBuffData.IsSameBuff`.
- The skills it stops: `FightSkill.OnChangeTeam`, `FightSkill.StopAttack`,
  `SkillManager.IsPermanentPreemptiveSkillActive`,
  `SkillIdleState.Enter`, `SkillAttackState.Update`.
- A turned unit's death: `FightTeamController.AddActor`, `FightTeam.AddMech`,
  `ExpSystem.AddRangeUnit`, `TeamTranslationSystem.OnMechDead`,
  `TeamTranslationSystem.IsMechInFightGroup`, `DeadEffectSystem.Update`.
- The search after it: `ScoreRatingTargetSelector.TrySelect`,
  `MainSkillSearchTargetController.PrepareSearch`.
- The motion around it: `MotionAttackState.Update`,
  `MotionAttackState.AttackRotate`, `SkillIdleState.CanStartSearchTarget`.

### Not established

- **The turned unit's own skills.** Nothing the build runs on a turn reaches
  the turned unit's skills: only the skills locked on it are subscribed. Its
  own, the main skill and each extra skill, read as if `FightSkill.OnChangeTeam`
  had run on each, cooling where it cools and idle and searching where it does
  not; which method does it is not read.
- **The weapons at the fight's end.** That the end clears what an idle skill
  names is recorded where a turned unit's death ends the fight; which method
  clears it is not read.
- **A turned unit with a shield of its own side's.** A unit carrying a Barrier
  that is turned, or one turned inside its old side's shield, is not recorded;
  `AdvancedEnergyShieldSystem` listens for a change of side.
- **Technology.** No technology of the Hacker's is read.
