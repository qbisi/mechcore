# Terrain

What the terrains a battle skill leaves do to the units standing in them, and
when they go. [`terrain.md`](../../docs/rules/terrain.md) states the rule, and
[`config/commander_skill_effects.yaml`](../../config/commander_skill_effects.yaml)
holds each skill's numbers.

Each fight is its layout fought once in the game, read back as a fight. A
terrain is a recording's `terrains` row, with the units it affects as its
`applications`, from its `terrain_created` to its `terrain_removed`.
