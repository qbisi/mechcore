# RVO 四叉树实现（build 1.11.1.3.2259）

[English](quadtree.md)

本文说明 `crates/simulation/src/rvo.rs::NativeQuadtree` 的当前实现。它复现 sampled-RVO
的 agent 邻居索引，服务于 [RVO 移动避让](rvo.md)，不是
`crates/simulation/src/kernel.rs` 中用于锁敌的目标四叉树。

这棵树不仅用于加速查询。叶容量、链表插入顺序、分裂时的重排、分支访问顺序以及
Q32.32 比较规则都会影响等距候选和最终 20 邻居，因此属于战斗确定性算法的一部分。

## 1. 数据结构

```text
NativeQuadtree
  inputs : &[AgentInput]       # 本轮固定的 agent 数组
  nodes  : Vec<QuadtreeNode>   # 连续节点池
  next   : Vec<Option<usize>>  # 每个 agent 一项的单向链表
  bounds : QuadtreeRect

QuadtreeNode
  child00  # 第一个子节点下标；等于自身下标表示叶节点
  head     # 叶链表头 agent 下标
  count    # 叶内数量
  max_speed
```

四个子节点总是一次连续追加，所以节点只需保存 `child00`，其余子节点为
`child00 + 1..3`。分支节点不保留 agent；所有 agent 最终都在叶链表中。

当前常量：

| 常量 | 值 | 含义 |
| --- | ---: | --- |
| `QUADTREE_LEAF_SIZE` | 15 | 第 16 个 agent 插入叶时触发分裂 |
| `QUADTREE_MAX_DEPTH` | 11 | 达到该深度后不再分裂 |
| `MAX_NEIGHBOURS` | 20 | 单次查询最终保留的最近邻上限 |
| `DEFAULT_AGENT_TIME_HORIZON` | 12 s | 节点可达范围使用的速度时间窗 |

## 2. 两套位置

每个输入同时携带：

- `tree_position`：`BuildQuadtree` 使用的旧内部位置；
- `position`：BufferSwitch 后的当前位置，用于查询中心和候选真实距离。

这是一项有意的双缓冲语义，不是数据冗余。树的 bounds、象限归属与查询时的距离计算可以
来自不同时间片。RVO 坐标的 `y` 对应世界 `z`，不对应高度。

## 3. Bounds 与中心

构建时从第一个 agent 的 `tree_position` 初始化根 bounds，再依输入顺序使用原生式
`FPoint.Min/Max` 扩张。空输入不生成树，查询返回空集合。

矩形中心按以下运算顺序计算：

```text
center.x = min.x + (max.x - min.x) * 0.5
center.y = min.y + (max.y - min.y) * 0.5
```

不能改写为 `(min + max) / 2`。当 raw 宽度为奇数时，两者的 Q32.32 截断结果不同。
`FPoint.Min/Max` 对容差相等值返回第二个参数，因此 bounds 也依赖输入顺序。

## 4. 象限编号

象限由 `center < tree_position` 的原生容差比较决定。等于中心或只相差 43 raw 时归入低侧。

| quadrant | x 区间 | y 区间 | 编号公式 |
| ---: | --- | --- | --- |
| 0 | low | low | `0 + 0` |
| 1 | low | high | `0 + 1` |
| 2 | high | low | `2 + 0` |
| 3 | high | high | `2 + 1` |

```text
                 y high
          +---------+---------+
          |    1    |    3    |
          +---------+---------+
          |    0    |    2    |
          +---------+---------+  x high
```

## 5. 插入与分裂

agent 严格按输入数组顺序插入。当前 kernel 先加入 `self.buildings` 中启用碰撞的存活建筑，
再加入 `BTreeMap` 中按 Unit ID 排列的存活单位。

在叶节点中：

1. 若 `count < 15`，将新 agent 前插到叶链表头；
2. 若 `count >= 15` 且深度小于 11，连续创建四个子节点；
3. 按旧叶链表的头到尾顺序，把原有 agent 前插到各自子叶；
4. 清空父叶，再继续向下插入当前的新 agent；
5. 深度达到 11 后，即使超过 15 个也保留在同一叶中。

前插会反转顺序，分裂时再次逐项前插又会重排一次。这个链表顺序会成为叶扫描顺序，进而
决定等距候选的稳定先后；不能用 `Vec` 排序或 hash 容器替换。

当所有点重合、根 bounds 宽高均为零时，分裂仍会发生。所有点都会沿 quadrant 0 下沉，
直到最大深度，最终叶可以超过 15 个。

## 6. 节点最大速度

全部插入完成后，自底向上计算 `max_speed`：

- 叶节点取所有成员 `published_calculated_speed` 的最大值；
- 分支节点按 quadrant 0、1、2、3 的顺序取四个子树最大值。

这里聚合的是 agent 上次已发布的求解速度，不是本轮 `desired_speed` 或 `max_speed`。
建筑的聚合速度为 0。

## 7. 查询范围

进入一个节点时，先计算该节点的粗可达距离：

```text
reachable = max((node.max_speed + query.max_speed) * 12,
                query.outer_radius)
            + query.outer_radius
distance  = min(reachable, current_20th_neighbour_distance)
```

尚未收满 20 个邻居时没有第二项限制。该范围故意不逐 candidate 使用其相对速度或半径；
它只负责保守地判断节点是否值得访问。

### 7.1 分支访问

分支不做通用矩形距离计算，而是比较以查询方当前 `position` 为中心、半径为 `distance` 的
轴对齐范围是否跨过节点中心。子节点固定按 0、1、2、3 顺序访问。

每访问完一个子树，如果已经有 20 个邻居，就用当前第 20 名的距离缩小后续分支的
`distance`。因此较早分支找到的候选可以阻止较晚分支被访问。

### 7.2 叶扫描

叶节点按 `head -> next` 扫描，并依次过滤：

```text
candidate.key != query.key
candidate.main_layer == query.main_layer
query.collides_with & candidate.layer != 0
distance_squared(candidate.position, query.position) < leaf_range_squared
```

距离使用双方当前 `position`，不是它们的 `tree_position`。合法候选按距离升序插入结果，
只保留前 20 个。距离比较是严格的原生容差比较；等距项追加在已有等距项之后。

叶扫描开始时只计算一次 `leaf_range_squared`。即使扫描途中已经收满 20 个并更新第 20 名
距离，本叶剩余成员仍使用进入该叶时的范围判断；20 项截断负责保留更近者。缩小后的范围只
影响离开该叶后的其他分支。这一细节不能优化成逐 candidate 缩圈。

## 8. 首次零位置树

新建 sampled agent 的公开位置已是真实坐标，但求解器内部 position 缓冲仍为零。原生调用
顺序是先建树、再 BufferSwitch、再查邻居，所以首次 RVO 边界满足：

```text
所有 agent 的 tree_position = (0, 0)
每个 agent 的 position      = 当前真实位置
```

它与叶容量组合出两种不同情况：

- agent 数不超过 15：根保持叶节点。查询不经过空间分支，扫描全体成员，再用真实位置过滤；
- agent 数超过 15：树按零坐标持续分裂。查询从真实位置做中心跨越判断，可能根本到不了保存
  agent 的 quadrant 0 分支。

已用于区分实现的两个 native 场景是：

- Steel Ball 双方共 8 个单位，加 4 个碰撞建筑，共 12 个 agent；首次根不分裂；
- Rhino 对 Crawlers 共 25 个单位，加 4 个碰撞建筑，共 29 个 agent；首次树分裂。

首次求解结果在下一个 RVO 边界才发布；现有 native 采集中对应 MCFR tick 8。以后每次建树
改用上一个 RVO 边界保存的位置，不再使用零位置。

## 9. 确定性不变量

修改四叉树时必须保持以下行为：

- bounds 只使用 `tree_position`，查询中心和叶距离只使用 `position`；
- 叶容量为 15，第 16 项触发分裂，最大深度为 11；
- 节点池连续追加四个 child；
- 叶成员前插，分裂按旧链表顺序再次前插；
- 分支按 0、1、2、3 访问；
- 等距邻居保留先遍历者；
- 只在 20 项满时用第 20 名距离收窄后续分支；
- Q32.32 center、Min/Max 和比较均保持原生运算顺序。

直接覆盖双缓冲的单元测试为
`quadtree_builds_from_the_previous_buffer_but_queries_current_positions`。完整行为还由
[RVO 验证边界](rvo.zh.md#8-验证边界)中的 native smoke hash 约束。
