# Units and their technologies

A unit is a `ConfigDataContainer.cardDatas` row and the `mechDatas` row its
`mechID` names. Its technologies are the IDs its card lists in `technologies`,
each a row of one of `TechnologyGroupData`'s lists in `level0`.

## Naming and applying

A layout or a state names a unit by the snake case of its official English
name, which the document crate's unit catalog carries, and names its
technologies under it in `techs` by the name
[`config/names.yaml`](../../config/names.yaml) gives each. `apply_layout` adds
and activates each declared technology in ascending ID order, then verifies
the active state.

A technology belongs to the unit whose card lists it. Reading the owner off the
end of an ID is a rule of thumb that fails on some (`1106` is Melting Point's,
`503101` Vortex's); [`config/unit_techs.yaml`](../../config/unit_techs.yaml)
and the tables below group by the card.

## Prices

[`config/unit_techs.yaml`](../../config/unit_techs.yaml) gives each technology's
own `supply`, and [`config/unit_prices.yaml`](../../config/unit_prices.yaml) what
a unit costs to buy, to unlock and to raise one level; `scripts/extract_prices.py`
writes both. What researching one costs in a match is
`UnitUtility.CalculateUpgradeTechnologyCost`: its own supply plus the number of
the unit's technologies already active times a step, the unit's positive
`techUpgradeIncreaseSupplyPerCount` or the match-wide
`Config.upgradeTechnologyCostIncreaseDelta` otherwise, capped by a positive
`techUpgradeMaxSupplyLimit`. [`config/economy.yaml`](../../config/economy.yaml)
carries the step. The sum also takes the technology's own
`PlayerDataChangeInt.Supply`, which no standard officer writes.

A card's `defaultTechnologies` are what a new account may unlock without
paying, an account rule rather than a match one; nothing here uses them.

What a technology writes onto its unit's numbers is
[technology_effects.md](technology_effects.md).

## Units

<!-- names: units -->
| ID | 中文 | English | Document name |
| ---: | --- | --- | --- |
| 1 | 堡垒 | Fortress | `fortress` |
| 2 | 长弓 | Marksman | `marksman` |
| 3 | 火神 | Vulcan | `vulcan` |
| 4 | 熔点 | Melting Point | `melting_point` |
| 5 | 犀牛 | Rhino | `rhino` |
| 6 | 兵蜂 | Wasp | `wasp` |
| 7 | 野马 | Mustang | `mustang` |
| 8 | 钢球 | Steel Ball | `steel_ball` |
| 9 | 尖牙 | Fang | `fang` |
| 10 | 爬虫 | Crawler | `crawler` |
| 11 | 霸主 | Overlord | `overlord` |
| 12 | 暴雨 | Stormcaller | `stormcaller` |
| 13 | 铁锤 | Sledgehammer | `sledgehammer` |
| 14 | 骇客 | Hacker | `hacker` |
| 15 | 弧光 | Arclight | `arclight` |
| 16 | 凤凰 | Phoenix | `phoenix` |
| 17 | 战争工厂 | War Factory | `war_factory` |
| 18 | 恶灵 | Wraith | `wraith` |
| 19 | 狂蝎 | Scorpion | `scorpion` |
| 20 | 火獾 | Fire Badger | `fire_badger` |
| 21 | 剑齿虎 | Sabertooth | `sabertooth` |
| 22 | 台风 | Typhoon | `typhoon` |
| 23 | 沙虫 | Sandworm | `sandworm` |
| 24 | 狼蛛 | Tarantula | `tarantula` |
| 25 | 鬼鳐 | Phantom Ray | `phantom_ray` |
| 26 | 先知 | Farseer | `farseer` |
| 27 | 雷霆 | Raiden | `raiden` |
| 28 | 猎犬 | Hound | `hound` |
| 29 | 深渊 | Abyss | `abyss` |
| 30 | 魔眼 | Void Eye | `void_eye` |
| 31 | 磁暴 | Vortex | `vortex` |
| 32 | 百夫长 | Centurion | `centurion` |
| 2002 | 泰山 | Mountain | `mountain` |
<!-- /names -->

## Technologies

<!-- names: technologies -->
| Unit | ID | 中文 | English | Document name |
| --- | ---: | --- | --- | --- |
| `abyss` | 2329 | 残骸利用 | Wreckage Recycling | `wreckage_recycling` |
| `abyss` | 4329 | 纵扫 | Vertical Sweep | `vertical_sweep` |
| `abyss` | 10229 | 射程强化 | Range Enhancement | `range_enhancement` |
| `abyss` | 11029 | 裂解 | Disintegration | `disintegration` |
| `abyss` | 12029 | 暗黑伙伴 | Dark Companion | `dark_companion` |
| `abyss` | 110291 | 蜂群导弹 | Swarm Missiles | `swarm_missiles` |
| `abyss` | 180329 | 光子涂层 | Photon Coating | `photon_coating` |
| `arclight` | 1815 | 电磁弹 | Electromagnetic Shot | `electromagnetic_shot` |
| `arclight` | 3015 | 装甲强化 | Armor Enhancement | `armor_enhancement` |
| `arclight` | 3115 | 防空弹药 | Anti-Aircraft Ammunition | `anti_aircraft_ammunition` |
| `arclight` | 4515 | 震荡波 | Shockwave | `shockwave` |
| `arclight` | 10215 | 射程强化 | Range Enhancement | `range_enhancement` |
| `arclight` | 10815 | 精英射手 | Elite Marksman | `elite_marksman` |
| `arclight` | 10915 | 蓄能攻击 | Charged Shot | `charged_shot` |
| `centurion` | 1232 | 召唤猎犬 | Summon Hounds | `summon_hounds` |
| `centurion` | 5132 | 反应装甲 | Reactive Armor | `reactive_armor` |
| `centurion` | 5532 | 近战模式 | Melee Mode | `melee_mode` |
| `centurion` | 10232 | 射程强化 | Range Enhancement | `range_enhancement` |
| `centurion` | 110321 | 双持 | Dual Wield | `dual_wield` |
| `centurion` | 110322 | 追踪导弹 | Homing Missile | `homing_missile` |
| `crawler` | 2610 | 潜地行动 | Subterranean Blitz | `subterranean_blitz` |
| `crawler` | 2710 | 酸性爆炸 | Acidic Explosion | `acidic_explosion` |
| `crawler` | 3510 | 松散队列 | Loose Formation | `loose_formation` |
| `crawler` | 10510 | 机械狂暴 | Mechanical Rage | `mechanical_rage` |
| `crawler` | 10710 | 冲击钻头 | Impact Drill | `impact_drill` |
| `crawler` | 180110 | 复制 | Replicate | `replicate` |
| `fang` | 209 | 随身护盾 | Portable Shield | `portable_shield` |
| `fang` | 3109 | 榴弹发射器 | Grenade Launcher | `grenade_launcher` |
| `fang` | 10209 | 射程强化 | Range Enhancement | `range_enhancement` |
| `fang` | 10509 | 机械狂暴 | Mechanical Rage | `mechanical_rage` |
| `fang` | 10609 | 穿甲弹 | Armor-Piercing Bullets | `armor_piercing_bullets` |
| `fang` | 180209 | 引燃 | Ignite | `ignite` |
| `farseer` | 726 | 全弹发射 | Burst Mode | `burst_mode` |
| `farseer` | 1826 | 电磁爆炸 | Electromagnetic Explosion | `electromagnetic_explosion` |
| `farseer` | 3226 | 防空专精 | Aerial Specialization | `aerial_specialization` |
| `farseer` | 3326 | 导弹拦截 | Missile Interceptor | `missile_interceptor` |
| `farseer` | 10226 | 射程强化 | Range Enhancement | `range_enhancement` |
| `farseer` | 180326 | 光子投射 | Photon Emission | `photon_emission` |
| `farseer` | 180526 | 搜索雷达 | Scanning Radar | `scanning_radar` |
| `fire_badger` | 820 | 液态火 | Napalm | `napalm` |
| `fire_badger` | 920 | 战地维修 | Field Maintenance | `field_maintenance` |
| `fire_badger` | 10220 | 射程强化 | Range Enhancement | `range_enhancement` |
| `fire_badger` | 10620 | 高温火焰 | Scorching Fire | `scorching_fire` |
| `fire_badger` | 11020 | 焦土冲击 | Scorching Charge | `scorching_charge` |
| `fire_badger` | 180220 | 引燃 | Ignite | `ignite` |
| `fire_badger` | 180620 | 逆火  | Counter-Fire | `counter_fire` |
| `fortress` | 701 | 双发 | Doubleshot | `doubleshot` |
| `fortress` | 1001 | 保护屏障 | Barrier | `barrier` |
| `fortress` | 1105 | 防空弹幕 | Anti-Air Barrage | `anti_air_barrage` |
| `fortress` | 1201 | 尖牙制造 | Fang Production | `fang_production` |
| `fortress` | 3001 | 装甲强化 | Armor Enhancement | `armor_enhancement` |
| `fortress` | 10201 | 射程强化 | Range Enhancement | `range_enhancement` |
| `fortress` | 10301 | 发射器过载 | Launcher Overload | `launcher_overload` |
| `fortress` | 10401 | 实心弹 | Solid Shot | `solid_shot` |
| `fortress` | 10801 | 精英射手 | Elite Marksman | `elite_marksman` |
| `fortress` | 110201 | 火箭拳 | Rocket Punch | `rocket_punch` |
| `hacker` | 1014 | 保护屏障 | Barrier | `barrier` |
| `hacker` | 1714 | 强化控制 | Enhanced Control | `enhanced_control` |
| `hacker` | 1814 | 电磁干扰 | Electromagnetic Interference | `electromagnetic_interference` |
| `hacker` | 10214 | 射程强化 | Range Enhancement | `range_enhancement` |
| `hacker` | 11014 | 多重控制 | Multi Control | `multi_control` |
| `hound` | 3028 | 装甲强化 | Armor Enhancement | `armor_enhancement` |
| `hound` | 4228 | 消防装置 | Fire Extinguisher | `fire_extinguisher` |
| `hound` | 10228 | 射程强化 | Range Enhancement | `range_enhancement` |
| `hound` | 10528 | 机械狂暴 | Mechanical Rage | `mechanical_rage` |
| `hound` | 11028 | 燃烧弹 | Incendiary Bomb | `incendiary_bomb` |
| `hound` | 180828 | 枪膛增压 | Chamber Compression | `chamber_compression` |
| `marksman` | 702 | 双发 | Doubleshot | `doubleshot` |
| `marksman` | 1202 | 射击小队 | Shooting Squad | `shooting_squad` |
| `marksman` | 1802 | 电磁弹 | Electromagnetic Shot | `electromagnetic_shot` |
| `marksman` | 3202 | 防空专精 | Aerial Specialization | `aerial_specialization` |
| `marksman` | 10102 | 突击模式 | Assault Mode | `assault_mode` |
| `marksman` | 10202 | 射程强化 | Range Enhancement | `range_enhancement` |
| `marksman` | 10402 | 快速弹夹 | Quick Reload | `quick_reload` |
| `marksman` | 10802 | 精英射手 | Elite Marksman | `elite_marksman` |
| `melting_point` | 304 | 能量汲取 | Energy Absorption | `energy_absorption` |
| `melting_point` | 1106 | 电磁弹幕 | Electromagnetic Barrage | `electromagnetic_barrage` |
| `melting_point` | 1107 | 能量散射 | Energy Diffraction | `energy_diffraction` |
| `melting_point` | 1204 | 爬虫制造 | Crawler Production | `crawler_production` |
| `melting_point` | 3004 | 装甲强化 | Armor Enhancement | `armor_enhancement` |
| `melting_point` | 10204 | 射程强化 | Range Enhancement | `range_enhancement` |
| `mountain` | 72002 | 饱和打击 | Saturation Bombardment | `saturation_bombardment` |
| `mountain` | 302002 | 巨山装甲 | Mountain Plating | `mountain_plating` |
| `mountain` | 312002 | 防空弹药 | Anti-Aircraft Ammunition | `anti_aircraft_ammunition` |
| `mountain` | 1022002 | 射程强化 | Range Enhancement | `range_enhancement` |
| `mountain` | 1032002 | 增程炮弹 | Extended Range Ammo | `extended_range_ammo` |
| `mountain` | 11020021 | 炮射火箭 | Gun-launched Missile | `gun_launched_missile` |
| `mountain` | 11020022 | 烟雾弹 | Smoke Bomb | `smoke_bomb` |
| `mountain` | 18032002 | 光子循环 | Photon Loop | `photon_loop` |
| `mustang` | 407 | 高爆弹药 | High-Explosive Ammo | `high_explosive_ammo` |
| `mustang` | 3207 | 防空专精 | Aerial Specialization | `aerial_specialization` |
| `mustang` | 3307 | 导弹拦截 | Missile Interceptor | `missile_interceptor` |
| `mustang` | 4607 | 斩杀弹 | Culling Rounds | `culling_rounds` |
| `mustang` | 10207 | 射程强化 | Range Enhancement | `range_enhancement` |
| `mustang` | 10607 | 穿甲弹 | Armor-Piercing Bullets | `armor_piercing_bullets` |
| `overlord` | 411 | 高爆弹药 | High-Explosive Ammo | `high_explosive_ammo` |
| `overlord` | 911 | 战地维修 | Field Maintenance | `field_maintenance` |
| `overlord` | 1108 | 舰炮 | Overlord Artillery | `overlord_artillery` |
| `overlord` | 1211 | 母舰 | Mothership | `mothership` |
| `overlord` | 1611 | 高速引擎 | Jump Drive | `jump_drive` |
| `overlord` | 3011 | 装甲强化 | Armor Enhancement | `armor_enhancement` |
| `overlord` | 10211 | 射程强化 | Range Enhancement | `range_enhancement` |
| `overlord` | 10311 | 发射器过载 | Launcher Overload | `launcher_overload` |
| `overlord` | 180311 | 光子投射 | Photon Emission | `photon_emission` |
| `phantom_ray` | 225 | 能量护盾 | Energy Shield | `energy_shield` |
| `phantom_ray` | 425 | 高爆弹药 | High-Explosive Ammo | `high_explosive_ammo` |
| `phantom_ray` | 725 | 全弹发射 | Burst Mode | `burst_mode` |
| `phantom_ray` | 3025 | 装甲强化 | Armor Enhancement | `armor_enhancement` |
| `phantom_ray` | 3225 | 对地锁定 | Ground Targeting | `ground_targeting` |
| `phantom_ray` | 3925 | 隐形 | Stealth Cloak | `stealth_cloak` |
| `phantom_ray` | 10225 | 射程强化 | Range Enhancement | `range_enhancement` |
| `phantom_ray` | 11025 | 黏油弹 | Sticky Oil Bomb | `sticky_oil_bomb` |
| `phoenix` | 216 | 能量护盾 | Energy Shield | `energy_shield` |
| `phoenix` | 1616 | 高速引擎 | Jump Drive | `jump_drive` |
| `phoenix` | 1816 | 电磁弹 | Electromagnetic Shot | `electromagnetic_shot` |
| `phoenix` | 2916 | 量子重组 | Quantum Reassembly | `quantum_reassembly` |
| `phoenix` | 10216 | 射程强化 | Range Enhancement | `range_enhancement` |
| `phoenix` | 10316 | 发射器过载 | Launcher Overload | `launcher_overload` |
| `phoenix` | 10816 | 精英射手 | Elite Marksman | `elite_marksman` |
| `phoenix` | 10916 | 蓄能攻击 | Charged Shot | `charged_shot` |
| `raiden` | 227 | 能量护盾 | Energy Shield | `energy_shield` |
| `raiden` | 1827 | 电磁弹 | Electromagnetic Shot | `electromagnetic_shot` |
| `raiden` | 4027 | 连锁 | Chain | `chain` |
| `raiden` | 4127 | 电离 | Ionization | `ionization` |
| `raiden` | 10227 | 射程强化 | Range Enhancement | `range_enhancement` |
| `raiden` | 110271 | 分叉 | Fork | `fork` |
| `rhino` | 905 | 战地维修 | Field Maintenance | `field_maintenance` |
| `rhino` | 1109 | 旋风斩 | Whirlwind | `whirlwind` |
| `rhino` | 2305 | 残骸利用 | Wreckage Recycling | `wreckage_recycling` |
| `rhino` | 2505 | 动力装甲 | Power Armor | `power_armor` |
| `rhino` | 2805 | 最后一击 | Final Blitz | `final_blitz` |
| `rhino` | 3005 | 装甲强化 | Armor Enhancement | `armor_enhancement` |
| `rhino` | 10505 | 机械狂暴 | Mechanical Rage | `mechanical_rage` |
| `rhino` | 180305 | 光子涂层 | Photon Coating | `photon_coating` |
| `rhino` | 180805 | 战斗进化 | Combat Evolvement | `combat_evolvement` |
| `sabertooth` | 721 | 双发 | Doubleshot | `doubleshot` |
| `sabertooth` | 3321 | 导弹拦截 | Missile Interceptor | `missile_interceptor` |
| `sabertooth` | 4721 | 野战工事 | Field Entrenchment  | `field_entrenchment` |
| `sabertooth` | 10221 | 射程强化 | Range Enhancement | `range_enhancement` |
| `sabertooth` | 10321 | 战地维修 | Field Maintenance | `field_maintenance` |
| `sabertooth` | 110211 | 副炮 | Secondary Armament | `secondary_armament` |
| `sabertooth` | 110212 | 防空导弹 | Anti-Air Missile | `anti_air_missile` |
| `sandworm` | 923 | 潜地维修 | Burrow Maintenance | `burrow_maintenance` |
| `sandworm` | 3023 | 装甲强化 | Armor Enhancement | `armor_enhancement` |
| `sandworm` | 3123 | 对空 | Anti-Aerial | `anti_aerial` |
| `sandworm` | 3623 | 复制 | Replicate | `replicate` |
| `sandworm` | 3723 | 沙暴 | Sandstorm | `sandstorm` |
| `sandworm` | 3823 | 突袭 | Strike | `strike` |
| `sandworm` | 10523 | 机械狂暴 | Mechanical Rage | `mechanical_rage` |
| `sandworm` | 13023 | 机械分裂 | Mechanical Division | `mechanical_division` |
| `scorpion` | 719 | 双发 | Doubleshot | `doubleshot` |
| `scorpion` | 919 | 战地维修 | Field Maintenance | `field_maintenance` |
| `scorpion` | 3019 | 装甲强化 | Armor Enhancement | `armor_enhancement` |
| `scorpion` | 10019 | 攻城模式 | Siege Mode | `siege_mode` |
| `scorpion` | 10219 | 射程强化 | Range Enhancement | `range_enhancement` |
| `scorpion` | 10419 | 收束射击 | Convergent Fire | `convergent_fire` |
| `scorpion` | 180519 | 酸性攻击 | Acid Attack | `acid_attack` |
| `sledgehammer` | 613 | 伤害分摊 | Damage Sharing | `damage_sharing` |
| `sledgehammer` | 913 | 战地维修 | Field Maintenance | `field_maintenance` |
| `sledgehammer` | 1813 | 电磁弹 | Electromagnetic Shot | `electromagnetic_shot` |
| `sledgehammer` | 3013 | 装甲强化 | Armor Enhancement | `armor_enhancement` |
| `sledgehammer` | 10213 | 射程强化 | Range Enhancement | `range_enhancement` |
| `sledgehammer` | 10513 | 机械狂暴 | Mechanical Rage | `mechanical_rage` |
| `sledgehammer` | 10613 | 穿甲弹 | Armor-Piercing Bullets | `armor_piercing_bullets` |
| `steel_ball` | 308 | 能量汲取 | Energy Absorption | `energy_absorption` |
| `steel_ball` | 608 | 伤害分摊 | Damage Sharing | `damage_sharing` |
| `steel_ball` | 1308 | 机械分裂 | Mechanical Division | `mechanical_division` |
| `steel_ball` | 2408 | 重装锁定 | Fortified Target Lock | `fortified_target_lock` |
| `steel_ball` | 3008 | 装甲强化 | Armor Enhancement | `armor_enhancement` |
| `steel_ball` | 10208 | 射程强化 | Range Enhancement | `range_enhancement` |
| `steel_ball` | 180808 | 滚动充能 | Kinetic Charge | `kinetic_charge` |
| `stormcaller` | 412 | 高爆弹药 | High-Explosive Ammo | `high_explosive_ammo` |
| `stormcaller` | 812 | 燃烧弹 | Incendiary Bomb | `incendiary_bomb` |
| `stormcaller` | 1812 | 电磁爆炸 | Electromagnetic Explosion | `electromagnetic_explosion` |
| `stormcaller` | 10212 | 射程强化 | Range Enhancement | `range_enhancement` |
| `stormcaller` | 10312 | 发射器过载 | Launcher Overload | `launcher_overload` |
| `stormcaller` | 10912 | 重型导弹 | Heavy Missile | `heavy_missile` |
| `tarantula` | 424 | 高爆弹药 | High-Explosive Ammo | `high_explosive_ammo` |
| `tarantula` | 924 | 战地维修 | Field Maintenance | `field_maintenance` |
| `tarantula` | 3024 | 装甲强化 | Armor Enhancement | `armor_enhancement` |
| `tarantula` | 3124 | 防空弹药 | Anti-Aircraft Ammunition | `anti_aircraft_ammunition` |
| `tarantula` | 10224 | 射程强化 | Range Enhancement | `range_enhancement` |
| `tarantula` | 10524 | 机械狂暴 | Mechanical Rage | `mechanical_rage` |
| `tarantula` | 10624 | 穿甲弹 | Armor-Piercing Bullets | `armor_piercing_bullets` |
| `tarantula` | 11024 | 蜘蛛雷 | Spider Mine | `spider_mine` |
| `typhoon` | 2922 | 战地重组 | Field Reassembly | `field_reassembly` |
| `typhoon` | 4722 | 野战工事 | Field Entrenchment  | `field_entrenchment` |
| `typhoon` | 5122 | 反应装甲 | Reactive Armor | `reactive_armor` |
| `typhoon` | 5222 | 维修阵列 | Maintenance Array | `maintenance_array` |
| `typhoon` | 5322 | 残骸引爆 | Wreckage Detonation | `wreckage_detonation` |
| `typhoon` | 10222 | 射程强化 | Range Enhancement | `range_enhancement` |
| `typhoon` | 1102022 | 防空标记 | Air Defense Mark | `air_defense_mark` |
| `void_eye` | 230 | 能量护盾 | Energy Shield | `energy_shield` |
| `void_eye` | 330 | 能量汲取 | Energy Absorption | `energy_absorption` |
| `void_eye` | 4430 | 飞行模式 | Aerial Mode | `aerial_mode` |
| `void_eye` | 10230 | 射程强化 | Range Enhancement | `range_enhancement` |
| `void_eye` | 10930 | 蓄能攻击 | Charged Shot | `charged_shot` |
| `void_eye` | 180430 | 压制射击 | Suppression Shots | `suppression_shots` |
| `void_eye` | 180530 | 电磁装甲 | Electromagnetic Armor | `electromagnetic_armor` |
| `vortex` | 631 | 并网 | Grid Integration | `grid_integration` |
| `vortex` | 931 | 战地维修 | Field Maintenance | `field_maintenance` |
| `vortex` | 4531 | 电磁云 | Electromagnetic Cloud | `electromagnetic_cloud` |
| `vortex` | 10131 | 机器学习 | Machine Learning | `machine_learning` |
| `vortex` | 10231 | 射程强化 | Range Enhancement | `range_enhancement` |
| `vortex` | 123101 | 电磁双生 | Electromagnetic Twin | `electromagnetic_twin` |
| `vortex` | 180931 | 移动电站 | Mobile Power Station | `mobile_power_station` |
| `vortex` | 493101 | 储能护盾 | Accumulator Shield | `accumulator_shield` |
| `vortex` | 503101 | 应急装甲 | Emergency Armor | `emergency_armor` |
| `vulcan` | 1103 | 燃烧弹 | Incendiary Bomb | `incendiary_bomb` |
| `vulcan` | 1203 | 最佳搭档 | Best Partner | `best_partner` |
| `vulcan` | 3003 | 装甲强化 | Armor Enhancement | `armor_enhancement` |
| `vulcan` | 10203 | 射程强化 | Range Enhancement | `range_enhancement` |
| `vulcan` | 10603 | 高温火焰 | Scorching Fire | `scorching_fire` |
| `vulcan` | 11010 | 黏油弹 | Sticky Oil Bomb | `sticky_oil_bomb` |
| `vulcan` | 180203 | 引燃 | Ignite | `ignite` |
| `war_factory` | 417 | 高爆弹药 | High-Explosive Ammo | `high_explosive_ammo` |
| `war_factory` | 3017 | 装甲强化 | Armor Enhancement | `armor_enhancement` |
| `war_factory` | 3317 | 导弹拦截 | Missile Interceptor | `missile_interceptor` |
| `war_factory` | 10217 | 射程强化 | Range Enhancement | `range_enhancement` |
| `war_factory` | 10317 | 发射器过载 | Launcher Overload | `launcher_overload` |
| `war_factory` | 12017 | 凤凰制造 | Phoenix Production | `phoenix_production` |
| `war_factory` | 12117 | 钢球制造 | Steel Ball Production | `steel_ball_production` |
| `war_factory` | 12217 | 铁锤制造 | Sledgehammer Production | `sledgehammer_production` |
| `war_factory` | 180317 | 光子涂层 | Photon Coating | `photon_coating` |
| `wasp` | 206 | 能量护盾 | Energy Shield | `energy_shield` |
| `wasp` | 406 | 高爆弹药 | High-Explosive Ammo | `high_explosive_ammo` |
| `wasp` | 506 | 对地专精 | Ground Specialization | `ground_specialization` |
| `wasp` | 1606 | 高速引擎 | Jump Drive | `jump_drive` |
| `wasp` | 1806 | 电磁弹 | Electromagnetic Shot | `electromagnetic_shot` |
| `wasp` | 3206 | 防空专精 | Aerial Specialization | `aerial_specialization` |
| `wasp` | 10206 | 射程强化 | Range Enhancement | `range_enhancement` |
| `wasp` | 10606 | 穿甲弹 | Armor-Piercing Bullets | `armor_piercing_bullets` |
| `wasp` | 10806 | 精英射手 | Elite Marksman | `elite_marksman` |
| `wasp` | 180206 | 引燃 | Ignite | `ignite` |
| `wraith` | 418 | 高爆弹药 | High-Explosive Ammo | `high_explosive_ammo` |
| `wraith` | 918 | 战地维修 | Field Maintenance | `field_maintenance` |
| `wraith` | 3018 | 装甲强化 | Armor Enhancement | `armor_enhancement` |
| `wraith` | 4418 | 地面巡航 | Land Cruiser | `land_cruiser` |
| `wraith` | 10218 | 射程强化 | Range Enhancement | `range_enhancement` |
| `wraith` | 110181 | 浮游炮阵 | Floating Artillery Array | `floating_artillery_array` |
| `wraith` | 180418 | 退化光束 | Degeneration Beam | `degeneration_beam` |
<!-- /names -->

## Evidence

### Recorded

- A technology a layout names under its unit is active in the fight, alone and
  beside an officer's correction to the same number:
  `tests/modifier/regressions.mcscript`.

### Read

- A unit is its card and the mech row the card names, and its technologies are
  the card's list: `CardData.mechID`, `CardData.technologies`.
- Researching costs the technology's own supply plus a step per technology
  already active, capped: `UnitUtility.CalculateUpgradeTechnologyCost`,
  `CardData.techUpgradeIncreaseSupplyPerCount`,
  `CardData.techUpgradeMaxSupplyLimit`,
  `Config.upgradeTechnologyCostIncreaseDelta`.

### Not established

- **That `defaultTechnologies` is an account rule.** No match code read here
  reads `CardData.defaultTechnologies`, and what does is not traced.
- **A research's price in a match.** The formula is read; no pin under `tests/`
  checks a paid price against it.
