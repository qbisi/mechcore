# What a turret does

[简体中文](turrets.zh.md)

This document is pinned to game build 2259. It states what a turret does once it stands:
when it locks a target, when it fires, how often, at what, and what its shot
does. What a turret *is* — one object, 3650 or 5028 of life in a 24 × 24 box —
is [`constructions.md`](constructions.md)'s.

The numbers are in [`config/constructions.yaml`](../../config/constructions.yaml):
the construction row carries the damage, the attack angle, the rotate speed and
the radius, and its `skills` section carries the `ProjectileSkillData` row each
turret's `skill_id` names, in the shape a unit's `attack` has.
`scripts/extract-constructions.py` writes both. It reads the skill rows out of
`level0` with `scripts/extract-skills.py`, which checks itself against the
Marksman's row first.

| | Rapid-Fire Turret | Anti-Armor Turret |
| --- | ---: | ---: |
| skill | 3003001 | 3002001 |
| damage, splash | 82, 10 m | 2748, 5 m |
| attack range | 115 | 125 |
| interval, random | 0.3 s ± 0.1 (6 ± 1 ticks) | 2.5 s ± 0.2 (50 ± 4 ticks) |
| magazine, reload | 10 rounds, 2.5 s (50 ticks) | 6 rounds, 10 s (200 ticks) |
| bullet speed | 400 | 300 |
| attack angle, rotate speed | 20°, 120°/s | 20°, 120°/s |
| targets | ground | ground |

Every prepare, attack point, backswing, cooling and initial cooldown is zero.

The rules were measured against four fights in `tests/turret/`.
`rapid-fire-head-on.yaml`, `rapid-fire-flank.yaml` and `anti-armor-head-on.yaml`
are recorded by `skill.mcscript`, with a control recording that repeats the
head-on fight exactly. `anti-armor-arclights.yaml` is recorded by
`arclights.mcscript`, twice. It puts the Anti-Armor Turret through a reload in
a fight no tower falls in, because the Crawlers of `anti-armor-head-on.yaml`
take a tower at tick 509, and what losing a tower does to its side is a
mechanism of its own. The build was read at `FightConstruction`,
`ConstructionSearchTargetController`, `FightSkill`, `SkillIdleState`,
`SkillAttackState`, `SkillReloadingState`, `SkillAttackableChecker` and
`SkillAttackAngleChecker`.

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

The simulator updates a turret after every unit of its side. The three fights
do not separate that from the other order: no unit of the turret's side acts
in them. The order of the deployment draw is measured, and it is below.

## It locks through the skill's search, even out of reach

The skill searches as a unit's main skill does. `SkillIdleState` searches when
it has no lock, when its lock is dead, or when its search timer has run out. It
uses the unit's selector: the other side's candidates in the order the target
trees hold them, positions as they stood at the tick's start, scored by edge
distance weighted by the angle from where the **weapon** points, with the
out-of-range penalty. A turret therefore holds a lock before anything is in
reach, and turns its weapon onto it.

The search timer is a unit's skill's, ten updates. A search resets it, and so
does leaving the reload; idle counts it down, and a live lock is kept. The
attack never searches on it. Whether the attack counts it down too, and whether
entering the attack resets it, these fights do not separate: they play back
tick for tick with both and with neither. The simulator does neither, as for a
unit.

- When the Rapid-Fire Turret's target dies, the next target is the one the
  selector scores from the weapon's rotation, not the nearest. At tick 146 of
  the head-on fight it takes unit 25, 85.1 m away and 5° off the weapon's line,
  over unit 19, which is 81.7 m away but 11° off. The flank and Anti-Armor fights
  choose the same way.
- The 11th shot, after the reload, goes to the Crawler the 10th shot went to,
  although a search from the weapon's rotation at that tick would have chosen
  another. The reload's end reset the timer, and the lock was alive.

## It fires on the tick after its lock is in reach and in angle

`TryStartAttack` enters the attack when the lock is within reach and the weapon
is within the attack angle of it. Reach is edge to edge: the attack range plus
the turret's 12 m radius plus the target's. A state is not updated on the tick
it is entered, so the first shot leaves on the tick after. From there a shot is
due once the interval since the last one is up and the weapon still faces the
target.

| Fight | First tick a Crawler starts within reach | First shot |
| --- | ---: | ---: |
| rapid-fire-head-on | 90 (115.4 m at the end of 88, 114.6 at the end of 89) | 91 |
| rapid-fire-flank | 86 (115.5 m, then 114.7) | 87 |
| anti-armor-head-on | 77 (125.3 m, then 124.5) | 78 |
| anti-armor-arclights | 166 (125.0 m, just outside, then 124.65) | 167 |

The flank fight's Crawlers come in 36.6° off the turret's resting line, and its
first shot is as early, from reach, as the head-on one. That is not because
the angle is not checked; `SkillAttackAngleChecker` checks it for a construction
as for a unit, and only an attack angle of 360° or more turns it off. It is
because the weapon has been turning onto its lock since before the lock was in
reach.

## The weapon turns at the construction's rotate speed

The turret's skill has weapon mode `Standalone` (2), which gives its weapon a
transform of its own. `FightSkill.Update` turns it towards the lock by at most
the rotate speed times the tick, 120°/s or 6° a tick, after the state has
updated, so a tick's angle check sees the rotation of the tick before. Blue's
weapon rests at 0° and red's at 180°, as a side's units face.

## Each shot draws its interval from the side's stream

The interval is drawn from the owning side's random stream, the one a unit's
stagger comes from: once when the skill enters the fight, and once at every
shot. A turret draws after every unit of its side. In both Rapid-Fire fights
the gaps between shots are 7 6 6 6 6 7 6 5 6, then 53, then 6 6 6 6 5 7. The
Anti-Armor Turret's are 49 48 52 50 50 against the Crawlers, and 49, 52, 50
and 52 between its two shots at each Arclight. Each is blue's stream seeded
`(round + team) × 4444`, past the Marksman's draw of range 12 and the turret's
own draw at deployment.

That stream's range projection masks with every bit up to the range's highest.
For a range that is itself a power of two it keeps that bit. The Rapid-Fire
Turret's 6 ± 1 ticks is a range of two, and a mask that drops its top bit never
draws the 7s. No unit's stagger is such a range, so the projection had not been
tested there before.

## It fires from a magazine, and a reload keeps the lock

A skill of the loading type starts the fight full and spends a round with every
shot. On the update after its last round, the attack enters
`SkillReloadingState`, which keeps the lock, counts the reload in whole ticks,
refills, and hands the skill to idle. From there the idle state enters the
attack again, and the attack fires the tick after. The last round's interval is
drawn as every other shot's is; the reload draws nothing. For the Rapid-Fire
Turret that makes the 53-tick gap:

| Tick | What happens |
| ---: | --- |
| 146 | the 10th shot, and the magazine is empty |
| 147 | the attack enters the reload |
| 148–197 | 50 ticks of reload; the last refills and returns to idle |
| 198 | idle enters the attack |
| 199 | the 11th shot |

The Anti-Armor Turret does the same with six rounds and 200 ticks: its sixth
shot at 574 empties the magazine, and its seventh leaves at 777.

## A lock that dies is replaced on the spot

`SkillAttackableChecker` asks for a new lock on the update it sees the old one
dead, and the skill, which switches targets quickly, goes on attacking. A lock
that walks out of reach is searched again the same way; one still in reach but
outside the weapon's attack angle finishes the attack, as a unit's does. If
nothing is found in reach, the attack finishes and the skill is idle without a
lock. No shot in
the three fights waits for a switch: every gap between shots is its drawn
interval.

## The shot is a projectile the building fires

The shot leaves from the turret's centre at ground height and moves on the tick
it is released, as a unit's does. It flies at the bullet speed, 20 m a tick for
the Rapid-Fire Turret and 15 for the Anti-Armor. It deals the construction's
damage with the skill's splash. Its source, in every event and in each death it
causes, is the building. A Rapid-Fire shot released at 91 lands at 98 and deals
82 to each of the eleven Crawlers its 10 m splash reaches. An Anti-Armor shot
released at 78 lands at 87 on three Crawlers at 263 each, which is their life,
not the 2748.

## A building that was the lock is held like a unit

A Crawler whose own lock was the turret, swinging at it when it falls, reads
idle on that tick. It stays on the fallen turret until its swing is over and then
looks for the next target, as it would for a unit that died. That is
unlike a wall block that stood in the way of another lock, which keeps a Rhino
attacking to the end of its swing ([`constructions.md`](constructions.md)). At
tick 335 of the Anti-Armor fight, the four Crawlers mid-swing go idle holding
building 3, and the one that was not mid-swing drops it and walks on. That
fight is recorded but not pinned: the simulator agrees with it through tick
508, and at 509 the tower-loss debuff begins.

## Scope

Everything above is build 2259, the 1v1 board, round one, and the two turrets
a layout can place. It covers them firing at ground units, with every timing
the rows carry at zero. It holds the simulator to both Rapid-Fire fights,
and to the Anti-Armor Arclight fight, physics and content. It agrees with the
Anti-Armor Crawler fight through tick 508, which is recorded and not pinned.

It does not cover:

- **The Anti-Armor Crawler fight after tick 508.** There the Crawlers destroy
  blue's Energy Tower with the Marksman still standing, and the game weakens
  the Marksman for it, which is not a turret's mechanism. The Arclight
  fight asks the Anti-Armor Turret the same without a tower falling.
- **A turret beside an officer or a unit technology.** Whether either reaches a
  construction's skill is not read, so a side that places a turret and carries
  either is refused.
- **Tower buffs on a turret.** A row may say `can_be_effected_by_tower_buff`, and
  a side with energy-tower skills or strengthened towers is refused before
  that matters.
- **A construction skill with a wind-up, a swing, a cooling, a burst or a
  scattered target.** The two turrets have none. A row that has one is refused
  by name.
- **A turret against air.** Both rows target ground only, so an air unit is not
  a candidate. That is the rows' rule, not a gap.
- **The Magnetic Barrier.** It is refused for where its objects stand, as
  [`constructions.md`](constructions.md) says, before its skill is asked about.
- **What the construction's own lock is for.** The build keeps it beside the
  skill's and nothing read here aims or fires from it.
