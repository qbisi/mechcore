# Equipment

An equipment is an item a formation wears, and an equipment ID is also the ID
of the card that grants it: taking card `13030001` adds equipment `13030001`,
and nothing has to be looked up in between. What an item writes onto its unit
is [equipment_effects.md](equipment_effects.md)'s.

## Where the data is

An equipment is a row of `EquipmentGroupData` in `level0`: the plain
`ICommonMechDataChangeDataSource` corrections in `equipmentDatas`, and each
other mechanism in its own list (buff, lifesteal, shield, production line and
the rest). What taking one costs, and the two amounts an equipment can change
rather than a stat (`roundSupply`, `upgradeSupplyChangeValue`), are
[`config/reinforce_items.yaml`](../../config/reinforce_items.yaml), which
`scripts/extract_prices.py` writes; what an ordinary one writes onto its unit
is [`config/equipment_effects.yaml`](../../config/equipment_effects.yaml),
which `scripts/extract-equipment-effects.py` writes. The card system that deals
one is [reinforce_items.md](reinforce_items.md).

## Wearing one

A unit type can wear equipment when its `CardData.canAddEquipment` is set;
War Factory, Abyss and Mountain's rows do not set it. The native
`CanUseEquipment` check enforces that, a free slot, ownership, and the item's
own target restrictions (`mechType`, `unitID`).

**A formation can wear more than one.** `CardElement` holds a list,
`equipments`, and `CardElement.GetEquipmentSlotCount` is one plus the card's
`UnitDataChangeInt.EquipmentSlotCount`. An officer writes that through
`equipmentCountChangeValue`: Equipment Expansion (`10540`) adds one, so a side
holding it fits two to each formation. Nothing else in a standard match sets
it. The slot is there as soon as the officer is added, and two items write
their corrections as an item and an officer do, each in its own channel. A
layout's and a state's `equipment` is a list in fitting order, and the document
compiler gives a formation the slot count above from the side's officers.

The Training Ground executor does not charge a card's acquisition cost: it
creates one inventory object through `MAD_AddEquipment` and lets
`CanUseEquipment` decide.

## Names

<!-- names: equipment -->
| ID | 中文 | English | Document name |
| ---: | --- | --- | --- |
| 1305003 | 光子涂层 | Photon Coating | `photon_coating` |
| 1306001 | 坦克生产线 | Tank Production Line | `tank_production_line` |
| 1306002 | 野马生产线 | Mustang Production Line | `mustang_production_line` |
| 1306003 | 钢球生产线 | Steel Ball Production Line | `steel_ball_production_line` |
| 1307001 | 保护屏障 | Barrier | `barrier` |
| 1308001 | 抗干扰模块 | Anti-Interference Module | `anti_interference_module` |
| 1309001 | 汲取模块 | Absorption Module | `absorption_module` |
| 13010001 | 便携式护盾 | Portable Shield | `portable_shield` |
| 13020001 | 纳米维修包 | Nano Repair Kit | `nano_repair_kit` |
| 13030001 | 激光瞄具 | Laser Sights | `laser_sights` |
| 13030002 | 重型装甲 | Heavy Armor | `heavy_armor` |
| 13030003 | 改良火控系统 | Improved Firepower Control System | `improved_firepower_control_system` |
| 13030004 | 强化模块 | Enhancement Module | `enhancement_module` |
| 13030005 | 速攻模块 | Haste Module | `haste_module` |
| 13030006 | 超重型装甲 | Super Heavy Armor | `super_heavy_armor` |
| 13030007 | 增幅核心 | Amplifying Core | `amplifying_core` |
| 13030009 | 次级增幅核心 | Small Amplifying Core | `small_amplifying_core` |
| 13030010 | 统御核心 | Dominion Core | `dominion_core` |
| 13030521 | 次级激光瞄具 | Secondary Laser Sight | `secondary_laser_sight` |
| 13030522 | 次级火控系统 | Secondary Fire Control System | `secondary_fire_control_system` |
| 13030523 | 次级重型装甲 | Secondary Heavy Armor | `secondary_heavy_armor` |
| 13040001 | 部署模块 | Deployment Module | `deployment_module` |
| 13150101 | 爆裂弹药 | Explosive Ammo | `explosive_ammo` |
<!-- /names -->

## Evidence

### Recorded

- Under Equipment Expansion a Marksman wears two items, and each writes its
  correction in its own channel, as an item and an officer do:
  `tests/equipment/regressions.mcscript`.

### Read

- A unit type wears equipment only if its card allows it:
  `CardData.canAddEquipment`.
- Fitting checks the card, a free slot, ownership and the item's own targets:
  `EquipmentManager.CanUseEquipment`.
- A formation holds a list of items, and its slots are one plus its
  `EquipmentSlotCount`: `CardElement.equipments`,
  `CardElement.GetEquipmentSlotCount`.
- An officer adds slots: `OfficerData.equipmentCountChangeValue`.

### Not established

- **Two items of the same kind on one formation.** Whether they stack, as two
  different items do, is not recorded.
- **An item that is not an ordinary correction** worn beside another: a
  production line, a shield or a buff in a second slot is not recorded.
