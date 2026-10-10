# Battle skills

This index covers the position-targeted battle skills that affect combat and
can occur in standard 1v1 matches. Skills that require a unit or construction
target, and configurations absent from standard 1v1, are outside the layout
contract. A skill is a row of `CommanderSkillGroupData` in `level0`, whose
list says what kind of skill it is (`damageCommanderSkills`,
`supportUnitCommanderSkills`, `oilCommanderSkills` and the rest).

## ID and type

The layout names a skill by its `type`, not its native ID, and each type is
one ID: the skill a standard 1v1 side can hold. The build's other rows of the
same name, such as an Electromagnetic Blast whose cooldown is eight rounds
rather than four, reach no standard side and are in no table.

A skill reaches a side by one of two routes, and both name it by this ID. A
reinforcement card's own ID is the skill it grants, which
[reinforce_items.md](reinforce_items.md) sets out, and a blueprint names the
skill it activates in `grants_skill`, which
[`config/economy.yaml`](../../config/economy.yaml) states alongside what the
blueprint costs. A state names the same ID in `battle_skills`, so the panel,
the card and the blueprint speak one ID space. A skill's two cooldowns are
[`config/commander_skills.yaml`](../../config/commander_skills.yaml)'s.

| Layout `type` | ID | Positions | Geometry | Map rule |
| --- | ---: | ---: | --- | --- |
| `incendiary_bomb` | 100002 | 2 | line | overlap |
| `electromagnetic_impact` | 200001 | 1 | circle | overlap |
| `electromagnetic_blast` | 200002 | 1 | circle | overlap |
| `photon_emission` | 200003 | 1 | circle | overlap |
| `missile_strike` | 300001 | 1 | circle | overlap |
| `orbital_bombardment` | 300003 | 1 | random circle | overlap |
| `nuke` | 300004 | 1 | circle | overlap |
| `lightning_storm` | 300005 | 1 | random circle | overlap |
| `ion_blast` | 300006 | 2 | line | overlap |
| `orbital_javelin` | 300007 | 1 | circle | overlap |
| `heavy_missile_strike` | 300016 | 1 | circle | overlap |
| `sticky_oil_bomb` | 400002 | 2 | line | overlap |
| `acid_blast` | 500002 | 2 | line | overlap |
| `smoke_bomb` | 600002 | 2 | line | overlap |
| `shield_airdrop` | 800001 | 1 | circle | center |
| `underground_threat` | 1200001 | 1 | support circle | summon |
| `rhino_assault` | 1200002 | 1 | support circle | summon |
| `wasp_swarm` | 1200003 | 1 | support circle | summon |
| `mobilize_battleship` | 1200004 | 1 | support circle | summon |
| `vulcans_descent` | 1200005 | 1 | support circle | summon |
| `mobile_beacon` | 1500001 | 3 | path | contained |
| `mobile_beacon_card` | 1500002 | 3 | path | contained |

Mobile Beacon has two independent standard-1v1 sources: `1500001` is granted
by Research Center Blueprint 3, while `1500002` is granted by the ordinary
reinforcement card. Activating the Blueprint does not exclude the card, and the
native skill manager does not deduplicate them by ID or name, so both objects
may coexist. A document names them apart, as
[`config/names.yaml`](../../config/names.yaml) does, and the compiler maps each
name to its own ID. The two share one path and one cooldown.

Heavy Missile Strike (`300016`) is no card: Missile Specialist hands it out,
in place of the two Missile Strikes it once did. Its row is Missile Strike's in
every field that places or times it, and differs in damage alone.

## Effect geometry and target regions

A skill row's `effectRange` and `subEffectRange` are metres, and their meaning
depends on the geometry. `subEffectRange` is not a second, universally visible
effect radius.

- For a `circle`, `effectRange` is the effect's radius.
- For a `random circle`, `effectRange` is the outer distribution radius and
  `subEffectRange` is one sub-effect's radius.
- For a `line`, the two positions determine the length; native `LineRange`
  receives `subEffectRange` as its full width, so neither value is a radius.
- A `path` is Mobile Beacon's, not a damaging area.

The values are the row's after runtime preprocessing:
`CommanderSkillData.PreProcess()` rewrites every `CircleSingle` skill before use
by assigning `subEffectRange = effectRange` and `subEffectCount = 1`.

The map rules, the map being the board's 720 by 620 metre bound
([map.md](map.md#the-board)):

- `contained`: the complete path or effect footprint must remain inside the
  battlefield map.
- `center`: the release center must be inside the map; the effect edge may
  extend outside it.
- `overlap`: the effect footprint must intersect the map. The release center
  and even most of the effect may be outside it.
- `summon`: the release center must remain at least the skill's effective
  `subEffectRange` inside the map and must satisfy the tower exclusion below.

The support fight controller receives the effective `subEffectRange` as a spawn
`randomRange` only when the support configuration has `maxCount >= 2`;
single-unit support skills pass zero instead. Placement checks occur before
that fight-time branch and always read the effective, preprocessed value.

The native point-release check rejects a support skill when the Euclidean
distance from its requested release center to either enemy tower's map-bound
center is at most the tower's protection range
(`towerDefaultDatas.protectionRange`) plus the skill's effective
`subEffectRange`. Shield Airdrop uses the native `AreaExtendExcludeCrystal`
target type and the same threshold. These thresholds are derived from the
check, not measured against coordinate boundaries in the Training Ground.

## Position contract

Every coordinate component is an `i32`. Decimal values, including integral
spellings such as `10.0`, are invalid. Battle-skill positions are not unit
placements and do not inherit the unit grid-alignment or footprint rules.
The native skill check remains authoritative for target regions and other
skill-specific restrictions.

- A one-position skill uses one target position.
- A two-position skill uses `[start_position, end_position]`. The second value
  is the concrete skill endpoint, not a direction vector. Both order and
  distance are semantic.
- `mobile_beacon` uses three ordered positions. The first position selects the
  affected units and starts the first path segment; the second is the
  intermediate path endpoint; the third is the final endpoint.

The compiler requires the declared position count to equal the runtime skill's
`GetEffectPositionCount()`. It does not truncate, duplicate, coerce, or
synthesize coordinates.

## When a released skill lands

A released position skill other than Mobile Beacon is `CSRC_Common`: a
`CommanderSkillReleaseState` that prepares, then performs by dropping one
sub-effect on each position. Write `S` for the row's `startTime`, `T` for its
`subEffectMoveTime`, `v` for its `subEffectMoveSpeed`, and `s` and `m` for
`S` and `T` as whole ticks, each divided by `LogicDeltaTime` and truncated.
Count ticks from the fight's first update, tick 1.

- Preparing counts one a tick from tick 1 and hands over once its count
  reaches `s - m`, which is never before tick 1: it hands over on tick
  `P = max(1, s - m)`.
- Performing activates the first sub-effect on its first update, tick `P + 1`,
  after that tick's sub-effects have moved, so it moves from tick `P + 2`.
- A sub-effect starts `v × min(S, T)` above the height it lands at,
  `subEffectDefaultHeight`, and falls `v × LogicDeltaTime` a tick. It lands on
  the tick it reaches that height, and what it does is done on that tick.
  `LogicDeltaTime` is a hair under a twentieth of a second, so a fall of `n`
  whole twentieths takes `n + 1` ticks, whatever `v` is.

So the first sub-effect lands on tick `P + 1 + N`, `N` being the ticks its fall
takes: `s + 3` when `S` is no longer than `T`, and `s + 2` when it is longer.
The Electromagnetic Impact's row, read from
[`config/commander_skill_effects.yaml`](../../config/commander_skill_effects.yaml),
lands on tick `s + 3`.

A skill with several sub-effects activates the later ones as it performs.
Performing starts its count at `(S - T) / LogicDeltaTime`, cast to an integer
after the division rounds its magnitude, so -1.5 seconds counts from -31, and
adds one on every update. After that update's sub-effects have moved, it
activates the next once `s + I × k - m` is no later than its count, `I` being
the row's `subEffectIntervalTime` as whole ticks and `k` the number already
activated, and at most one an update. Each falls as the first does, from its
own activation. Orbital Bombardment's land 20 ticks apart from the first;
Ion Blast's second lands five ticks after its first and every later one six
after the one before.

## The Electromagnetic Impact

The Electromagnetic Impact and the Electromagnetic Blast are rows of
`buffCommanderSkills` that differ in their range alone; both write the same
buff. [`config/commander_skill_effects.yaml`](../../config/commander_skill_effects.yaml),
which `scripts/extract/extract-commander-skill-effects.py` reads out of
`CommanderSkillGroupData`, holds them with that buff.

Where its sub-effect lands it reaches every live unit of either side, the
releasing side's own included, ground and air alike, whose edge stands within
the skill's range of the landing point in the plane. The range is `effectRange`,
which preprocessing made the circle's one sub-effect's. Equal counts as within,
by `FPoint`'s tolerant comparison. A construction or a tower is never reached.
The units are taken side by side, blue's first, each side in the order its
target tree holds them, and the buff is written on each in that order.

It meets battlefield shields as a damage strike does, for its row does not
cross them. Its fall stops at the first shield it comes inside, and it
performs there, at the point its way met the shield's surface. It is a
damage modifier, so where it performs it strikes its circle as a damage
strike does, with no damage: a shield the circle reaches takes the row's
`energyShieldDamage`, 20000, added to that nothing. A unit one of its own
side's shields holds, by the shield holding its centre, is left out of the
units it reaches, though the circle reaches its edge; the list is taken
before the strike, so a shield the strike breaks still spares it.

The buff slows the unit by its `move_speed_rate` for its `duration`, and while
it runs the unit's technologies are off, whether it carries a technology or
not. A second
one on a unit it still runs on restarts it. A buff of divide 0 merges only
with its own row, so another divide-0 buff, a missile's slow among them, runs
beside it rather than merging.

A fight's buffs are cleared as the fight is left, on its last tick: after the
towers it tore down when a side's last unit fell, and after everything else
that tick did when a projectile landing after the last death decided it, or
landed after the towers fell. Until then, nothing updates them.

## Photon Emission

Photon Emission is a row of `buffCommanderSkills` with no fall: its
`subEffectMoveSpeed` and `subEffectMoveTime` are zero, so its one sub-effect
lands on tick `s + 2`, 14. Its buff, `40000`, takes 0.3 off the damage the unit
takes and makes it invincible for 20 seconds, so a debuff, the Electromagnetic
Impact's among them, does not reach it while the buff runs.

A sub-effect with no speed is never tested against a shield: it is finished
where it stands, without moving. Photon Emission released inside a shield
lands on tick 14 as it does with none, and writes its buff on the units the
shield holds as on any other.

Whose units a buff skill reaches turns on its buff (`BuffData.IsHarmful`). A
harmful one, which slows or raises the damage taken, as the Electromagnetic
Impact's does, reaches every unit in range of either side
(`PerformNegativeEffect`). Any other reaches the releasing side's units alone
(`PerformPositiveEffect`), in the order its target tree holds them: Photon
Emission's reaches no enemy, however near.

## A summon

A support skill, a row of `supportUnitCommanderSkills`, lands as any
`CSRC_Common` skill does; with no fall, its sub-effect lands on tick
`max(1, s) + 2`, and, with no speed, is never tested against a shield, so a
summon is made inside a shield of either side as anywhere else. Its landing hands the side a creator, which
[`config/commander_skill_effects.yaml`](../../config/commander_skill_effects.yaml)
reads off the row.

**Creation.** The creator updates later in the landing tick, after every
unit has updated and every projectile landed, and on each tick after. It
creates `createCountPerTime` summons on its first update and again every
`createInterval`, until it has made `maxCount`; one that dies is never
replaced. It lives until its updates reach `startTime` plus its creations
times their interval, and while it lives the fight cannot end.

**Where a summon stands.** A skill that summons one puts it exactly on the
release point. One that summons two or more moves each from the release point
by two draws of its side's stream, X and then Z, each a whole number of
hundredths uniform within the skill's `effectRange`, rounded to the nearest
raw unit; the draws of one summon come before the next's. A flying summon
stands at its flying height.

**What a summon is.** It is its side's unit at level 1, facing its side's way,
with no formation: it counts alone in the statistics, from the first hit it
counts, never takes experience, and takes no share of another's. Its side's
officers, technologies and Energy Tower skills write onto it what they write
onto a unit of its type deployed without equipment: the side registers them
by mech, and a summon's mech is its unit's, so the lookup that finds them for
a deployed unit finds them for the summon. **A summon that dies loses them**
as the tick's deaths are taken, after every shot of the tick has landed: a
shell it fired that lands later strikes for its unit's damage without its
side's corrections. A deployed unit is no summon, and keeps them.

**Appearing.** For a second after it is made, a summon is out of the fight: in
no tree, never updated, and not counted for its side, though a side with a
summon still appearing has not lost and the fight cannot end. Its movement
agent is there from the start, locked where it stands, and the units moving
around it turn aside, from the second solve on: the first after it is made
finds it at zero, as any new agent ([rvo.md](../spec/simulation/rvo.md)). It draws its skills' first intervals from its side's
stream when it joins, not when it is made.

**Joining.** At the start of the tick a second on, before anything updates,
the summon joins. The searches of that tick were prepared without it, so it is
found from the next, and its own was not prepared: it searches where
everything stands as it updates, and finds the summons of either side that
joined with it. Its agent, locked while it appeared, was handed no speed,
and keeps the place it has in the solver's tree. A solve on the tick it joins
gives it a speed only if entering a state handed it one first: a summon that
attacks at once is handed one by `StopMove` and is pushed off the summons it
overlaps, while one that sets off moving is handed its speed by `Move` only on
the update before the next solve, and moves from the solve after.

**An air drop.** A summon of `appearType` 2 that does not fly, the Rhino of
Rhino Assault or the Vulcan of Vulcan's Descent, deals its life, as it joins,
to every unit and construction whose edge its own collision radius covers, of
either side, ground and air, blue's first, in the order its side's tree holds
them. A tower it covers takes nothing: the drop is a special attack. The
hit has no owner: its damage and the deaths it causes are credited to no one,
under the summon's side, and a kill counts for the dead unit's enemies. The
summon then loses the life its hit took in all, with no rate on damage taken,
and counts that as taken.

## A damage strike

A damage skill that strikes one circle, a row of `damageCommanderSkills` with
`effectRangeType` 0 (Missile Strike, Heavy Missile Strike, Nuke, Orbital
Javelin), lands as any `CSRC_Common` skill does. Where it lands it deals its
row's `subEffectDamage` to every unit, of either side and either domain, and
every construction whose edge its `effectRange` reaches in the plane, in the
order each side's target tree holds them, as a hit with no owner: its damage
and the deaths it causes are credited to no one, under the releasing side, and
a kill counts for the dead unit's enemies.

A battle skill's strike and a summon's drop are special attacks
(`IDamageProvider.IsSpecialAttack`). Their range takes every actor of each
side's tree, towers included, but a special attack passes over each tower it
reached, and strikes a construction as any hit does, less the construction
row's `specialDamageReduceRate`, which every row holds at zero. A tower is so
never struck.
The hit meets shields as a splash does, which
[`contraptions.md`](contraptions.md) states.

A strike that does not cross shields (`isCrossAdvancedShield`) is tested as it
falls: on the first tick of its fall that it stands strictly inside an active
shield of either side, it strikes there instead of landing, at the point where
its way met the shield's surface, half a metre out; of several such shields,
the one its last point was nearest. `scope` and `isDirectHit` change nothing
in the fight.

## A scattered strike

A damage skill of `effectRangeType` 2, a random circle (Orbital Bombardment,
Lightning Storm), or 1, a line (Ion Blast), drops its row's `subEffectCount` sub-effects as the
section above times them, each striking as a damage strike does over its row's
`subEffectRange` about where it lands. Where they land is drawn as the fight
starts, release by release, before any unit draws its first interval:

- **A random circle** draws each sub-effect from its side's stream, an x
  within `r` of the release and then a z within `sqrt(r² - x²)`, `r` being
  the row's `effectRange` less its `subEffectRange`. Each draw is a whole
  number of tenths up to the range in tenths, rounded half to even, and
  stays strictly inside it; a tenth is `FPoint`'s truncated `0.1`. Each side's
  draws come from its own stream, and a line draws nothing.
- **A line** places them evenly from its first position to its second: the
  `k`-th the way there clamped to `k` times the length over `subEffectCount -
  1`.

A terrain skill's row, a line of `terrain_skills`, places and times its
sub-effects as a line strike does, and each leaves a terrain where it lands
rather than striking; one that falls inside a shield ends there and leaves
nothing: [`terrain.md`](terrain.md).

A strike whose row names a buff, Lightning Storm's slow, takes the units its
circle reaches as it lands, deals its damage, and then writes the buff on
those of them still alive. A unit struck again takes the buff again, as a
missile's slow does ([`contraptions.md`](contraptions.md)). The list leaves
out each unit one of its own side's shields holds, as an Electromagnetic
Impact's does, for both skills' sub-effects are one controller's.



A Shield Airdrop is a row of `energyShieldCommanderSkills`, in
[`config/commander_skill_effects.yaml`](../../config/commander_skill_effects.yaml).
It lands as any `CSRC_Common` skill does, and its sub-effect crosses every
shield on its way down, whatever its row says. Where it lands it stands a
battlefield shield of its side, on the ground, active and full: its radius is
the skill's `effectRange`, which preprocessing makes the one sub-effect's, and
its energy the row's `energy`. The shield takes the side's next shield
identity, joins after every shield the side holds, and is recorded
`shield_created` after everything else its tick did. From then on it is a
shield as a contraption's is, which [`contraptions.md`](contraptions.md) states.

A Shield Airdrop an earlier round left standing is a shield of its side from
the fight's first tick, full, sorted among the side's other shields by
`CompareEnergyShield` as they are.

## A Mobile Beacon

A Mobile Beacon lands nothing. It is `CSRC_WayPoint`, a row of
`wayPointCommanderSkills` in
[`config/commander_skill_effects.yaml`](../../config/commander_skill_effects.yaml),
and its three positions are a path of two segments, each as wide as the row's
`subEffectRange`, 40 m. As the fight starts, `CSRC_WayPoint.OnFightStart`
selects the releasing side's units whose bounds meet a circle of that width
around the first position, and `CSRC_WayPoint.AddMech` gives each one a
`MoveAttackCommand` through `PilotAI.SetCommand`, keeping where it stood from
the first position as its offset. A unit already walking a beacon of its own
side keeps that one. `startTime` and `effectRange` are the release's, and the
fight reads neither.

A command faces its unit along the first segment before the fight's presearch,
which scores from that facing. It then takes the place of the unit's
`AutoMoveBehaviour`:

- Its point, `MoveAttackCommand.RefreshCurrentTargetInfo`, is the segment's
  start moved by the offset, carried along the segment for its length and 20 m
  more. `MotionController.Move` steps towards it, stopping 20 m short, at the
  unit's own speed. A unit already within those 20 m steps 0.2 m towards it
  (`MotionController.MIN_MOVE_DISTANCE`). A Rhino on a beacon whose last two
  positions are one point is past that segment's point as it takes it, and
  turns back towards it.
- It is active with or without a lock. A unit whose target is out of range, or
  that has none, or whose target died out of range, walks towards the point
  rather than the lock. Where the default behaviour would go idle, the motion
  changes to moving, and it walks on while its skill cools or reloads.
- A unit with a target in range attacks it. `MoveAttackCommand.IsEnableAttackMove`
  decides whether it walks on while it fires. A melee unit stops. A ranged unit
  stops only for a live enemy both near what is left of its segment, within
  half the segment's width of it (`LineRange.Overlaps`) or the whole width of
  its end, and within its range edge to edge (`FightActor.Distance2D`), capped
  at 140 m; otherwise it keeps walking. Walking on, `MotionController.Move`
  does not turn its body, which `MotionAttackState.AttackRotate` turns to its
  target. A unit that fires all round and has no body, an Overlord or a Wraith,
  turns its root to where it moves rather than to its target.
- Under a command `MotionAttackState.Update` does not ask whether the lock
  lives: the command's `IsIdle` and `IsActive` are constants, and the attack
  ends only once its target is out of range. A lock that dies within range
  keeps the unit attacking, turning to the dead unit and walking on or
  stopping as above, until its skill takes another, a burst it still fires at
  the dead unit among it: the unit changes to moving only as the burst's last
  shot is out and the skill lets the target go. A skill whose attack
  ends without a lock cools naming what its last check found, and a target it
  names in range keeps the unit attacking through the cooling, walking on or
  stopping as above; a unit moving through such a cooling changes to
  attacking once what it names comes into range (`MotionMoveState.Update`
  asks `IsAttackTargetInAttackRange`), a state not updated the update it is
  entered. With no lock `MotionController.CalculateTargetDirection`
  answers the velocity, so the unit turns only to where it walks, and a unit
  standing still turns nothing; it changes to moving as the cooling ends and
  names nothing.
- An extra skill that holds the motion (`MotionController.attacker`) is what
  the command's motion asks for range, as the main skill is otherwise. A
  Rhino on a beacon whose spin starts, its main skill locked, changes to
  attacking that update when what the spin fires at is in the spin's range,
  stops (a melee unit does not walk on) and turns its root to it, and keeps
  attacking while it is in range, alive or not: `IsAttackTargetInAttackRange`
  asks the target's visibility and distance only. Out of range it walks
  towards the point.
- A command outlives a won fight. When the skill lets its target go as the
  fight stops, an attacking unit changes to moving, and a unit on the beacon
  goes on moving and turning to where it moves until the fight ends, which
  idles it ([combat.md](combat.md)).
- With no lock, `MotionMoveState.NormalRotate` turns its weapons to its
  velocity, and a unit standing still turns them nothing.

`PilotAI.Update`, after the unit's motion, moves the command on once the unit,
less its radius, is within 20 m of the point. After the last segment the command
ends and the unit returns to `AutoMoveBehaviour` where it stands. Nothing is
drawn from any stream, and no event is written.

## Names

<!-- names: commander_skills -->
| ID | English | Document name |
| ---: | --- | --- |
| 100002 | Incendiary Bomb | `incendiary_bomb` |
| 200001 | Electromagnetic Impact | `electromagnetic_impact` |
| 200002 | Electromagnetic Blast | `electromagnetic_blast` |
| 200003 | Photon Emission | `photon_emission` |
| 300001 | Missile Strike | `missile_strike` |
| 300003 | Orbital Bombardment | `orbital_bombardment` |
| 300004 | Nuke | `nuke` |
| 300005 | Lightning Storm | `lightning_storm` |
| 300006 | Ion Blast | `ion_blast` |
| 300007 | Orbital Javelin | `orbital_javelin` |
| 300016 | Heavy missile strike | `heavy_missile_strike` |
| 400002 | Sticky Oil Bomb | `sticky_oil_bomb` |
| 500002 | Acid Blast | `acid_blast` |
| 600002 | Smoke Bomb | `smoke_bomb` |
| 800001 | Shield Airdrop | `shield_airdrop` |
| 900001 | Field Recovery | `field_recovery` |
| 1000001 | Redeployment | `redeployment` |
| 1100001 | Intensive Training | `intensive_training` |
| 1200001 | Underground Threat | `underground_threat` |
| 1200002 | Rhino Assault | `rhino_assault` |
| 1200003 | Wasp Swarm | `wasp_swarm` |
| 1200004 | Mobilize Battleship | `mobilize_battleship` |
| 1200005 | Vulcan's Descent | `vulcans_descent` |
| 1500001 | Mobile Beacon | `mobile_beacon` |
| 1500002 | Mobile Beacon | `mobile_beacon_card` |
<!-- /names -->

## Evidence

### Recorded

- An Electromagnetic Impact lands on tick `s + 3`, writes its buff for its
  whole duration, and holds the unit's technologies off while it runs, on a
  unit with none: `tests/battle_skill/rhino-slowed.yaml`.
- It reaches a unit by its edge, whose centre stands beyond the range:
  `tests/battle_skill/reached-by-its-edge.yaml`.
- It reaches air units, and a fight a projectile's landing decided clears its
  buff on the last tick, after that projectile's removal:
  `tests/battle_skill/wasps.yaml`.
- It reaches the releasing side's own units, blue's before red's:
  `tests/battle_skill/own-side.yaml`.
- Photon Emission lands on tick 14, writes its buff on the releasing side's
  units in range and on no enemy, and its invincibility keeps an Electromagnetic
  Impact's debuff off them: `tests/battle_skill/photon-emission.yaml`,
  `tests/battle_skill/photon-emission-allies.yaml`.
- A random circle's sub-effects are drawn from its side's stream before any
  first interval, land `subEffectIntervalTime` apart and strike
  `subEffectRange` about where each lands; each side draws from its own
  stream, and a line's sub-effects stand evenly along it and land as the
  activation rule times them:
  `tests/battle_skill/orbital-bombardment.yaml`,
  `tests/battle_skill/ion-blast.yaml`,
  `tests/battle_skill/scattered-both-sides.yaml`.
- A strike's buff is written after its damage on the units it reached that
  are still alive, and again on a unit struck again:
  `tests/battle_skill/lightning-storm.yaml`,
  `tests/battle_skill/lightning-storm-repeated.yaml`.
- A support skill lands on tick `s + 2`, and a summon stands on the release
  point, joins a second later, is found from the tick after, and moves from
  the next solve: `tests/battle_skill/rhino-assault.yaml`,
  `tests/battle_skill/mobilize-battleship.yaml`.
- An air drop deals the summon's life around it, and the summon loses what it
  took; an appearing summon turns units aside:
  `tests/battle_skill/rhino-drop.yaml`.
- Several summons are scattered by two draws each of their side's stream, and
  Wasps that join setting off to move take no speed from the solve on their
  join tick:
  `tests/battle_skill/wasp-swarm.yaml`.
- Wasps that join attacking are pushed off the Wasps they overlap by the
  solve on their join tick: `tests/corpus/67158166-r2.yaml`, ticks 48
  to 52.
- A creator makes its summons in batches, and a surfacing summon is a locked
  obstacle: `tests/battle_skill/underground-threat.yaml`.
- A Production Line's Sledgehammer that dies with a shell in the air lands
  it without its side's 30% on damage: `tests/corpus/67159970-r6.yaml`,
  tick 318.
- Summons of both sides that join on one tick lock each other on it, and all
  72 search with `Select` on the tick they join:
  `tests/battle_skill/underground-threat-both-sides.yaml`, tick 48.
- A summon is no neighbour of the first solve after it is made: the
  Crawlers made on tick 53 are not among a Crawler's twenty neighbours on
  tick 56, and are on tick 60, `tests/corpus/67156354-r3.yaml`.
- An air drop reaches both sides, blue's first, and its kills count for the
  dead ones' enemies: `tests/battle_skill/vulcans-descent.yaml`.
- A side's officer, technology and Energy Tower skill reach its summon as they
  reach a deployed unit of the summon's type:
  `tests/battle_skill/summon-officer.yaml`,
  `tests/battle_skill/summon-technology.yaml`,
  `tests/battle_skill/summon-energy-tower.yaml`.
- A damage strike lands on tick `s + 3` or `s + 2`, strikes both sides'
  units with no owner, and spares towers:
  `tests/battle_skill/missile-strike-both-sides.yaml`,
  `tests/battle_skill/heavy-missile-strike.yaml`,
  `tests/battle_skill/strike-spares-tower.yaml`,
  `tests/battle_skill/nuke-beside-shield.yaml`.
- A strike deals its whole damage to the constructions it reaches, in the
  order their side's tree holds them after the units, and a summon's drop
  strikes its side's own wall and spares its side's own tower:
  `tests/battle_skill/strike-construction.yaml`,
  `tests/battle_skill/drop-spares-tower.yaml`.
- A falling strike stops at a shield, and one that crosses shields does not:
  `tests/battle_skill/strike-stopped-by-shield.yaml`,
  `tests/battle_skill/javelin-crosses-shield.yaml`.
- An Electromagnetic Impact stops at a shield as it falls, a shield its
  circle reaches takes 20000, and a unit its own side's shield holds takes no
  buff, whether the Impact stopped or landed:
  `tests/battle_skill/impact-stopped-by-shield.yaml`,
  `tests/battle_skill/impact-beside-shield.yaml`.
- A sub-effect with no speed lands inside a shield as it does with none:
  Photon Emission writes its buff on the units its own side's shield holds,
  and a support skill makes its summons inside an enemy shield, a Rhino's
  air drop striking the units the shield holds:
  `tests/battle_skill/photon-in-shield.yaml`,
  `tests/battle_skill/rhino-in-shield.yaml`,
  `tests/battle_skill/underground-in-shield.yaml`.
- A Shield Airdrop lands on tick `s + 3`, and stands a full shield on the
  ground at its release point, which takes shots until it breaks:
  `tests/shield/airdrop-lands.yaml`.
- One an earlier round left standing stands full from the first tick, and
  sorts among its side's shields by position:
  `tests/shield/airdrops-standing.yaml`,
  `tests/shield/airdrop-beside-contraption.yaml`.

- A Mobile Beacon faces its units along the path, walks each at its own
  speed, turns them onto the next segment 20 m past each end, and leaves
  them to their own behaviour at the last:
  `tests/battle_skill/beacon-marksman.yaml`,
  `tests/battle_skill/beacon-hounds.yaml`,
  `tests/battle_skill/beacon-red-rhino.yaml`.
- A ranged unit on a beacon fires as it walks, walks on when its target dies
  and while its skill cools, and a free-firing unit faces where it moves:
  `tests/battle_skill/beacon-fires-walking.yaml`,
  `tests/battle_skill/beacon-free-fire.yaml`.
- A Wasp on a beacon walks on unless an enemy within half the segment's width
  is within its range edge to edge, keeps attacking a lock that dies within
  range, and does not turn its body as it walks on; what it walks towards
  reaches its agent only on the update before each RVO solve:
  `tests/battle_skill/beacon-wasps.yaml`.
- Wasps still on a beacon when the fight is won go on moving until it ends:
  `tests/battle_skill/beacon-wasps-won.yaml`,
  `tests/corpus/134270595-r4.yaml`.
- A Centurion on a beacon whose lock an ally kills attacks on through its
  cooling while what the cooling names is in range, standing still and
  turning nothing, and walks on as the cooling ends; one moving through its
  cooling with nothing locked keeps its turret still:
  `tests/corpus/67157394-r7.yaml`.
- Phantom Rays on a beacon keep attacking a Vortex that dies between two
  shots of a burst, turning to it, and change to moving only once the burst
  is out; one moving through its cooling with no lock changes to attacking
  as the Tarantula its cooling names comes into range:
  `tests/corpus/67160345-r4.yaml`, ticks 237, 246 and 357.

### Replayed

- Missile Specialist hands out Heavy Missile Strike as round 3 opens, and a
  side releases it at one position: `scripts/corpus/verify-matches.py`.

### Read

- A skill's kind is the list of `CommanderSkillGroupData` holding its row:
  `CommanderSkillGroupData.damageCommanderSkills`,
  `CommanderSkillGroupData.supportUnitCommanderSkills`,
  `CommanderSkillGroupData.wayPointCommanderSkills`.
- A side's skills are a list the manager appends to without looking for one of
  the same ID: `CommanderSkillManager.AddCommanderSkill`.
- A `CircleSingle` row has its sub-effect radius set to its effect radius and
  its sub-effect count to one before use: `CommanderSkillData.PreProcess`,
  `CommanderSkillEffectRangeType.CircleSingle`.
- A line's width is the row's `subEffectRange`, split evenly either side:
  `ReleaseCommanderSkillController.CanRelease`, `LineRange.width`.
- A support skill spawns within its `subEffectRange` only when it summons two
  or more: `CS_SupportUnit.CreateSubEffectController`,
  `CSD_SupportUnit.maxCount`.
- A support skill or Shield Airdrop is refused within the enemy tower's
  protection range plus its `subEffectRange`:
  `CommanderSkillManager.CanReleaseCommanderSkill`,
  `TowerDefaultData.protectionRange`, `CommanderSkillData.subEffectRange`,
  `CommanderSkillTargetType.AreaExtendExcludeCrystal`.
- A skill states how many positions it takes:
  `CommanderSkillBase.GetEffectPositionCount`,
  `CS_WayPoint.GetEffectPositionCount`.
- A buff skill and a damage skill release through the common controller:
  `CS_Buff.CreateReleaseController`, `CS_Damage.CreateReleaseController`,
  `CSRC_Common.ActiveSubEffect`.
- Preparing counts from its first update and hands over at `startTime` less
  the sub-effect's move time, the handover running no update of the next
  state: `CSRS_Prepare.Update`, `SimpleFSM.ChangeState`,
  `CommanderSkillBase.GetSubEffectTime`.
- Performing starts its count where preparing stopped, moves every live
  sub-effect before it activates the next, and activates at most one a tick:
  `CSRS_Perform.Enter`, `CSRS_Perform.Update`.
- A sub-effect starts its speed times the shorter of the two times above its
  default height, and lands when it falls to that height:
  `CSRC_Common.ActiveSubEffect`, `CommanderSkillSubEffectAgent.Update`.
- A sub-effect reaches every live, visible actor of every group in its range
  by edge distance in the plane, group by group and team by team in tree
  order, and keeps the units no shield of their own side holds:
  `CommanderSkillSubEffectController.PerformNegativeEffect`,
  `RangeTargetCalculator.CalculateRangeActors`,
  `RangeTargetCalculator.CalculateRangeActorsInternal`,
  `FightMech.IsBuffTarget`,
  `FightCalculator.IsActorInEnergyShield`.
- The buff is written on each unit still alive, and merges with a running
  buff of the same row, or of the same nonzero divide, by restarting it:
  `BuffSystem.AddBuff`, `BuffManager.AddBuff`, `BuffData.IsSameBuff`,
  `Buff.Reset`.
- A buff that disables technology counts the unit's disabling up, and the
  first switches its technology effects off: `BuffManager.AddBuff`,
  `CBEC_DisableTechnology.Enter`, `FightMech.DisableTechnology`.
- A support skill's sub-effect hands its side a creator, which scatters its
  summons only when it makes two or more: `CS_SupportUnit.CreateReleaseController`,
  `CS_SupportUnit.CreateSubEffectController`,
  `SupportUnitEffectController.PerformEffect`,
  `SupportUnitSystem.AddTemporaryCreator`.
- A creator creates on its first update and every interval after, never
  replaces a dead summon, lives for its start time plus its creations, and
  holds the fight while it lives: `SupportUnitCreator.Update`,
  `SupportUnitCreator.IsFinished`, `CSD_SupportUnit.PreProcess`,
  `TeamSupportUnitManager.Update`, `TeamSupportUnitManager.IsStepFinish`.
- A summon is scattered by two draws of its side's stream, made at level 1 in
  no formation, and kept out of the fight for a second:
  `SummonSystem.CreateMech`, `SummonSystem.DoCreateMech`,
  `SummonSystem.CreateMechDelay`, `FightTeam.DeactiveMech`,
  `SupportUnitCreator.APPEAR_DURATION`.
- A summon that dies is destroyed and its side's effects cleared from it,
  where a deployed unit's are not: `DeadEffectSystem.Update`,
  `FightMech.IsSummonMech`, `SummonSystem.RemoveMech`,
  `FightController.DestroyMech`, `FightEffectSystem.ClearEffect`.
- A summon is given what its side registered for its mech as it is made:
  `SummonSystem.DoCreateMech`, `FightEffectSystem.AddEffect`,
  `TeamFightEffectManager.TryGetFightEffectMananger`,
  `FightEffectSystem.AddProviderDataSource`.
- Joining runs before any module, drops the air drop's damage, and then puts
  the summon in its side's trees; a side with a summon appearing keeps its
  towers: `FightController.Update`, `SummonSystem.AddMechDelay`,
  `FightTeam.ActiveMech`, `FightCoreSystem.TryDstroyTower`,
  `SummonSystem.HaveProcessingMech`.
- An air drop hits both sides with the summon's life over its radius, with no
  owner, and the summon then loses what it took:
  `SupportUnitCreator.OnMechEneterFight`,
  `SupportUnitCreator.PerformAirDropDamage`,
  `SupportUnitDamageProvider.GetDamage`,
  `FightCalculator.CalculateHitActorDamage`.
- A summon takes no experience: `ExpSystem.IsValidOwner`.
- A damage strike is `CommanderSkillDamageProvider`'s damage over the skill's
  range, on every group, with no owner:
  `CommanderSkillDamageProvider.GetDamage`,
  `CommanderSkillDamageProvider.GetEffectTargetType`,
  `DamagePerformer.PrepareRangeTargets`,
  `RangeTargetCalculator.CalculateRangeActors`.
- Its range takes every actor of each side's tree, towers and constructions
  among them, as a unit's splash does: `FightTeam.AddTower`,
  `FightTeam.AddConstruction`, `FightTeam.AddActor`,
  `RangeTargetCalculator.CalculateRangeActorsInternal`.
- A special attack, a battle skill's or a summon's drop, passes over a tower
  and a unit whose main skill is empty, and strikes a construction less its
  special damage rate: `CommanderSkillDamageProvider.IsSpecialAttack`,
  `SupportUnitDamageProvider.IsSpecialAttack`,
  `DamagePerformer.PerformHitTargetsEffect`, `FightCrystal.IsTower`,
  `FightMech.IsSimulateMech`, `FightCalculator.PerformHitTargetEffect`,
  `FightConstruction.GetSpecialDamageReduceRate`.
- A sub-effect writes on either side when its buff is harmful and on its own
  side's group otherwise: `CommanderSkillSubEffectController.PerformHitEffect`,
  `CS_Buff.IsHarmful`, `BuffData.IsHarmful`, `BuffData.HarmfulCheck`,
  `CommanderSkillSubEffectController.PerformPositiveEffect`.
- A strike's sub-effects are placed as the fight starts, a random circle's
  by two draws each of its side's stream in tenths and a line's evenly along
  it: `CSRC_Common.OnFightStart`,
  `CommanderSkillManager.CalculateAttackPositions`,
  `GRRandom.NextFixInRange10`, `GRRandom.NextFixInRangePrecision`,
  `FPoint.RoundToInt`, `FPoint.Round`, `FVector3.ClampMagnitude`.
- Its later sub-effects are activated by the perform state's count, at most
  one an update: `CSRS_Perform.Enter`, `CSRS_Perform.Update`,
  `CSRS_Perform.ActiveSubEffect`, `FSMState.Update`,
  `CommanderSkillReleaseState..ctor`, `CommanderSkillBase.GetSubEffectTime`.
- Each sub-effect strikes its `subEffectRange`, and one that carries a buff
  lists what its circle reaches before the damage and writes the buff after,
  on the living: `CommanderSkillSubEffectController.PerformNegativeEffect`,
  `CommanderSkillSubEffectController.PerformHitEffect`,
  `BuffSystem.AddBuff`.
- A buff skill's sub-effect is the same controller as a damage skill's,
  whose harmful landing strikes when the skill deals damage or modifies it,
  as `CS_Buff` always does; its modifier adds `energyShieldDamage` to what a
  shield takes when that is above zero, and its fall passes shields as its
  row's `isCrossAdvancedShield` says: `CS_Buff.CreateSubEffectController`,
  `CS_Damage.CreateSubEffectController`,
  `CommanderSkillSubEffectController.IsDamageEffect`,
  `CS_Buff.ChangeHitEnergyShieldDamage`,
  `CS_Buff.IsChangeHitEnergyShieldDamage`,
  `CommanderSkillBase.CanCrossAdvancedEnergyShield`,
  `CommanderSkillBase.IsHarmful`, `CS_Buff.IsHarmful`.
- A sub-effect stopped by a shield performs where it stopped:
  `CommanderSkillSubEffectAgent.OnHitEnergyShield`,
  `CommanderSkillSubEffectController.InterruptEffect`.
- Its fall stops at the first shield it comes inside:
  `CommanderSkillSubEffectAgent.Update`,
  `CommanderSkillSubEffectAgent.IsHitEnergyShield`,
  `CSRC_Common.InterruptSubEffect`.
- A Shield Airdrop's landing stands a shield of its side on the ground where
  it landed, full and active, after the side's others:
  `CS_EnergyShield.CreateSubEffectController`,
  `AdvancedEnergyShieldEffectController.PerformEffect`,
  `CommanderSkillManager.CalculateAttackPositions`,
  `AdvancedEnergyShieldSystem.Create`, `GroupAdvancedEnergyShieldManager.Create`,
  `CSD_EnergyShield.GetAdvancedEnergyShieldValue`.
- Its fall crosses every shield: `CS_EnergyShield.CanCrossAdvancedEnergyShield`,
  `CommanderSkillSubEffectAgent.Update`.
- A sub-effect whose row gives it no speed is finished where it stands, never
  moved or tested against a shield: `CommanderSkillSubEffectAgent.Update`,
  `CommanderSkillBase.GetSubEffectMoveSpeed`. A support skill's crosses no
  shield, as no override of `CanCrossAdvancedEnergyShield` is its own.
- It stands into the next round, refilled, as a contraption's shield does:
  `CS_EnergyShield.IsShortLifeTime`, `CS_EnergyShield.IsResetNextRound`,
  `GroupAdvancedEnergyShieldManager.OnFightEnd`,
  `GroupAdvancedEnergyShieldManager.OnFightStart`.

- A Mobile Beacon selects its units as the fight starts and walks them by a
  command: `CSRC_WayPoint.OnFightStart`, `CSRC_WayPoint.AddMech`,
  `PilotAI.SetCommand`, `PilotAI.Update`,
  `MoveAttackCommand.RefreshCurrentTargetInfo`, `MoveAttackCommand.Perform`,
  `MoveAttackCommand.IsEnableAttackMove`,
  `MoveAttackCommand.CalculateMoveLineRange`, `MotionController.Move`.
- A unit attacking under a command walks on or stops, turns to its target and
  leaves the attack only once its target is out of range; how near and how far
  an enemy stops it: `MotionAttackState.Update`, `MotionAttackState.AttackRotate`,
  `MotionAttackState.AttackMove`, `MoveAttackCommand.IsIdle`,
  `MoveAttackCommand.IsActive`, `LineRange.Overlaps`, `FightActor.Distance2D`,
  `FightTransform.Distance2D`.
- Without a lock a unit under a command turns to its velocity, attacking or
  moving, and its weapons with it; a cooling's attack target keeps its attack
  in range: `MotionController.CalculateTargetDirection`,
  `MotionMoveState.NormalRotate`, `FightSkill.IsAttackTargetInAttackRange`,
  `SkillAttackRangeChecker.IsAttackTargetInAttackRange`.
- `MotionController.Move` hands the agent nothing but on the update the RVO
  counter reads 3: `MotionController.Move`, `RVOSimulatorFixed.IsUpdateFrame`,
  `RVOSimulatorFixed.DoFixedUpdate`, `RVOControllerFixed.Move`.

### Not established

- **A strike's buff beside a shield.** A Lightning Storm's list is read to
  leave out what its own side's shields hold, as an Electromagnetic Impact's
  is recorded to, and no fight pins it.
- **How a beacon's `LineRange` meets a unit's circle.** The simulator reads it
  as the distance to the segment against the width and the radius, which the
  recordings agree with and the build's `LineRange.Overlaps` is not read for.

- **Why a summon's first intervals are drawn as it joins,** and that its
  agent is handed no speed while it appears. Both are measured, not read: the
  game's `rvo_solve` rows read an appearing summon's agent locked with a
  maximum speed of 0.
- **A summon killed by its own air drop.** `PerformAirDropDamage` hands
  `FightCalculator.CalculateHitActorDamage` a direct hit (`isDirectHit`)
  under the summon's side with no attacker, which goes straight to
  `FightActor.ReduceLife`: no shield, rate or `OnHitted`. A summon that loses
  all its life so dies as any unit does, credited to no one, its damage
  recorded before its death: a level 1 Rhino of Rhino Assault dropped on a
  level 9 Fortress deals it 19297 and dies. Read from the build; no recording
  holds it.
- **A support skill whose row places its summons at set offsets**, or makes
  them in capped batches. None of the standard ones does, and the simulator
  refuses such a row.

- **Another skill's landing.** The rule is read for every `CSRC_Common` skill,
  and a Missile Strike and an Orbital Javelin were seen landing where it puts
  them, but only the Electromagnetic Impact's is pinned, and the simulator
  releases no other.
- **A later sub-effect.** When a skill with several sub-effects activates each
  after the first is not stated.
- **What disabling a technology switches off** beyond a plain technology's
  numbers ([technology_effects.md](technology_effects.md)); the simulator
  refuses a unit that carries any other.
- **The Electromagnetic Blast.** Its row differs in its range alone, and no
  fight pins it.
- **A Training Ground release of Heavy Missile Strike.** Its geometry and map
  rule are Missile Strike's by the row, and the corpus releases it, but no
  layout naming it has been run in the Training Ground.
- **The `contained`, `center` and `overlap` map rules.** They were measured
  against the Training Ground's refusals in another version, and no test pins
  them; the region check they come from is not read.
- **The tower threshold's boundary.** It is derived from the check, not
  measured against coordinates in the Training Ground.
- **Which skills a match can deal.** The table is the layout contract's, and
  which of its skills a standard match reaches is
  [reinforce_items.md](reinforce_items.md)'s and
  [`config/economy.yaml`](../../config/economy.yaml)'s.
