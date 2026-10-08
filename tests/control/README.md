# Control

What a control beam turns, what it strikes instead, and what an item or a
shield changes about it. [`control.md`](../../docs/rules/control.md) states the
rule. The Hacker's own fights, in which its beam turns Crawlers, Rhinos and
another Hacker, are its standard fights in [`../hacker/`](../hacker/fights/).

Each fight here is its layout fought once in the game, read back as a fight.
A hit the beam strikes is a `damage` from the Hacker; a hit that turns writes
none, and shows in the unit's `control`, the progress its beams have added and
whose beams they are. They were recorded with:

```sh
mechcore convert <layout> --to mcfr --backend game <out.mcfr> --seed 4242 --instrument target_refs
```
