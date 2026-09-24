# Commander skill cooldowns

This index is pinned to game build 2259. It states how a commander skill's
cooldown counts, per skill and per round, and the evidence for it.

The machine-readable table is
[`config/commander_skills.yaml`](../../config/commander_skills.yaml).
`scripts/extract_prices.py` reads it out of each `CommanderSkillData` in
`level0`: `initial_cooldown` is `initialCoolDown` and `cooldown` is
`releaseInterval`, both in rounds.

## The rule

A slot joins the panel at its skill's `initial_cooldown`, which is 0 for every
skill of this build. As each round opens, a slot the round before spent restarts
at its skill's `cooldown`, and every other slot drops by one, down to 0. A round
spends a slot by releasing its skill, and a deployment skill such as Intensive
Training or Redeploy spends its slot the same way. A skill whose `cooldown` is
0, such as Field Recovery or Mobile Beacon, can be spent every round.

A replay's snapshot of a round is taken before that round's count-down, so the
cooldowns it records are the previous round's.

## Evidence

In the local observation set, every one of the 23 skills a side spent shows its
table `cooldown` as the next round opens, across 549 spends. Every one of 487
slots a round did not spend dropped by exactly one, down to 0, and all 18 slots
seen joining a panel started at 0.

Converted with this rule, and with the round-opening shop allowance and
equipment income, the opening state of all 668 side-rounds of the 41 tracked
battles matches the native opening in every field the oracle compares.

Of the table's 43 skills, 20 were never spent in the corpus. Their cooldowns are
read and not observed.
