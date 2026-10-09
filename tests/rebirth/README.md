# Rebirth

What a rebirth technology does as its unit dies: when the unit stands again,
where, with what, and what its side and the fight's score make of it while it
waits and after. [`technology_effects.md`](../../docs/rules/technology_effects.md#rebirth-technologies)
is the rule. Each fight puts Field Reassembly on blue's Typhoons against a
Fortress, with Anti-Air Barrage or beside blue's Rhinos.

A recording holds a unit waiting to stand again as its row in `rebirths`,
where it fell, from the tick of its `unit_died`; it stands again under the same
unit id, with no event, and its `rebirth_count` reads 1. The last tick's
`team_scored` is what the reborn score.
