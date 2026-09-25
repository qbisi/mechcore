# Officers

An officer is what a layout or a state lists under `officers`, named by the
snake case of its English name in [`config/names.yaml`](../../config/names.yaml).
It is a row of `ConfigDataContainer.officerDatas`. The rules below are read
from build 2.0.0.1.2324's dump; the replays behind them were recorded on
1.11.1.3.2259.

## What an officer is

An officer ID is also the ID of the card that grants it, so taking card `20023`
adds officer `20023`. [reinforce_items.md](reinforce_items.md) explains the
card system that deals one.

An officer may grant a commander skill, equipment, or a squad of units, but
those are not additional `officers` entries. The skills are `CommanderSkillData`
IDs, a separate ID space that `battle_skills` uses. An officer hands out what
it hands out in each round its `activeRound` lists: an absolute round, not one
counted from its arrival. `activeRound` is a list from build 2.0 on, and one
officer (Secondary Equipment Specialist, `10015`) lists every round.

Where its effects live:

- [`config/officers.yaml`](../../config/officers.yaml): what it does to a
  ledger, a discount and the units it applies to, an income, the bounty for a
  giant, and the skills, equipment and opening squad it hands out.
  `scripts/extract_prices.py` writes it.
- [`config/officer_effects.yaml`](../../config/officer_effects.yaml): the
  corrections it writes onto units in a fight, which
  [officer_effects.md](officer_effects.md) explains.
  `scripts/extract-officer-effects.py` writes it.
- [`config/reinforcements.yaml`](../../config/reinforcements.yaml): when the
  pool may deal it, its level, group and appear condition, which
  [reinforcements.md](reinforcements.md) explains.

## Kinds

- **Opening specialists** are the officers whose `scope` is 2: a side picks one
  as its opening in round 0, and [`config/advance_teams.yaml`](../../config/advance_teams.yaml)
  lists them beside the unit teams.
- **Reinforcement officers** have `scope` 1 and are dealt by the pool.
- **Unit modifications** are reinforcement officers with a positive `typeID`,
  one group per unit: the pool holds one representative per group and swaps it
  for another of the group when its appear condition fails. Build 2.0 gives
  them the `SupplyPercent` condition.
- **Research Center levels** `20300`, `20301`, `20310` and `20311` are owned by
  the `blueprints` field; a document states them there and never in
  `officers`.

Rows that are test data or limited to Interstellar Expedition are none of
these and are not named.

## Additional Deployment Slot

Additional Deployment Slot, `10004`, raises the purchases a round allows by
one, in the round it is taken and in every round after, since the side keeps
the officer. A round opens with two purchases plus one per copy held. That is
measured on the 2259 replay corpus in mechcore-replay, whose every round
opening holds exactly that many.

## Equipment Expansion

Equipment Expansion, `10540`, new in build 2.0, sets `equipmentCountChangeValue`
to 1, which raises every formation's equipment slots from one to two
([equipment.md](equipment.md)); `tests/equipment/two-items.yaml` records it.

## Names

<!-- names: officers -->
| ID | 中文 | English | Document name |
| ---: | --- | --- | --- |
| 10002 | 补给专家 | Supply Specialist | `supply_specialist` |
| 10003 | 超级补给强化 | Super Supply Enhancement | `super_supply_enhancement` |
| 10004 | 额外部署位 | Additional Deployment Slot | `additional_deployment_slot` |
| 10007 | 先进护盾装置 | Advanced Shield Device | `advanced_shield_device` |
| 10008 | 先进飞弹装置 | Advanced Missile Device | `advanced_missile_device` |
| 10009 | 快速传送 | Quick Teleport | `quick_teleport` |
| 10010 | 快速补给专家 | Quick Supply Specialist | `quick_supply_specialist` |
| 10011 | 导弹专家 | Missile Specialist | `missile_specialist` |
| 10014 | 训练专家 | Training Specialist | `training_specialist` |
| 10015 | 次级装备专家 | Secondary Equipment Expert | `secondary_equipment_expert` |
| 10524 | 量产激光瞄具 | Mass-produced laser sight | `mass_produced_laser_sight` |
| 10525 | 量产火控系统 | Mass Production Fire Control System | `mass_production_fire_control_system` |
| 10526 | 量产重型装甲 | Mass Production Heavy Armor | `mass_production_heavy_armor` |
| 10540 | 装备扩容 | Equipment Expansion | `equipment_expansion` |
| 20001 | 先进防御战术 | Advanced Defensive Tactics | `advanced_defensive_tactics` |
| 20002 | 先进进攻战术 | Advanced Offensive Tactics | `advanced_offensive_tactics` |
| 20003 | 高效科技研发 | Efficient Tech Research | `efficient_tech_research` |
| 20004 | 先进动力系统 | Advanced Power System | `advanced_power_system` |
| 20005 | 巨型专家 | Giant Specialist | `giant_specialist` |
| 20006 | 先进瞄准系统 | Advanced Targeting System | `advanced_targeting_system` |
| 20007 | 补给强化 | Supply Enhancement | `supply_enhancement` |
| 20021 | 空军专家 | Aerial Specialist | `aerial_specialist` |
| 20022 | 高效巨型制造 | Efficient Giant Manufacturing | `efficient_giant_manufacturing` |
| 20023 | 高效小型制造 | Efficient Light Manufacturing | `efficient_light_manufacturing` |
| 20024 | 速度专家 | Speed Specialist | `speed_specialist` |
| 20029 | 长弓专家 | Marksman Specialist | `marksman_specialist` |
| 20032 | 精英专家 | Elite Specialist | `elite_specialist` |
| 20033 | 犀牛专家 | Rhino Specialist | `rhino_specialist` |
| 20034 | 成本控制专家 | Cost Control Specialist | `cost_control_specialist` |
| 20035 | 重装专家 | Fortified Specialist | `fortified_specialist` |
| 20036 | 剑齿虎专家 | Sabertooth Specialist | `sabertooth_specialist` |
| 20038 | 火獾专家 | Fire Badger Specialist | `fire_badger_specialist` |
| 20039 | 台风专家 | Typhoon Specialist | `typhoon_specialist` |
| 30101 | 量产堡垒 | Mass-Produced Fortress | `mass_produced_fortress` |
| 30102 | 突击堡垒 | Assault Fortress | `assault_fortress` |
| 30104 | 改进型堡垒 | Improved Fortress | `improved_fortress` |
| 30105 | 增程堡垒 | Extended Range Fortress | `extended_range_fortress` |
| 30201 | 增程长弓 | Extended Range Marksman | `extended_range_marksman` |
| 30202 | 智能长弓 | Smart Marksman | `smart_marksman` |
| 30203 | 长弓补贴 | Subsidized Marksman | `subsidized_marksman` |
| 30204 | 精英长弓 | Elite Marksman | `elite_marksman` |
| 30301 | 增程火神 | Extended Range Vulcan | `extended_range_vulcan` |
| 30302 | 突击火神 | Assault Vulcan | `assault_vulcan` |
| 30401 | 突击熔点 | Assault Melting Point | `assault_melting_point` |
| 30402 | 改进型熔点 | Improved Melting Point | `improved_melting_point` |
| 30403 | 量产熔点 | Mass-Produced Melting Point | `mass_produced_melting_point` |
| 30501 | 量产犀牛 | Mass-Produced Rhino | `mass_produced_rhino` |
| 30502 | 狂暴犀牛 | Berserk Rhino | `berserk_rhino` |
| 30503 | 精英犀牛 | Elite Rhino | `elite_rhino` |
| 30601 | 量产兵蜂 | Mass-Produced Wasp | `mass_produced_wasp` |
| 30602 | 改进型兵蜂 | Improved Wasp | `improved_wasp` |
| 30604 | 精英兵蜂 | Elite Wasp | `elite_wasp` |
| 30701 | 野马补贴 | Subsidized Mustang | `subsidized_mustang` |
| 30702 | 重装野马 | Fortified Mustang | `fortified_mustang` |
| 30703 | 精英野马 | Elite Mustang | `elite_mustang` |
| 30801 | 钢球补贴 | Subsidized Steel Ball | `subsidized_steel_ball` |
| 30803 | 改进型钢球 | Improved Steel Ball | `improved_steel_ball` |
| 30804 | 精英钢球 | Elite Steel Ball | `elite_steel_ball` |
| 30901 | 精英尖牙 | Elite Fang | `elite_fang` |
| 30902 | 突击尖牙 | Assault Fang | `assault_fang` |
| 31001 | 爬虫补贴 | Subsidized Crawler | `subsidized_crawler` |
| 31002 | 精英爬虫 | Elite Crawler | `elite_crawler` |
| 31101 | 重装霸主 | Fortified Overlord | `fortified_overlord` |
| 31102 | 量产霸主 | Mass-Produced Overlord | `mass_produced_overlord` |
| 31104 | 改进型霸主 | Improved Overlord | `improved_overlord` |
| 31201 | 突击暴雨 | Assault Stormcaller | `assault_stormcaller` |
| 31202 | 增程暴雨 | Extended Range Stormcaller | `extended_range_stormcaller` |
| 31203 | 暴雨补贴 | Subsidized Stormcaller | `subsidized_stormcaller` |
| 31205 | 精英暴雨 | Elite Stormcaller | `elite_stormcaller` |
| 31301 | 量产铁锤 | Mass-Produced Sledgehammer | `mass_produced_sledgehammer` |
| 31302 | 增程铁锤 | Extended Range Sledgehammer | `extended_range_sledgehammer` |
| 31304 | 改进型铁锤 | Improved Sledgehammer | `improved_sledgehammer` |
| 31305 | 精英铁锤 | Elite Sledgehammer | `elite_sledgehammer` |
| 31402 | 重装骇客 | Fortified Hacker | `fortified_hacker` |
| 31403 | 精英骇客 | Elite Hacker | `elite_hacker` |
| 31501 | 弧光补贴 | Subsidized Arclight | `subsidized_arclight` |
| 31502 | 智能弧光 | Smart Arclight | `smart_arclight` |
| 31503 | 重装弧光 | Fortified Arclight | `fortified_arclight` |
| 31504 | 增程弧光 | Extended Range Arclight | `extended_range_arclight` |
| 31505 | 精英弧光 | Elite Arclight | `elite_arclight` |
| 31601 | 量产凤凰 | Mass-Produced Phoenix | `mass_produced_phoenix` |
| 31602 | 增程凤凰 | Extended Range Phoenix | `extended_range_phoenix` |
| 31603 | 改进型凤凰 | Improved Phoenix | `improved_phoenix` |
| 31604 | 精英凤凰 | Elite Phoenix | `elite_phoenix` |
| 31701 | 增程战争工厂 | Extended Range War Factory | `extended_range_war_factory` |
| 31702 | 改进型战争工厂 | Improved War Factory | `improved_war_factory` |
| 31801 | 量产恶灵 | Mass-Produced Wraith | `mass_produced_wraith` |
| 31802 | 改进型恶灵 | Improved Wraith | `improved_wraith` |
| 31901 | 突击狂蝎 | Assault Scorpion | `assault_scorpion` |
| 31902 | 量产狂蝎 | Mass-Produced Scorpion | `mass_produced_scorpion` |
| 31903 | 改进型狂蝎 | Improved Scorpion | `improved_scorpion` |
| 32101 | 增程剑齿虎 | Extended Range Sabertooth | `extended_range_sabertooth` |
| 32102 | 量产剑齿虎 | Mass-Produced Sabertooth | `mass_produced_sabertooth` |
| 32103 | 改进型剑齿虎 | Improved Sabertooth | `improved_sabertooth` |
| 32301 | 改进型沙虫 | Improved Sandworm | `improved_sandworm` |
| 32302 | 量产沙虫 | Mass-Produced Sandworm | `mass_produced_sandworm` |
| 32401 | 改进型狼蛛 | Improved Tarantula | `improved_tarantula` |
| 32402 | 精英狼蛛 | Elite Tarantula | `elite_tarantula` |
| 32501 | 增程鬼鳐 | Extended Range Phantom Ray | `extended_range_phantom_ray` |
| 32601 | 量产先知 | Mass-Produced Farseer | `mass_produced_farseer` |
| 32602 | 重装先知 | Fortified Farseer | `fortified_farseer` |
| 32701 | 突击雷霆 | Assault Raiden | `assault_raiden` |
| 32801 | 强击猎犬 | Strike Hound | `strike_hound` |
| 33001 | 改进型魔眼 | Improved Void Eye | `improved_void_eye` |
<!-- /names -->
