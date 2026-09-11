# 1v1 地图

[English](map.md)

本文记录 Mechabellum build `1.11.1.3.2259` 中，Mechcore 单回合布局支持选择的
1v1 地图 ID，以及地图会改变战斗结果这一已验证事实。

## 支持的地图 ID

| MapID | 地图 | 场景变体 |
| ---: | --- | --- |
| 1001 | 铁道小镇 | 日间 |
| 1011 | 森林巨眼 | 普通模式 |
| 1021 | 训练基地 | 普通模式 |
| 1031 | 铁道小镇 | 黄昏 |
| 1032 | 铁道小镇 | 深夜 |

Layout 顶层可选字段 `map_id` 选择地图：

```yaml
map_id: 1001
seed: 2038621361
round: 7
sides:
  # ...
```

录像导出的 layout 会记录原始 MapID，`apply_layout` 在创建试验场前加载该地图。
显式指定的 ID 保持不变；省略 `map_id` 时固定使用 1021。1021 是当前大部分
Simulator 回归样本所对应的基准地图，也避免结果依赖游戏自身可能变化的缺省选择。

build 2259 还存在 1012（森林巨眼竞赛模式）和 1022（训练基地引导模式）。它们
复用上述地图资源，但包含特殊比赛规则，当前不属于普通单回合布局的支持范围。

## 地图确实影响战斗结果

MapID 不只是场景外观。地图会带入自己的中立 `FightCrystal`，其中激活的
RVO controller 会作为静态 agent 参与 RVO 空间索引和邻居求解，从而改变单位速度、
战斗持续时间和最终 physics/content hash。

同一个 TUFF 第 7 回合布局曾在未选择录像地图时使用到另一套地图环境：试验场出现
891 个中立 `FightCrystal`，其中 73 个带 RVO controller；录像与试验场在 tick 12
开始出现单位速度差异，最终 tick 数为 1294/1235。

改为录像原始地图 1021 后，试验场得到 27 个中立 `FightCrystal`、0 个 RVO
controller。未删除或补造任何水晶，录像与试验场均为 1294 ticks，physics hash 和
content hash 完全一致。

BORK/Caine 第 7 回合使用地图 1001。该地图确实需要保留 891 个中立
`FightCrystal` 和其中 73 个 RVO controller；录像与试验场均为 1610 ticks，两个
结果 hash 完全一致。

因此地图对象既不能按“无阵营”统一删除，也不能把某张地图的对象统一补进其他地图。
正确规则是先按 `map_id` 加载原生地图，再保留该地图自然生成的对象。

本结论仅证明原生录像/试验场的地图选择与战斗结果关系。Simulator 当前不会读取
原生地图资源；`map_id` 存在于 layout 不等于 Simulator 已模拟地图水晶或地图碰撞。
