# Officer index: global effects and unit modifications

[简体中文](officers.zh.md)

[TOC]

This is the build `1.11.1.2.2227` index for IDs accepted by
`layout.yaml` under `techs.officers`. Names and effect text are the game's
official English and Simplified Chinese localizations; placeholders have been
expanded with the serialized `OfficerData.descriptionParams` values.

An Officer may grant a commander skill, equipment, or an extra formation, but
those resulting objects are not additional `techs.officers` entries. Native
`CommanderSkillData` used by `battle_skills` is a separate ID space.

An Officer ID is also the ID of the card that grants it, so taking card `20023`
adds Officer `20023`. [`config/officers.yaml`](../config/officers.yaml) states,
for build 2259, the 81 Officers that go on affecting the match after they
arrive: what they discount and for which units, what they add to a round's
income, the bounty two of them collect for destroying a giant, and the
commander skills, equipment and opening formations nine of them hand out. That
last group is why taking an Officer card can put a skill on the panel.
[`docs/reinforce_items.md`](reinforce_items.md) explains the card system, and
the effect text below has no machine-readable counterpart yet.

`Test-only` entries exist in the runtime catalog and are listed for completeness.
They are not ordinary opening or reinforcement choices. Derived/internal entries
may be owned by another layout field; in particular Research Center attack and
defense levels own IDs `20300`, `20301`, `20310`, and `20311`, so they must not be
declared again in `techs.officers`.
The nine test-only rows have no English entry in the build-2227 localization
asset; their editorial English translations are marked with `†`.

## Opening specialists

| ID | Chinese name | English name | Target | Effect |
| ---: | --- | --- | --- | --- |
| `10002` | 补给专家 | Supply Specialist | All/system | Obtain 50 additional supplies per round |
| `10010` | 快速补给专家 | Quick Supply Specialist | All/system | Obtain 200 supplies in the first round |
| `10011` | 导弹专家 | Missile Specialist | All/system | Get 2 Missile Strike for free on round 2 |
| `10013` | 增幅专家 | Amplify Specialist | All/system | Get 3 Small Amplifying Core for free on round 1 |
| `10014` | 训练专家 | Training Specialist | All/system | Obtain extra 50 supplies in the first round, and get 1 Intensive Training for free. |
| `20005` | 巨型专家 | Giant Specialist | 1 Fortress, 3 Vulcan, 4 Melting Point, 11 Overlord, 17 War Factory, 23 Sandworm, 27 Raiden, 2001 Death Knell, 29 Abyss, 2002 Mountain | Decreases unlock cost of Giant and Titan units by 200 |
| `20021` | 空军专家 | Aerial Specialist | 6 Wasp, 11 Overlord, 16 Phoenix, 18 Wraith, 25 Phantom Ray, 27 Raiden, 29 Abyss | Decreases unlock cost of Aerial units by 200, Increases their ATK by 13% and HP by 13% |
| `20024` | 速度专家 | Speed Specialist | All/system | Increases the movement speed of all units by 3 |
| `20029` | 长弓专家 | Marksman Specialist | 2 Marksman | Get 1 Rank 3 Marksman(s) for free on round 2 |
| `20032` | 精英专家 | Elite Specialist | All/system | Obtain extra 100 supplies in the first round, able to immediately recruit Rank 2 units |
| `20033` | 犀牛专家 | Rhino Specialist | 5 Rhino | Get 1 Rank 2 Rhino(s) for free on round 4 |
| `20034` | 成本控制专家 | Cost Control Specialist | All/system | Grants 100 additional supplies per round but decreases the ATK of all units by 11% and HP by 11% |
| `20035` | 重装专家 | Fortified Specialist | All/system | Increases the HP of all units by 17% |
| `20036` | 剑齿虎专家 | Sabertooth Specialist | 21 Sabertooth | Decreases Sabertooth’s Tech Upgrade cost by 50. Get 1 Rank 1 Sabertooth on Round 3. |
| `20038` | 火獾专家 | Fire Badger Specialist | 20 Fire Badger | Decreases Fire Badger’s Tech Upgrade cost by 50. Get 1 Rank 1 Fire Badger on Round 3. |
| `20039` | 台风专家 | Typhoon Specialist | 22 Typhoon | Get 1 Rank 1 Typhoon(s) for free on round 4 |

## General reinforcement Officers

| ID | Chinese name | English name | Target | Effect |
| ---: | --- | --- | --- | --- |
| `10003` | 超级补给强化 | Super Supply Enhancement | All/system | Starting from the next round, obtain 150 additional supplies per round |
| `10004` | 额外部署位 | Additional Deployment Slot | All/system | The number of units that can be deployed increases by 1 each round. |
| `10007` | 先进护盾装置 | Advanced Shield Device | All/system | Increases the Shield of Energy Shield devices by 40% |
| `10008` | 先进飞弹装置 | Advanced Missile Device | All/system | Increases the damage of Sentry Missile devices by 200% |
| `10009` | 快速传送 | Quick Teleport | All/system | Decreases the teleportation time required for strike units deployed at the enemy's rear by 50% |
| `20001` | 先进防御战术 | Advanced Defensive Tactics | All/system | Increases the HP of all units by 30% |
| `20002` | 先进进攻战术 | Advanced Offensive Tactics | All/system | Increases the attack of all units by 30% |
| `20003` | 高效科技研发 | Efficient Tech Research | All/system | Decreases the Tech upgrade cost of all units by 50 |
| `20004` | 先进动力系统 | Advanced Power System | All/system | Increases the movement speed of all units by 3 |
| `20006` | 先进瞄准系统 | Advanced Targeting System | All/system | Increases the range of all ranged units by 10 |
| `20007` | 补给强化 | Supply Enhancement | All/system | Obtain 50 additional supplies per round |
| `20022` | 高效巨型制造 | Efficient Giant Manufacturing | 1 Fortress, 3 Vulcan, 4 Melting Point, 11 Overlord, 17 War Factory, 23 Sandworm, 27 Raiden, 2001 Death Knell, 29 Abyss, 2002 Mountain | Decreases the recruitment cost of giant units by 50 |
| `20023` | 高效小型制造 | Efficient Light Manufacturing | 2 Marksman, 5 Rhino, 6 Wasp, 7 Mustang, 8 Steel Ball, 9 Fang, 10 Crawler, 12 Stormcaller, 13 Sledgehammer, 14 Hacker, 15 Arclight, 16 Phoenix, 18 Wraith, 19 Scorpion, 20 Fire Badger, 21 Sabertooth, 22 Typhoon, 24 Tarantula, 26 Farseer, 25 Phantom Ray, 28 Hound, 30 Void Eye, 31 Vortex | Decreases the recruitment cost of non-giant units by 50 |

## Unit modification Officers

### 1 — Fortress / 堡垒

| ID | Chinese name | English name | Target | Effect |
| ---: | --- | --- | --- | --- |
| `30101` | 量产堡垒 | Mass-Produced Fortress | 1 Fortress | Decreases the recruitment cost of Fortress by 100 but decreases Fortress' ATK by 35% and HP by 35% |
| `30102` | 突击堡垒 | Assault Fortress | 1 Fortress | Increases Fortress' ATK by 20%, HP by 20%, and movement speed by 3, but decreases range by 10 |
| `30104` | 改进型堡垒 | Improved Fortress | 1 Fortress | Increases Fortress' HP by 50%, movement speed by 3, Attack by 30% and increases recruitment cost by 100 |
| `30105` | 增程堡垒 | Extended Range Fortress | 1 Fortress | Increases Fortress' range by 20 but decreases HP by 30% |

### 2 — Marksman / 长弓

| ID | Chinese name | English name | Target | Effect |
| ---: | --- | --- | --- | --- |
| `30201` | 增程长弓 | Extended Range Marksman | 2 Marksman | Increases Marksman's range by 30 but decreases HP by 20% and ATK by 20% |
| `30202` | 智能长弓 | Smart Marksman | 2 Marksman | Increases Marksman's EXP Growth Rate by 75% |
| `30203` | 长弓补贴 | Subsidized Marksman | 2 Marksman | Decreases the recruitment cost of Marksman by 50 |
| `30204` | 精英长弓 | Elite Marksman | 2 Marksman | Able to immediately recruit Rank 2 Marksman |

### 3 — Vulcan / 火神

| ID | Chinese name | English name | Target | Effect |
| ---: | --- | --- | --- | --- |
| `30301` | 增程火神 | Extended Range Vulcan | 3 Vulcan | Increases Vulcan's range by 20 but decreases HP by 35% |
| `30302` | 突击火神 | Assault Vulcan | 3 Vulcan | Increases Vulcan's HP by 40% and movement speed by 3 but decreases range by 10 |

### 4 — Melting Point / 熔点

| ID | Chinese name | English name | Target | Effect |
| ---: | --- | --- | --- | --- |
| `30401` | 突击熔点 | Assault Melting Point | 4 Melting Point | Increases Melting Point's HP by 70% and movement speed by 5 but decreases range by 15 |
| `30402` | 改进型熔点 | Improved Melting Point | 4 Melting Point | Increases Melting Point's HP by 20%, and range by 10 but increases recruitment cost by 100 |
| `30403` | 量产熔点 | Mass-Produced Melting Point | 4 Melting Point | Decreases the recruitment cost of Melting Point by 100, but decreases Melting Point's range by 10, ATK by 20%, and HP by 20%. |

### 5 — Rhino / 犀牛

| ID | Chinese name | English name | Target | Effect |
| ---: | --- | --- | --- | --- |
| `30501` | 量产犀牛 | Mass-Produced Rhino | 5 Rhino | Decreases the recruitment cost of Rhino by 100 but decreases Rhino's ATK by 20% and HP by 20% |
| `30502` | 狂暴犀牛 | Berserk Rhino | 5 Rhino | Every unit Rhino kills increases its ATK by 10% for the current round |
| `30503` | 精英犀牛 | Elite Rhino | 5 Rhino | Able to immediately recruit Rank 2 Rhino |

### 6 — Wasp / 兵蜂

| ID | Chinese name | English name | Target | Effect |
| ---: | --- | --- | --- | --- |
| `30601` | 量产兵蜂 | Mass-Produced Wasp | 6 Wasp | Decreases the recruitment cost of Wasp by 100 but decreases Wasp's ATK by 35% and HP by 35% |
| `30602` | 改进型兵蜂 | Improved Wasp | 6 Wasp | Increases Wasp's ATK by 60%, HP by 60%, and range by 10 but increases recruitment cost by 100 |
| `30604` | 精英兵蜂 | Elite Wasp | 6 Wasp | Able to immediately recruit Rank 2 Wasp |

### 7 — Mustang / 野马

| ID | Chinese name | English name | Target | Effect |
| ---: | --- | --- | --- | --- |
| `30701` | 野马补贴 | Subsidized Mustang | 7 Mustang | Decreases the recruitment cost of Mustang by 50 |
| `30702` | 重装野马 | Fortified Mustang | 7 Mustang | Increases Mustang's HP by 200% but decreases range by 20 |
| `30703` | 精英野马 | Elite Mustang | 7 Mustang | Able to immediately recruit Rank 2 Mustang |

### 8 — Steel Ball / 钢球

| ID | Chinese name | English name | Target | Effect |
| ---: | --- | --- | --- | --- |
| `30801` | 钢球补贴 | Subsidized Steel Ball | 8 Steel Ball | Decreases the recruitment cost of Steel Ball by 50 |
| `30803` | 改进型钢球 | Improved Steel Ball | 8 Steel Ball | Increases Steel Ball's ATK by 50%, HP by 30%, movement speed by 3, and the range by 10, but increases recruitment cost by 50 |
| `30804` | 精英钢球 | Elite Steel Ball | 8 Steel Ball | Able to immediately recruit Rank 2 Steel Ball |

### 9 — Fang / 尖牙

| ID | Chinese name | English name | Target | Effect |
| ---: | --- | --- | --- | --- |
| `30901` | 精英尖牙 | Elite Fang | 9 Fang | Able to immediately recruit Rank 2 Fang |
| `30902` | 突击尖牙 | Assault Fang | 9 Fang | Increases Fang's ATK by 50% and movement speed by 3 but decreases range by 15 |

### 10 — Crawler / 爬虫

| ID | Chinese name | English name | Target | Effect |
| ---: | --- | --- | --- | --- |
| `31001` | 爬虫补贴 | Subsidized Crawler | 10 Crawler | Decreases the upgrade cost of Crawler by 50 |
| `31002` | 精英爬虫 | Elite Crawler | 10 Crawler | Able to immediately recruit Rank 5 Crawler |

### 11 — Overlord / 霸主

| ID | Chinese name | English name | Target | Effect |
| ---: | --- | --- | --- | --- |
| `31101` | 重装霸主 | Fortified Overlord | 11 Overlord | Increases Overlord's HP by 75% but decreases ATK by 25% |
| `31102` | 量产霸主 | Mass-Produced Overlord | 11 Overlord | Decreases the recruitment cost of Overlord by 100 but decreases Overlord's range by 10, ATK by 15%, and HP by 15% |
| `31104` | 改进型霸主 | Improved Overlord | 11 Overlord | Increases Overlord's ATK by 20%, HP by 20%, and splash range by 5 but increases recruitment cost by 100 |

### 12 — Stormcaller / 暴雨

| ID | Chinese name | English name | Target | Effect |
| ---: | --- | --- | --- | --- |
| `31201` | 突击暴雨 | Assault Stormcaller | 12 Stormcaller | Increases Stormcaller's HP by 300% and movement speed by 5 but decreases range by 30 |
| `31202` | 增程暴雨 | Extended Range Stormcaller | 12 Stormcaller | Increases Stormcaller's range by 35 but decreases ATK by 30% |
| `31203` | 暴雨补贴 | Subsidized Stormcaller | 12 Stormcaller | Decreases the recruitment cost of Stormcaller by 50 |
| `31205` | 精英暴雨 | Elite Stormcaller | 12 Stormcaller | Able to immediately recruit Rank 2 Stormcaller |

### 13 — Sledgehammer / 铁锤

| ID | Chinese name | English name | Target | Effect |
| ---: | --- | --- | --- | --- |
| `31301` | 量产铁锤 | Mass-Produced Sledgehammer | 13 Sledgehammer | Decreases the recruitment cost of Sledgehammer by 100 but decreases Sledgehammer's ATK by 30% and HP by 30% |
| `31302` | 增程铁锤 | Extended Range Sledgehammer | 13 Sledgehammer | Increases Sledgehammer's range by 30 but decreases HP by 20% and ATK by 20% |
| `31304` | 改进型铁锤 | Improved Sledgehammer | 13 Sledgehammer | Increases Sledgehammer's ATK by 100% and movement speed by 3 but increases recruitment cost by 50 |
| `31305` | 精英铁锤 | Elite Sledgehammer | 13 Sledgehammer | Able to immediately recruit Rank 2 Sledgehammer |

### 14 — Hacker / 骇客

| ID | Chinese name | English name | Target | Effect |
| ---: | --- | --- | --- | --- |
| `31402` | 重装骇客 | Fortified Hacker | 14 Hacker | Increases Hacker's HP by 500%, movement speed by 5, and ATK by 20%, but decreases range by 35 |
| `31403` | 精英骇客 | Elite Hacker | 14 Hacker | Able to immediately recruit Rank 2 Hacker |

### 15 — Arclight / 弧光

| ID | Chinese name | English name | Target | Effect |
| ---: | --- | --- | --- | --- |
| `31501` | 弧光补贴 | Subsidized Arclight | 15 Arclight | Decreases the recruitment cost of Arclight by 50 |
| `31502` | 智能弧光 | Smart Arclight | 15 Arclight | Increases Arclight's EXP Rate by 75% |
| `31503` | 重装弧光 | Fortified Arclight | 15 Arclight | Increases Arclight's HP by 180% but decreases range by 15 |
| `31504` | 增程弧光 | Extended Range Arclight | 15 Arclight | Increases Arclight's range by 20 but decreases ATK by 20% |
| `31505` | 精英弧光 | Elite Arclight | 15 Arclight | Able to immediately recruit Rank 2 Arclight |

### 16 — Phoenix / 凤凰

| ID | Chinese name | English name | Target | Effect |
| ---: | --- | --- | --- | --- |
| `31601` | 量产凤凰 | Mass-Produced Phoenix | 16 Phoenix | Decreases the recruitment cost of Phoenix by 100, but decreases Phoenix's ATK by 30% and HP by 30% |
| `31602` | 增程凤凰 | Extended Range Phoenix | 16 Phoenix | Increases Phoenix's range by 20 but decreases ATK by 20% |
| `31603` | 改进型凤凰 | Improved Phoenix | 16 Phoenix | Increases Phoenix's ATK by 20% and HP by 250% but increases recruitment cost by 100 |
| `31604` | 精英凤凰 | Elite Phoenix | 16 Phoenix | Able to immediately recruit Rank 2 Phoenix |

### 17 — War Factory / 战争工厂

| ID | Chinese name | English name | Target | Effect |
| ---: | --- | --- | --- | --- |
| `31701` | 增程战争工厂 | Extended Range War Factory | 17 War Factory | Increases War Factory's range by 20, but decreases HP by 40% and ATK by 20%. |
| `31702` | 改进型战争工厂 | Improved War Factory | 17 War Factory | Increases War Factory's HP by 40%, ATK by 15%, range by 10, and decreases attack interval by 0.2, but increases recruitment cost by 200. |

### 18 — Wraith / 恶灵

| ID | Chinese name | English name | Target | Effect |
| ---: | --- | --- | --- | --- |
| `31801` | 量产恶灵 | Mass-Produced Wraith | 18 Wraith | Decreases the recruitment cost of Wraith by 100, but decreases Wraith's ATK by 20% and HP by 20% |
| `31802` | 改进型恶灵 | Improved Wraith | 18 Wraith | Increases Wraith's ATK by 30% and range by 5 but increases recruitment cost by 50. |

### 19 — Scorpion / 狂蝎

| ID | Chinese name | English name | Target | Effect |
| ---: | --- | --- | --- | --- |
| `31901` | 突击狂蝎 | Assault Scorpion | 19 Scorpion | Increases Scorpion's HP by 20%, movement speed by 3, and decreases attack interval by 1.5s, but decreases range by 30. |
| `31902` | 量产狂蝎 | Mass-Produced Scorpion | 19 Scorpion | Decreases the recruitment cost of Scorpion by 100, but decreases Scorpion's ATK by 20%, HP by 20% and range by 10 |
| `31903` | 改进型狂蝎 | Improved Scorpion | 19 Scorpion | Increases Scorpion's ATK by 15%, HP by 15%, and range by 10 but increases recruitment cost by 50. |

### 21 — Sabertooth / 剑齿虎

| ID | Chinese name | English name | Target | Effect |
| ---: | --- | --- | --- | --- |
| `32101` | 增程剑齿虎 | Extended Range Sabertooth | 21 Sabertooth | Increases Sabertooth's range by 30 but decreases HP by 40% and ATK by 20% |
| `32102` | 量产剑齿虎 | Mass-Produced Sabertooth | 21 Sabertooth | Decreases the recruitment cost of Sabertooth by 100, but decreases Sabertooth's ATK by 40% and HP by 40%. |
| `32103` | 改进型剑齿虎 | Improved Sabertooth | 21 Sabertooth | Increases Sabertooth's HP by 20%,  and decreases attack interval by 0.9s, but increases recruitment cost by 50. |

### 23 — Sandworm / 沙虫

| ID | Chinese name | English name | Target | Effect |
| ---: | --- | --- | --- | --- |
| `32301` | 改进型沙虫 | Improved Sandworm | 23 Sandworm | Increases Sandworm's movement speed by 3, ATK by 40% and splash range by 5, but increases recruitment cost by 100 |
| `32302` | 量产沙虫 | Mass-Produced Sandworm | 23 Sandworm | Decreases the recruitment cost of Sandworm by 100, but decreases Sandworm's ATK by 30% and HP by 30% |

### 24 — Tarantula / 狼蛛

| ID | Chinese name | English name | Target | Effect |
| ---: | --- | --- | --- | --- |
| `32401` | 改进型狼蛛 | Improved Tarantula | 24 Tarantula | Increases Tarantula's movement speed by 3 and splash range by 4, but increases recruitment cost by 50 |
| `32402` | 精英狼蛛 | Elite Tarantula | 24 Tarantula | Able to immediately recruit Rank 2 Tarantula |

### 25 — Phantom Ray / 鬼鳐

| ID | Chinese name | English name | Target | Effect |
| ---: | --- | --- | --- | --- |
| `32501` | 增程鬼鳐 | Extended Range Phantom Ray | 25 Phantom Ray | Increases Phantom Ray's range by 30 but decreases HP by 40% |

### 26 — Farseer / 先知

| ID | Chinese name | English name | Target | Effect |
| ---: | --- | --- | --- | --- |
| `32601` | 量产先知 | Mass-Produced Farseer | 26 Farseer | Decreases the recruitment cost of Farseer by 100, but decreases Farseer's ATK by 40% and range by 20 |
| `32602` | 重装先知 | Fortified Farseer | 26 Farseer | Increases Farseer's HP by 80% but decreases ATK by 30% |

### 27 — Raiden / 雷霆

| ID | Chinese name | English name | Target | Effect |
| ---: | --- | --- | --- | --- |
| `32701` | 突击雷霆 | Assault Raiden | 27 Raiden | Increases Raiden's HP by 80% and movement speed by 3 but decreases range by 20 |

### 28 — Hound / 猎犬

| ID | Chinese name | English name | Target | Effect |
| ---: | --- | --- | --- | --- |
| `32801` | 强击猎犬 | Strike Hound | 28 Hound | Increases Hound's ATK 270%, but increases ATK interval by 2.6s |

### 30 — Void Eye / 魔眼

| ID | Chinese name | English name | Target | Effect |
| ---: | --- | --- | --- | --- |
| `33001` | 改进型魔眼 | Improved Void Eye | 30 Void Eye | Increases Void Eye's ATK by 40%, HP by 40%, and range by 5 but increases recruitment cost by 50. |

## Derived and internal Officers

| ID | Chinese name | English name | Target | Effect |
| ---: | --- | --- | --- | --- |
| `10001` | 快速冷却 | Quick Cooldown | All/system | Decreases the cooldown of all Battlefield Powers by 1 rounds |
| `10005` | 巨型猎手 | Giant Hunter | All/system | Obtain 50 supplies for every enemy giant unit destroyed |
| `10012` | 核弹专家 | Nuke Specialist | All/system | Get 1 Tactical Nuke for free on round 3 |
| `20037` | 先知专家 | Farseer Specialist | 26 Farseer | Get 1 Rank 1 Farseer(s) for free on round 4 |
| `20300` | 防御强化 | Defense Enhancement | All/system | Increases the HP of all units by 15% |
| `20301` | 防御强化II | Defense Enhancement II | All/system | Increases the HP of all units by 45% |
| `20310` | 进攻强化 | Attack Enhancement | All/system | Increases the attack of all units by 12% |
| `20311` | 进攻强化II | Attack Enhancement II | All/system | Increases the attack of all units by 36% |
| `30103` | 智能堡垒 | Smart Fortress | 1 Fortress | Increases Fortress' EXP Growth Rate by 100% |
| `30303` | 智能火神 | Smart Vulcan | 3 Vulcan | Increases Vulcan's EXP Growth Rate by 100% |
| `30603` | 狂暴兵蜂 | Berserk Wasp | 6 Wasp | Every unit Wasp kills increases its ATK by 20% and HP by 20% for the current round |
| `30802` | 增程钢球 | Extended Range Steel Ball | 8 Steel Ball | Increases Steel Ball's range by 40 but decreases HP by 40% and ATK by 30% |
| `31003` | 改进型爬虫 | Improved Crawler | 10 Crawler | Increases Crawler's ATK by 80% and HP by 40 but increases recruitment cost by 50 |
| `31103` | 智能霸主 | Smart Overlord | 11 Overlord | Increases Overlord's EXP Growth Rate by 100% |
| `31204` | 改进型暴雨 | Improved Stormcaller | 12 Stormcaller | Increases Stormcaller's HP by 400% and movement speed by 5, but decreases range by 20 and increases recruitment cost by 50 |
| `31303` | 狂暴铁锤 | Berserk Sledgehammer | 13 Sledgehammer | Every unit Sledgehammer kills increases its ATK by 8% and HP by 8% for the current round |
| `31401` | 增程骇客 | Extended Range Hacker | 14 Hacker | Increases Hacker's range by 30 but decreases ATK by 30% |
| `50008` | 补给增援 | Supply Reinforcements | All/system | Gain 400 Supply. |
| `50009` | 补给增援 | Supply Reinforcements | All/system | Gain 500 Supply. |
| `50010` | 补给增援 | Supply Reinforcements | All/system | Gain 600 Supply. |
| `50011` | 补给增援 | Supply Reinforcements | All/system | Gain 700 Supply. |
| `50012` | 补给增援 | Supply Reinforcements | All/system | Gain 800 Supply. |
| `50013` | 补给增援 | Supply Reinforcements | All/system | Gain 900 Supply. |

## Test-only Officers

| ID | Chinese name | English name | Target | Effect |
| ---: | --- | --- | --- | --- |
| `40001` | 巨型杀手 | Giant Slayer† | 1 Fortress, 3 Vulcan, 4 Melting Point, 11 Overlord, 17 War Factory, 23 Sandworm, 27 Raiden, 2001 Death Knell, 29 Abyss, 2002 Mountain | Reduces unlock and recruitment costs of all Giant and Titan units by 50, and grants 50 supplies whenever an enemy Giant or Titan unit is destroyed |
| `40002` | 数不过三 | Three's the Limit† | All/system | Reduces all unit unlock, recruitment, and technology upgrade costs by 50 |
| `40003` | 没有单位增援 | No Unit Reinforcements† | All/system | Grants 50 additional supplies each round, but unit reinforcements will not appear in this match |
| `40004` | 量产护盾装置 | Mass-Produced Shield Device† | All/system | Reduces Shield Device cost by 50 and Shield Device shield strength by 50% |
| `40005` | 紧急避险 | Emergency Evasion† | All/system | When one side's HP falls below 1, resets it to 1 and grants an Orbital Javelin, while the opponent receives a Nuclear Missile |
| `40006` | 能量塔过载 | Energy Tower Overload† | All/system | Increases Energy Tower HP and upgraded HP by 100%, and increases disable duration by 5 seconds |
| `40007` | 保卫丧钟 | Defend Death Knell† | 4001 Experimental Death Knell | Grants one Death Knell |
| `40008` | 磁暴先兆 | Vortex Omen† | 31 Vortex, 31 Vortex | Grants two Vortexes |
| `40009` | 台风先兆 | Typhoon Omen† | 22 Typhoon | Grants one Typhoon |

## Source boundary

The 132-row catalog and effect parameters are extracted from build-2227
`OfficerData`; localized names and descriptions come from the same build's
`I2LanguagesForConfigData` asset. This index describes configured effects; it
does not replace native action acceptance or authoritative state readback.
