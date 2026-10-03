# Modifier fixtures

How a correction composes with the number it corrects: each fight puts one
clause of [`officer_effects.md`](../../docs/rules/officer_effects.md)'s
formula on one unit, and its comment names the clause and what the game
answered. Most are one Marksman shooting one Rhino, differing only in their
`officers`; an Arclight shoots in the range fights, and the targeting pair
fields six formations to ask which a Ranged row reaches. The three
`officer-exp-rate-*` fights put a Marksman and an Arclight against a squad of
Crawlers, with no officer and with an experience rate on each in turn, and read
each formation's experience. The disabled
technology and the four Sledgehammer intervals, which the simulator does not
fight, are layouts in [`../../layouts/`](../../layouts/README.md).

These fights were designed so that the outcome separates the candidates, which
is why one reads a tick count and another the life left. Since MCFR 0.4.0 a
recording carries each unit's derived numbers beside its corrections, so a new
clause needs no such design: put the correction on a unit, record one tick,
and read both with `show --view stats`.
