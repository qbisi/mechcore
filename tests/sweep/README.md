# Sweep

What a sweep, the Abyss's main skill, strikes beyond its standard fights in
[`../abyss/`](../abyss/). [`sweep.md`](../../docs/rules/sweep.md) states
the rule. Each fight here is its layout fought once in the game, read back as
a fight; a strike is a `damage` from the Abyss. They were recorded with the
`target_refs` channel:

```sh
mechcore convert <layout> --to mcfr --backend game <out.mcfr> --seed <seed> --instrument target_refs
```
