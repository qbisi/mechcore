# Turret fixtures

Every layout here exists to measure **what a turret's skill does**: when it
fires, at what, and how often. That is the half of a construction
[`constructions.md`](../../docs/rules/constructions.md) leaves out, and the
one the simulator refuses a layout for.

| Fixture | What it separates | Approach |
| --- | --- | --- |
| `rapid-fire-head-on.yaml` | the search cadence and the first shot from the tick a target is in reach | 200 m straight down the turret's column |
| `rapid-fire-flank.yaml` | whether the turret turns before it fires, and what the turn costs | the same Crawlers, 40 degrees off that line |
| `anti-armor-head-on.yaml` | whether the other turret is the same machine with other numbers | as head-on, on the Anti-Armor Turret |

`skill.mcscript` records them with the `target_refs_v1` sidecar and records
the head-on fight twice, requiring the two to be one recording. Nothing here
runs the simulator: a turret is what the research question these fixtures
were recorded for asks a claimant to build, and the offline
`regressions.mcscript` that pins the answer is that claimant's to add.

## What the build says a turret's skill is

Read by `scripts/extract-skills.py` from `level0`, the `MechSkillGroupData`
object at path 173, by the field order `GRCore`'s `SkillData` and
`ProjectileSkillData` declare, and checked against the Marksman's row, which
reproduces `config/units/marksman.yaml` exactly. Both turrets are
`ProjectileSkillData` rows; the construction row (`config/constructions.yaml`)
carries the damage, the attack angle and the rotate speed.

| | Rapid-Fire Turret, skill 3003001 | Anti-Armor Turret, skill 3002001 |
| --- | ---: | ---: |
| damage | 82 | 2748 |
| attack range | 115 | 125 |
| attack interval | 0.3 s, random ± 0.1 s | 2.5 s, random ± 0.2 s |
| prepare, attack point, backswing, cooling | 0, 0, 0, 0 | 0, 0, 0, 0 |
| initial cooldown | 0 | 0 |
| bullet speed | 400 | 300 |
| splash range | 10 | 5 |
| targets | ground only | ground only |
| quick switch target | yes | yes |
| weapon mode | Released (2) | Released (2) |
| attack angle (construction) | 20° | 20° |
| rotate speed (construction) | 120°/s | 120°/s |
| radius (construction) | 12 m | 12 m |

A unit's skill carries weapon mode `Held` (0); what `Released` changes is not
read.
