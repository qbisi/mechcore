# Multi-attack

What a multi-attack technology adds to its unit's bursts: how many projectiles
an attack fires, the time between two, how far each lands from its target,
and which weapon each leaves.
[`technology_effects.md`](../../docs/rules/technology_effects.md#multi-attack-technologies)
is the rule. Each fight puts Doubleshot or Burst Mode on one blue unit facing
a Rhino: Doubleshot on a Marksman and a Scorpion, Burst Mode on a Farseer
and a Phantom Ray.

A recording holds each projectile's `projectile_released` event with the
weapon it leaves, and the projectiles table every tick after.
