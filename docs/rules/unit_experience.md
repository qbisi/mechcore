# Unit experience

This index is pinned to game build 2259. It states how much experience fills
each level of every unit a standard 1v1 match sells, the formula those amounts
follow, what a full bar means, and what Intensive Training does with it.

A unit's `exp` is its experience within its current level. Paying to
upgrade a unit starts the new level at zero, whatever the old level held;
[`action.md`](../spec/document/action.md#upgrade_unit) states that rule. A
document writes `exp` as `current/maximum`, such as `124/450`, whose `maximum`
is the full bar of the unit's level from 1 through 9.

The machine-readable table is
[`config/unit_experience.yaml`](../../config/unit_experience.yaml).
`scripts/extract_prices.py` copies it verbatim out of the build's
`mechExpDatas`, whose `upgradeLv2` through `upgradeLv9` are the eight columns
below.

## Where a bar is full

A unit's bar is full at `GetUpgradeExp(level + 1)`, and level 9 fills at
the same amount as level 8. Both halves are read from the build's code rather
than measured:

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

The column for level n below is therefore the full bar of a unit at level
n, and level 9 repeats level 8's column.

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

Vulcan is the one exception. Its first level needs 2200, and its levels 2
through 8 follow the formula from a base of 2420, which is 2200 × 1.1. The
table is what a reader should use; the formula is a statement about how the
table was built, and it does not describe Vulcan's first column.

## Intensive Training fills the bar

Commander skill `1100001`, 强化训练 (Intensive Training), sets the unit it
targets to a full bar: `current` becomes the `maximum` of its level, whatever it
held before. It changes the level of nothing, costs nothing to
release, and does its work during deployment, so it is not a release the fight
sees; [`action.md`](../spec/document/action.md#release_commander_skill) states
the transition.

It cannot target a unit at level 9, or one whose bar is already full.
Both refusals were observed in Training Ground on this build, and the second is
the `IsExpMax` check its availability test makes. A level 9 unit can still
hold a full bar; it cannot be trained into one.

Every release of it in the local observation set reaches exactly the table's
value, across units at levels 1 through 4. Levels 5 through 8 are
unobserved.

A full bar is also what a fight can leave behind. A unit that no training
touched can open a round holding exactly its level's bar, so a full bar is a
state a position holds rather than a trace of the skill. When a full bar turns into a
level, and what a fight does with experience past it, belong to the fight and
are not established here.

## Experience per level

Level 9's full bar equals level 8's column.

| ID | Unit | 单位 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 |
| ---: | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | Fortress | 堡垒 | 1600 | 3605 | 5699 | 7184 | 8337 | 9278 | 10074 | 10764 |
| 2 | Marksman | 长弓 | 650 | 1465 | 2315 | 2919 | 3387 | 3769 | 4093 | 4373 |
| 3 | Vulcan | 火神 | 2200 | 5453 | 8620 | 10866 | 12609 | 14033 | 15237 | 16280 |
| 4 | Melting Point | 熔点 | 1400 | 3154 | 4987 | 6286 | 7295 | 8118 | 8815 | 9418 |
| 5 | Rhino | 犀牛 | 1150 | 2591 | 4096 | 5164 | 5992 | 6669 | 7241 | 7736 |
| 6 | Wasp | 兵蜂 | 900 | 2028 | 3206 | 4041 | 4689 | 5219 | 5667 | 6055 |
| 7 | Mustang | 野马 | 1200 | 2704 | 4274 | 5388 | 6253 | 6959 | 7556 | 8073 |
| 8 | Steel Ball | 钢球 | 1000 | 2253 | 3562 | 4490 | 5210 | 5799 | 6296 | 6727 |
| 9 | Fang | 尖牙 | 600 | 1352 | 2137 | 2694 | 3126 | 3479 | 3778 | 4036 |
| 10 | Crawler | 爬虫 | 450 | 1014 | 1603 | 2021 | 2345 | 2609 | 2833 | 3027 |
| 11 | Overlord | 霸主 | 1750 | 3943 | 6233 | 7858 | 9118 | 10148 | 11019 | 11773 |
| 12 | Stormcaller | 暴雨 | 1100 | 2479 | 3918 | 4939 | 5732 | 6379 | 6926 | 7400 |
| 13 | Sledgehammer | 铁锤 | 1300 | 2929 | 4630 | 5837 | 6774 | 7539 | 8185 | 8746 |
| 14 | Hacker | 骇客 | 560 | 1262 | 1995 | 2515 | 2918 | 3247 | 3526 | 3767 |
| 15 | Arclight | 弧光 | 750 | 1690 | 2671 | 3368 | 3908 | 4349 | 4722 | 5046 |
| 16 | Phoenix | 凤凰 | 1300 | 2929 | 4630 | 5837 | 6774 | 7539 | 8185 | 8746 |
| 17 | War Factory | 战争工厂 | 3200 | 7210 | 11398 | 14369 | 16673 | 18556 | 20148 | 21528 |
| 18 | Wraith | 恶灵 | 2250 | 5070 | 8014 | 10103 | 11724 | 13047 | 14167 | 15137 |
| 19 | Scorpion | 狂蝎 | 1500 | 3380 | 5343 | 6735 | 7816 | 8698 | 9445 | 10091 |
| 20 | Fire Badger | 火獾 | 1400 | 3154 | 4987 | 6286 | 7295 | 8118 | 8815 | 9418 |
| 21 | Sabertooth | 剑齿虎 | 1300 | 2929 | 4630 | 5837 | 6774 | 7539 | 8185 | 8746 |
| 22 | Typhoon | 台风 | 1950 | 4394 | 6946 | 8756 | 10160 | 11308 | 12278 | 13118 |
| 23 | Sandworm | 沙虫 | 2000 | 4506 | 7124 | 8981 | 10421 | 11598 | 12593 | 13455 |
| 24 | Tarantula | 狼蛛 | 1500 | 3380 | 5343 | 6735 | 7816 | 8698 | 9445 | 10091 |
| 25 | Phantom Ray | 鬼鳐 | 1000 | 2253 | 3562 | 4490 | 5210 | 5799 | 6296 | 6727 |
| 26 | Farseer | 先知 | 1800 | 4056 | 6411 | 8083 | 9379 | 10438 | 11334 | 12109 |
| 27 | Raiden | 雷霆 | 2400 | 5408 | 8548 | 10777 | 12505 | 13917 | 15111 | 16146 |
| 28 | Hound | 猎犬 | 750 | 1690 | 2671 | 3368 | 3908 | 4349 | 4722 | 5046 |
| 29 | Abyss | 深渊 | 4800 | 10815 | 17097 | 21553 | 25010 | 27835 | 30223 | 32291 |
| 30 | Void Eye | 魔眼 | 650 | 1465 | 2315 | 2919 | 3387 | 3769 | 4093 | 4373 |
| 31 | Vortex | 磁暴 | 750 | 1690 | 2671 | 3368 | 3908 | 4349 | 4722 | 5046 |
| 2002 | Mountain | 泰山 | 3200 | 7210 | 11398 | 14369 | 16673 | 18556 | 20148 | 21528 |
