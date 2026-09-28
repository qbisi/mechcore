# Rhino

The Rhino's standard fights: the six
[standard layouts](../README.md#standard-unit-layouts) under seeds 4242 and
1787720817, 10 fight documents in `fights/`, each named for its layout and
seed. CI verifies them; `scripts/record-fights.py --check
tests/rhino/fights/*.yaml` records them again where the game runs.

The Rhino has no M2: it is its own opponent there. Its M6 with seed
1787720817 was the last reference fight to play back: a Crawler pushed out of
reach during its backswing, and back on the next tick, starts its next blow on
the tick it returns, which a skill-state capture of the game read and the
simulator had deferred a tick
([`architecture.md`](../../docs/spec/simulation/architecture.md)).
