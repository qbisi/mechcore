# Terminology

What the game and this repository call things, in English and in Simplified
Chinese. Every other tracked document is English only; this directory is
where a Chinese name is written, so that a discussion held in Chinese uses one
word for one thing and maps it back to the English the code and the documents
use.

| File | What it names | Written |
| --- | --- | --- |
| [`game.md`](game.md) | the game's own concepts: rounds, supply, formations, the shop | by hand |
| [`mechcore.md`](mechcore.md) | this repository's own concepts: fights, recordings, pins | by hand |
| [`units.md`](units.md) | units | generated |
| [`technologies.md`](technologies.md) | each unit's technologies | generated |
| [`officers.md`](officers.md) | officers | generated |
| [`commander_skills.md`](commander_skills.md) | commander skills | generated |
| [`equipment.md`](equipment.md) | equipment | generated |
| [`blueprints.md`](blueprints.md) | blueprints | generated |
| [`energy_tower_skills.md`](energy_tower_skills.md) | energy tower skills | generated |
| [`maps.md`](maps.md) | maps | generated |

A generated table is the game's official English and Simplified Chinese
names, from [`config/localization.yaml`](../../config/localization.yaml), and
`scripts/extract/name-tables.py` writes it between its markers;
`scripts/check/check-docs.py` fails when one is stale. A name the game gives
is never translated by hand.

A hand-written entry gives the English term, the Chinese one, and what it
means in a sentence. Its Chinese is the game's own wording where the game
shows one, marked `game`; otherwise it is this repository's choice, marked
`ours`, and one Chinese term stands for one English term. A new concept that
a discussion keeps needing a word for is added here rather than coined in
the discussion.
