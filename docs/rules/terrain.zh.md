# 动态地形

[English](terrain.md)

build `1.11.1.3.2259` 如何创建、塑形并结束由 `RangeItemSystem` 管理的战斗内区域效果。

地图装饰、部署占地和单位移动碰撞不是区域效果，不在本文之内。单个来源产生的具体半径、
效果时钟和寿命也不在：那些是按来源取值的，每一个都依赖它自己的采集，下文没有确立其中
任何一个。

## 对象与控制器

权威对象是 `RangeItem`。`RangeItemSystem` 为每种 `RangeItemType` 提供一个
`RangeItemController`，`RangeItemController.GetItems()` 返回当前存在的集合。这个集合
是权威的：一个对象在场，当且仅当它是其中的成员。

原生共有六种类型：`fire`、`oil`、`fog`、`acid`、`recovery_zone` 和 `fog_sand`。其中
四种是可部署的战斗技能：燃烧弹是 `fire`，粘性油弹是 `oil`，烟雾弹是 `fog`，酸液弹是
`acid`。`recovery_zone` 和 `fog_sand` 做什么，没有确立。

## 地形怎么被创建

战斗技能的地面命中通过唯一一个入口进入物件集合：

```text
战斗技能地面命中
  -> RangeItemEffectController.PerformEffect(position, index)
  -> RangeItemSystem.AddItem(...)
  -> 对应 RangeItemController 的物件集合
```

在 build 2259 上 `PerformEffect` 直接调用 `AddItem`，正是这一点把入口钉死，而不只是
让它看起来合理。

单位科技的弹道也会创建 `RangeItem`。它的路径没有在静态调用边上闭合，而是从位置和时间
重建出来的：弹道的移除与地形的创建在 tick 和位置上重合。这是一种对应关系，不是被证明
的调用链，它无法把弹道和同一 tick、同一位置上起作用的另一个原因区分开。

## 地形影响哪些单位

`RangeItemController.Update()` 调用 `UpdateAffectedActorChange()` 刷新受影响集合，该
过程里包含 `FightActor.IsValidTarget` 和一个二维范围检查。每个控制器自己的效果通过
虚分派施加，所以到底跑的是哪个效果，从静态调用图判定不了。

受影响关系可以带一个周期时钟 `{elapsed, duration}`：重复生效的效果需要它，持续减速
这类连续修正不需要。在记录到的 fire 与 acid 控制器中，`elapsed` 每次逻辑推进自增一次，
超过 `duration` 后归零。因此这些计数器用的是录像的 `DurableContext.logic_step`，而不是
它另外那个 `time_units_per_second` 刻度；文件字段由
[mcfr.md](../spec/mcfr/mcfr.md) 定义。

## 圆与网格

普通地形就是一个 `position` 和一个 `radius`，以 Q32.32 原始整数存储，没有网格。

当需要从这个圆里减去一部分空间时，对象进入网格模式，带上原点、尺寸和每行一个位掩码。
原生的 `GridBlockInt.grids` 把 x 编码为列、y 放在高位；录像存储的是它的转置，y 为行、
x 在低位。转置发生在读取边界上，所以消费者永远看不到原生布局。

## 护盾从地形里减去空间

`RangeItemEffectLayerGrid.GenerateGrid` 调用
`AdvancedEnergyShieldSystem.GetActiveEnergyShields`、
`FightEnergyShield.GetFightTransform` 和 `FightEnergyShield.GetRadius`，然后是
`GridBlockInt.TryDisableGrid`、`GenerateMask` 和 `Sync`。也就是说，网格生成读取战场上
实时的护盾，并减去落在护盾内的格子。

战场护盾只参与弹道和区域效果的相交判定。它不是 RVO 代理，不阻挡移动，也不是布局中的
部署占地。

## 身份与生命周期

地形的身份在一份录像内跟随它的原生指针；离开控制器集合的指针会被标记为墓碑，以免之后
有东西继承这个身份。

录像通过相邻两次成员采样之间的差异来确认一次创建或移除。这个方法确立的是事实而不是
原因，所以移除原因是 `unknown`。在单次逻辑推进内创建又移除的实例，对它完全不可见。

地形改变原生类型，被表达为同一位置上的一次移除加一次创建、两个独立身份，而不是一次
转换。

## 读取跨回合地形的两条路径

布局里的跨回合地形可以只从录像文件恢复，也可以从游戏里恢复，两者互相印证而不互相依赖。

文件路径从 GRBR 的 BinaryFormatter 包装里取出 `BattleRecord` XML，读取指定的
`PlayerRoundRecord`，把 `activeState` 和扁平的 `gridInfo/ByteMask` 解码成零基的活跃
索引与规范网格，并保留原始控制点。

游戏路径在战斗开始前从 `RangeItemController.GetItems()` 枚举被恢复的对象，按共享的
provider 分组，从存活的端点恢复控制点，并按 `RangeItem.Index` 导出网格。回放恢复之后
管理器不再持有 provider 的释放数据，所以端点缺失时这条路径会明确失败。文件路径没有这
个要求，而只有游戏路径才能观察到某个具体 build 实际恢复出了什么。

build 2259 的 `CommanderSkillManager.CalculateAttackPositions` 从存储的控制点生成内部
中心点，所以只需要存端点。该方法是私有的，也不在运行时反射表里；要复现它必须使用同样
的原生 `FVector3` 与 `FPoint` 运算，因为定点取整是结果的一部分。

向游戏里恢复时会先算出这些中心点，再按索引顺序添加对象。非空网格必须同时覆写原生的
`Queue<ByteMask>` 和 `GridBlockInt.grids`，并立即回读。`Sync` 不能用来反序列化一个最终
状态：它只会从已经存在的网格里继续减去格子。

## 本文没有确立的东西

- 任何类型对单位的效果，除了"存在一个受影响集合"这一点。
- 任何半径、效果时钟或寿命。它们随来源而变，同一类型的两个来源也可以不同，所以没有
  哪个数值能由类型推出。
- `recovery_zone` 与 `fog_sand` 的行为。
- 是否存在把一个身份内的类型转换产生出来的原生生产者。
- 科技弹道究竟是与之重合的地形的原因，还是只是它的可靠相关物。
