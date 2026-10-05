# Technology buffs

What a buff technology adds, to whom, and how the buff runs.
[`technology_effects.md`](../../docs/rules/technology_effects.md#buff-technologies)
is the rule; [`equipment_buff/`](../equipment_buff/README.md) measures the
same buff sources on an item.

- `combat-evolvement.yaml` puts Combat Evolvement on a Rhino among Crawlers,
  whose buff stacks every second and moves the Rhino's damage and maximum
  life with it; the corpus rounds `tests/corpus/fights/134260717-r2.yaml` and
  `134260717-r3.yaml` hold it on Rhinos taking hits.
- `mobile-power-station.yaml` and `degeneration-beam.yaml` keep a buff on the
  units around a Vortex and a Wraith: its side's ground units, and the
  enemies of either domain.
- `three-buffs.yaml` holds all three on one Rhino at once, and one of them
  ending while the others run.

A recording holds the buffs' damage rate in the `buff` channel's
`damage_rate`, their speed rate in its `move_speed_rate`, and a life rate in
the unit's own `life_rate`.
