# 1v1 maps

The 1v1 map IDs a single-round layout may select, and what choosing one
changes. A map is a `ConfigDataContainer.matchSettings` row, whose `mapData`
names the `MapData` object of `sharedassets0` that places its buildings.
[`config/maps.yaml`](../../config/maps.yaml) holds what each places.

## Supported map IDs

| MapID | Map | Scene variant | Map data |
| ---: | --- | --- | --- |
| 1001 | 铁道小镇 | day | `map_1V1Desert_default` |
| 1011 | 森林巨眼 | ordinary mode | `map_1V1Grass` |
| 1021 | 训练基地 | ordinary mode | `map_1V1MilitaryBase` |
| 1031 | 铁道小镇 | dusk | `map_1V1Desert_default` |
| 1032 | 铁道小镇 | deep night | `map_1V1Desert_default` |

The build also has 1012, 森林巨眼 in competition mode, and 1022, 训练基地 in
tutorial mode. Both reuse the map resources above and carry special match rules,
which puts them outside what an ordinary single-round layout supports, and the
simulator refuses them by name.

A layout selects one through its optional top-level `map_id`, which
[layout.md](../spec/document/layout.md) defines:

```yaml
map_id: 1001
seed: 2038621361
round: 7
# ...
```

A layout exported from a replay records the original MapID, and `apply_layout`
loads that map before creating the Training Ground. An explicit ID is used as
given. Omitting `map_id` selects 1021, which keeps a result from depending on
whatever default the game itself might change.

## A map is its buildings

Every map places the same four towers where the Training Ground does, and
around them neutral `FightCrystal`s: the `MapData.buildingDatas` entries whose
`teamType` is -1. What a map changes in a fight is its crystals, so two map IDs
that name one map data fight alike: 1001, 1031 and 1032 differ only in the
scene's lighting.

`FightBuildingLoader.Load` creates the buildings in the order the map data
lists them. A crystal whose `enablePathfinding` holds gets an
`RVOControllerFixed` in the `FightCrystal` constructor, whose radius is the
row's `radius` and whose collider priority is its
`pathfindingColliderPriority`; every crystal of the three map datas has one.

## A crystal of priority 2 or more is an obstacle

A crystal whose collider priority is 2 or more is an immovable agent in the
RVO tree for the whole fight, on its own priority's layer as a construction is
on its row's, and a unit avoids it when its layer is one the unit's query
accepts: a unit accepts its own layer and every higher bit, so a priority-3
crystal holds off a unit of priority 3 or less, which in the standard units is
the Fang, the Crawler and the Mustang.

A crystal of priority 1 takes no part in movement. It is in no neighbour set,
which the layers alone would already give, and it is not in the tree either,
where it would move the root's bounds and the leaves' order.

The towers enter the tree first, then the crystals in the order the map lists
them, then the constructions, then the units.

Only `map_1V1Desert_default` has crystals of priority 2 or more: 73 of priority
3, all on blue's half of the board. `map_1V1Grass` and `map_1V1MilitaryBase`
place crystals of priority 1 only, so a fight on 1011 is the same fight as on
1021.

## Evidence

### Recorded

- A Desert crystal of priority 3 changes a fight the Training Ground fights
  otherwise, and the simulator reproduces it with every crystal of priority 2
  or more in the tree, the towers before them: `tests/map/fights/`.
- 1001, 1031 and 1032 fight alike: each fight under `tests/map/fights/`
  recorded on 1031 and 1032 has 1001's hash.

### Read

- A map is a match setting naming its `MapData`: `MatchSetting.mapData`.
- A map's crystals are its building entries with no team:
  `MapData.buildingDatas`, `BuildingData.teamType`.
- A crystal that pathfinds gets an RVO controller with the row's radius and
  priority: `FightCrystal..ctor`, `BuildingData.enablePathfinding`,
  `BuildingData.pathfindingColliderPriority`, `RVOControllerFixed.SetRadius`.
- An immovable controller's layer is the odd bit of its priority:
  `RVOControllerFixed.RefreshCollideInfo`.
- The buildings are created in the map's order, a tower through its own call:
  `FightBuildingLoader.Load`, `FightController.CreateFightTower`,
  `FightController.CreateFightBuilding`.

### Not established

- **Why a crystal of priority 1 is not in the tree.** Its controller is built
  like any other's, and nothing read leaves it out. With the Training Ground's
  nineteen that overlap a deployment region in the tree, 29 of the fights
  pinned on 1021 part from the game; with none they all hold. On every map a
  crystal's priority is above 1 exactly when its life and its strength are,
  so which of the three the build asks is not separated.
- **Where `BattleSystem.RefreshBuildingState` fits.** It hides a building that
  overlaps no deployment region of the first player, and a hidden crystal
  activates no controller. Leaving out every Desert crystal it would hide parts
  the recordings earlier than keeping them all, so it is not what decides which
  crystals a fight has.
- **Why the towers go in before the crystals.** In the map's order the towers
  stand among the crystals; with them first, every fight pinned on 1021
  recorded again on 1001 plays back but one, the Scorpion's M4 with seed
  1787720817, which parts at tick 208 far from any crystal.
- **Whether a crystal can be damaged.** Desert's priority-3 crystals have 600
  life. No recording here has a unit attack one or splash reach one.
