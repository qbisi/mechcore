# Commander skill cooldowns

How a commander skill's cooldown counts, per skill and per round. The replays
behind this rule are build 1.11.1.3.2259's.

[`config/commander_skills.yaml`](../../config/commander_skills.yaml) holds each
skill's two cooldowns in rounds, which `scripts/extract_prices.py` reads out of
every `CommanderSkillGroupData` row: `initial_cooldown` is `initialCoolDown`
and `cooldown` is `releaseInterval`.

## The rule

A slot joins the panel at its skill's `initial_cooldown`. As each round opens,
a slot the round before spent restarts at its skill's `cooldown`, and every
other slot drops by one, down to 0. A round spends a slot by releasing its
skill, and a deployment skill such as Intensive Training or Redeploy spends its
slot the same way. A skill whose `cooldown` is 0, such as Field Recovery or
Mobile Beacon, can be spent every round.

A replay's snapshot of a round is taken before that round's count-down, so the
cooldowns it records are the previous round's.

Build 2.0 starts some skills on a cooldown when they join (Nuke, Lightning
Storm and Ion Bombardment at 1, where 2259 started every skill at 0), and gives
Unit Recycle a `releaseInterval` of -1. The getter is inlined wherever it is
read, so what -1 does is not read; the rule above is not stated for it.

## Evidence

The rule was checked against the 2259 replay corpus `replay/REPLAY_REV` names:
a spent skill shows its `cooldown` as the next round opens, an unspent slot
drops by one, and a slot joins at its `initial_cooldown`. With the round-opening
shop allowance and equipment income, it reproduces the native opening of every
side-round `scripts/verify-battles.py` replays. A skill the corpus never spends
has its cooldowns read, not observed.
