# Repair

How a unit repairs itself while it is hurt: when it starts, how often, by
how much. [`combat.md`](../../docs/rules/combat.md#repair) is the rule. Each
fight gives one unit a repair source and changes nothing else from its
control, a fight pinned elsewhere that it names: an item, Nano Repair Kit,
and a technology, Field Maintenance. The two repair by the same numbers, so
no fight here tells which of them a unit with both keeps.

A recording holds each repair as a `healing` event on the unit repaired.
