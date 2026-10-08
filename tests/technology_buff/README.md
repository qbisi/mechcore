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
- `suppression-shots.yaml` and `suppression-shots-melee.yaml` add a Void
  Eye's buff to what its hits strike, cutting a Fortress's range and leaving
  a Rhino's melee reach.
- `ignite.yaml` burns a Rhino with a Vulcan's hits and holds its Field
  Maintenance off while it burns.
- `counter-fire.yaml` adds Counter-Fire's range to a Fire Badger from the
  tick a Marksman's hit takes life from it.
- `replicate.yaml` and `replicate-swarm.yaml` make Crawlers of a Marksman
  and a Rhino killed under the Crawlers' buff.
- `kinetic-charge.yaml` and `kinetic-charge-stops.yaml` stack Kinetic
  Charge's range on Steel Balls as they roll, and hold it as they stop; the
  corpus round `tests/corpus/fights/268477093-r4.yaml` stacks it to 80.

A recording holds the buffs' damage rate in the `buff` channel's
`damage_rate`, their speed rate in its `move_speed_rate`, their range in its
`attack_range_add_value` and `attack_range_rate`, and a life rate in the
unit's own `life_rate`.
