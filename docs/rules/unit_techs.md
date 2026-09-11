# Unit technology index

[简体中文](unit_techs.zh.md)

[TOC]

This index contains all 233 ordinary unit technologies referenced by the 32
ordinary build `1.11.1.2.2227` cards. Names and effects are the game's official
English and Simplified Chinese localizations with serialized description
parameters expanded.

Use the listed ID in `techs.units`. `apply_layout` adds and activates each
declared technology in array order, then verifies active state. `Base supply`
documents the normal-game base price; Training Ground test actions do not make
that price part of the layout schema.

The machine-readable table is [`config/unit_techs.yaml`](../../config/unit_techs.yaml),
extracted from build 2259 and grouped by unit, carrying an ID and a price and
nothing else, with
[`config/unit_prices.yaml`](../../config/unit_prices.yaml) beside it for what a
unit costs to buy, to unlock and to raise one level. Every price this index and
that table share agrees, across the 233 rows below.

Reading the owner off the end of an ID is a rule of thumb rather than a rule:
it accounts for most technologies and fails on the rest, `1106` belonging to
Melting Point and `503101` to Vortex. The grouping below, and the one in the
configuration file, come from the catalogue instead.

## 1 — Fortress / 堡垒

| ID | Chinese name | English name | Base supply | Default | Effect |
| ---: | --- | --- | ---: | --- | --- |
| `1001` | 保护屏障 | Barrier | 500 | yes | Generates a Shield Dome that protects all allies within it. Shield HP increases by 40000 per unit Rank increased |
| `10201` | 射程强化 | Range Enhancement | 300 | yes | Increases attack range by 40 |
| `1105` | 防空弹幕 | Anti-Air Barrage | 200 | yes | Launches 16 anti-air barrages at the enemy every 10s. The barrages only damage aerial units. Each guided missile deals 900 damage and has a range of 170m |
| `1201` | 尖牙制造 | Fang Production | 300 | yes | Produces 8 Fangs to join the battle every 36s for 3 times |
| `10301` | 发射器过载 | Launcher Overload | 150 | no | Decreases Fortress' attack interval by 50% and range by 20m |
| `10801` | 精英射手 | Elite Marksman | 150 | no | Effect increases with unit Rank, increasing range by 5 and ATK by 25% per Rank increased |
| `701` | 双发 | Doubleshot | 100 | no | Each attack fires 2 shells in succession but increases reload time by 12% |
| `3001` | 装甲强化 | Armor Enhancement | 150 | no | Increases HP by 50%, and blocks 60 damage when attacked. The damage blocking effect increases with the Rank. And for each Rank, blocks an additional 60 damage. |
| `110201` | 火箭拳 | Rocket Punch | 300 | no | Fortress Launches its fists to attack enemies, triggering when Fortress's HP is below 85% and 55% respectively. The Rocket Punch has a range of 180m, dealing 12000 damage to the target and enemies within 25m. The damage of Rocket Punch increases by 12000 per unit Rank increased. |
| `10401` | 实心弹 | Solid Shot | 200 | no | Increases Fortress's Attack Range by 60 and Attack Interval by 0.7s, but decreases splash range by 2. |

## 2 — Marksman / 长弓

| ID | Chinese name | English name | Base supply | Default | Effect |
| ---: | --- | --- | ---: | --- | --- |
| `702` | 双发 | Doubleshot | 250 | yes | Each attack fires 2 bullets in succession but increases reload time by 15% |
| `10202` | 射程强化 | Range Enhancement | 300 | yes | Increases attack range by 40 |
| `10402` | 快速弹夹 | Quick Reload | 150 | yes | Decreases attack interval by 50% and attack by 60% |
| `1802` | 电磁弹 | Electromagnetic Shot | 250 | yes | Causes electromagnetic interference on hit, temporarily disabling the target unit's Tech and decreasing its movement speed by 40% |
| `10802` | 精英射手 | Elite Marksman | 400 | no | Effect increases with unit Rank, increasing range by 5 and ATK by 17% per Rank increased |
| `1202` | 射击小队 | Shooting Squad | 300 | no | At the beginning of the combat, summon 4 Fangs at the same Rank as Marksman to fight with you |
| `10102` | 突击模式 | Assault Mode | 200 | no | Marksman's HP is increased by 500%, movement speed is increased by 3, ATK is increased by 60%, and attacks have an area damage within 9m, but range is reduced by 70. |
| `3202` | 防空专精 | Aerial Specialization | 250 | no | Increases ATK against aerial units by 90% and increases range by 30 when attacking aerial units |

## 3 — Vulcan / 火神

| ID | Chinese name | English name | Base supply | Default | Effect |
| ---: | --- | --- | ---: | --- | --- |
| `180203` | 引燃 | Ignite | 300 | yes | Attacks ignite the target unit. For 2s, ignited units lose 6% HP per second and cannot restore HP. |
| `10203` | 射程强化 | Range Enhancement | 300 | yes | Increases attack range by 40 |
| `1103` | 燃烧弹 | Incendiary Bomb | 250 | yes | Launches a barrage of Incendiary Bombs every 16s to ignite the ground beneath the enemy's feet. Incendiary Bombs have a max range of 180m and a minimum range of 40m. |
| `10603` | 高温火焰 | Scorching Fire | 300 | yes | Increases Vulcan's ATK by 85% |
| `1203` | 最佳搭档 | Best Partner | 350 | no | At the beginning of the combat, summon 1 Marksman at the same Rank as Vulcan to fight with you |
| `11010` | 黏油弹 | Sticky Oil Bomb | 150 | no | Launches slowing Sticky Oil Bombs at enemies every 16s. Sticky Oil Bombs have a range of 180m and a minimum range of 40m. |
| `3003` | 装甲强化 | Armor Enhancement | 150 | no | Increases HP by 50%, and blocks 60 damage when attacked. The damage blocking effect increases with the Rank. And for each Rank, blocks an additional 60 damage. |

## 4 — Melting Point / 熔点

| ID | Chinese name | English name | Base supply | Default | Effect |
| ---: | --- | --- | ---: | --- | --- |
| `304` | 能量汲取 | Energy Absorption | 200 | yes | Increases HP by 60% and converts damage dealt to HP |
| `10204` | 射程强化 | Range Enhancement | 300 | yes | Increases attack range by 40 |
| `1107` | 能量散射 | Energy Diffraction | 150 | yes | Decreases Melting Point's range by 30 but allows Melting Point to fire 5 rays that each deal 17% of their original damage |
| `1106` | 电磁弹幕 | Electromagnetic Barrage | 300 | yes | Launches 16 electromagnetic shots at the enemy every 15s. Each electromagnetic shot deals 6000 damage to the Energy Shield, slows all hit units, and disables their Tech for 8s. Electromagnetic shots have a range of 180m |
| `1204` | 爬虫制造 | Crawler Production | 300 | no | Produces 8 Crawlers to join the battle every 36s for 3 times |
| `3004` | 装甲强化 | Armor Enhancement | 100 | no | Increases HP by 50%, and blocks 60 damage when attacked. The damage blocking effect increases with the Rank. And for each Rank, blocks an additional 60 damage. |

## 5 — Rhino / 犀牛

| ID | Chinese name | English name | Base supply | Default | Effect |
| ---: | --- | --- | ---: | --- | --- |
| `1109` | 旋风斩 | Whirlwind | 150 | yes | When there are multiple enemies nearby, Rhino attacks them with a whirlwind, dealing 1.4 times the damage of Rhino's ATK over a long time. |
| `180305` | 光子涂层 | Photon Coating | 300 | yes | Covers the unit with a photon coating, decreasing damage received by 30% within 30s after the start of the battle, and grants immunity to Electromagnetic, Ignited, Acid, and Degeneration Beam effects |
| `905` | 战地维修 | Field Maintenance | 200 | yes | Automatically restores 4.5% of Max HP per second upon taking damage |
| `2805` | 最后一击 | Final Blitz | 250 | yes | Rhinos activate a self-destruct mechanism at 0 HP, dealing damage equivalent to Rhino's Max HP to all units within 48m. |
| `10505` | 机械狂暴 | Mechanical Rage | 100 | no | Increases the units' movement speed by 5 and reduces attack interval by 0.3s |
| `2305` | 残骸利用 | Wreckage Recycling | 100 | no | Increases ATK by 60%, upon destroying an enemy, restores HP equal to the enemy's HP |
| `2505` | 动力装甲 | Power Armor | 300 | no | Increases HP by 25% and grants immunity to slow |
| `3005` | 装甲强化 | Armor Enhancement | 200 | no | Increases HP by 50%, and blocks 60 damage when attacked. The damage blocking effect increases with the Rank. And for each Rank, blocks an additional 60 damage. |
| `180805` | 战斗进化 | Combat Evolvement | 150 | no | Rhino's HP increases by 2.5% and ATK by 4.5% per second during battle. |

## 6 — Wasp / 兵蜂

| ID | Chinese name | English name | Base supply | Default | Effect |
| ---: | --- | --- | ---: | --- | --- |
| `206` | 能量护盾 | Energy Shield | 300 | yes | Obtain an Energy Shield that absorbs damage equal to the unit's HP and can block at least one instance of damage. |
| `10206` | 射程强化 | Range Enhancement | 300 | yes | Increases attack range by 40 |
| `1606` | 高速引擎 | Jump Drive | 100 | yes | Increases Wasp's movement speed by 5 and allows Wasp to be freely repositioned during the deployment phase of every round |
| `506` | 对地专精 | Ground Specialization | 200 | yes | Increases ATK against ground units by 200% |
| `10806` | 精英射手 | Elite Marksman | 400 | no | Effect increases with unit Rank, increasing range by 5 and ATK by 25% per Rank increased |
| `180206` | 引燃 | Ignite | 100 | no | Attacks have a chance to ignite the target unit. For 2s, ignited units lose 6% HP per second and cannot restore HP. |
| `1806` | 电磁弹 | Electromagnetic Shot | 100 | no | Causes electromagnetic interference on hit, temporarily disabling the target unit's Tech and decreasing its movement speed by 40% |
| `406` | 高爆弹药 | High-Explosive Ammo | 100 | no | Increases splash damage range by 7m but decreases ATK by 30% |
| `10606` | 穿甲弹 | Armor-Piercing Bullets | 100 | no | Increases ATK by 50% |
| `3206` | 防空专精 | Aerial Specialization | 200 | no | Increases ATK against aerial units by 90% and increases range by 30 when attacking aerial units |

## 7 — Mustang / 野马

| ID | Chinese name | English name | Base supply | Default | Effect |
| ---: | --- | --- | ---: | --- | --- |
| `3307` | 导弹拦截 | Missile Interceptor | 200 | yes | Try to intercept enemy missiles within 150m (Battlefield Powers cannot be intercepted), continuous interception for a long time will lead to a decrease in interception efficiency. Interception efficiency is independent of Mustang's ATK and Rank. |
| `10207` | 射程强化 | Range Enhancement | 300 | yes | Increases range by 40 |
| `407` | 高爆弹药 | High-Explosive Ammo | 50 | yes | Increases splash damage range by 7m but decreases ATK by 35% |
| `3207` | 防空专精 | Aerial Specialization | 300 | yes | Increases ATK against aerial units by 90% and increases range by 30 when attacking aerial units |
| `10607` | 穿甲弹 | Armor-Piercing Bullets | 250 | no | Increases ATK by 50% |
| `4607` | 斩杀弹 | Culling Rounds | 250 | no | Attacks instantly destroy targets with current HP below 320 (+200 per additional Rank). Decreases ATK by 35%. |

## 8 — Steel Ball / 钢球

| ID | Chinese name | English name | Base supply | Default | Effect |
| ---: | --- | --- | ---: | --- | --- |
| `308` | 能量汲取 | Energy Absorption | 200 | yes | Increases HP by 40% and converts damage dealt to HP |
| `608` | 伤害分摊 | Damage Sharing | 250 | yes | Chains adjacent Steel Balls together, increasing HP by 120% and sharing damage received equally between them |
| `10208` | 射程强化 | Range Enhancement | 300 | yes | Increases range by 40 |
| `1308` | 机械分裂 | Mechanical Division | 300 | yes | Destroyed Steel Balls create 5 Rank 1 Crawlers to continue fighting |
| `3008` | 装甲强化 | Armor Enhancement | 300 | no | Increases HP by 35%, and blocks 60 damage when attacked. The damage blocking effect increases with the Rank. And for each Rank, blocks an additional 60 damage. |
| `2408` | 重装锁定 | Fortified Target Lock | 200 | no | When it switches the target, lock-on priority is given to the enemy unit with the highest HP in range |
| `180808` | 滚动充能 | Kinetic Charge | 150 | no | The Steel Ball gains 1m extra range for every 7m it moves, up to a maximum of 100m. |

## 9 — Fang / 尖牙

| ID | Chinese name | English name | Base supply | Default | Effect |
| ---: | --- | --- | ---: | --- | --- |
| `180209` | 引燃 | Ignite | 150 | yes | Attacks have a chance to ignite the target unit. For 2s, ignited units lose 6% HP per second and cannot restore HP. |
| `10209` | 射程强化 | Range Enhancement | 300 | yes | Increases range by 40 |
| `10509` | 机械狂暴 | Mechanical Rage | 250 | yes | Increases the units' movement speed by 5 and reduces attack interval by 0.5s |
| `209` | 随身护盾 | Portable Shield | 500 | yes | Every Fang obtains an Energy Shield that absorbs damage equal to the unit's HP and can block at least one instance of damage. |
| `10609` | 穿甲弹 | Armor-Piercing Bullets | 100 | no | Increases ATK by 50% |
| `3109` | 榴弹发射器 | Grenade Launcher | 150 | no | Fangs switch to grenade launchers. Increases Range by 10. Attacks deal splash damage in a 7m radius, but cannot target air units. |

## 10 — Crawler / 爬虫

| ID | Chinese name | English name | Base supply | Default | Effect |
| ---: | --- | --- | ---: | --- | --- |
| `10510` | 机械狂暴 | Mechanical Rage | 100 | yes | Increases the units' movement speed by 5 and reduces attack interval by 0.2s |
| `180110` | 复制 | Replicate | 250 | yes | Uses destroyed enemies to create more Crawlers |
| `2610` | 潜地行动 | Subterranean Blitz | 350 | yes | Increases movement speed by 3. Tunnels underground when there are no enemies within 50 meters and nullify 45% of damage received, and emerge from beneath the ground and attack upon arriving near an enemy |
| `2710` | 酸性爆炸 | Acidic Explosion | 100 | yes | Destroyed Crawlers leave Acid liquid on the ground in a radius of 9m. Units within the Acid liquid lose 3% HP per second and will take 250% damage when attacked. |
| `10710` | 冲击钻头 | Impact Drill | 150 | no | Increases ATK by 125% |
| `3510` | 松散队列 | Loose Formation | 250 | no | Decreases HP by 40%, Crawlers will move in a looser formation. |

## 11 — Overlord / 霸主

| ID | Chinese name | English name | Base supply | Default | Effect |
| ---: | --- | --- | ---: | --- | --- |
| `1108` | 舰炮 | Overlord Artillery | 300 | yes | Install two ground-only cannons that have a range of 140 for the Overlord. The cannons attack every 3s to deal 7000 damage. Damage increases by 7000 per unit Rank increased |
| `10311` | 发射器过载 | Launcher Overload | 300 | yes | Decreases Overlord's attack interval by 50% and range by 20m |
| `1211` | 母舰 | Mothership | 250 | yes | Produces 5 Wasps to join the battle every 32s for 3 times |
| `1611` | 高速引擎 | Jump Drive | 300 | yes | Increases Overlord's movement speed by 5 and allows Overlord to be freely repositioned during the deployment phase of every round |
| `180311` | 光子投射 | Photon Emission | 300 | no | After the start of the battle, emits a photon coating on all allied forces (excluding the user) within 100m, decreasing their damage received by 30% for the within 20s and granting immunity to Electromagnetic, Ignited, Acid, and Degeneration Beam effects |
| `10211` | 射程强化 | Range Enhancement | 300 | no | Increases range by 40 |
| `3011` | 装甲强化 | Armor Enhancement | 150 | no | Increases HP by 35%, and blocks 60 damage when attacked. The damage blocking effect increases with the Rank. And for each Rank, blocks an additional 60 damage. |
| `911` | 战地维修 | Field Maintenance | 150 | no | Increases HP by 30% and automatically restores 4.5% of Max HP per second upon taking damage |
| `411` | 高爆弹药 | High-Explosive Ammo | 200 | no | Increases the splash damage range of Overlord's missiles by 7m, but decreases ATK by 40% |

## 12 — Stormcaller / 暴雨

| ID | Chinese name | English name | Base supply | Default | Effect |
| ---: | --- | --- | ---: | --- | --- |
| `812` | 燃烧弹 | Incendiary Bomb | 350 | yes | Decreases range by 20, the unit's attacks will ignite an area of 5.5m for 15s, dealing 270 damage per second. Allied forces are vulnerable to this damage. |
| `10212` | 射程强化 | Range Enhancement | 300 | yes | Increases range by 40 |
| `10312` | 发射器过载 | Launcher Overload | 250 | yes | The attack interval of the Stormcaller is reduced by 50%, the range is reduced by 40m, and the movement speed is increased by 5 |
| `412` | 高爆弹药 | High-Explosive Ammo | 150 | yes | Increases splash damage range by 5m but decreases ATK by 50% |
| `1812` | 电磁爆炸 | Electromagnetic Explosion | 300 | no | Causes electromagnetic interference on hit, temporarily disabling the target unit's Tech and decreasing its movement speed by 40% |
| `10912` | 重型导弹 | Heavy Missile | 200 | no | Increases ATK by 80%, increases Missile HP by 200%, but increases Attack Interval by 25% |

## 13 — Sledgehammer / 铁锤

| ID | Chinese name | English name | Base supply | Default | Effect |
| ---: | --- | --- | ---: | --- | --- |
| `913` | 战地维修 | Field Maintenance | 200 | yes | Increases HP by 30% and automatically restores 4.5% of Max HP per second upon taking damage |
| `613` | 伤害分摊 | Damage Sharing | 200 | yes | Chains adjacent Sledgehammers together, increasing HP by 120% and sharing damage received equally between them |
| `10513` | 机械狂暴 | Mechanical Rage | 250 | yes | Increases the units' movement speed by 6 and reduces attack interval by 1s |
| `10213` | 射程强化 | Range Enhancement | 300 | yes | Increases range by 40 |
| `1813` | 电磁弹 | Electromagnetic Shot | 350 | no | Causes electromagnetic interference on hit, temporarily disabling the target unit's Tech and decreasing its movement speed by 40% |
| `10613` | 穿甲弹 | Armor-Piercing Bullets | 100 | no | Increases ATK by 250% but increases attack interval by 30% |
| `3013` | 装甲强化 | Armor Enhancement | 250 | no | Increases HP by 35%, and blocks 60 damage when attacked. The damage blocking effect increases with the Rank. And for each Rank, blocks an additional 60 damage. |

## 14 — Hacker / 骇客

| ID | Chinese name | English name | Base supply | Default | Effect |
| ---: | --- | --- | ---: | --- | --- |
| `11014` | 多重控制 | Multi Control | 250 | yes | Decreases Hacker's range by 25m but allows Hacker to fire 5 Control Beams that each has 17% of their original control efficiency. |
| `1014` | 保护屏障 | Barrier | 400 | yes | Generates a Shield Dome that protects all allies within it. Shield HP increases by 16000 per unit Rank increased |
| `10214` | 射程强化 | Range Enhancement | 300 | yes | Increases range by 40 |
| `1714` | 强化控制 | Enhanced Control | 300 | yes | Units under Hacker's control will immediately recover their maximum HP |
| `1814` | 电磁干扰 | Electromagnetic Interference | 100 | no | Causes electromagnetic interference on hit, temporarily disabling the target unit's Tech and decreasing its movement speed by 40% |

## 15 — Arclight / 弧光

| ID | Chinese name | English name | Base supply | Default | Effect |
| ---: | --- | --- | ---: | --- | --- |
| `10215` | 射程强化 | Range Enhancement | 300 | yes | Increases range by 40 |
| `1815` | 电磁弹 | Electromagnetic Shot | 400 | yes | Causes electromagnetic interference on hit, temporarily disabling the target unit's Tech and decreasing its movement speed by 40% |
| `10915` | 蓄能攻击 | Charged Shot | 100 | yes | Increases ATK by 300% but increases attack interval by 35% |
| `3015` | 装甲强化 | Armor Enhancement | 100 | yes | Increases HP by 50%, and blocks 60 damage when attacked. The damage blocking effect increases with the Rank. And for each Rank, blocks an additional 60 damage. |
| `3115` | 防空弹药 | Anti-Aircraft Ammunition | 300 | no | Can be used against aerial units |
| `10815` | 精英射手 | Elite Marksman | 400 | no | Effect increases with unit Rank, increasing range by 5 and ATK by 17% per Rank increased |
| `4515` | 震荡波 | Shockwave | 250 | no | Decreases Arclight's Attack Range by 5. Attacks generate a shockwave, dealing 75 damage to enemies within 30m of the target. |

## 16 — Phoenix / 凤凰

| ID | Chinese name | English name | Base supply | Default | Effect |
| ---: | --- | --- | ---: | --- | --- |
| `2916` | 量子重组 | Quantum Reassembly | 150 | yes | The cockpit of a destroyed Phoenix will follow behind the nearest Phoenix of Our Forces and will be fully restored in 12s. This effect can only be triggered 1 times per battle |
| `10216` | 射程强化 | Range Enhancement | 300 | yes | Increases range by 40 |
| `10316` | 发射器过载 | Launcher Overload | 200 | yes | Decreases attack interval by 50% and range by 25m |
| `216` | 能量护盾 | Energy Shield | 200 | yes | Obtain an Energy Shield that absorbs damage equal to the unit's HP and can block at least one instance of damage. |
| `1616` | 高速引擎 | Jump Drive | 100 | no | Increases Phoenix's movement speed by 5 and allows Phoenix to be freely repositioned during the deployment phase of every round |
| `1816` | 电磁弹 | Electromagnetic Shot | 200 | no | Causes electromagnetic interference on hit, temporarily disabling the target unit's Tech and decreasing its movement speed by 40% |
| `10816` | 精英射手 | Elite Marksman | 400 | no | Effect increases with unit Rank, increasing range by 5 and ATK by 20% per Rank increased |
| `10916` | 蓄能攻击 | Charged Shot | 200 | no | Increases ATK by 200% but reduces range by 25 |

## 17 — War Factory / 战争工厂

| ID | Chinese name | English name | Base supply | Default | Effect |
| ---: | --- | --- | ---: | --- | --- |
| `10217` | 射程强化 | Range Enhancement | 500 | yes | Increases range by 40 |
| `12017` | 凤凰制造 | Phoenix Production | 500 | yes | Produces 1 Phoenix(es) to join the battle every 17.2s for 6 times |
| `12117` | 钢球制造 | Steel Ball Production | 450 | yes | Produces 1 Steel Ball(s) to join the battle every 9.7s for 10 times |
| `12217` | 铁锤制造 | Sledgehammer Production | 400 | no | Produces 1 Sledgehammer(s) to join the battle every 6.6s for 14 times |
| `3317` | 导弹拦截 | Missile Interceptor | 350 | yes | Try to intercept enemy missiles within 150m (Battlefield Powers cannot be intercepted), continuous interception for a long time will lead to a decrease in interception efficiency. Interception efficiency is independent of War Factory's ATK and Rank. |
| `10317` | 发射器过载 | Launcher Overload | 300 | yes | Decreases War Factory's attack interval by 50% and range by 20m |
| `180317` | 光子涂层 | Photon Coating | 200 | yes | Covers the unit with a photon coating, decreasing damage received by 30% within 30s after the start of the battle, and grants immunity to Electromagnetic, Ignited, Acid, and Degeneration Beam effects |
| `3017` | 装甲强化 | Armor Enhancement | 350 | no | Increases HP by 50%, and blocks 60 damage when attacked. The damage blocking effect increases with the Rank. And for each Rank, blocks an additional 60 damage. |
| `417` | 高爆弹药 | High-Explosive Ammo | 350 | no | Increases the splash damage range of the main guns by 7m, but decreases ATK by 40% |

## 18 — Wraith / 恶灵

| ID | Chinese name | English name | Base supply | Default | Effect |
| ---: | --- | --- | ---: | --- | --- |
| `110181` | 浮游炮阵 | Floating Artillery Array | 400 | yes | Increases the number of Wraith's floating cannons from 4 to 8. |
| `10218` | 射程强化 | Range Enhancement | 300 | yes | Increases range by 40 |
| `3018` | 装甲强化 | Armor Enhancement | 200 | yes | Increases HP by 35%, and blocks 60 damage when attacked. The damage blocking effect increases with the Rank. And for each Rank, blocks an additional 60 damage. |
| `180418` | 退化光束 | Degeneration Beam | 200 | yes | Projects a Degeneration Beam to enemy units within a range of 120m. The affected enemy units' movement speed is reduced by 40%, their ATK is reduced by 20%, and they take an additional 30% damage when attacked. |
| `918` | 战地维修 | Field Maintenance | 200 | no | Increases HP by 30% and automatically restores 4.5% of Max HP per second upon taking damage |
| `418` | 高爆弹药 | High-Explosive Ammo | 150 | no | Increases the splash damage range of the floating cannon by 7m, but decreases ATK by 30% |
| `4418` | 地面巡航 | Land Cruiser | 300 | no | Wraith becomes a ground unit. It can no longer attack air targets, but its Attack Range increases by 50, and its attack interval increases by 0.6 seconds. |

## 19 — Scorpion / 狂蝎

| ID | Chinese name | English name | Base supply | Default | Effect |
| ---: | --- | --- | ---: | --- | --- |
| `180519` | 酸性攻击 | Acid Attack | 250 | yes | Scorpion's attack will create an Acid area with a radius of 15m at the hit point. The Acid area lasts for 18s, units within the Acid liquid lose 3% HP per second and will take 250% damage when attacked. |
| `10019` | 攻城模式 | Siege Mode | 300 | yes | Scorpion enters siege mode, increasing range by 100m, but decreases ATK by 40% and increases attack interval by 1.5s, and it cannot attack enemies within 75m around it. |
| `10219` | 射程强化 | Range Enhancement | 300 | yes | Increases range by 40 |
| `719` | 双发 | Doubleshot | 100 | yes | Each attack fires 2 shells in succession but increases reload time by 12% |
| `919` | 战地维修 | Field Maintenance | 150 | no | Increases HP by 30% and automatically restores 4.5% of Max HP per second upon taking damage |
| `3019` | 装甲强化 | Armor Enhancement | 100 | no | Increases HP by 50%, and blocks 60 damage when attacked. The damage blocking effect increases with the Rank. And for each Rank, blocks an additional 60 damage. |
| `10419` | 收束射击 | Convergent Fire | 100 | no | Increases attack range by 30 and reduces attack interval by 1s, but decreases splash range by 8 |

## 20 — Fire Badger / 火獾

| ID | Chinese name | English name | Base supply | Default | Effect |
| ---: | --- | --- | ---: | --- | --- |
| `10220` | 射程强化 | Range Enhancement | 300 | yes | Increases range by 40 |
| `820` | 液态火 | Napalm | 250 | yes | Decreases HP by 30%, attacks will ignite an area of 12m around the target. The area burns for 8s, dealing 270 damage per second. |
| `180220` | 引燃 | Ignite | 100 | yes | Attacks have a chance to ignite the target unit. For 2s, ignited units lose 6% HP per second and cannot restore HP. |
| `920` | 战地维修 | Field Maintenance | 150 | yes | Increases HP by 30% and automatically restores 4.5% of Max HP per second upon taking damage |
| `10620` | 高温火焰 | Scorching Fire | 300 | no | Increases Fire Badger’s ATK by 85% |
| `11020` | 焦土冲击 | Scorching Charge | 200 | no | Increases the Fire Badger's HP by 80%. When its HP drops below 50%, it charges an enemy and detonates, dealing damage equal to its remaining HP to all units in a 40m radius and igniting the ground at the impact site. |
| `180620` | 逆火  | Counter-Fire | 200 | no | Increases HP by 50%. Taking damage increases the Fire Badger's Range by 70 for 20s. |

## 21 — Sabertooth / 剑齿虎

| ID | Chinese name | English name | Base supply | Default | Effect |
| ---: | --- | --- | ---: | --- | --- |
| `10221` | 射程强化 | Range Enhancement | 300 | yes | Increases range by 40 |
| `10321` | 战地维修 | Field Maintenance | 200 | yes | Increases HP by 30% and automatically restores 4.5% of Max HP per second upon taking damage |
| `3321` | 导弹拦截 | Missile Interceptor | 100 | yes | Try to intercept enemy missiles within 150m (Battlefield Powers cannot be intercepted). Continuous interception for a long time will lead to a decrease in interception efficiency. Interception efficiency is independent of Sabertooth's ATK and Rank. |
| `721` | 双发 | Doubleshot | 150 | yes | Each attack fires 2 shells in succession but increases reload time by 12% |
| `110211` | 副炮 | Secondary Armament | 200 | no | Install one secondary gun on both left and right sides of the Sabretooth. Secondary guns have a range of 95, attack every 4s to deal 2000 damage. Damage increases by 2000 per unit Rank increased. |
| `4721` | 野战工事 | Field Entrenchment  | 200 | no | Deploys a trench at the start of battle. While entrenched, Sabertooth cannot move, HP increases by 50%, Range increases by 20, and Attack Interval decreases by 20%. If no enemies are in range for 7 seconds, Sabertooth will leave and destroy the trench. |

## 22 — Typhoon / 台风

| ID | Chinese name | English name | Base supply | Default | Effect |
| ---: | --- | --- | ---: | --- | --- |
| `10222` | 射程强化 | Range Enhancement | 300 | yes | Increases range by 40 |
| `1102022` | 防空标记 | Air Defense Mark | 300 | yes | Every 10 seconds, fires a marker missile with a range of 160 meters, marking enemy air units within a 100-meter radius of the target. Marked units have their range reduced by 20 and take 30% additional damage for 10 seconds. |
| `5122` | 反应装甲 | Reactive Armor | 150 | yes | Reduces incoming damage by 80%, up to 5 times. |
| `5222` | 维修阵列 | Maintenance Array | 200 | yes | Every 3 seconds, restores 500 HP to the Typhoon and friendly units within 100 meters. The healing amount increases by 500 per level. |
| `4722` | 野战工事 | Field Entrenchment  | 200 | no | At the start of battle, deploys a trench in place. While garrisoned, cannot move. HP increased by 70%, range increased by 20. If no enemies are within range for 7s, the Typhoon leaves and destroys the trench. |
| `2922` | 战地重组 | Field Reassembly | 300 | no | When destroyed, the Typhoon begins field reassembly and returns to combat in 5 seconds with 100% HP. Limited to 1 activation per round. |
| `5322` | 残骸引爆 | Wreckage Detonation | 200 | no | Units destroyed by the Typhoon explode violently, dealing 115 damage to all units within a 12m radius. |

## 23 — Sandworm / 沙虫

| ID | Chinese name | English name | Base supply | Default | Effect |
| ---: | --- | --- | ---: | --- | --- |
| `10523` | 机械狂暴 | Mechanical Rage | 150 | yes | Increases the units' movement speed by 5 and reduces attack interval by 0.8s |
| `3023` | 装甲强化 | Armor Enhancement | 250 | no | Increases HP by 50%, and blocks 60 damage when attacked. The damage blocking effect increases with the Rank. And for each Rank, blocks an additional 60 damage. |
| `13023` | 机械分裂 | Mechanical Division | 200 | yes | Destroyed Sandworm creates 4 Larvas of the same Rank as Sandworm to continue fighting. |
| `3123` | 对空 | Anti-Aerial | 100 | no | Increases Range by 20, allowing Sandworm to attack aerial targets. |
| `923` | 潜地维修 | Burrow Maintenance | 150 | yes | Increases Sandworm's HP by 20% and  Sandworm automatically restores 20% of Max HP per second when burrowing. |
| `3623` | 复制 | Replicate | 250 | yes | Each time Sandworm emerges from the ground, creates 1 Larvas of the same Rank as Sandworm to continue fighting. |
| `3723` | 沙暴 | Sandstorm | 200 | no | When Sandworm emerges out of the ground, creates a sandstorm with a radius of 120m for 7 seconds. Units in the sandstorm: Range is reduced by 50% and damage taken from ranged attacks is reduced by 30% |
| `3823` | 突袭 | Strike | 100 | no | Sandworms emerge from the ground faster, and their first attack after emerging is stronger: ATK increased by 30%, splash range increased by 10m. |

## 24 — Tarantula / 狼蛛

| ID | Chinese name | English name | Base supply | Default | Effect |
| ---: | --- | --- | ---: | --- | --- |
| `11024` | 蜘蛛雷 | Spider Mine | 150 | yes | Fires 2 Spider Mines with 750 HP every 15 seconds. The Spider Mines will charge at the enemy and self-destruct, dealing 2500 damage to all units within 12 m. Spider Mine's attributes enhance with Tarantula's level, HP increases by 750 and damage increases by 2500 per unit Rank increased. |
| `10224` | 射程强化 | Range Enhancement | 300 | yes | Increases range by 40 |
| `10524` | 机械狂暴 | Mechanical Rage | 400 | yes | Increases the units' movement speed by 4 and reduces attack interval by 0.2s |
| `10624` | 穿甲弹 | Armor-Piercing Bullets | 150 | yes | Increases ATK by 50% |
| `924` | 战地维修 | Field Maintenance | 150 | no | Increases HP by 30% and automatically restores 4.5% of Max HP per second upon taking damage |
| `3024` | 装甲强化 | Armor Enhancement | 100 | no | Increases HP by 50%, and blocks 60 damage when attacked. The damage blocking effect increases with the Rank. And for each Rank, blocks an additional 60 damage. |
| `3124` | 防空弹药 | Anti-Aircraft Ammunition | 150 | no | Can be used against aerial units |
| `424` | 高爆弹药 | High-Explosive Ammo | 300 | no | Increases splash damage range by 7m but decreases ATK by 45% |

## 25 — Phantom Ray / 鬼鳐

| ID | Chinese name | English name | Base supply | Default | Effect |
| ---: | --- | --- | ---: | --- | --- |
| `725` | 全弹发射 | Burst Mode | 200 | yes | Each attack launches 10 missiles, but increases reload time by 150%. |
| `10225` | 射程强化 | Range Enhancement | 300 | yes | Increases range by 40 |
| `3025` | 装甲强化 | Armor Enhancement | 250 | no | Increases HP by 35%, and blocks 60 damage when attacked. The damage blocking effect increases with the Rank. And for each Rank, blocks an additional 60 damage. |
| `11025` | 黏油弹 | Sticky Oil Bomb | 100 | yes | Launches a slowing Sticky Oil Bomb at enemies every 12 seconds. The Sticky Oil Bomb has a range of 180m and can be ignited by fire. |
| `3925` | 隐形 | Stealth Cloak | 100 | yes | Increases Phantom Ray's ATK by 20%, HP by 20%, and it is cloaked by default. Phantom Ray cannot be targeted by the enemy while cloaked, but will reveal itself when attacking. |
| `425` | 高爆弹药 | High-Explosive Ammo | 150 | no | Increases splash damage range by 7m but decreases ATK by 40% |
| `225` | 能量护盾 | Energy Shield | 400 | no | Obtain an Energy Shield that absorbs damage equal to the unit's HP and can block at least one instance of damage. |
| `3225` | 对地锁定 | Ground Targeting | 200 | no | Increases Phantom Ray's Attack Range against ground units by 60. |

## 26 — Farseer / 先知

| ID | Chinese name | English name | Base supply | Default | Effect |
| ---: | --- | --- | ---: | --- | --- |
| `180326` | 光子投射 | Photon Emission | 400 | yes | After the start of the battle, emits a photon coating on all Ground allied forces (excluding the user) within 100m, decreasing their damage received by 30% for the within 20s and granting immunity to Electromagnetic, Ignited, Acid, and Degeneration Beam effects |
| `180526` | 搜索雷达 | Scanning Radar | 200 | yes | Activate Scanning Radar, increases range of allied units within 100m by 10. Effects of multiple Scanning Radars do not stack up. |
| `3326` | 导弹拦截 | Missile Interceptor | 200 | yes | Try to intercept enemy missiles within 150m (Battlefield Powers cannot be intercepted), continuous interception for a long time will lead to a decrease in interception efficiency. Interception efficiency is independent of Farseer's ATK and Rank. |
| `1826` | 电磁爆炸 | Electromagnetic Explosion | 150 | yes | Causes electromagnetic interference on hit, temporarily disabling the target unit's Tech and decreasing its movement speed by 40% |
| `10226` | 射程强化 | Range Enhancement | 300 | no | Increases range by 40 |
| `726` | 全弹发射 | Burst Mode | 150 | no | Each attack launches 12 missiles, but increases reload time by 200%. |
| `3226` | 防空专精 | Aerial Specialization | 200 | no | Increases ATK against aerial units by 90% and increases range by 30 when attacking aerial units |

## 27 — Raiden / 雷霆

| ID | Chinese name | English name | Base supply | Default | Effect |
| ---: | --- | --- | ---: | --- | --- |
| `110271` | 分叉 | Fork | 250 | yes | Decreases Raiden's range by 10, but increases the number of lightning bolts fired per attack from 3 to 5. |
| `4027` | 连锁 | Chain | 200 | yes | Decreases Raiden's range by 20, the lightning bolts it fires will jump to other enemies within 60 radius after hitting the enemy, dealing 25% damage. |
| `4127` | 电离 | Ionization | 100 | yes | Decreases Raiden's ATK by 70%, but attack will cause an additional damage equal to 50% of the target's current HP. |
| `10227` | 射程强化 | Range Enhancement | 300 | yes | Increases range by 40 |
| `1827` | 电磁弹 | Electromagnetic Shot | 300 | no | Causes electromagnetic interference on hit, temporarily disabling the target unit's Tech and decreasing its movement speed by 40% |
| `227` | 能量护盾 | Energy Shield | 150 | no | Obtain an Energy Shield that absorbs damage equal to the unit's HP and can block at least one instance of damage. |

## 28 — Hound / 猎犬

| ID | Chinese name | English name | Base supply | Default | Effect |
| ---: | --- | --- | ---: | --- | --- |
| `10528` | 机械狂暴 | Mechanical Rage | 300 | yes | Increases the units' movement speed by 4 and reduces attack interval by 0.8s |
| `10228` | 射程强化 | Range Enhancement | 300 | yes | Increases range by 40 |
| `4228` | 消防装置 | Fire Extinguisher | 200 | yes | Hound will clear fire, acid and smoke effects on the ground within 40m |
| `11028` | 燃烧弹 | Incendiary Bomb | 250 | yes | Launches 1 Incendiary Bombs every 16s to ignite the ground beneath the enemy's feet. Incendiary Bombs have a max range of 160m and a minimum range of 40m. |
| `3028` | 装甲强化 | Armor Enhancement | 250 | no | Increases HP by 20%, and blocks 60 damage when attacked. The damage blocking effect increases with the Rank. And for each Rank, blocks an additional 60 damage. |
| `180828` | 枪膛增压 | Chamber Compression | 300 | no | Increases Hound's attack interval by 0.6 seconds. During battle, Hound's ATK increases by 65% per second. This bonus resets after attacking. |

## 29 — Abyss / 深渊

| ID | Chinese name | English name | Base supply | Default | Effect |
| ---: | --- | --- | ---: | --- | --- |
| `10229` | 射程强化 | Range Enhancement | 500 | yes | Increases range by 40 |
| `12029` | 暗黑伙伴 | Dark Companion | 300 | yes | At the beginning of the battle, summon 1 Wraith at the same Rank as Abyss. |
| `180329` | 光子涂层 | Photon Coating | 250 | yes | Covers the unit with a photon coating, decreasing damage received by 30% within 30s after the start of the battle, and grants immunity to Electromagnetic, Ignited, Acid, and Degeneration Beam effects |
| `11029` | 裂解 | Disintegration | 350 | yes | Bombards enemy ground units within a 250m radius every 15s, causing them to lose 20% of their current HP and reducing their movement speed by 40% for 5s. |
| `110291` | 蜂群导弹 | Swarm Missiles | 500 | yes | Launches 46 missiles at nearby ground enemies every 15 seconds. Each missile deals 400 with a range of 200m. Damage increases by 400 per unit Rank increased. |
| `2329` | 残骸利用 | Wreckage Recycling | 200 | no | Increases ATK by 35%, upon destroying an enemy, restores HP equal to the enemy's HP |
| `4329` | 纵扫 | Vertical Sweep | 350 | yes | Abyss's ATK increased by 50%, and the laser now sweeps vertically. |

## 30 — Void Eye / 魔眼

| ID | Chinese name | English name | Base supply | Default | Effect |
| ---: | --- | --- | ---: | --- | --- |
| `10230` | 射程强化 | Range Enhancement | 300 | yes | Increases range by 40 |
| `230` | 能量护盾 | Energy Shield | 250 | yes | Obtain an Energy Shield that absorbs damage equal to the unit's HP and can block at least one instance of damage. |
| `10930` | 蓄能攻击 | Charged Shot | 100 | yes | Increases ATK by 120% but increases attack interval by 25% |
| `4430` | 飞行模式 | Aerial Mode | 150 | yes | Void Eye transforms into a flying unit that can attack air and its movement speed is increased by 3, but its range is reduced by 15. |
| `330` | 能量汲取 | Energy Absorption | 50 | no | Increases HP by 60% and converts damage dealt to HP |
| `180430` | 压制射击 | Suppression Shots | 100 | no | Increases Void Eye's range by 10,  and its attacks reduce target's range by 30% for 3.5 seconds |
| `180530` | 电磁装甲 | Electromagnetic Armor | 300 | no | Void Eye causes electromagnetic interference on enemies attacking it, temporarily disabling their Tech and reducing their movement speed by 40% for 3 seconds. |

## 31 — Vortex / 磁暴

| ID | Chinese name | English name | Base supply | Default | Effect |
| ---: | --- | --- | ---: | --- | --- |
| `10231` | 射程强化 | Range Enhancement | 300 | yes | Increases range by 40 |
| `180931` | 移动电站 | Mobile Power Station | 250 | yes | Increases Vortex's range by 10. Increases ATK of ground friendly units within 100m by 30% (Does not stack). |
| `4531` | 电磁云 | Electromagnetic Cloud | 400 | yes | Attacks inflict Electromagnetic interference on the target and enemies within a 10m radius. |
| `123101` | 电磁双生 | Electromagnetic Twin | 350 | yes | At the start of battle, spawns 1 Level 1 Vortex Mirage behind itself. The Mirage has 1 HP and 30% of a standard Vortex's ATK. |
| `493101` | 储能护盾 | Accumulator Shield | 300 | no | Accumulates energy while attacking. Every 5 attacks, deploys a shield with a radius of 40 at the current location , and increases the required attacks for the next shield by 5. The shield has a base HP of 2000, increasing by an additional 2000 HP per Vortex level. |
| `631` | 并网 | Grid Integration | 250 | no | Links with other Vortex within 35m. Increases ATK by 35% for each additional linked unit, up to a maximum of 105%. |
| `503101` | 应急装甲 | Emergency Armor | 150 | no | When HP first falls below 50%, activates Emergency Armor, resisting all damage and becoming untargetable for 4 seconds. Emergency Armor will not activate if HP reaches 0. |
| `931` | 战地维修 | Field Maintenance | 150 | no | Increases HP by 30% and automatically restores 4.5% of Max HP per second upon taking damage |

## 2002 — Mountain / 泰山

| ID | Chinese name | English name | Base supply | Default | Effect |
| ---: | --- | --- | ---: | --- | --- |
| `11020021` | 炮射火箭 | Gun-launched Missile | 400 | yes | Launches a rocket from its arm cannons every 14 seconds. The rocket has a range of 180, causing 22500 damage to enemies within a 15m radius of the hit point. Damage increases by 22500 per unit Rank increased. |
| `302002` | 巨山装甲 | Mountain Plating | 400 | yes | Blocks 700 damage when attacked. The damage blocking effect increases with the Rank. And for each Rank, blocks an additional 700 damage. |
| `72002` | 饱和打击 | Saturation Bombardment | 500 | yes | Increases splash range by 3, and each attack fires 16 shells in succession, and shells spread out over a wide area, but increases attack interval by 120%. |
| `1032002` | 增程炮弹 | Extended Range Ammo | 400 | yes | Increases range by 160, but decreases ATK by 75%. |
| `11020022` | 烟雾弹 | Smoke Bomb | 350 | yes | Launches 8 Smoke Bombs towards the enemy every 25s. The Smoke Bombs have a max range of 180m and a minimum range of 60m. The range of units within the smoke is reduced by 35%. |
| `18032002` | 光子循环 | Photon Loop | 400 | yes | Every 30s, covers the unit with a photon coating, decreasing damage received by 30% within 25s and grants immunity to Electromagnetic, Ignited, Acid, and Degeneration Beam effects |
| `312002` | 防空弹药 | Anti-Aircraft Ammunition | 300 | no | Decreases ATK by 40%, allowing Mountain to attack aerial targets. |
| `1022002` | 射程强化 | Range Enhancement | 500 | no | Increases range by 40 |

## Source boundary

Ownership, ID order, default membership, and base supply come from the
build-2227 `CardData.technologies` and `TechnologyData` catalog. Localized names
and descriptions come from the same build's `I2LanguagesForConfigData` asset.
The text records configured effects; specialized runtime behavior still depends
on the native implementation.
