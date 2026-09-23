# Turret fixtures

Every layout here exists to measure **what a turret's skill does**: when it
fires, at what, and how often. That is the half of a construction
[`constructions.md`](../../docs/rules/constructions.md) leaves out, and
[`turrets.md`](../../docs/rules/turrets.md) states what these fights settled.

| Fixture | What it separates | Approach |
| --- | --- | --- |
| `rapid-fire-head-on.yaml` | the search cadence and the first shot from the tick a target is in reach | 200 m straight down the turret's column |
| `rapid-fire-flank.yaml` | whether the turret turns before it fires, and what the turn costs | the same Crawlers, 40 degrees off that line |
| `anti-armor-head-on.yaml` | whether the other turret is the same machine with other numbers, and how a unit holds a turret that falls in its swing | as head-on, on the Anti-Armor Turret |
| `anti-armor-arclights.yaml` | the same, through a reload, in a fight no tower falls in | four Arclights down the Anti-Armor Turret's column, two shots each |

`skill.mcscript` records them with the `target_refs_v1` sidecar and records
the head-on fight twice, requiring the two to be one recording.
`arclights.mcscript` records the Arclight fight twice, requiring the two to be
one recording. `regressions.mcscript` needs no game: it runs both Rapid-Fire
fights and the Arclight fight through the simulator and holds them to the
game's physics and content hashes. The Anti-Armor Crawler fight is not
pinned. The simulator agrees with it through tick 1026, across blue's tower
lost at 509 and red's at 771 ([`towers.md`](../../docs/rules/towers.md)). At
1027 the Marksman's Crawler walks out of its range: the game ends the attack,
and the simulator switches to a nearer Crawler.

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
| weapon mode | Standalone (2) | Standalone (2) |
| magazine, reload | 10, 2.5 s | 6, 10 s |
| attack angle (construction) | 20° | 20° |
| rotate speed (construction) | 120°/s | 120°/s |
| radius (construction) | 12 m | 12 m |

The build's `WeaponMode` is `Normal` (0), `Group` (1) and `Standalone` (2).
A unit's skill is `Normal`; a turret's is `Standalone`, which gives its weapon
a transform of its own that turns at the construction's rotate speed. A row
of the loading type fires from a magazine: 10 rounds and 2.5 s to reload for
the Rapid-Fire Turret, 6 and 10 s for the Anti-Armor.
