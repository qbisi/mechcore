# Missile

When a missile fires, at what, from where, and what its hit does.
[`contraptions.md`](../../docs/rules/contraptions.md) states the rule, and
[`config/contraptions.yaml`](../../config/contraptions.yaml) holds the
missile's numbers and its buff.

Each fight is its layout fought once in the game, read back as a fight. The
projectile a missile fires is the one whose `projectile_released` names no
source, and the fight in
[`../interceptor/missiles.yaml`](../interceptor/missiles.yaml) is
the one where interceptors take missiles out of the air.
