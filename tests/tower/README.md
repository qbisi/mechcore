# Tower loss

What losing a tower writes on its side, and what strengthening one does to
it. [`docs/rules/towers.md`](../../docs/rules/towers.md) is the rule; these are
the fights it was read from, recorded by the game with seed 4242.

| Fixture | What it separates | Ticks |
| --- | --- | ---: |
| `tower-level-0.yaml` .. `tower-level-4.yaml` | one level each: the tower's life, and how long its loss lasts, 180 to 20 ticks | 870, 925, 928, 964, 959 |
| `both-towers-0-0.yaml` | a second level-0 loss inside the first: whether it refreshes, stacks, is ignored or lengthens | 581 |
| `both-towers-0-4.yaml` | a level-4 loss inside a level-0 one: the lengthening is the second row's 20 ticks | 641 |

Red's Steel Balls take blue's towers; blue's Fangs stand at the back, out of
the fight until a tower falls, and carry what the loss writes. In the two-tower
fights the Energy Tower's lane is ahead, so the second tower falls while the
first loss runs.

| Script | Needs the game | What it does |
| --- | --- | --- |
| `levels.mcscript` | yes | records the five level fights |
| `both.mcscript` | yes | records the two two-tower fights |
| `regressions.mcscript` | **no** | replays all seven through the simulator and asserts both hash layers |

The fights pinned more than the buff. A tower is an actor of its own: the Steel
Ball whose beam fells it reads idle on that tick, and its `building_destroyed`
waits for the end of the tick as a blow's does. A Fang stopped to attack keeps
the speed it took the field with as its RVO maximum. A projectile takes its
owner's damage when it lands, and a dead owner's buffs go on its next update.
The rule states each.
