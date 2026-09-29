# Battle skill

When a released battle skill lands, what it reaches, and what it writes or
summons.
[`battle_skill.md`](../../docs/rules/battle_skill.md) states the rule, and
[`config/commander_skill_effects.yaml`](../../config/commander_skill_effects.yaml)
holds the numbers of the skills the simulator releases.

Each fight is its layout fought once in the game, read back as a fight. A
skill's effect names no source: its `buff_applied` events, and an air drop's
`damage` and the deaths it causes, carry only the team that released it. A
summon is the next unit from its `unit_created` on, though it first stands in
a snapshot a second later, as it joins the fight.
