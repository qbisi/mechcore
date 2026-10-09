# Shield

What a battlefield shield does to the hits meant for the units it covers, and
when it breaks. [`contraptions.md`](../../docs/rules/contraptions.md) states
the rule, and [`config/contraptions.yaml`](../../config/contraptions.yaml)
holds the shield's range and energy.

Each fight is its layout fought once in the game, read back as a fight. A hit
a shield takes is a `damage` whose target is the shield, and a projectile it
takes is removed `absorbed_by` it.

The Barrier fights put a shield on a unit, which carries it: `barrier-wasps.yaml`
and `barrier-crawlers.yaml`.
The `barrier-technology-*.yaml` fights give it by Barrier the technology,
by the unit's level, and switch it off with a Void Eye's Electromagnetic
Armor.

A missile's projectile meets a shield in
[`../missile/into-shield.yaml`](../missile/into-shield.yaml),
[`../missile/splash-beside-shield.yaml`](../missile/splash-beside-shield.yaml),
[`../missile/into-airdrop.yaml`](../missile/into-airdrop.yaml),
[`../missile/fired-inside-airdrop.yaml`](../missile/fired-inside-airdrop.yaml) and
[`../missile/fired-inside-standing-airdrop.yaml`](../missile/fired-inside-standing-airdrop.yaml).
