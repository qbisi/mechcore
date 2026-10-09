# Move abilities

What a technology adds to its unit's move ability: what it makes, writes or
repairs as the unit burrows, moves below or surfaces.
[`underground.md`](../../docs/rules/underground.md) is the rule. Each fight
puts one technology on blue's Sandworm, which a Rhino and a Marksman meet:
Replicate, whose Larvas the Sandworm makes as it surfaces, Burrow
Maintenance, whose repair runs only while it is below, and Strike, which
shortens its surfacing and strengthens its first attack after it. The move ability
itself, without technologies, is `sandworm/`'s
[standard fights](../README.md#standard-unit-layouts).

A recording holds the units made in their `unit_created` events and the
units table, the transitions in each unit's `motion_state`, a repair in the
unit's `life`, and the first attack's strength in its skill's
`attack_damage` and `splash_range`.
