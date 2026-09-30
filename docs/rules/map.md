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

A crystal does not stand where the map data places it but on whole metres:
`CrystalElement.GetPosition` makes its position a `MapVector`, which keeps each
coordinate's integer part, the floor of the map's. Of the crystals that take
part in movement, four stand at a fraction of a metre, and one of them,
Desert's at (95.48, -360.93), stands at (95, -361).

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

The buildings enter the tree side by side, each side's towers and then its
constructions, blue's before red's; then the crystals in the order the map
lists them; then the units. The order matters: the tree's node array grows
only when a split finds it nearly full, the split that grows it loses the
agents it was distributing, and which split that is follows from the order
the agents go in.

## A crystal a formation stands on is not in the fight

A crystal whose circle reaches into the footprint of a unit formation placed on
the board, strictly, is not in the fight: it is in no neighbour set and not in
the RVO tree. A formation's footprint is its row's width and depth, turned with
the formation, around its centre. A crystal whose circle only touches the
footprint's edge stays. Only Desert places crystals of priority 2 or more where
a unit can stand, all of them on blue's half.

Only `map_1V1Desert_default` has crystals of priority 2 or more: 73 of priority
3, all on blue's half of the board. `map_1V1Grass` and `map_1V1MilitaryBase`
place crystals of priority 1 only, so a fight on 1011 is the same fight as on
1021.

## Evidence

### Recorded

- A Desert crystal of priority 3 changes a fight the Training Ground fights
  otherwise, and the simulator reproduces it with every crystal of priority 2
  or more in the tree, the towers before them: `tests/map/fights/`.
- A crystal stands on its map position floored to whole metres: the Scorpion's
  M4 on Desert parts from the game at tick 208 with the crystal at
  (95.48, -360.93) where the map data places it, and plays back with it at
  (95, -361), where an RVO neighbour list of the game measured it:
  `tests/map/fights/scorpion-m4-wasp-1787720817.yaml`.
- A crystal a formation stands on is left out of the fight, and one whose
  circle only touches the formation's footprint stays: blue's Mustangs of
  `tests/map/fights/mustangs-over-crystals.yaml`, and of round 2 of replay
  134265566, stand over the crystals at (-125, -310) and (-115, -310) and
  touch the one at (-135, -320). In that round the game's RVO tree held 299
  agents, two fewer than the simulator's with the two crystals, and no
  neighbour list of the game names them.
- Each side's constructions enter the RVO tree right after its towers, before
  the other side's towers and before the crystals: in round 2 of replay
  134265566, a temporary read of the game's `Simulator` (not committed) found
  blue's two towers, blue's five wall blocks, red's two towers and red's five
  wall blocks, then the 71 crystals, as the first 85 of its 299 agents. With
  the constructions after the crystals, the simulator's tree grew its node
  array on tick 52 and lost the fifteen agents of one leaf from it, red
  Crawlers among them, where the game's did not: `tests/corpus/fights/134265566-r2.yaml`.
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
- A crystal's position is its map position as a `MapVector`, whose
  coordinates are integers: `CrystalElement.GetPosition`, `MapVector..ctor`.
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
- **Which method leaves a crystal under a formation out.** The recordings
  say it goes; no method read here has been matched to it.
- **Why the towers go in before the crystals.** In the map's order the towers
  stand among the crystals; with them first, every fight pinned on 1021
  recorded again on 1001 plays back, and with them after the crystals, five do
  not.
- **Whether a crystal can be damaged.** Desert's priority-3 crystals have 600
  life. No recording here has a unit attack one or splash reach one.
