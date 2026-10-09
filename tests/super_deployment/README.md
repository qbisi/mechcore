# Flank deployments

What a formation deployed on a flank does in a fight: where its members
stand, and how a travelling one comes in. [`unit-rules.md`](../../docs/spec/simulation/unit-rules.md)
states how a formation's grid is laid out and turned, and
[`super_deployment.md`](../../docs/rules/super_deployment.md) how a travelling
unit arrives. `flank-facing.yaml` is settled, its flank units legacy; every other fight here travels
in round 2, and `arrives.yaml` is the control the rest each change one thing
from, but `extra-weapons.yaml`, which travels on all four flanks at once.

A travelling unit's reactive armor, written only as it arrives, is in
[`../reactive_armor/typhoon-travelling.yaml`](../reactive_armor/typhoon-travelling.yaml).
