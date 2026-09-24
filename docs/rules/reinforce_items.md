# Reinforcement cards

Every round from the second on, a match deals both sides the same four cards and
each side takes one. This document says what the five kinds of card are, what
taking one changes, and what it costs. The catalogues themselves are machine
readable and live in `config/`.

The evidence is the build 1.11.1.3.2259 replay corpus `replay/REPLAY_REV` names,
which `replay/README.md` describes.

## The five kinds

| Kind | What taking it changes | Catalogue |
| --- | --- | --- |
| Commander skill | The skill joins `battle_skills` | [`config/reinforce_items.yaml`](../../config/reinforce_items.yaml) |
| Equipment | The item joins the side's `equipment` | the same file |
| Officer | The officer joins a state's `officers` | the same file, effects in [`config/officers.yaml`](../../config/officers.yaml) |
| Unit | Units arrive | [`config/unit_reinforcements.yaml`](../../config/unit_reinforcements.yaml) |
| Advance team | The round 0 opening, a force or a specialist officer | [`config/advance_teams.yaml`](../../config/advance_teams.yaml) |

The first three grant the thing their own ID names. There is no second mapping
to look up: taking card `13030001` adds equipment `13030001`, and taking card
`1100001` puts commander skill `1100001` on the panel. In the corpus every
equipment and commander skill card taken puts its own ID into the side's next
snapshot, and every officer card does too unless the side already held that
officer.

That is why `config/reinforce_items.yaml` carries a `kind`: the ID alone says
what the thing is, but not which of the three fields it belongs in.

A unit card is the exception. Its ID names the card and not the units, so its
table states which unit it hands out, how many squads of it, the level they
arrive at, and the first round it can be offered. No card mixes two kinds of
unit; `scripts/extract_prices.py` refuses one that does.

## The opening

An advance team is the same shape of thing, chosen once in round 0. In the
corpus every side takes exactly one, and its round 1 roster is exactly the unit
list of the team it took.

Two kinds share that one choice, and
[`config/advance_teams.yaml`](../../config/advance_teams.yaml) holds both. A team
hands out a force of units. A specialist grants the officer its own ID names
instead: Marksman Specialist unlocks its unit and hands out a squad of it,
Supply Specialist adds to every round's income, Missile Specialist puts
commander skills on the panel.

Both move the reactor core by their row's `reactorCore`, and that is how the
stronger openings are paid for: no opening costs supply.

`BattleInfo.EnableAdvanceTeam` is true in every ranked match of the corpus and
false in the Training Ground ones, so the opening is part of standard play
rather than of a mode this format does not describe.

## What a card costs

Taking a card is not free. A card either carries its own price or is sold at its
level's price, `reinforceItemPrices`, which
[`config/economy.yaml`](../../config/economy.yaml) states as `reinforce_levels`.

A price of `-1` in the game's own data means the level decides, and the
extracted tables resolve it, so every row in `config/` states a price a ledger
can charge.

## What the pool will not deal

Two filters decide whether a card is in the tables at all.

`limitedScene` names the modes a card belongs to, and a card that names others
and not the standard one is left out. An empty list restricts nothing rather
than everything: that is how Field Recovery is listed, and reading it the other
way dropped the one skill supply most needs to price.

`scope` says whether the reinforcement pool can deal the card. Only one value
means it can: every card chosen in the corpus carries it, and no card carrying
another was ever chosen. Two familiar groups carry another:

- a commander skill a blueprint researches. Every blueprint-granted skill is
  excluded, among them Sticky Oil Bomb, Field Recovery, Mobile Beacon and
  Interference Beacon. A side gets them by activating the blueprint, and its
  `grants_skill` in [`config/economy.yaml`](../../config/economy.yaml) says which.
- an officer that belongs to an opening rather than to the pool, which is where
  the specialists are, and an officer neither route reaches, such as Giant
  Hunter and Giant Slayer.

The filter is not universal. Unit cards and advance teams carry a different
value and are dealt all the same, so `scope` is read for the three kinds whose
ID is the thing they grant and for nothing else. Officers excluded here still
appear in [`config/officers.yaml`](../../config/officers.yaml), because a side that
holds one through its opening still gets its discount.

## What an officer does beyond joining the list

An officer is the only kind of card that keeps changing the match after it
arrives. [`config/officers.yaml`](../../config/officers.yaml) states the ones
that do, and the shapes are these:

- a discount on buying, unlocking or upgrading a unit, scoped to an explicit
  list of units, or on researching a technology, which is not scoped;
- an addition to every round's income, or to the first round's alone, or a lump
  sum granted once;
- a bounty the fight pays for destroying a giant, which is the one effect no
  document can predict;
- a commander skill, an equipment, or an opening unit the officer hands
  out, which is why taking an officer card can put a skill on the panel.

## Where the tables come from

`scripts/extract_prices.py` writes all of them from one build's typed export:
the officers, unit cards and openings of `ConfigDataContainer`, and the
commander skill and equipment cards of `CommanderSkillGroupData` and
`EquipmentGroupData` in `level0`. Build 2.0 adds an appear condition that
depends on both sides' investment, which [reinforcements.md](reinforcements.md)
states.
