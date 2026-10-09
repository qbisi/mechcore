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
a unit costs to buy, to unlock and to raise one level; `scripts/extract/extract_prices.py`
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
| ID | English | Document name |
| ---: | --- | --- |
| 1 | Fortress | `fortress` |
| 2 | Marksman | `marksman` |
| 3 | Vulcan | `vulcan` |
| 4 | Melting Point | `melting_point` |
| 5 | Rhino | `rhino` |
| 6 | Wasp | `wasp` |
| 7 | Mustang | `mustang` |
| 8 | Steel Ball | `steel_ball` |
| 9 | Fang | `fang` |
| 10 | Crawler | `crawler` |
| 11 | Overlord | `overlord` |
| 12 | Stormcaller | `stormcaller` |
| 13 | Sledgehammer | `sledgehammer` |
| 14 | Hacker | `hacker` |
| 15 | Arclight | `arclight` |
| 16 | Phoenix | `phoenix` |
| 17 | War Factory | `war_factory` |
| 18 | Wraith | `wraith` |
| 19 | Scorpion | `scorpion` |
| 20 | Fire Badger | `fire_badger` |
| 21 | Sabertooth | `sabertooth` |
| 22 | Typhoon | `typhoon` |
| 23 | Sandworm | `sandworm` |
| 24 | Tarantula | `tarantula` |
| 25 | Phantom Ray | `phantom_ray` |
| 26 | Farseer | `farseer` |
| 27 | Raiden | `raiden` |
| 28 | Hound | `hound` |
| 29 | Abyss | `abyss` |
| 30 | Void Eye | `void_eye` |
| 31 | Vortex | `vortex` |
| 32 | Centurion | `centurion` |
| 2002 | Mountain | `mountain` |
<!-- /names -->

## Technologies

<!-- names: technologies -->
| Unit | ID | English | Document name |
| --- | ---: | --- | --- |
| `abyss` | 2329 | Wreckage Recycling | `wreckage_recycling` |
| `abyss` | 4329 | Vertical Sweep | `vertical_sweep` |
| `abyss` | 10229 | Range Enhancement | `range_enhancement` |
| `abyss` | 11029 | Disintegration | `disintegration` |
| `abyss` | 12029 | Dark Companion | `dark_companion` |
| `abyss` | 110291 | Swarm Missiles | `swarm_missiles` |
| `abyss` | 180329 | Photon Coating | `photon_coating` |
| `arclight` | 1815 | Electromagnetic Shot | `electromagnetic_shot` |
| `arclight` | 3015 | Armor Enhancement | `armor_enhancement` |
| `arclight` | 3115 | Anti-Aircraft Ammunition | `anti_aircraft_ammunition` |
| `arclight` | 4515 | Shockwave | `shockwave` |
| `arclight` | 10215 | Range Enhancement | `range_enhancement` |
| `arclight` | 10815 | Elite Marksman | `elite_marksman` |
| `arclight` | 10915 | Charged Shot | `charged_shot` |
| `centurion` | 1232 | Summon Hounds | `summon_hounds` |
| `centurion` | 5132 | Reactive Armor | `reactive_armor` |
| `centurion` | 5532 | Melee Mode | `melee_mode` |
| `centurion` | 10232 | Range Enhancement | `range_enhancement` |
| `centurion` | 110321 | Dual Wield | `dual_wield` |
| `centurion` | 110322 | Homing Missile | `homing_missile` |
| `crawler` | 2610 | Subterranean Blitz | `subterranean_blitz` |
| `crawler` | 2710 | Acidic Explosion | `acidic_explosion` |
| `crawler` | 3510 | Loose Formation | `loose_formation` |
| `crawler` | 10510 | Mechanical Rage | `mechanical_rage` |
| `crawler` | 10710 | Impact Drill | `impact_drill` |
| `crawler` | 180110 | Replicate | `replicate` |
| `fang` | 209 | Portable Shield | `portable_shield` |
| `fang` | 3109 | Grenade Launcher | `grenade_launcher` |
| `fang` | 10209 | Range Enhancement | `range_enhancement` |
| `fang` | 10509 | Mechanical Rage | `mechanical_rage` |
| `fang` | 10609 | Armor-Piercing Bullets | `armor_piercing_bullets` |
| `fang` | 180209 | Ignite | `ignite` |
| `farseer` | 726 | Burst Mode | `burst_mode` |
| `farseer` | 1826 | Electromagnetic Explosion | `electromagnetic_explosion` |
| `farseer` | 3226 | Aerial Specialization | `aerial_specialization` |
| `farseer` | 3326 | Missile Interceptor | `missile_interceptor` |
| `farseer` | 10226 | Range Enhancement | `range_enhancement` |
| `farseer` | 180326 | Photon Emission | `photon_emission` |
| `farseer` | 180526 | Scanning Radar | `scanning_radar` |
| `fire_badger` | 820 | Napalm | `napalm` |
| `fire_badger` | 920 | Field Maintenance | `field_maintenance` |
| `fire_badger` | 10220 | Range Enhancement | `range_enhancement` |
| `fire_badger` | 10620 | Scorching Fire | `scorching_fire` |
| `fire_badger` | 11020 | Scorching Charge | `scorching_charge` |
| `fire_badger` | 180220 | Ignite | `ignite` |
| `fire_badger` | 180620 | Counter-Fire | `counter_fire` |
| `fortress` | 701 | Doubleshot | `doubleshot` |
| `fortress` | 1001 | Barrier | `barrier` |
| `fortress` | 1105 | Anti-Air Barrage | `anti_air_barrage` |
| `fortress` | 1201 | Fang Production | `fang_production` |
| `fortress` | 3001 | Armor Enhancement | `armor_enhancement` |
| `fortress` | 10201 | Range Enhancement | `range_enhancement` |
| `fortress` | 10301 | Launcher Overload | `launcher_overload` |
| `fortress` | 10401 | Solid Shot | `solid_shot` |
| `fortress` | 10801 | Elite Marksman | `elite_marksman` |
| `fortress` | 110201 | Rocket Punch | `rocket_punch` |
| `hacker` | 1014 | Barrier | `barrier` |
| `hacker` | 1714 | Enhanced Control | `enhanced_control` |
| `hacker` | 1814 | Electromagnetic Interference | `electromagnetic_interference` |
| `hacker` | 10214 | Range Enhancement | `range_enhancement` |
| `hacker` | 11014 | Multi Control | `multi_control` |
| `hound` | 3028 | Armor Enhancement | `armor_enhancement` |
| `hound` | 4228 | Fire Extinguisher | `fire_extinguisher` |
| `hound` | 10228 | Range Enhancement | `range_enhancement` |
| `hound` | 10528 | Mechanical Rage | `mechanical_rage` |
| `hound` | 11028 | Incendiary Bomb | `incendiary_bomb` |
| `hound` | 180828 | Chamber Compression | `chamber_compression` |
| `marksman` | 702 | Doubleshot | `doubleshot` |
| `marksman` | 1202 | Shooting Squad | `shooting_squad` |
| `marksman` | 1802 | Electromagnetic Shot | `electromagnetic_shot` |
| `marksman` | 3202 | Aerial Specialization | `aerial_specialization` |
| `marksman` | 10102 | Assault Mode | `assault_mode` |
| `marksman` | 10202 | Range Enhancement | `range_enhancement` |
| `marksman` | 10402 | Quick Reload | `quick_reload` |
| `marksman` | 10802 | Elite Marksman | `elite_marksman` |
| `melting_point` | 304 | Energy Absorption | `energy_absorption` |
| `melting_point` | 1106 | Electromagnetic Barrage | `electromagnetic_barrage` |
| `melting_point` | 1107 | Energy Diffraction | `energy_diffraction` |
| `melting_point` | 1204 | Crawler Production | `crawler_production` |
| `melting_point` | 3004 | Armor Enhancement | `armor_enhancement` |
| `melting_point` | 10204 | Range Enhancement | `range_enhancement` |
| `mountain` | 72002 | Saturation Bombardment | `saturation_bombardment` |
| `mountain` | 302002 | Mountain Plating | `mountain_plating` |
| `mountain` | 312002 | Anti-Aircraft Ammunition | `anti_aircraft_ammunition` |
| `mountain` | 1022002 | Range Enhancement | `range_enhancement` |
| `mountain` | 1032002 | Extended Range Ammo | `extended_range_ammo` |
| `mountain` | 11020021 | Gun-launched Missile | `gun_launched_missile` |
| `mountain` | 11020022 | Smoke Bomb | `smoke_bomb` |
| `mountain` | 18032002 | Photon Loop | `photon_loop` |
| `mustang` | 407 | High-Explosive Ammo | `high_explosive_ammo` |
| `mustang` | 3207 | Aerial Specialization | `aerial_specialization` |
| `mustang` | 3307 | Missile Interceptor | `missile_interceptor` |
| `mustang` | 4607 | Culling Rounds | `culling_rounds` |
| `mustang` | 10207 | Range Enhancement | `range_enhancement` |
| `mustang` | 10607 | Armor-Piercing Bullets | `armor_piercing_bullets` |
| `overlord` | 411 | High-Explosive Ammo | `high_explosive_ammo` |
| `overlord` | 911 | Field Maintenance | `field_maintenance` |
| `overlord` | 1108 | Overlord Artillery | `overlord_artillery` |
| `overlord` | 1211 | Mothership | `mothership` |
| `overlord` | 1611 | Jump Drive | `jump_drive` |
| `overlord` | 3011 | Armor Enhancement | `armor_enhancement` |
| `overlord` | 10211 | Range Enhancement | `range_enhancement` |
| `overlord` | 10311 | Launcher Overload | `launcher_overload` |
| `overlord` | 180311 | Photon Emission | `photon_emission` |
| `phantom_ray` | 225 | Energy Shield | `energy_shield` |
| `phantom_ray` | 425 | High-Explosive Ammo | `high_explosive_ammo` |
| `phantom_ray` | 725 | Burst Mode | `burst_mode` |
| `phantom_ray` | 3025 | Armor Enhancement | `armor_enhancement` |
| `phantom_ray` | 3225 | Ground Targeting | `ground_targeting` |
| `phantom_ray` | 3925 | Stealth Cloak | `stealth_cloak` |
| `phantom_ray` | 10225 | Range Enhancement | `range_enhancement` |
| `phantom_ray` | 11025 | Sticky Oil Bomb | `sticky_oil_bomb` |
| `phoenix` | 216 | Energy Shield | `energy_shield` |
| `phoenix` | 1616 | Jump Drive | `jump_drive` |
| `phoenix` | 1816 | Electromagnetic Shot | `electromagnetic_shot` |
| `phoenix` | 2916 | Quantum Reassembly | `quantum_reassembly` |
| `phoenix` | 10216 | Range Enhancement | `range_enhancement` |
| `phoenix` | 10316 | Launcher Overload | `launcher_overload` |
| `phoenix` | 10816 | Elite Marksman | `elite_marksman` |
| `phoenix` | 10916 | Charged Shot | `charged_shot` |
| `raiden` | 227 | Energy Shield | `energy_shield` |
| `raiden` | 1827 | Electromagnetic Shot | `electromagnetic_shot` |
| `raiden` | 4027 | Chain | `chain` |
| `raiden` | 4127 | Ionization | `ionization` |
| `raiden` | 10227 | Range Enhancement | `range_enhancement` |
| `raiden` | 110271 | Fork | `fork` |
| `rhino` | 905 | Field Maintenance | `field_maintenance` |
| `rhino` | 1109 | Whirlwind | `whirlwind` |
| `rhino` | 2305 | Wreckage Recycling | `wreckage_recycling` |
| `rhino` | 2505 | Power Armor | `power_armor` |
| `rhino` | 2805 | Final Blitz | `final_blitz` |
| `rhino` | 3005 | Armor Enhancement | `armor_enhancement` |
| `rhino` | 10505 | Mechanical Rage | `mechanical_rage` |
| `rhino` | 180305 | Photon Coating | `photon_coating` |
| `rhino` | 180805 | Combat Evolvement | `combat_evolvement` |
| `sabertooth` | 721 | Doubleshot | `doubleshot` |
| `sabertooth` | 3321 | Missile Interceptor | `missile_interceptor` |
| `sabertooth` | 4721 | Field Entrenchment  | `field_entrenchment` |
| `sabertooth` | 10221 | Range Enhancement | `range_enhancement` |
| `sabertooth` | 10321 | Field Maintenance | `field_maintenance` |
| `sabertooth` | 110211 | Secondary Armament | `secondary_armament` |
| `sabertooth` | 110212 | Anti-Air Missile | `anti_air_missile` |
| `sandworm` | 923 | Burrow Maintenance | `burrow_maintenance` |
| `sandworm` | 3023 | Armor Enhancement | `armor_enhancement` |
| `sandworm` | 3123 | Anti-Aerial | `anti_aerial` |
| `sandworm` | 3623 | Replicate | `replicate` |
| `sandworm` | 3723 | Sandstorm | `sandstorm` |
| `sandworm` | 3823 | Strike | `strike` |
| `sandworm` | 10523 | Mechanical Rage | `mechanical_rage` |
| `sandworm` | 13023 | Mechanical Division | `mechanical_division` |
| `scorpion` | 719 | Doubleshot | `doubleshot` |
| `scorpion` | 919 | Field Maintenance | `field_maintenance` |
| `scorpion` | 3019 | Armor Enhancement | `armor_enhancement` |
| `scorpion` | 10019 | Siege Mode | `siege_mode` |
| `scorpion` | 10219 | Range Enhancement | `range_enhancement` |
| `scorpion` | 10419 | Convergent Fire | `convergent_fire` |
| `scorpion` | 180519 | Acid Attack | `acid_attack` |
| `sledgehammer` | 613 | Damage Sharing | `damage_sharing` |
| `sledgehammer` | 913 | Field Maintenance | `field_maintenance` |
| `sledgehammer` | 1813 | Electromagnetic Shot | `electromagnetic_shot` |
| `sledgehammer` | 3013 | Armor Enhancement | `armor_enhancement` |
| `sledgehammer` | 10213 | Range Enhancement | `range_enhancement` |
| `sledgehammer` | 10513 | Mechanical Rage | `mechanical_rage` |
| `sledgehammer` | 10613 | Armor-Piercing Bullets | `armor_piercing_bullets` |
| `steel_ball` | 308 | Energy Absorption | `energy_absorption` |
| `steel_ball` | 608 | Damage Sharing | `damage_sharing` |
| `steel_ball` | 1308 | Mechanical Division | `mechanical_division` |
| `steel_ball` | 2408 | Fortified Target Lock | `fortified_target_lock` |
| `steel_ball` | 3008 | Armor Enhancement | `armor_enhancement` |
| `steel_ball` | 10208 | Range Enhancement | `range_enhancement` |
| `steel_ball` | 180808 | Kinetic Charge | `kinetic_charge` |
| `stormcaller` | 412 | High-Explosive Ammo | `high_explosive_ammo` |
| `stormcaller` | 812 | Incendiary Bomb | `incendiary_bomb` |
| `stormcaller` | 1812 | Electromagnetic Explosion | `electromagnetic_explosion` |
| `stormcaller` | 10212 | Range Enhancement | `range_enhancement` |
| `stormcaller` | 10312 | Launcher Overload | `launcher_overload` |
| `stormcaller` | 10912 | Heavy Missile | `heavy_missile` |
| `tarantula` | 424 | High-Explosive Ammo | `high_explosive_ammo` |
| `tarantula` | 924 | Field Maintenance | `field_maintenance` |
| `tarantula` | 3024 | Armor Enhancement | `armor_enhancement` |
| `tarantula` | 3124 | Anti-Aircraft Ammunition | `anti_aircraft_ammunition` |
| `tarantula` | 10224 | Range Enhancement | `range_enhancement` |
| `tarantula` | 10524 | Mechanical Rage | `mechanical_rage` |
| `tarantula` | 10624 | Armor-Piercing Bullets | `armor_piercing_bullets` |
| `tarantula` | 11024 | Spider Mine | `spider_mine` |
| `typhoon` | 2922 | Field Reassembly | `field_reassembly` |
| `typhoon` | 4722 | Field Entrenchment  | `field_entrenchment` |
| `typhoon` | 5122 | Reactive Armor | `reactive_armor` |
| `typhoon` | 5222 | Maintenance Array | `maintenance_array` |
| `typhoon` | 5322 | Wreckage Detonation | `wreckage_detonation` |
| `typhoon` | 10222 | Range Enhancement | `range_enhancement` |
| `typhoon` | 1102022 | Air Defense Mark | `air_defense_mark` |
| `void_eye` | 230 | Energy Shield | `energy_shield` |
| `void_eye` | 330 | Energy Absorption | `energy_absorption` |
| `void_eye` | 4430 | Aerial Mode | `aerial_mode` |
| `void_eye` | 10230 | Range Enhancement | `range_enhancement` |
| `void_eye` | 10930 | Charged Shot | `charged_shot` |
| `void_eye` | 180430 | Suppression Shots | `suppression_shots` |
| `void_eye` | 180530 | Electromagnetic Armor | `electromagnetic_armor` |
| `vortex` | 631 | Grid Integration | `grid_integration` |
| `vortex` | 931 | Field Maintenance | `field_maintenance` |
| `vortex` | 4531 | Electromagnetic Cloud | `electromagnetic_cloud` |
| `vortex` | 10131 | Machine Learning | `machine_learning` |
| `vortex` | 10231 | Range Enhancement | `range_enhancement` |
| `vortex` | 123101 | Electromagnetic Twin | `electromagnetic_twin` |
| `vortex` | 180931 | Mobile Power Station | `mobile_power_station` |
| `vortex` | 493101 | Accumulator Shield | `accumulator_shield` |
| `vortex` | 503101 | Emergency Armor | `emergency_armor` |
| `vulcan` | 1103 | Incendiary Bomb | `incendiary_bomb` |
| `vulcan` | 1203 | Best Partner | `best_partner` |
| `vulcan` | 3003 | Armor Enhancement | `armor_enhancement` |
| `vulcan` | 10203 | Range Enhancement | `range_enhancement` |
| `vulcan` | 10603 | Scorching Fire | `scorching_fire` |
| `vulcan` | 11010 | Sticky Oil Bomb | `sticky_oil_bomb` |
| `vulcan` | 180203 | Ignite | `ignite` |
| `war_factory` | 417 | High-Explosive Ammo | `high_explosive_ammo` |
| `war_factory` | 3017 | Armor Enhancement | `armor_enhancement` |
| `war_factory` | 3317 | Missile Interceptor | `missile_interceptor` |
| `war_factory` | 10217 | Range Enhancement | `range_enhancement` |
| `war_factory` | 10317 | Launcher Overload | `launcher_overload` |
| `war_factory` | 12017 | Phoenix Production | `phoenix_production` |
| `war_factory` | 12117 | Steel Ball Production | `steel_ball_production` |
| `war_factory` | 12217 | Sledgehammer Production | `sledgehammer_production` |
| `war_factory` | 180317 | Photon Coating | `photon_coating` |
| `wasp` | 206 | Energy Shield | `energy_shield` |
| `wasp` | 406 | High-Explosive Ammo | `high_explosive_ammo` |
| `wasp` | 506 | Ground Specialization | `ground_specialization` |
| `wasp` | 1606 | Jump Drive | `jump_drive` |
| `wasp` | 1806 | Electromagnetic Shot | `electromagnetic_shot` |
| `wasp` | 3206 | Aerial Specialization | `aerial_specialization` |
| `wasp` | 10206 | Range Enhancement | `range_enhancement` |
| `wasp` | 10606 | Armor-Piercing Bullets | `armor_piercing_bullets` |
| `wasp` | 10806 | Elite Marksman | `elite_marksman` |
| `wasp` | 180206 | Ignite | `ignite` |
| `wraith` | 418 | High-Explosive Ammo | `high_explosive_ammo` |
| `wraith` | 918 | Field Maintenance | `field_maintenance` |
| `wraith` | 3018 | Armor Enhancement | `armor_enhancement` |
| `wraith` | 4418 | Land Cruiser | `land_cruiser` |
| `wraith` | 10218 | Range Enhancement | `range_enhancement` |
| `wraith` | 110181 | Floating Artillery Array | `floating_artillery_array` |
| `wraith` | 180418 | Degeneration Beam | `degeneration_beam` |
<!-- /names -->

## Evidence

### Recorded

- A technology a layout names under its unit is active in the fight, alone and
  beside an officer's correction to the same number:
  `tests/modifier/fights/`.

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
