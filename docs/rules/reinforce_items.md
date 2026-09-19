# Reinforcement cards

Every round from the second on, a match deals both sides the same four cards and
each side takes one. This document says what the five kinds of card are, what
taking one changes, and what it costs. The catalogues themselves are machine
readable and live in `config/`.

Evidence is the locally recorded ranked replays of this machine's Steam
installation, and `tests/grbr/README.md` explains which replays are usable.

## The five kinds

| Kind | What taking it changes | Catalogue |
| --- | --- | --- |
| Commander skill | The skill joins `battle_skills` | [`config/reinforce_items.yaml`](../../config/reinforce_items.yaml) |
| Equipment | The item joins the side's `equipment` | the same file |
| Officer | The officer joins a state's `officers` | the same file, effects in [`config/officers.yaml`](../../config/officers.yaml) |
| Unit | Formations arrive | [`config/unit_reinforcements.yaml`](../../config/unit_reinforcements.yaml) |
| Advance team | The round 0 opening, a force or a specialist officer | [`config/advance_teams.yaml`](../../config/advance_teams.yaml) |

The first three grant the thing their own ID names. There is no second mapping
to look up: taking card `13030001` adds equipment `13030001`, and taking card
`1100001` puts commander skill `1100001` on the panel. Across the corpus 38
equipment cards and 29 commander skill cards put their own ID into the side's
next snapshot, and 24 of 26 officer cards do the same, the two exceptions being
officers a side already held.

That is why `config/reinforce_items.yaml` carries a `kind`: the ID alone says
what the thing is, but not which of the three fields it belongs in.

A unit card is the exception. Its ID names the card and not the units, so its
table states which unit it hands out, how many squads of it, the level they
arrive at, and the first round it can be offered. No card in build 2259 mixes
two kinds of unit.

## The opening

An advance team is the same shape of thing, chosen once in round 0. Every one of
the 26 player slots takes exactly one, and the round 1 roster of every one of
them is exactly the unit list of the team it took.

Two kinds share that one choice, and
[`config/advance_teams.yaml`](../../config/advance_teams.yaml) holds both. A team
hands out a force of five formations. A specialist grants the officer its own ID
names instead, and 16 of them can be picked: Marksman Specialist unlocks its
unit and hands out a rank 3 squad of it, Supply Specialist adds 50 to every
round, Missile Specialist puts two Missile Strikes on the panel.

Both move the reactor core, and that is how the stronger openings are paid for.
A team's adjustment runs from -300 to +700 and a specialist's from -600 to +500:
Supply Specialist costs 600 core for its income, while Fast Supply Specialist
costs 500 for 200 supply in round 1.

`BattleInfo.EnableAdvanceTeam` is true in all 13 ranked matches and false in
both Training Ground ones, so the opening is part of standard play rather than
of a mode this format does not describe.

## What a card costs

Taking a card is not free. A card either carries its own price or is sold at its
level's price:

| Level | Price |
| ---: | ---: |
| 1 | 0 |
| 2 | 50 |
| 3 | 100 |
| 4 | 200 |

A price of `-1` in the game's own data means the level decides, and the
extracted tables resolve it, so every row in `config/` states a price a ledger
can charge. Missile Strike and Shield Airdrop are level 2 and cost 50 each,
which is how the rule was found.

## What the pool will not deal

Two filters decide whether a card is in the tables at all.

`limitedScene` names the modes a card belongs to, and a card that names others
and not the standard one is left out. An empty list restricts nothing rather
than everything: that is how Field Recovery is listed, and reading it the other
way dropped the one skill supply most needs to price.

`scope` says whether the reinforcement pool can deal the card. Only one value
means it can, and the corpus is unambiguous about it: of the 120 distinct cards
chosen across the local set, every one carries that value, and no card carrying
another was ever chosen. Two familiar groups carry another:

- a commander skill a blueprint researches. All twelve blueprint-granted skills
  are excluded, among them Sticky Oil Bomb, Field Recovery, Mobile Beacon and
  Interference Beacon. A side gets them by activating the blueprint, and its
  `grants_skill` in [`config/economy.yaml`](../../config/economy.yaml) says which.
- an officer that belongs to an opening rather than to the pool, which is where
  the 16 specialists are, and an officer neither route reaches, such as Giant
  Hunter and Giant Slayer.

The filter is not universal. Unit cards and advance teams carry a different
value and are dealt all the same, so `scope` is read for the three kinds whose
ID is the thing they grant and for nothing else. Officers excluded here still
appear in [`config/officers.yaml`](../../config/officers.yaml), because a side that
holds one through its opening still gets its discount.

## What an officer does beyond joining the list

An officer is the only kind of card that keeps changing the match after it
arrives. [`config/officers.yaml`](../../config/officers.yaml) states the 81 that
do, and the shapes are these:

- a discount on buying, unlocking or upgrading a unit, scoped to an explicit
  list of units, or on researching a technology, which is not scoped;
- an addition to every round's income, or to the first round's alone, or a lump
  sum granted once;
- a bounty the fight pays for destroying a giant, which is the one effect no
  document can predict;
- a commander skill, an equipment, or an opening formation the officer hands
  out, which is why taking an officer card can put a skill on the panel.

## Where the tables come from

`scripts/extract_prices.py` writes all of them from one game build. The
commander skill and equipment catalogues are two `level0` objects that the
config data container does not carry, and the parse walks their entries by the
declaration order of `ReinforceItemData`, which every drawable card shares.
