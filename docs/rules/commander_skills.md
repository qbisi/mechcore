# Commander skill cooldowns

How a commander skill's cooldown counts, per skill and per round.

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

Some skills join the panel already cooling down, with an `initial_cooldown`
above 0, and one, Unit Recycle, has a `cooldown` of -1. The getter is inlined
wherever it is read, so what -1 does is not read, and the rule above is not
stated for it.

## Evidence

### Replayed

- A spent skill shows its `cooldown` as the next round opens, an unspent slot
  drops by one, and a slot joins at its `initial_cooldown`, Nuke's and Ion
  Blast's 1 among them: every panel of this version's corpus:
  `scripts/verify-battles.py`.

### Read

- A skill's two cooldowns are its row's `initialCoolDown` and
  `releaseInterval`: `CommanderSkillData.initialCoolDown`,
  `CommanderSkillData.releaseInterval`.
- A skill is in its initial cooldown while its `initialCoolDown` is above 0
  and fewer rounds than that have passed since the round it joined the panel:
  `CommanderSkillBase.IsInInitialCoolDown`.

### Not established

- **A `cooldown` of -1.** What it does is not read.
- **A skill the corpus never spends.** Its cooldowns are read, not observed.
