# Shield

What a battlefield shield does to the hits meant for the units it covers, and
when it breaks. [`contraptions.md`](../../docs/rules/contraptions.md) states
the rule, and [`config/contraptions.yaml`](../../config/contraptions.yaml)
holds the shield's range and energy.

Each fight is its layout fought once in the game, read back as a fight. A hit
a shield takes is a `damage` whose target is the shield, and a projectile it
takes is removed `absorbed_by` it.
