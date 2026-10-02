# Lifesteal

How a hit hands life back to the unit whose skill dealt it, and which of a
unit's lifesteal sources does. [`combat.md`](../../docs/rules/combat.md#lifesteal)
is the rule. Each fight puts a lifesteal source on a unit that is hurt while
it fights, so the hits it deals have life to give back: an item, a
technology, both at once, and a beam with no splash, which strikes outside
the splash path. The Marksman fight's control is
[`../regression/fights/marksman-vs-arclight.yaml`](../regression/fights/marksman-vs-arclight.yaml)
and the Steel Ball's is
[`../steel_ball/fights/m2-rhino-4242.yaml`](../steel_ball/fights/m2-rhino-4242.yaml).

A recording holds each heal as a `healing` event on the unit healed, right
after the damage of the hit that caused it.
