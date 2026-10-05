# Secondary damage

What a technology's second damage deals around each of its unit's hits:
whom it reaches, how much, and what scales it.
[`combat.md`](../../docs/rules/combat.md#a-second-damage-around-a-hit) is the
rule. Each fight puts Shockwave on Arclights among Crawlers; the corpus round
`tests/corpus/fights/268447927-r6.yaml` holds the second damage raised by a
struck unit's damage taken.

A recording holds the second damage as `damage` events of the shell that
carried the hit, after the hit's own.
