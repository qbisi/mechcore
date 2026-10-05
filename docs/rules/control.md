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
and its splash nothing either.

## Progress and the turn

`ControllEffect.Start`, as the beam's attack begins or changes target, adds the
skill to its target's entry in `TeamTranslationSystem.translatingDatas`, which
keeps its entries in the order they were added; `ControllEffect.Stop` takes it
away, and the entry goes with its last skill. Each hit that turns adds its
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
- **moves it** to the other side's lists (`FightActor.ChangeTeam`), at
  their end (`FightTeamController.AddActor`): it updates after every unit
  already on that side, its summons among them, and a unit handed back on
  its death after every unit of the side it returns to. It leaves
  its formation for one of its own on the new side, made when first asked for
  (`FightMech.GetMechTeam`): the units turned on one tick take theirs in
  identity order. Its damage and its kills from then are recorded under a
  recorder of its own. A summon, a Spider Mine among them, has no `MechTeam`
  to leave: it is recorded under the formation it was made with, before the
  turn and after.
- **stops every skill locked on it** (`FightSkill.OnChangeTeam`): `StopAttack`
  drops the lock and keeps the attack target. A skill idle already stays idle,
  its weapons naming what they named until it takes another; a cooling one
  goes on cooling. Any other enters its cooling, naming what it fired at, or
  without one enters `SkillIdleState` with its targets cleared. The motion is
  left alone. The turned unit's own skill ends the same way: the beam's change
  runs before any unit updates, so a skill sent cooling is updated in it on
  the same tick.

A side whose every unit a beam has turned has none left, and loses as a
wiped-out side does: its towers are torn down.

## A turned unit's death

A hit that kills a turned unit only queues it with `DeadEffectSystem`, whose
update comes after every unit and projectile: what it took until then is
counted under its own recorder. Its `OnDead` then hands it back to the side it
was deployed on (`TeamTranslationSystem.OnMechDead`), and every skill still
locked on it stops its attack as above. A unit that updated after the blow
has met the death in its own update already. A summon stands in no side's
`FightTeam` (`TeamTranslationSystem.IsMechInFightGroup`), so it is not handed
back: it dies on the side that turned it, and the skills locked on it hold
their lock on it until they next update.

When the fight ends there, the weapons of the units left name nothing.

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

- The Hacker turns a Crawler whose progress reaches its life on tick 103, cools
  for a tick naming it, and searches again; the turned Crawler takes a new lock
  on that tick: `tests/hacker/fights/m3-crawler-4242.yaml`, ticks 101 to 105.
- A turn with one Hacker hands its formation the Crawler's 4 and the whole
  assist pool, 8 in all; with two, 4 to each formation in range:
  `tests/hacker/fights/m3-crawler-4242.yaml`, tick 103;
  `tests/hacker/fights/m6-formations-4242.yaml`, tick 97.
- A turned Crawler dies on its new side: the Crawlers that struck it in their
  attack state name nothing from that tick, those idle on it keep naming it,
  one that updated after the blow searches only on the next tick, and its last
  blow is counted under its own recorder:
  `tests/hacker/fights/m3-crawler-4242.yaml`, ticks 125 and 126.
- An idle turned Crawler whose lock is the Hacker searches at once:
  `tests/hacker/fights/m6-formations-4242.yaml`, tick 61. A turned Crawler
  caught in its backswing goes idle as it turns, turns to its new lock and
  starts its attack ten ticks on: `tests/hacker/fights/m3-crawler-4242.yaml`,
  ticks 263 to 273. A Crawler whose target is turned turns to the next while
  idle and walks on it once it is out of range:
  `tests/hacker/fights/m3-crawler-1787720817.yaml`, ticks 199 to 203, and
  `tests/hacker/fights/m3-crawler-4242.yaml`, tick 126. A bodyless start fires
  a tick later: `tests/hacker/fights/m6-formations-4242.yaml`, tick 156.
- Two Hackers turned on one tick take their formations in identity order:
  `tests/hacker/fights/m6-formations-1787720817.yaml`, tick 189.
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

### Replayed

- A Hacker turns a Spider Mine, which stays in its formation, and its death
  is not handed back, so the Crawlers locked on it hold the lock for a tick:
  replay 2324_20260925--134259672 round 5, ticks 122 and 156, fought by the
  game with `scripts/corpus/match-replays.py`.
- A blue Crawler a Hacker turned updates after every red unit: replay
  2324_20260925--134259672 round 5, tick 161, fought by the game with
  `scripts/corpus/match-replays.py`.
- A red Crawler whose prepared winner, its own lock, was turned searches
  again with a `Select` and takes the nearer of two blue Crawlers as they
  stand after blue's update: replay 2324_20260925--134259672 round 5, tick
  210, fought by the game with `scripts/corpus/match-replays.py`.

### Read

- The two effects and their damage: `FightControllBeamSkill.GetAttackEffect`,
  `FightControllBeamSkill.GetDamage`, `FightControllBeamSkill.DAMAGE_MODIFIER`,
  `ControlBeamDamageCalculator.GetAttackDamage`, `FightMech.IsSimulateMech`,
  `TeamTranslationSystem.IsIgnoredMech`, `IIgnoreBuffDataSouce.IgnoreControllerBeam`.
- Progress: `ControllEffect.Start`, `ControllEffect.Stop`,
  `ControllEffect.Perform`, `TeamTranslationSystem.Add`,
  `TeamTranslationSystem.Remove`, `TeamTranslationSystem.Translate`,
  `TeamTranslationSystem.translatingDatas`.
- The turn: `TeamTranslationSystem.Update`, `TeamTranslationSystem.ChangeTeam`,
  `FightActor.ChangeTeam`, `FightTeamController.AddActor`, `ExpSystem.CalculateExp`, `ExpSystem.IsValidOwner`,
  `TeamTranslationSystem.IsTranslatedMech`, `FightMech.GetMechTeam`.
- The skills it stops: `FightSkill.OnChangeTeam`, `FightSkill.StopAttack`,
  `SkillIdleState.Enter`, `SkillAttackState.Update`.
- A turned unit's death: `TeamTranslationSystem.OnMechDead`,
  `TeamTranslationSystem.IsMechInFightGroup`, `DeadEffectSystem.Update`.
- The search after it: `ScoreRatingTargetSelector.TrySelect`,
  `MainSkillSearchTargetController.PrepareSearch`.
- The motion around it: `MotionAttackState.Update`,
  `MotionAttackState.AttackRotate`, `SkillIdleState.CanStartSearchTarget`.

### Not established

- **The turned unit's own skill.** Nothing the build runs on a turn reaches
  the turned unit's skills: only the skills locked on it are subscribed. Its
  own reads as if `FightSkill.OnChangeTeam` had run on it, cooling where it
  cools and idle and searching where it does not; which method does it is not
  read.
- **When a turned unit's formation is made.** That the recorder makes it, in
  identity order, is inferred from one recording of two turns on one tick.
- **The weapons at the fight's end.** That the end clears what an idle skill
  names is recorded where a turned unit's death ends the fight; which method
  clears it is not read.
- **A turned unit with a shield of its own side's.** A unit carrying a Barrier
  that is turned, or one turned inside its old side's shield, is not recorded;
  `AdvancedEnergyShieldSystem` listens for a change of side.
- **Technology.** No technology of the Hacker's is read.
