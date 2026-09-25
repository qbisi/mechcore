# Unit experience

How much experience fills each level of a unit, the formula those amounts
follow, what a full bar means, and what Intensive Training does with it.

A unit's `exp` is its experience within its current level. Paying to
upgrade a unit starts the new level at zero, whatever the old level held;
[`action.md`](../spec/document/action.md#upgrade_unit) states that rule. A
document writes `exp` as `current/maximum`, such as `124/450`, whose `maximum`
is the full bar of the unit's level from 1 through 9.

The machine-readable table is
[`config/unit_experience.yaml`](../../config/unit_experience.yaml).
`scripts/extract_prices.py` copies it verbatim out of the build's
`mechExpDatas`, whose `upgradeLv2` through `upgradeLv9` are its eight entries
per unit, and checks the formula below on every row it writes.

## Where a bar is full

A unit's bar is full at `GetUpgradeExp(level + 1)`, unless something has
replaced its bar (below), and level 9 fills at
the same amount as level 8. Both halves are read from the build's code:

- `MechExpData.PreProcess` builds the list `GetUpgradeExp` reads as
  `[0, upgradeLv2, …, upgradeLv9]`, indexed by `CardLevel`, whose `Level1` is
  0. The entry at a level is the experience it takes to arrive there.
- Every caller that sets or checks a unit's bar asks for the level above
  its own: `UnitSystem.ChangeLevel` passes the new level plus one to
  `UnitUtility.GetUpgradeExp`, and `CardElement` does the same through
  `CardData.GetUpgradeExp`.
- `MechExpData.GetData` clamps the index to the list's last entry, and nothing
  between it and those callers checks the level. So a level 9 unit, asking
  for a tenth level that does not exist, reads `upgradeLv9`.

The table's n-th entry is therefore the full bar of a unit at level n, and
level 9 repeats level 8's.

## What a full bar means

A full bar is a state the game keeps, not only a number it compares:
`MechTeam` carries `hasEnterMaxExp` beside its experience, and `IsExpMax`
answers from it. Three kinds of reader consult it:

- `ExpSystem.IsValidOwner`, which decides which units share the
  experience a fight hands out, so a full unit takes no further share;
- `CS_AddExp.CheckAvaliable`, where Intensive Training decides whether a
  unit is a target it can take, which is why it refuses a full one;
- the upgrade icon and the AI's upgrade action, consistent with a full bar
  being one of the conditions a level-up rests on. The other conditions, and
  when a full bar becomes a level, are the fight's and round's, and not
  established here.

## The formula

Every row but Vulcan's is one number, the first level's experience, times a
factor that depends only on the level, rounded half up:

```text
upgrade_exp(unit, n) = round_half_up(base(unit) × F(n)),  base(unit) = upgrade_exp(unit, 1)
```

| Level n | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| F(n) | 1 | 2.25321 | 3.5618 | 4.49028 | 5.21046 | 5.79888 | 6.29639 | 6.72735 |

The factors are fitted, not read: no shipped field carries them. Each is a
value that every standard row admits once rounded half up, and the admissible
interval is narrow enough that the five-digit figure above is not a choice
between materially different answers. Floor and ceiling rounding admit no
common factor at all, so the rounding is half up.

Vulcan is the one exception: its levels 2 through 8 follow the formula from a
base 1.1 times its own first level. The table is what a reader should use; the
formula is a statement about how the table was built, and it does not describe
Vulcan's first entry.

## Intensive Training fills the bar

Commander skill `1100001`, 强化训练 (Intensive Training), sets the unit it
targets to a full bar: `current` becomes the `maximum` of its level, whatever it
held before. It changes the level of nothing, costs nothing to
release, and does its work during deployment, so it is not a release the fight
sees; [`action.md`](../spec/document/action.md#release_commander_skill) states
the transition.

It cannot target a unit at level 9, or one whose bar is already full; the
second is the `IsExpMax` check its availability test makes. A level 9 unit can
still hold a full bar; it cannot be trained into one.

A full bar is also what a fight can leave behind. A unit that no training
touched can open a round holding exactly its level's bar, so a full bar is a
state a position holds rather than a trace of the skill. When a full bar turns
into a level, and what a fight does with experience past it, belong to the
fight and are not established here.

## Evidence

### Read

- The list a bar is read from is `[0, upgradeLv2, …, upgradeLv9]`, indexed by
  level, and the index is clamped to its last entry: `MechExpData.PreProcess`,
  `MechExpData.GetUpgradeExp`, `MechExpData.upgradeLv2`.
- A unit's bar is the entry for the level above its own:
  `UnitSystem.ChangeLevel`, `UnitUtility.GetUpgradeExp`,
  `CardData.GetUpgradeExp`.
- A full bar is kept as a state: `MechTeam.hasEnterMaxExp`, `MechTeam.IsExpMax`,
  `CardElement.IsExpMax`.
- A unit's bar is the table's entry unless its data set holds an
  `UpgradeExp`, which then replaces it: `CardElement.GetNextLevelExp`.
- A full unit takes no share of a fight's experience: `ExpSystem.IsValidOwner`.
- Intensive Training refuses a full unit: `CS_AddExp.CheckAvaliable`.
- Every row but Vulcan's follows the formula; `scripts/extract_prices.py`
  checks it on every row it writes, which it did on this version's table:
  `MechExpData.upgradeLv2`.

### Not established

- **Intensive Training's refusals in play.** That it refuses a level 9 unit
  and a full one was observed in the Training Ground on another version; no
  test pins it.
- **Intensive Training reaching exactly the table's value.** Every release of
  it in another version's corpus did, at levels 1 through 4. This version's
  corpus holds 63 releases, but its replay leaves a unit's experience to the
  fight and compares none: `scripts/verify-battles.py`. Levels 5 through 8
  are unobserved.
- **When a full bar becomes a level**, and what a fight does with experience
  past it.
- **What writes a unit's `UpgradeExp`.** An officer's `expChangeRate` is the one
  table field that touches experience; whether it reaches the bar through this
  value is not read, and no recording has a bar changed.
