# What a turret does

What a turret does once it stands: when it locks a target, when it fires, how
often, at what, and what its shot does. What a turret *is*, one object with its
own life and box, is [constructions.md](constructions.md)'s.

The numbers are in [`config/constructions.yaml`](../../config/constructions.yaml):
the construction row carries the damage, the attack angle, the rotate speed and
the radius, and its `skills` section carries the `ProjectileSkillData` row each
turret's `skill_id` names, in the shape a unit's `attack` has: range, interval
and its random part, magazine and reload, bullet speed, splash and targets.
`scripts/extract-constructions.py` writes both from the build's typed export,
the skill rows through `scripts/extract-skills.py`. For the Rapid-Fire Turret
and the Anti-Armor Turret, the two a layout can place, every prepare, attack
point, backswing, cooling and initial cooldown is zero, and both target ground
only.

The fights are in `tests/turret/`. `rapid-fire-head-on.yaml`,
`rapid-fire-flank.yaml` and `anti-armor-head-on.yaml` are recorded by
`skill.mcscript`, with a control recording that repeats the head-on fight
exactly. `anti-armor-arclights.yaml`, recorded by `arclights.mcscript`, puts the
Anti-Armor Turret through a reload in a fight no tower falls in.

## A turret is a building that owns a unit's skill

`FightConstruction.Update` runs its `ConstructionSearchTargetController`, then
its `SkillManager`, the same manager a `FightMech` updates, and nothing else:
no motion, no rotation of its own body. The skill is a `FightProjectileSkill`
with the states a unit's skill has, idle, attack and the rest, plus
`SkillReloadingState`. The construction's own search controller is what makes
the skill search at all: a construction whose row answers
`IsEnableSearchTarget` with false has none, and its skill never looks for a
target. The lock that controller keeps is not the one the turret aims or fires
at; the skill keeps its own.

The simulator runs a unit's skill code for it. What a construction answers
differently reaches that code only through what the build's `ISkillOwner` and
`IAttacker` ask: where it stands, its radius and reach, that its attack angle
is measured against its weapon, its rotate speed, and whether it searches. Its
`MotionController` is never updated, so it reads idle.

The simulator updates a turret after every unit of its side. The fights here do
not separate that from the other order: no unit of the turret's side acts in
them.

## It locks through the skill's search, even out of reach

The skill searches as a unit's main skill does. `SkillIdleState` searches when
it has no lock, when its lock is dead, or when its search timer has run out. It
uses the unit's selector: the other side's candidates in the order the target
trees hold them, positions as they stood at the tick's start, scored by edge
distance weighted by the angle from where the **weapon** points, with the
out-of-range penalty. A turret therefore holds a lock before anything is in
reach, and turns its weapon onto it.

The search timer is a unit's skill's. A search resets it, and so does leaving
the reload; idle counts it down, and a live lock is kept. The attack never
searches on it. Whether the attack counts it down too, and whether entering the
attack resets it, these fights do not separate: they play back tick for tick
with both and with neither. The simulator does neither, as for a unit.

When a Rapid-Fire Turret's target dies, the next target is the one the selector
scores from the weapon's rotation, not the nearest: in the head-on fight it
takes a Crawler farther away but nearer the weapon's line over a nearer one
further off it. After a reload the next shot goes to the Crawler the last one
went to, although a search from the weapon's rotation at that tick would have
chosen another: the reload's end reset the timer, and the lock was alive.

## It fires on the tick after its lock is in reach and in angle

`TryStartAttack` enters the attack when the lock is within reach and the weapon
is within the attack angle of it. Reach is edge to edge: the attack range plus
the turret's radius plus the target's. A state is not updated on the tick it is
entered, so the first shot leaves on the tick after the first tick a target
starts within reach. From there a shot is due once the interval since the last
one is up and the weapon still faces the target.

The flank fight's Crawlers come in well off the turret's resting line, and its
first shot is as early, from reach, as the head-on one. That is not because the
angle is not checked; `SkillAttackAngleChecker` checks it for a construction as
for a unit. It is because
the weapon has been turning onto its lock since before the lock was in reach.

## The weapon turns at the construction's rotate speed

The turret's skill has weapon mode `Standalone`, which gives its weapon a
transform of its own. `FightSkill.Update` turns it towards the lock by at most
the construction's rotate speed times the tick, after the state has updated, so
a tick's angle check sees the rotation of the tick before. Blue's weapon rests
facing red's side and red's facing blue's, as a side's units face.

## Each shot draws its interval from the side's stream

The interval is drawn from the owning side's random stream, the one a unit's
stagger comes from: once when the skill enters the fight, and once at every
shot. A turret draws after every unit of its side.

That stream's range projection masks with every bit up to the range's highest.
For a range that is itself a power of two it keeps that bit. The Rapid-Fire
Turret's interval has a random part whose range is two, and a mask that drops
its top bit would never draw its longest gaps; the recorded gaps include them.

## It fires from a magazine, and a reload keeps the lock

A skill of the loading type starts the fight full and spends a round with every
shot. On the update after its last round, the attack enters
`SkillReloadingState`, which keeps the lock, counts the reload in whole ticks,
refills, and hands the skill to idle. From there the idle state enters the
attack again, and the attack fires the tick after. The last round's interval is
drawn as every other shot's is; the reload draws nothing. So the gap across a
reload is the reload's ticks plus three: the tick the attack enters the reload,
the tick idle enters the attack, and the tick the shot leaves.

## A lock that dies is replaced on the spot

`SkillAttackableChecker` asks for a new lock on the update it sees the old one
dead, and the skill, which switches targets quickly, goes on attacking. A lock
that walks out of reach is searched again the same way; one still in reach but
outside the weapon's attack angle finishes the attack, as a unit's does. If
nothing is found in reach, the attack finishes and the skill is idle without a
lock. No shot in these fights waits for a switch: every gap between shots is its
drawn interval.

## The shot is a projectile the building fires

The shot leaves from the turret's centre at ground height and moves on the tick
it is released, as a unit's does. It flies at the bullet speed and deals the
construction's damage with the skill's splash, capped at what each target has
left. Its source, in every event and in each death it causes, is the building.

## A building that was the lock is held like a unit

A Crawler whose own lock was the turret, swinging at it when it falls, reads
idle on that tick. It stays on the fallen turret until its swing is over and
then looks for the next target, as it would for a unit that died. That is
unlike a wall block that stood in the way of another lock
([`constructions.md`](constructions.md)). In the Anti-Armor fight the Crawlers
mid-swing when the turret falls go idle holding it, and one that was not
mid-swing drops it and walks on.

## Scope

The fights are on the 1v1 board, round one, with the two turrets a layout can
place firing at ground units, every timing the rows carry at zero.

## Evidence

### Recorded

Each holds in both Rapid-Fire fights and both Anti-Armor fights, physics and
content, as `tests/turret/regressions.mcscript` replays them.

- A turret locks through its skill's search before anything is in reach, and
  turns its weapon onto the lock: `tests/turret/regressions.mcscript`.
- A dead target is replaced by the one the selector scores from the weapon's
  rotation, not the nearest: `tests/turret/regressions.mcscript`.
- A reload keeps the lock, and the next shot goes to it: `tests/turret/regressions.mcscript`.
- The first shot leaves on the tick after a target starts within reach, reach
  measured edge to edge: `tests/turret/regressions.mcscript`.
- The weapon turns at the construction's rotate speed after the state has
  updated: `tests/turret/regressions.mcscript`.
- Each shot draws its interval from the owning side's stream, after every unit
  of the side, including the top bit of a power-of-two range:
  `tests/turret/regressions.mcscript`.
- The gap across a reload is the reload's ticks plus three:
  `tests/turret/regressions.mcscript`.
- The shot is a projectile from the turret's centre, at the bullet speed, with
  the construction's damage and the skill's splash, and the building is its
  source: `tests/turret/regressions.mcscript`.
- A unit whose lock was the fallen turret stays on it through its swing and
  then looks for the next target: `tests/turret/regressions.mcscript`.

### Read

- A construction updates its search controller and then its skill manager, and
  nothing else: `FightConstruction.Update`.
- Whether a construction searches at all is its row's:
  `ConstructionData.IsEnableSearchTarget`.
- The idle state searches without a lock, on a dead one, or when its timer has
  run out: `SkillIdleState.TrySearchLockTarget`, `SkillIdleState.TryStartAttack`.
- The weapon turns towards the lock after the state has updated:
  `FightSkill.Update`.
- The attack angle is checked for a construction as for a unit:
  `SkillAttackAngleChecker.IsActorInAttackAngle`.
- A dead lock is searched again inside the attack: `SkillAttackableChecker.Check`,
  `SkillAttackableChecker.CheckWhenLoseTarget`.

### Not established

- **A turret beside an officer or a unit technology.** Whether either reaches a
  construction's skill is not read, so a side that places a turret and carries
  either is refused.
- **Tower buffs on a turret.** A row may say `can_be_effected_by_tower_buff`,
  and a side that loses a tower while such a turret stands is refused when it
  happens ([`towers.md`](towers.md)); no recording has one.
- **A construction skill with a wind-up, a swing, a cooling, a burst or a
  scattered target.** The two turrets have none. A row that has one is refused
  by name.
- **The Magnetic Barrier.** It is refused for where its objects stand, as
  [`constructions.md`](constructions.md) says, before its skill is asked about.
- **What the construction's own lock is for.** The build keeps it beside the
  skill's and nothing read here aims or fires from it.
- **Whether the attack counts the search timer down, or entering it resets it.**
  These fights play back the same either way.
- **Whether an attack angle of 360° or more turns the angle check off.** It did
  before `SkillAttackAngleChecker` was rewritten, and the rewrite is not re-read
  for it.
