# Battle skill index

[简体中文](battle_skill.zh.md)

This index is pinned to game build 2227. It covers position-targeted battle
skills that affect combat and can occur in standard 1v1 matches. Skills that
require a unit or construction target, and configurations absent from standard
1v1, are outside this layout contract.

The layout uses the public `type`, not a native ID. When several native IDs
have the same `type`, the canonical row is the deterministic ID used by the
Training Ground compiler. The remaining rows document equivalent standard-1v1
sources such as a Blueprint or Research Center result; their differing
acquisition route or cooldown does not create another layout type.

A skill reaches a side by one of two routes, and both name it by this ID. A
reinforcement card's own ID is the skill it grants, which
[`docs/reinforce_items.md`](reinforce_items.md) sets out, and a blueprint names
the skill it activates in `grants_skill`, which
[`config/economy.yaml`](../../config/economy.yaml) states alongside what the
blueprint costs. A state names the same ID in `battle_skills`, so the panel, the
card and the blueprint all speak one ID space.

## ID mapping

Descriptions below use the official English localization with the build-2227
configuration parameters substituted.

| Native ID | Layout `type` | In-game name | Positions | Canonical | Description |
| ---: | --- | --- | --- | --- | --- |
| 100002 | `incendiary_bomb` | Incendiary Bomb | 2 | yes | Fires multiple incendiary bombs in a straight line to form a wall of fire that deals 270 damage per second |
| 200001 | `electromagnetic_impact` | Electromagnetic Impact | 1 | yes | Launches an electromagnetic impact shot that deals 20000 damage to Energy Shields, temporarily disables Tech on hit, and decreases movement speed by 40% for 25 seconds |
| 200002 | `electromagnetic_blast` | Electromagnetic Blast | 1 | yes | Creates a large-scale electromagnetic blast that deals 20000 damage to Energy Shields, temporarily disables Tech on hit, and decreases movement speed by 40% for 25 seconds |
| 200003 | `photon_emission` | Photon Emission | 1 | yes | Covers allied forces in the target area with a photon coating, decreasing damage received by 30% for 20 seconds and granting immunity to Electromagnetic, Ignited, Acid, and Degeneration Beam effects |
| 200004 | `electromagnetic_blast` | Electromagnetic Blast | 1 | no | Creates a large-scale electromagnetic blast that deals 20000 damage to Energy Shields, temporarily disables Tech on hit, and decreases movement speed by 40% for 25 seconds |
| 300001 | `missile_strike` | Missile Strike | 1 | yes | Launches 1 guided missile that deals 3000 damage |
| 300003 | `orbital_bombardment` | Orbital Bombardment | 1 | yes | Launches 15 guided missiles at the target area, dealing 2500 damage per missile |
| 300004 | `nuke` | Nuke | 1 | yes | Launches 1 tactical nuke that reaches the battlefield after 15 seconds and deals 70000 damage |
| 300005 | `lightning_storm` | Lightning Storm | 1 | yes | Creates a lightning storm at the target position; a struck unit takes 2000 damage and is slowed by 65% for 20 seconds |
| 300006 | `ion_blast` | Ion Blast | 2 | yes | Fires an ion beam that moves slowly across the ground and continuously deals 2400 damage to units hit by the beam |
| 300007 | `orbital_javelin` | Orbital Javelin | 1 | yes | Launches a high-speed tungsten rod from orbit, dealing 70000 damage within 30 m; Energy Shields cannot stop it |
| 300008 | `nuke` | Nuke | 1 | no | Launches 1 tactical nuke that reaches the battlefield after 15 seconds and deals 70000 damage |
| 300009 | `lightning_storm` | Lightning Storm | 1 | no | Creates a lightning storm at the target position; a struck unit takes 2000 damage and is slowed by 65% for 20 seconds |
| 300010 | `ion_blast` | Ion Blast | 2 | no | Fires an ion beam that moves slowly across the ground and continuously deals 2400 damage to units hit by the beam |
| 400002 | `sticky_oil_bomb` | Sticky Oil Bomb | 2 | yes | Sprays sticky oil in a straight line, decreasing the movement speed of units within it by 55%; the oil lasts for 2 rounds |
| 500002 | `acid_blast` | Acid Blast | 2 | yes | Sprays acid in a straight line; affected units lose 3% HP per second and take 250% damage when attacked; the acid lasts for 1 round |
| 600002 | `smoke_bomb` | Smoke Bomb | 2 | yes | Releases smoke in a straight line, decreasing the range of units within it by 35%; the smoke lasts for 1 round |
| 800001 | `shield_airdrop` | Shield Airdrop | 1 | yes | Airdrops an Energy Shield with 50000 HP to protect allied units within it |
| 1200001 | `underground_threat` | Underground Threat | 1 | yes | Crawlers continuously emerge from the target area |
| 1200002 | `rhino_assault` | Rhino Assault | 1 | yes | Airdrops 1 Rhino to attack the enemy |
| 1200003 | `wasp_swarm` | Wasp Swarm | 1 | yes | Summons an army of Wasps to attack the enemy |
| 1200004 | `mobilize_battleship` | Mobilize Battleship | 1 | yes | Summons 1 Overlord to attack the enemy |
| 1200005 | `vulcans_descent` | Vulcan's Descent | 1 | yes | Airdrops 1 Vulcan to attack the enemy |
| 1500001 | `mobile_beacon` | Mobile Beacon | 3 | no | Designates the movement path of selected units |
| 1500002 | `mobile_beacon` | Mobile Beacon | 3 | yes | Designates the movement path of selected units |

Mobile Beacon has two independent standard-1v1 sources: `1500001` is granted
by Research Center Blueprint 3, while `1500002` is granted by the ordinary
reinforcement card. Activating the Blueprint does not exclude the reinforcement
card, and the native skill manager does not deduplicate them by ID or name, so
both objects may coexist. The compiler retains both mappings and uses `1500002`
as the canonical ID when a layout declares only `type: mobile_beacon`.

## Effect geometry and target regions

All distances in this section are metres. `effectRange` and `subEffectRange`
are native configuration field names, but `subEffectRange` is not a second,
universally visible effect radius. Their meaning depends on the skill geometry.

| Layout `type` | Geometry | `effectRange` | `subEffectRange` | Layout map rule |
| --- | --- | ---: | ---: | --- |
| `incendiary_bomb` | line | 140 | 40 | overlap |
| `electromagnetic_impact` | circle | 60 | 60 | overlap |
| `electromagnetic_blast` | circle | 130 | 130 | overlap |
| `photon_emission` | circle | 110 | 110 | overlap |
| `missile_strike` | circle | 40 | 40 | overlap |
| `orbital_bombardment` | random circle | 130 | 30 | overlap |
| `nuke` | circle | 100 | 100 | overlap |
| `lightning_storm` | random circle | 130 | 30 | overlap |
| `ion_blast` | line | 180 | 20 | overlap |
| `orbital_javelin` | circle | 30 | 30 | overlap |
| `sticky_oil_bomb` | line | 105 | 30 | overlap |
| `acid_blast` | line | 140 | 40 | overlap |
| `smoke_bomb` | line | 160 | 50 | overlap |
| `shield_airdrop` | circle | 70 | 70 | center |
| `underground_threat` | support circle | 32 | 32 | summon |
| `rhino_assault` | support circle | 20 | 20 | summon |
| `wasp_swarm` | support circle | 25 | 25 | summon |
| `mobilize_battleship` | support circle | 25 | 25 | summon |
| `vulcans_descent` | support circle | 25 | 25 | summon |
| `mobile_beacon` | path | 200 | 40 | contained |

The layout map rules have the following precise meanings:

- `contained`: the complete path or effect footprint must remain inside the
  battlefield map.
- `center`: the release center must be inside the map; the effect edge may
  extend outside it.
- `overlap`: the effect footprint must intersect the map. The release center
  and even most of the effect may be outside it.
- `summon`: the release center must remain at least the skill's effective
  `subEffectRange` inside the map and must satisfy the tower exclusion described
  below.

For a `circle`, `effectRange` is the primary effect radius. For a
`random circle`, `effectRange` is the outer distribution radius and
`subEffectRange` is the individual sub-effect radius. For a line skill, the
two positions determine the length; native `LineRange` receives
`subEffectRange` as its full width, so neither configured value may be treated
as a circular radius. `mobile_beacon` is a path rather than a damaging area.

The table records values after runtime preprocessing.
`CommanderSkillData.PreProcess()` rewrites every `CircleSingle` skill before
use by assigning `subEffectRange = effectRange` and `subEffectCount = 1`.
This also explains why Underground Threat has a larger visible and effective
radius than Rhino Assault (32 m versus 20 m).

The support fight controller receives the effective `subEffectRange` as a spawn
`randomRange` only when the support configuration has `maxCount >= 2`;
single-unit support skills pass zero instead. Placement checks occur before
that fight-time branch and always read the effective, preprocessed value.

Build 2227 configures the enemy tower protection range as 140 m. The native
point-release check rejects a support skill when the Euclidean distance from
its requested release center to either enemy tower's map-bound center is less
than or equal to `140 + effective subEffectRange`. The statically derived strict
thresholds are therefore 172 m for Underground Threat, 160 m for Rhino Assault,
and 165 m for Wasp Swarm, Mobilize Battleship, and Vulcan's Descent. Shield
Airdrop uses the native `AreaExtendExcludeCrystal` target type and has an
effective `subEffectRange` of 70 m, producing a statically derived 210 m tower
threshold. These thresholds still require direct coordinate-boundary tests
before they are treated as observed Training Ground behavior.

## Position contract

Every coordinate component is an `i32`. Decimal values, including integral
spellings such as `10.0`, are invalid. Battle-skill positions are not formation
placements and do not inherit the formation grid-alignment or footprint rules.
The native skill check remains authoritative for target regions and other
skill-specific restrictions.

- A one-position skill uses one target position.
- A two-position skill uses `[start_position, end_position]`. The second value
  is the concrete skill endpoint, not a direction vector. Both order and
  distance are semantic.
- `mobile_beacon` uses three ordered positions. The first position selects the
  affected units and starts the first path segment; the second is the
  intermediate path endpoint; the third is the final endpoint.

The compiler must still require the declared position count to equal the
runtime skill's `GetEffectPositionCount()`. It must not truncate, duplicate,
coerce, or synthesize coordinates.
