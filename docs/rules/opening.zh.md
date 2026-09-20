# 由种子决定的开局初始化

[English](opening.md)

这些规则覆盖 build **1.11.1.3.2259**，标准 1v1，使用随游戏分发的开局池、无游戏规则，
地图为 1001、1011、1021、1031 和 1032。初始化流程决定两边的选项数组和初始防御建筑
布局。玩家的选择以及之后的增援发牌不在此范围内。

## 两条随机流出自同一个种子

`Match.random` 是开局／增援流，用 `BattleInfo.SystemSeed` 作种子。`MapSystem.Init` 用
`Match.random.seed` 另建一个 `GRRandom`，并不从对局流里抽取。因此建筑的抽取不会推进
开局／增援流。

`GRRandom` 委托给 Lua 的 `RanState`：xoshiro256** 数值，用 `[seed, 0xff, 0, 0]` 作种子
并丢弃十六个预热值。区间归约会做掩码，并拒绝落在所请求区间之外的值。一次随机调用可能
消耗多个原始值，所以初始化不能用固定的跳过次数来代替。
`ReinforcePool.ServerRand(bottom, top)` 返回 `math_random(bottom + 1, top) - 1`。

## 开局之前的增援初始化

`ReinforcementSystem.Init` 先调用 `NormalInit`，再调用 `InitUnitReinforce`。
`NormalInit` 先对开局专家应用 `RandomTypeGroup`，再对普通增援军官应用。合格的军官行
要有对应的 scope（开局为 2，普通为 1），且 `limitedScene` 为空或包含标准对战场景 1。
开局专家的 `typeID` 全为 0，在这一步不产生随机调用。

对普通军官来说，正的 `typeID` 定义分组。分组按类型 ID 升序访问，组内成员按军官 ID 排序。
单成员组直接保留、不抽取；多成员组调用一次 `ServerRand(0, member_count)` 保留一名成员。
类型为零的行不参与这次选择。随游戏分发的合格行给出 23 个多成员组，按抽取顺序其大小为：

```text
4, 4, 2, 3, 3, 3, 3, 3, 2, 2, 3, 4, 4, 2, 5, 4, 2, 2, 3, 3, 2, 2, 2
```

随后 `InitUnitReinforce` 按同一场景过滤 `unitReinforceRoundPool`，按 ID 排序，并调用
一次 `ServerRand(0, count)`。合格的池 ID 是 1 到 55。此时随机流停在第 0 回合记录的状态
上，`BattleOpeningController` 随后先给蓝方发牌、再给红方。之后的增援消耗见
[增援发牌](reinforcements.zh.md)。

[config/opening.yaml](../../config/opening.yaml) 保留的是组成员和池 ID，而不只是它们的
数量。所有 ID 和计数都是从 `ConfigDataContainer` 读出的整数，不涉及任何测量或缩放。

## 防御建筑布局

受支持的 `MatchSetting.constructionGroupID` 列表是 `[67, 68, 71]`，顺序如此。
`MapSystem.LoadConstructionLayout` 用地图流通过 `RandomElementSync` 选出一个。被选中的
每一组都允许水平翻转，且 `isOnlyMirrorSymmetry = false`、`isCompact = false`。地图流随后
为每一方各调用一次 `NextBool`，蓝方在先。`GRRandom.NextBool` 是 `Next(1000) < 500`，
包含拒绝重抽，而不是取单个随机位。结果为真会把该方的本地 x 坐标取负。

组数据按创建／索引顺序列出建筑：

| 组 | 单位 ID 与本地位置 |
| --- | --- |
| 67 | 1 在 (140, 105)，然后 2 在 (-140, 60) |
| 68 | 1 在 (140, 105)，然后 3 在 (-140, 60) |
| 71 | 1 在 (140, 105) |

所有条目的 `isRotate = false`。这些是建筑目录里的墙、反装甲炮塔和速射炮塔的 ID。
`MapRegion.ConvertToWorldPoint` 先按主区域的朝向旋转组位置，再加上它的中心。受支持的
地图都解析到 `map_layout_1V1_default`：蓝方主矩形是 `(-300, -310, 600, 300)`、朝向 0，
红方是 `(-300, 10, 600, 300)`、朝向 2。battle 文档把红方转半圈之后，两边的中心都是
`(0, -160)`。因此每一方在文档里的位置是 `(±local_x, local_y - 160)`。这些是整数地图
坐标，没有取整，也没有拟合出来的偏移。

一个座位开局时的反应堆由 `MatchSetting.GetReactorCore` 从地图的 `reactorCores` 读出：
按座位索引取，但条目少于两个时每个座位都取第一个。受支持的地图全部写着 4500，地图
1011 是每个座位各一条、其余是只有一条；之后才由开局的队伍和专家去改动它。同样这些行
没有列出默认的商店单位和指挥官技能，所以一方在开局之前两者都没有。

`scripts/extract_opening.py` 读取配置导出和 `sharedassets0.assets`：MapData 对象
243656、243657 和 243659 把地图名连到 MapLayout 对象 243676。抽取器在产出初始化表之前
会校验受支持标志、朝向和共享布局。

## 发牌数量

`BattleOpeningController.CalculateChooseCount` 返回
`Config.advanceTeamSetting.chooseCount` 与"每个池的大小除以共享它的玩家数"两者中的较
小值。`AdvanceTeamSetting.chooseCount` 在 `level0` 中是 **4**，MonoBehaviour path ID
**146**，位于其序列化体的第 0 字节。紧随其后的 `unitCountPerTeam` 是 **2**。标准池里的
队伍和专家足够多，所以上限仍然是四。

## 多样性比较

`BattleOpeningController.PrepareData` 通过 `Config.GetMatchSetting` 读取所选地图的
`MatchSetting`，并把原生偏移 `0xE0` 处的字段 `advanceSameUnitMaximum` 作为
`ReinforcePool.RandAdvance` 的 `diffUnitLimit` 传入。`ConfigDataContainer.matchSettings`
里标准对战的地图行该字段为 **3**。尽管名字如此，这个参数被用作单位多样性的下界，而不是
同一单位重复小队数的上界。

对候选涉及的每个价格档，比较会把已持有的不同类型数、候选带来的新类型数，以及**本次
选择之后**剩余的选择次数相加。总和低于 `diffUnitLimit` 就拒绝该候选。剩余选择次数是
`num - taken - 1`，`taken` 从零开始。在原生方法里，`not eax` 之后加上 `num` 计算的是
`num + ~taken`；用算术取负会把当前这次选择数两遍。

这些常数都是整数，没有缩放也没有精度转换。

## 证据与边界

字段取值来自随游戏分发的资源。字段访问与比较来自
`BattleOpeningController.CalculateChooseCount`、
`BattleOpeningController.PrepareData(PlayerController, List<AdvanceTeam>,
List<IReinforceItem>, int)` 和 `ReinforcePool.RandAdvance` 的原生指令。Cpp2IL 的 ISIL
把原生的 `not` 标成 `Neg`；以原生指令为准。

制品身份（SHA-256）：

| 制品 | 哈希 |
| --- | --- |
| `GameAssembly.dylib` | `9d2e3f163f74728da73b5dac474f76a0ebdaa3852718fbeb45ad8dec6b4360e5` |
| `global-metadata.dat` | `2488ba4661958da42438e91cb382259077ef7077dfa2261e74ade1c9b1b1fb43` |
| `level0` | `9276c12f99c188854c588e603220472d98623a8ffcc8c03f84516f4adbdce265` |
| `sharedassets0.assets` | `097a8cc560fbe218fb1b5b01cf2b639b42670910286c7ebc5ea352024eda8cb3` |
| ConfigDataContainer JSON 导出，path ID 160 | `92849e4b0cba65bb03448cac0c868ef94fcbbd476eabb8f2bc33d484e820ac4f` |

初始化的调用顺序与区间分支来自 `ReinforcementSystem.Init`、`NormalInit`、
`RandomTypeGroup`（含其排序回调）和 `InitUnitReinforce`。独立的地图流及其抽取顺序来自
`MapSystem.Init`、`LoadConstructionLayout`、`LoadConstruction`、`GRRandom.NextBool` 和
`MapRegion.ConvertToWorldPoint`。资源确立了合格池、组标志、位置和地图几何。

改动过的池、其它模式和负的对局种子未经验证。[增援发牌](reinforcements.zh.md) 描述开局
之后的消耗。种子决定的是被提供的备选项，而不是任何一方选了哪一个。制品身份变化、受支持
地图的初始化输入变化，或原生的随机状态、建筑布局、选项数组与本流程不一致时，重新打开
本文。
