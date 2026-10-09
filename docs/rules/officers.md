# Officers

An officer is what a layout or a state lists under `officers`, named by the
snake case of its English name in [`config/names.yaml`](../../config/names.yaml).
It is a row of `ConfigDataContainer.officerDatas`.

## What an officer is

An officer ID is also the ID of the card that grants it, so taking card `20023`
adds officer `20023`. [reinforce_items.md](reinforce_items.md) explains the
card system that deals one.

An officer may grant a commander skill, equipment, or a squad of units, but
those are not additional `officers` entries. The skills are `CommanderSkillData`
IDs, a separate ID space that `battle_skills` uses. An officer hands out what
it hands out in each round its `activeRound` lists: an absolute round, not one
counted from its arrival. An officer that lists no round hands out as it is
taken: the Mass-produced equipment officers (`10524` to `10526`) put their
three items into the inventory the moment the card is chosen.

Secondary Equipment Expert (`10015`), an opening specialist, lists every round
and hands out **one** of its four items each time, not all four. The item is
drawn from the side's own stream, `Player.random`, which the player's seed
starts and nothing else in a standard match draws from, so the stream stands
one value further on for every round the side has held the officer. An item
the side does not fit stays in its inventory.

Where its effects live:

- [`config/officers.yaml`](../../config/officers.yaml): what it does to a
  ledger, a discount and the units it applies to, an income, and the skills,
  equipment and opening squad it hands out. It holds only the officers a
  standard 1v1 side can hold: a card the pool deals, an opening's specialist,
  a chain blueprint's officer and a unit round's supply.
  `scripts/extract/extract_prices.py` writes it.
- [`config/officer_effects.yaml`](../../config/officer_effects.yaml): the
  corrections it writes onto units in a fight, which
  [officer_effects.md](officer_effects.md) explains.
  `scripts/extract/extract-officer-effects.py` writes it.
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
  for another of the group when its appear condition fails, which for them is
  `SupplyPercent`.
- **Research Center levels** `20300`, `20301`, `20310` and `20311` are owned by
  the `blueprints` field; a document states them there and never in
  `officers`.

- **Unit round supplies** `50008` to `50013` are what a unit round pays a side
  that declines its offer, `UnitReinforceRoundPool.supplyReinforceID`; they
  carry `granted_supply` and are not named.

No other officer reaches a standard 1v1 side. `officerDatas` holds more, test
data, Interstellar Expedition's and rows no standard pool deals, and they are
in no table and not named: a layout cannot hold one.

## Additional Deployment Slot

Additional Deployment Slot, `10004`, raises the purchases a round allows by
one, in the round it is taken and in every round after, since the side keeps
the officer. A round opens with two purchases plus one per copy held.

## Equipment Expansion

Equipment Expansion, `10540`, sets `equipmentCountChangeValue` to 1, which
raises every formation's equipment slots from one to two
([equipment.md](equipment.md)).

## Names

<!-- names: officers -->
| ID | English | Document name |
| ---: | --- | --- |
| 10002 | Supply Specialist | `supply_specialist` |
| 10003 | Super Supply Enhancement | `super_supply_enhancement` |
| 10004 | Additional Deployment Slot | `additional_deployment_slot` |
| 10007 | Advanced Shield Device | `advanced_shield_device` |
| 10008 | Advanced Missile Device | `advanced_missile_device` |
| 10009 | Quick Teleport | `quick_teleport` |
| 10010 | Quick Supply Specialist | `quick_supply_specialist` |
| 10011 | Missile Specialist | `missile_specialist` |
| 10014 | Training Specialist | `training_specialist` |
| 10015 | Secondary Equipment Expert | `secondary_equipment_expert` |
| 10524 | Mass-produced laser sight | `mass_produced_laser_sight` |
| 10525 | Mass Production Fire Control System | `mass_production_fire_control_system` |
| 10526 | Mass Production Heavy Armor | `mass_production_heavy_armor` |
| 10540 | Equipment Expansion | `equipment_expansion` |
| 20001 | Advanced Defensive Tactics | `advanced_defensive_tactics` |
| 20002 | Advanced Offensive Tactics | `advanced_offensive_tactics` |
| 20003 | Efficient Tech Research | `efficient_tech_research` |
| 20004 | Advanced Power System | `advanced_power_system` |
| 20005 | Giant Specialist | `giant_specialist` |
| 20006 | Advanced Targeting System | `advanced_targeting_system` |
| 20007 | Supply Enhancement | `supply_enhancement` |
| 20021 | Aerial Specialist | `aerial_specialist` |
| 20022 | Efficient Giant Manufacturing | `efficient_giant_manufacturing` |
| 20023 | Efficient Light Manufacturing | `efficient_light_manufacturing` |
| 20024 | Speed Specialist | `speed_specialist` |
| 20029 | Marksman Specialist | `marksman_specialist` |
| 20032 | Elite Specialist | `elite_specialist` |
| 20033 | Rhino Specialist | `rhino_specialist` |
| 20034 | Cost Control Specialist | `cost_control_specialist` |
| 20035 | Fortified Specialist | `fortified_specialist` |
| 20036 | Sabertooth Specialist | `sabertooth_specialist` |
| 20038 | Fire Badger Specialist | `fire_badger_specialist` |
| 20039 | Typhoon Specialist | `typhoon_specialist` |
| 30101 | Mass-Produced Fortress | `mass_produced_fortress` |
| 30102 | Assault Fortress | `assault_fortress` |
| 30104 | Improved Fortress | `improved_fortress` |
| 30105 | Extended Range Fortress | `extended_range_fortress` |
| 30201 | Extended Range Marksman | `extended_range_marksman` |
| 30202 | Smart Marksman | `smart_marksman` |
| 30203 | Subsidized Marksman | `subsidized_marksman` |
| 30204 | Elite Marksman | `elite_marksman` |
| 30301 | Extended Range Vulcan | `extended_range_vulcan` |
| 30302 | Assault Vulcan | `assault_vulcan` |
| 30401 | Assault Melting Point | `assault_melting_point` |
| 30402 | Improved Melting Point | `improved_melting_point` |
| 30403 | Mass-Produced Melting Point | `mass_produced_melting_point` |
| 30501 | Mass-Produced Rhino | `mass_produced_rhino` |
| 30502 | Berserk Rhino | `berserk_rhino` |
| 30503 | Elite Rhino | `elite_rhino` |
| 30601 | Mass-Produced Wasp | `mass_produced_wasp` |
| 30602 | Improved Wasp | `improved_wasp` |
| 30604 | Elite Wasp | `elite_wasp` |
| 30701 | Subsidized Mustang | `subsidized_mustang` |
| 30702 | Fortified Mustang | `fortified_mustang` |
| 30703 | Elite Mustang | `elite_mustang` |
| 30801 | Subsidized Steel Ball | `subsidized_steel_ball` |
| 30803 | Improved Steel Ball | `improved_steel_ball` |
| 30804 | Elite Steel Ball | `elite_steel_ball` |
| 30901 | Elite Fang | `elite_fang` |
| 30902 | Assault Fang | `assault_fang` |
| 31001 | Subsidized Crawler | `subsidized_crawler` |
| 31002 | Elite Crawler | `elite_crawler` |
| 31101 | Fortified Overlord | `fortified_overlord` |
| 31102 | Mass-Produced Overlord | `mass_produced_overlord` |
| 31104 | Improved Overlord | `improved_overlord` |
| 31201 | Assault Stormcaller | `assault_stormcaller` |
| 31202 | Extended Range Stormcaller | `extended_range_stormcaller` |
| 31203 | Subsidized Stormcaller | `subsidized_stormcaller` |
| 31205 | Elite Stormcaller | `elite_stormcaller` |
| 31301 | Mass-Produced Sledgehammer | `mass_produced_sledgehammer` |
| 31302 | Extended Range Sledgehammer | `extended_range_sledgehammer` |
| 31304 | Improved Sledgehammer | `improved_sledgehammer` |
| 31305 | Elite Sledgehammer | `elite_sledgehammer` |
| 31402 | Fortified Hacker | `fortified_hacker` |
| 31403 | Elite Hacker | `elite_hacker` |
| 31501 | Subsidized Arclight | `subsidized_arclight` |
| 31502 | Smart Arclight | `smart_arclight` |
| 31503 | Fortified Arclight | `fortified_arclight` |
| 31504 | Extended Range Arclight | `extended_range_arclight` |
| 31505 | Elite Arclight | `elite_arclight` |
| 31601 | Mass-Produced Phoenix | `mass_produced_phoenix` |
| 31602 | Extended Range Phoenix | `extended_range_phoenix` |
| 31603 | Improved Phoenix | `improved_phoenix` |
| 31604 | Elite Phoenix | `elite_phoenix` |
| 31701 | Extended Range War Factory | `extended_range_war_factory` |
| 31702 | Improved War Factory | `improved_war_factory` |
| 31801 | Mass-Produced Wraith | `mass_produced_wraith` |
| 31802 | Improved Wraith | `improved_wraith` |
| 31901 | Assault Scorpion | `assault_scorpion` |
| 31902 | Mass-Produced Scorpion | `mass_produced_scorpion` |
| 31903 | Improved Scorpion | `improved_scorpion` |
| 32101 | Extended Range Sabertooth | `extended_range_sabertooth` |
| 32102 | Mass-Produced Sabertooth | `mass_produced_sabertooth` |
| 32103 | Improved Sabertooth | `improved_sabertooth` |
| 32301 | Improved Sandworm | `improved_sandworm` |
| 32302 | Mass-Produced Sandworm | `mass_produced_sandworm` |
| 32401 | Improved Tarantula | `improved_tarantula` |
| 32402 | Elite Tarantula | `elite_tarantula` |
| 32501 | Extended Range Phantom Ray | `extended_range_phantom_ray` |
| 32601 | Mass-Produced Farseer | `mass_produced_farseer` |
| 32602 | Fortified Farseer | `fortified_farseer` |
| 32701 | Assault Raiden | `assault_raiden` |
| 32801 | Strike Hound | `strike_hound` |
| 33001 | Improved Void Eye | `improved_void_eye` |
<!-- /names -->

## Evidence

### Recorded

- Equipment Expansion gives a formation a second slot, and each of its two items
  writes: `tests/equipment/fights/`.

### Replayed

- A Mass-produced equipment officer's three items are in the inventory from the
  decision that took it, and can be fitted in the same round:
  `scripts/corpus/verify-matches.py`.
- Secondary Equipment Expert hands out one item a round, the one its side's
  stream draws: `scripts/corpus/verify-matches.py`.
- A side's own stream is where its seed puts it, advanced once for every
  hand-out an earlier round drew, on every round of every replay; conversion
  refuses a replay where it is not: `scripts/corpus/verify-matches.py`.
- An officer card taken puts its own ID into the side's officers:
  `scripts/corpus/verify-matches.py`.

### Read

- An officer hands out what it hands out in a round its `activeRound` lists,
  the match's round and not one counted from its arrival:
  `OfficerSystem.ActiveOfficerEffect`, `OfficerData.IsActiveRound`,
  `OfficerData.activeRound`.
- An officer that lists no round hands out when it is added with every effect,
  which is how a chosen card adds it and not how a snapshot restores it:
  `SystemOfficerController.PerformOfficerEffect`,
  `OfficerData.IsActiveRoundEmpty`, `ReinforcementSystem.AddReinforceItem`,
  `OfficerEffectMask.All`.
- An officer that draws hands out one of its items, drawn from the player's
  stream: `SystemOfficerController.PerformOfficerRoundEffect`,
  `OfficerData.randomEquipment`, `Player.random`.
- The player's stream is seeded once, from the seed the player was created
  with: `Player.Init`, `Player.RefreshRandom`, `RanState.math_randomseed`.
- A unit modification's group is its `typeID`: `OfficerData.typeID`.
- Equipment Expansion writes a slot: `OfficerData.equipmentCountChangeValue`.

### Not established

- **Additional Deployment Slot's purchase.** Every round opening in another
  version's corpus held two purchases plus one per copy held. No match of this
  version's corpus takes it, and the code that counts a round's purchases is
  not read.
- **How a draw picks among the items.** `RandomElementSync` is generic and its
  body is not readable in the dump; the pick is written as the stream's range
  draw over the list, and with four items every masked projection agrees, so
  the corpus cannot separate them.
- **The player seed a Training Ground match runs under.** An installed match
  with the officer draws from whatever stream the Training Ground starts, not
  the recorded one.
