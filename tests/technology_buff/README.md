# Technology buffs

What a buff technology adds to its unit, and how the buff runs.
[`technology_effects.md`](../../docs/rules/technology_effects.md#buff-technologies)
is the rule; [`equipment_buff/`](../equipment_buff/README.md) measures the
same buff sources on an item. The fight puts Combat Evolvement on a Rhino
among Crawlers, whose buff stacks every second and moves the Rhino's damage
and maximum life with it; the corpus rounds
`tests/corpus/fights/134260717-r2.yaml` and `134260717-r3.yaml` hold it on
Rhinos taking hits.

A recording holds the buff's damage rate in the `buff` channel's
`damage_rate` and its life rate in the unit's own `life_rate`.
