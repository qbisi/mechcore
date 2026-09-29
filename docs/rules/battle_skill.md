# Battle skills

This index covers the position-targeted battle skills that affect combat and
can occur in standard 1v1 matches. Skills that require a unit or construction
target, and configurations absent from standard 1v1, are outside the layout
contract. A skill is a row of `CommanderSkillGroupData` in `level0`, whose
list says what kind of skill it is (`damageCommanderSkills`,
`supportUnitCommanderSkills`, `oilCommanderSkills` and the rest).

## ID and type

The layout names a skill by its `type`, not its native ID. When several
native IDs share a `type`, the canonical one is the ID the Training Ground
compiler releases; the others are equivalent standard-1v1 sources, such as a
Blueprint or a Research Center result, whose acquisition route or cooldown
does not create another type.

A skill reaches a side by one of two routes, and both name it by this ID. A
reinforcement card's own ID is the skill it grants, which
[reinforce_items.md](reinforce_items.md) sets out, and a blueprint names the
skill it activates in `grants_skill`, which
[`config/economy.yaml`](../../config/economy.yaml) states alongside what the
blueprint costs. A state names the same ID in `battle_skills`, so the panel,
the card and the blueprint speak one ID space. A skill's two cooldowns are
[`config/commander_skills.yaml`](../../config/commander_skills.yaml)'s.

| Layout `type` | Canonical ID | Other IDs | Positions | Geometry | Map rule |
| --- | ---: | --- | ---: | --- | --- |
| `incendiary_bomb` | 100002 | | 2 | line | overlap |
| `electromagnetic_impact` | 200001 | | 1 | circle | overlap |
| `electromagnetic_blast` | 200002 | 200004 | 1 | circle | overlap |
| `photon_emission` | 200003 | | 1 | circle | overlap |
| `missile_strike` | 300001 | | 1 | circle | overlap |
| `orbital_bombardment` | 300003 | | 1 | random circle | overlap |
| `nuke` | 300004 | 300008 | 1 | circle | overlap |
| `lightning_storm` | 300005 | 300009 | 1 | random circle | overlap |
| `ion_blast` | 300006 | 300010 | 2 | line | overlap |
| `orbital_javelin` | 300007 | | 1 | circle | overlap |
| `heavy_missile_strike` | 300016 | | 1 | circle | overlap |
| `sticky_oil_bomb` | 400002 | | 2 | line | overlap |
| `acid_blast` | 500002 | | 2 | line | overlap |
| `smoke_bomb` | 600002 | | 2 | line | overlap |
| `shield_airdrop` | 800001 | | 1 | circle | center |
| `underground_threat` | 1200001 | | 1 | support circle | summon |
| `rhino_assault` | 1200002 | | 1 | support circle | summon |
| `wasp_swarm` | 1200003 | | 1 | support circle | summon |
| `mobilize_battleship` | 1200004 | | 1 | support circle | summon |
| `vulcans_descent` | 1200005 | | 1 | support circle | summon |
| `mobile_beacon` | 1500001 | | 3 | path | contained |
| `mobile_beacon_card` | 1500002 | | 3 | path | contained |

Mobile Beacon has two independent standard-1v1 sources: `1500001` is granted
by Research Center Blueprint 3, while `1500002` is granted by the ordinary
reinforcement card. Activating the Blueprint does not exclude the card, and the
native skill manager does not deduplicate them by ID or name, so both objects
may coexist. A document names them apart, as
[`config/names.yaml`](../../config/names.yaml) does, and the compiler maps each
name to its own ID. The two share one path and one cooldown.

Heavy Missile Strike (`300016`) is no card: Missile Specialist hands it out,
in place of the two Missile Strikes it once did. Its row is Missile Strike's in
every field that places or times it, and differs in damage alone. Unit Recycle
(`900010`) targets a friendly unit, and is not a layout type.

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

The map rules:

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

This is the first sub-effect's rule. When a skill with several sub-effects
activates the later ones is read to wait for their interval, at most one a
tick, and is not stated here.

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

The buff slows the unit by its `move_speed_rate` for its `duration`, and while
it runs the unit's technologies are off: `status_mask` reads
`technology_disabled` whether the unit carries a technology or not. A second
one on a unit it still runs on restarts it. A buff of divide 0 merges only
with its own row, so another divide-0 buff, a missile's slow among them, runs
beside it rather than merging.

A fight's buffs are cleared as the fight is left, on its last tick: after the
towers it tore down when a side's last unit fell, and after everything else
that tick did when a projectile landing after the last death decided it.

## Names

<!-- names: commander_skills -->
| ID | 中文 | English | Document name |
| ---: | --- | --- | --- |
| 100002 | 燃烧弹 | Incendiary Bomb | `incendiary_bomb` |
| 200001 | 电磁冲击 | Electromagnetic Impact | `electromagnetic_impact` |
| 200002 | 巨型电磁冲击 | Electromagnetic Blast | `electromagnetic_blast` |
| 200003 | 光子投射 | Photon Emission | `photon_emission` |
| 300001 | 导弹打击 | Missile Strike | `missile_strike` |
| 300003 | 轨道轰炸 | Orbital Bombardment | `orbital_bombardment` |
| 300004 | 核弹 | Nuke | `nuke` |
| 300005 | 闪电风暴 | Lightning Storm | `lightning_storm` |
| 300006 | 离子轰炸 | Ion Blast | `ion_blast` |
| 300007 | 轨道标枪 | Orbital Javelin | `orbital_javelin` |
| 300016 | 重型导弹打击 | Heavy missile strike | `heavy_missile_strike` |
| 400002 | 黏油弹 | Sticky Oil Bomb | `sticky_oil_bomb` |
| 500002 | 酸液弹 | Acid Blast | `acid_blast` |
| 600002 | 烟雾弹 | Smoke Bomb | `smoke_bomb` |
| 800001 | 空投护盾 | Shield Airdrop | `shield_airdrop` |
| 900001 | 战地回收 | Field Recovery | `field_recovery` |
| 1000001 | 再部署 | Redeployment | `redeployment` |
| 1100001 | 强化训练 | Intensive Training | `intensive_training` |
| 1200001 | 地底威胁 | Underground Threat | `underground_threat` |
| 1200002 | 犀牛来袭 | Rhino Assault | `rhino_assault` |
| 1200003 | 呼叫机群 | Wasp Swarm | `wasp_swarm` |
| 1200004 | 呼叫战舰 | Mobilize Battleship | `mobilize_battleship` |
| 1200005 | 天降火神 | Vulcan's Descent | `vulcans_descent` |
| 1500001 | 移动信标 | Mobile Beacon | `mobile_beacon` |
| 1500002 | 移动信标 | Mobile Beacon | `mobile_beacon_card` |
<!-- /names -->

## Evidence

### Recorded

- An Electromagnetic Impact lands on tick `s + 3`, writes its buff for its
  whole duration, and holds the unit's technologies off while it runs, on a
  unit with none: `tests/battle_skill/fights/rhino-slowed.yaml`.
- It reaches a unit by its edge, whose centre stands beyond the range:
  `tests/battle_skill/fights/reached-by-its-edge.yaml`.
- It reaches air units, and a fight a projectile's landing decided clears its
  buff on the last tick, after that projectile's removal:
  `tests/battle_skill/fights/wasps.yaml`.
- It reaches the releasing side's own units, blue's before red's:
  `tests/battle_skill/fights/own-side.yaml`.

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
  order, and keeps the units outside an energy shield:
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

### Not established

- **Another skill's landing.** The rule is read for every `CSRC_Common` skill,
  and a Missile Strike and an Orbital Javelin were seen landing where it puts
  them, but only the Electromagnetic Impact's is pinned, and the simulator
  releases no other.
- **A later sub-effect.** When a skill with several sub-effects activates each
  after the first is not stated.
- **What disabling a technology switches off.** No pinned fight hits a unit
  that carries one; the simulator refuses such a fight.
- **An Electromagnetic Impact on a unit running another buff.** The two run
  side by side, and how their rates compose is not read; the simulator
  refuses it.
- **A unit inside an energy shield.** It is read to be spared, and no fight
  pins it.
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
