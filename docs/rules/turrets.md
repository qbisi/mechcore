# What a turret does

What a turret does once it stands: when it locks a target, when it fires, how
often, at what, and what its shot does. What a turret *is*, one object with its
own life and box, is [constructions.md](constructions.md)'s.

The numbers are in [`config/constructions.yaml`](../../config/constructions.yaml):
the construction row carries the damage, the attack angle, the rotate speed and
the radius, and its `skills` section carries the `ProjectileSkillData` row each
turret's `skill_id` names, in the shape a unit's `attack` has: range, interval
and its random part, magazine and reload, bullet speed, splash and targets.
`scripts/extract/extract-constructions.py` writes both from the build's typed export,
the skill rows through `scripts/extract/extract-skills.py`. For the Rapid-Fire Turret
and the Anti-Armor Turret, the two a layout can place, every prepare, attack
point, backswing, cooling and initial cooldown is zero, and both target ground
only.

The fights are in `tests/turret/`: `rapid-fire-head-on.yaml`,
`rapid-fire-flank.yaml` and `anti-armor-head-on.yaml`, and
`anti-armor-arclights.yaml`, which puts the Anti-Armor Turret through a reload
in a fight no tower falls in, `anti-armor-beside-officers.yaml`, the same
fight with officers and a technology on the turret's side, and
`laser-fells-turret.yaml`, where Steel Balls' beams take the turret. The same fight
recorded twice is one recording.

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
trees hold them, scored by edge
distance weighted by the angle from where the **weapon** points, with the
out-of-range penalty. A turret therefore holds a lock before anything is in
reach, and turns its weapon onto it.

**A turret searches where its candidates stand when it searches.** A unit's
search is prepared on the positions of the tick's start; a turret's never is,
since `FightCoreSystem.PreCalculate` prepares the skills of the fight's units
alone. It scores the candidates where they stand by the time it updates, after
the units of the sides that update before its own have moved: red's
Anti-Armor Turret locks a Hound that has walked 0.5 metres into its 125
metres of reach during the tick, where its position at the tick's start
leaves it 0.33 out.

The search timer is a unit's skill's. A search resets it, and so does leaving
the reload; idle counts it down, and a live lock is kept. The attack never
searches on it. Whether the attack counts it down too, and whether entering the
attack resets it, these fights do not separate: they play back tick for tick
with both and with neither. The simulator does neither, as for a unit.

A candidate in reach but more than the attack angle off the weapon takes the
out-of-range penalty too. `CalculateScore` adds it to a candidate within
`maxAttackRange` whose side of the source rotation, the rotation less the
angle on the left and plus it on the right, `FightUtility.IsInRangeRotation`
puts outside `minRotation` to `maxRotation`, with `FPoint`'s tolerant
comparisons and a window that wraps through 0. For a unit's search the window
is `Angle0` to `Angle360`, which `CalculateScore` does not check. For a
turret's skill it is the weapon's rotation less and plus its attack angle of
20, at every rotation: the recorded arguments carry it on every search the
Rapid-Fire Turrets' skills made in replay 268447927 round 1, 70 of them with
the weapon at exactly 0 and the window 340 to 20, and the candidates in reach
they score are unpenalised up to 14.8 degrees off and penalised from 21.1. The
construction's own search, whose lock nothing aims or fires from, carries the
whole turn. `CalculateScore` checks a window only when it starts above
`Angle0` and ends below `Angle360`. Which code narrows the skill's window is
not read: `Selector.CalculateRotationData`, hooked, answers the whole turn
for the same searches.

An Anti-Armor Turret's skill keeps the window too, where its search is scored
on worker threads and the arguments are not seen: in replay 67152171 round 1
its weapon points at exactly 0 at tick 366, and its search passes over the
best-scored candidate, 22.4 degrees off, which the construction's own search
takes.

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

## As the fight starts it faces the target its selector scores best

As the fight starts, `BattleSystem.OnFightStart` turns each side's units and
then, through `TerritoryManager.RefreshConstructionDirection`, each
construction that does not block: `FightConstruction.UpdateRotation` sets it
to its side's attack facing, `PlayerTerritory.GetAttackFacing`, and then to
the direction `UnitDirectionCalculator.Calculate` finds. That gathers the
other side's targets, has the construction's skill choose one of them with a
`ScoreRatingTargetSelector`, the selector a search scores with
([combat.md](combat.md)), and faces where the chosen target stands; with none
chosen it keeps the attack facing. The score is taken from where the
construction stands and points, which is the attack facing, so the target is
the best scored, not the nearest: a turret faces a tower straight ahead rather
than a unit whose centre is nearer but whose edge is not, or which stands off
its line.

The targets are the other side's towers, the constructions it could fire at,
a turret but not a wall, and its old units, which a ground turret takes only
on the ground. `OldMechTargetFilter` passes a unit only when
`MechTeam.IsOldUnit`, which its round count makes true once the unit has
opened a round: `UnitSystem.OnEnterDeployment` counts each unit on the board
as a round opens, and a squad an officer delivers or a unit a snapshot
restores is counted as it arrives. A unit bought or taken as a reinforcement
during the round is not among them, wherever it stands, and in the first
round no unit is. A layout states each unit's `source`
([layout.md](../spec/document/layout.md#source-and-recovered)). Neither call turns
through `RotateTo` or `RotateWeaponTo`, so the weapon starts the fight facing
the same way, and a lock the turret then finds may already be in its attack
angle.

A squad an officer delivers as the round opens is among the old units, but
for this it stands at the origin, wherever it is moved. The selector scores
it there, at its distance from the map's centre and its angle off the attack
facing, and `CalculateMechDirection` turns towards the chosen target's
`FightTransform.recordPosition`, which the delivery leaves at the origin. A
turret that chooses it starts the fight facing the map's centre, whatever
stands on that line. A unit a snapshot restores, or one bought and moved, is
scored and faced where it stands.

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

## Officers and technologies do not reach a turret

A turret fires with its row's numbers whatever officers and technologies its
side holds, even one that corrects every unit, such as Advanced Offensive
Tactics. An officer or a technology is a data source the fight hands to units
by type: `BattleSystem.RecordFightEffect` passes it with a unit ID to
`FightEffectSystem.AddProviderDataSource`, and `TeamFightEffectManager` keeps it
for that ID and applies it to that type's units alone. A construction has an
effect manager of its own, which takes nothing but what
`FightEffectSystem.AddConstructionProviderDataSource` gives it: the
construction's own main skill, as `BattleSystem.OnPlayerReleaseConstruction`
places it. `FightConstruction` has no data modifier of its own either, and
nothing outside the fight's effects writes onto a `ConstructionElement`.

This covers the corrections an officer or a technology writes. A tower's loss
reaches a turret through its buff, which cuts its damage and raises what it
takes as it does a unit's, as [`towers.md`](towers.md#a-construction-takes-the-loss-too)
states; an energy tower's skills are a separate source.

## A building that was the lock is held like a unit

A Crawler whose own lock was the turret, swinging at it when it falls, reads
idle on that tick. It stays on the fallen turret until its swing is over and
then looks for the next target, as it would for a unit that died. That is
unlike a wall block that stood in the way of another lock
([`constructions.md`](constructions.md)). In the Anti-Armor fight the Crawlers
mid-swing when the turret falls go idle holding it, and one that was not
mid-swing drops it and walks on.

A beam is the same: the Steel Ball whose beam fells the turret it locked reads
idle on that tick, as one whose beam fells a tower does. What decides it is
that the fallen building was the lock, not what kind of building it is; a
block the beam fells while it only stood in the way of the lock leaves the
Steel Ball attacking until the next tick. A beam that kills the unit it locked
leaves it attacking on that tick too, as a blow does.

## Scope

The fights are on the 1v1 board with the two turrets a layout can place firing
at ground units, every timing the rows carry at zero: round one, and a corpus
round in which a turret that has stood since round one faces the other side's
legacy units.

## Evidence

### Recorded

- A turret searches on the positions the tick has moved its candidates to:
  `tests/corpus/268477093-r2.yaml`, tick 122.

Each holds in both Rapid-Fire fights and both Anti-Armor fights, physics and
content, as `tests/turret/` replays them.

- A turret locks through its skill's search before anything is in reach, and
  turns its weapon onto the lock: `tests/turret/`.
- A dead target is replaced by the one the selector scores from the weapon's
  rotation, not the nearest: `tests/turret/`.
- A candidate in reach more than the attack angle off the weapon takes the
  out-of-range penalty, at every rotation of the weapon:
  `tests/corpus/268447927-r1.yaml`,
  `tests/corpus/67152171-r1.yaml`.
- A reload keeps the lock, and the next shot goes to it: `tests/turret/`.
- The first shot leaves on the tick after a target starts within reach, reach
  measured edge to edge: `tests/turret/`.
- The weapon turns at the construction's rotate speed after the state has
  updated: `tests/turret/`.
- A turret starts the fight facing the best scored of the other side's
  towers, turrets and old units, the units that side bought this round not
  among them: `tests/corpus/134270595-r3.yaml`, where red's faces a
  blue unit, and `tests/corpus/67158946-r3.yaml`, where blue's faces a
  red tower although an old red unit's centre is nearer, and red's faces
  blue's turret. With no old unit it faces a tower: `tests/turret/`.
- A squad an officer delivers as the round opens is scored and faced at the
  origin, wherever it is moved: red's turret at world (140, 100) faces 234.44
  degrees, and at world (-140, 100) faces 125.56, with nothing of blue's on
  either line; the same squad restored from a snapshot is faced where it
  stands: `tests/turret/anti-armor-faces-delivered-squad.yaml`,
  `tests/turret/anti-armor-faces-delivered-squad-moved.yaml`,
  `tests/turret/anti-armor-faces-delivered-squad-mirrored.yaml`,
  `tests/turret/anti-armor-faces-restored-squad.yaml`, and
  `tests/corpus/67152781-r3.yaml`, where the facing leaves the
  turret's first target out of its attack angle.
- Each shot draws its interval from the owning side's stream, after every unit
  of the side, including the top bit of a power-of-two range:
  `tests/turret/`.
- The gap across a reload is the reload's ticks plus three:
  `tests/turret/`.
- The shot is a projectile from the turret's centre, at the bullet speed, with
  the construction's damage and the skill's splash, and the building is its
  source: `tests/turret/`.
- A unit whose lock was the fallen turret stays on it through its swing and
  then looks for the next target: `tests/turret/`.
- A Steel Ball whose beam fells the turret it locked reads idle on that tick:
  `tests/turret/laser-fells-turret.yaml`.
- A Steel Ball whose beam kills the unit it locked reads attacking on that
  tick: `tests/corpus/201370830-r6.yaml` and
  `tests/corpus/201370830-r7.yaml`.
- A turret fires with its row's damage and reach beside Advanced Offensive
  Tactics, Advanced Targeting System and a unit technology on its side:
  `tests/turret/anti-armor-beside-officers.yaml`.

### Read

- Only the units' skills are prepared on the tick's query snapshot:
  `FightCoreSystem.PreCalculate`, `SkillStateController.PreCalculate`,
  `SuperDeploymentSystem.IsTravelling`. The `target_search` channel names
  every recorded search of a construction `select` or `select_job`, the paths
  that score as they run, and none `team`.

- A construction updates its search controller and then its skill manager, and
  nothing else: `FightConstruction.Update`.
- Whether a construction searches at all is its row's:
  `ConstructionData.IsEnableSearchTarget`.
- The idle state searches without a lock, on a dead one, or when its timer has
  run out: `SkillIdleState.TrySearchLockTarget`, `SkillIdleState.TryStartAttack`.
- The weapon turns towards the lock after the state has updated:
  `FightSkill.Update`.
- As the fight starts each construction that does not block is turned to its
  attack facing and then towards the target its skill's selector chooses
  from the other side's, or kept at its attack facing without one:
  `BattleSystem.OnFightStart`,
  `TerritoryManager.RefreshConstructionDirection`,
  `FightConstruction.UpdateRotation`, `PlayerTerritory.GetAttackFacing`,
  `UnitDirectionCalculator.Calculate`,
  `UnitDirectionCalculator.CalculateMechDirection`,
  `UnitDirectionCalculator.GetTargetsNormal`, `FightSkill.SelectTarget`,
  `ScoreRatingTargetSelector.Select`.
- The targets are the other side's fight groups' active actors that
  `OldMechTargetFilter` and `AttackTargetFilter` pass: a unit only when
  `MechTeam.IsOldUnit`, a construction only when it does not block, and each
  only when the construction could fire at it: `GroupManager.GetOpponentGroups`,
  `FightTeam.PrepareActors`, `OldMechTargetFilter.Check`,
  `AttackTargetFilter.Check`, `FightCalculator.IsValidTarget`.
- A unit's round count rises as each round opens, and as an officer's squad
  or a snapshot's unit arrives: `UnitSystem.OnEnterDeployment`,
  `MechTeam.AddRoundCount`, `UnitOfficerController.AddExtraUnit`,
  `PlayerSnapshotController.ApplyUnitSnapshot`. A snapshot's unit is counted
  as many times as the snapshot's `RoundCount` says, and a unit is old when
  its count is above zero: `MechTeam.IsOldUnit`. Every replay of this
  version's corpus writes `RoundCount` 0 for the advance team in round 1 and
  again in round 2, and 1 in round 3, so no unit is old in the first round;
  a layout replay writes 0 for every unit it restores.
- The fight-start direction is taken towards the chosen target's
  `FightTransform.recordPosition`: `UnitDirectionCalculator.CalculateMechDirection`.
  An officer's squad is added through a callback,
  `OfficerSystem.ActiveOfficerEffect` to `UnitOfficerController.AddExtraUnit`.
- The attack angle is checked for a construction as for a unit:
  `SkillAttackAngleChecker.IsActorInAttackAngle`.
- A dead lock is searched again inside the attack: `SkillAttackableChecker.Check`,
  `SkillAttackableChecker.CheckWhenLoseTarget`.
- An officer or a technology reaches the units of one type and never a
  construction: `BattleSystem.RecordFightEffect`,
  `FightEffectSystem.AddProviderDataSource`,
  `TeamFightEffectManager.AddProviderDataSource`,
  `TeamFightEffectManager.CreateConstructionEffectMananger`,
  `FightEffectSystem.AddConstructionProviderDataSource`,
  `BattleSystem.OnPlayerReleaseConstruction`.

### Not established

- **The first round's facing, in a recording.** A turret in round 1 does not
  turn to the other side's advance team: in the game its first search scores
  from 180°, its attack facing, where the same unit as an old unit of round 3
  turns it to 204°, read from the `target_candidate` instrument channel. No
  recording holds a construction's weapon or lock, and in every fight tried
  the weapon has turned onto its lock before the lock is in reach, so no pin
  holds it.

- **Which rotation the fight-start score is taken from.**
  `ScoreRatingTargetSelector` reads it through the attacker's
  `CalculateRotationData`, which is not read; the construction's weapon and
  body both point at the attack facing then, and the recordings agree with
  either.
- **What leaves a delivered squad at the origin.** Recorded, not read: which
  call sets `FightTransform.recordPosition` for a restored or bought unit and
  not for a delivered one, and whether the selector reads the same field.
  A unit's own fight-start facing towards a delivered squad is not recorded.
- **A construction whose attack facing is a quarter turn off its main
  facing.** `UnitDirectionCalculator.GetTargets` then looks first in a defense
  region, `GetTargetsInRegion`, which finds no tower. Every turret recorded
  stands outside the defense areas, where the two agree, so it is not
  implemented.
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
