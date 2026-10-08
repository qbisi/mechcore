# Interceptor

How an interceptor takes enemy projectiles out of the air, and what happens to
it as a building of its side.
[`contraptions.md`](../../docs/rules/contraptions.md) states the rule, and
[`config/contraptions.yaml`](../../config/contraptions.yaml) holds the
interceptor's numbers. The `*-interception` fights are the interceptors
Missile Interception makes a unit, which
[`technology_effects.md`](../../docs/rules/technology_effects.md#missile-interception)
states and [`config/technology_effects.yaml`](../../config/technology_effects.yaml)
holds the numbers of.

Each fight is its layout fought once in the game, read back as a fight; the
recording's `projectile_removed` events with `intercepted` set are the
interceptions, and a projectile's `life` in each tick is what the hits took.
