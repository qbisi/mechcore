# Armour

What an armour technology takes off each hit on its unit.
[`combat.md`](../../docs/rules/combat.md#armour) is the rule. Each fight puts
an armour technology on a unit that is shot while it fights: Armor
Enhancement on a Rhino at levels 1 and 3, whose recordings hold the
reduction each level reads, and Mountain Plating on a Mountain shot by hits
smaller than its reduction.

A recording holds the reduction in the unit's `reduce_damage_value`
modifier, and each hit's `damage` event the life it took after it.
