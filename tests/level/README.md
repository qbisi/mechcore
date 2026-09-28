# Unit levels

How a unit's level scales its base life and damage, and where it sits against
an overlay's rate. [`unit_levels.md`](../../docs/rules/unit_levels.md) is the
rule. The control is the level-one Marksman of
[`../regression/fights/marksman-vs-arclight.yaml`](../regression/fights/marksman-vs-arclight.yaml);
each fight here changes one thing from it, which its comment names.
`crates/simulation/src/data.rs`'s tests check the same numbers without a fight.
