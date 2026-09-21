# 工事是什么

[English](constructions.md)

本索引锚定游戏 build 2259。它说明布阵里的一条 `constructions` 落到战斗里变成
了什么、带着哪些数、其中哪些是对着游戏量出来的而不只是从数据里读出来的。工事
**做什么**——炮台的技能、防御墙为友军沉入地面、磁力路障的减速——不在这里。

机读表是 [`config/constructions.yaml`](../../config/constructions.yaml)，由
`scripts/extract-constructions.py` 从 `ConfigDataContainer.constructionDatas`
写出。build 里共 **9** 行，布阵能点名的有 **4** 行，天梯对局里出现的是 3 行。

## 一条工事是多个对象

一条 `constructions` 是一个落点和 `count` 个对象。
`FightConstructionSystem.Create` 返回的是 `IReadOnlyList<FightConstruction>`，
`GroupConstructionManager.CreateConstruction` 逐个调
`FightController.CreateFightConstruction` 把它们建出来，录像里每个都是自己的一
行 building。所以表里每个数都是**单个对象的**：

| 工事 | `count` | 每个的生命 | 包围盒 | 伤害 |
| --- | ---: | ---: | ---: | ---: |
| 防御墙 | 5 | 1112 | 8 × 8 | — |
| 反装甲炮 | 1 | 5028 | 24 × 24 | 2748 |
| 速射炮 | 1 | 3650 | 24 × 24 | 82 |
| 磁力路障 | 10 | 300 | 8 × 8 | — |

也就是说一堵墙有 5560 点生命，分在五个要逐个打掉的目标上，既不是 1112，也不是
一个对象顶着 5560。包围盒是该行 `radius` 的两倍，而 `radius` 是表里唯一一个录
像会报回来的长度。

每个对象还带自己的**耐久度**（destroy 几次才彻底消失），以及把同一落点的对象
串在一起的 `combination`。布阵能点名的那四行 `has_durability` 都是 false，带耐
久的是 build 自己的那几行。

## 对象站在哪

对防御墙量出来的，落点 `(-140, -55)`：

```text
-164   -152   -140   -128   -116      一排，z 就是落点自己的 z
```

五块相隔 12 米，中间那块正落在落点上，横跨 48 米——而脚印占的是 60 米。炮台就
是一个对象，落点在哪它就在哪。

**这是一次读数，还推广不了。** 看着该给间距的那两个字段给不出来：
`block_width: 15` 配 `space: 2` 是 `5 × 15 + 4 × 2 = 83` 米，而脚印只有 60。偏
移量是 `GroupConstructionManager.CreateConstruction` 算的，它的反汇编里除了两
次、把行号乘了十——正是部署网格的十米——但索引没有方法体，它派发用的接口槽位也
没法从索引里认出来。

唯一能把规则和"凑上一次读数的数"分开的是磁力路障，它是 `real_row_count` 为 2
的那一行。**这个 build 放不下去**：布阵能编译，释放动作也建得出来、执行得下去，
但之后 `ConstructionManager` 在那个下标上没有元素，游戏是在 adapter 看不见的地
方拒绝的。在它变得可达之前，那个间距是一堵墙的读数，不是一条公式。

## 脚印是什么，不是什么

一行的 `grid_column_count` 和 `grid_row_count` 是十米部署网格的格数，乘十就是
[`layout.md`](../spec/document/layout.md) 校验落点用的脚印：6 × 1 是墙的
60 × 10，2 × 2 是炮台的 20 × 20。凡是布阵目录点名的行，网格和目录钉住的脚印对
不上就拒绝写表，而目录里的脚印是从游戏接受过的落点读出来的，两者互相独立。

脚印**不是**对象。炮台占 20 × 20 而站在 24 × 24 的盒子里；墙占 60 × 10 而它五
块 8 × 8 横跨 48。部署能不能重叠、子弹能不能打中，是两个问题两个盒子。

## 录像说了什么，没说什么

一行 building 带的是 `GameRiver.BuildingType`——`Normal`、`EnergyTower`、
`ResearchCenter`、`Special`——而所有工事都是 `Special`，所以**录像不说一行是哪
条工事来的**。由此有两件事：

- 认名字靠把它匹配回录像自带的布阵，`fight buildings` 干的就是这个，匹配不到唯
  一落点就拒绝，也是因为这个。
- 墙块和炮台是靠生命和盒子分开的，不是靠类型。build 那九行里若有两行这两样都一
  样，录像里就分不开。

地图自己的建筑是例外，是有名字的：每方一座 `EnergyTower` 和一座
`ResearchCenter`，3400 生命，20 × 20 的盒子。

## 边界

以上全部是 build 2259 和 1v1 棋盘，覆盖的是战斗开始时站着什么。它不覆盖工事做
什么、炮台的技能是什么、墙什么时候不再阻挡、打掉一个给多少，也不覆盖价钱——回
收价在 [`economy.yaml`](../../config/economy.yaml) 里，这里没有。

读数是用
[`tests/layouts/construction/shape.mcscript`](../../tests/layouts/construction/shape.mcscript)
取的，它的对照组是一个什么都不放、读回来只有两座塔的一方。
