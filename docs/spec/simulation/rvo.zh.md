# RVO 移动避让实现（build 1.11.1.3.2259）

[English](rvo.md)

本文说明 Simulator 当前的定点数 sampled-RVO 实现。其目标是复现 Mechabellum build
`1.11.1.3.2259` 中 `GRPF.RVO.Sampled.Agent`、`RVOAgentFixed` 和
`RVOControllerFixed` 的战斗移动行为，而不是提供一套通用 RVO 库。

实现位于 `crates/simulation/src/rvo.rs`，战斗循环的接入点位于
`crates/simulation/src/kernel.rs::step_rvo`。邻居搜索使用的原生式四叉树单独见
[四叉树实现](quadtree.md)。

## 1. 当前边界

当前求解器接收两类 agent：

- 所有存活单位；
- 所有存活且启用碰撞的建筑。建筑作为锁定、不可移动的地面 agent 参与邻居集合。工事只
  对另一方是这样：它对自己一方沉下去，自己一方的单位从不把它当邻居。

无阵营 `FightCrystal` 是否参与战斗取决于地图，不能一概视为试验场额外对象并删除。
原生录像/试验场通过 `layout.map_id` 选择同一地图并保留地图对象：build 2259 的
1021 地图有 0 个中立水晶 RVO controller，1001 地图有 73 个。当前 Simulator
未实现这套地图对象加载；上述原生对齐不代表 Simulator 已支持地图差异。
RVO 的私有双缓冲、邻居列表和 VO 列表不是 Layout 或 MCFR 的输入；Simulator
从可公开还原的单位、建筑和移动状态重新计算它们。

本模块只处理 agent-agent 避让。目标选择使用 `kernel.rs` 中另一棵目标四叉树，两者的数据
结构、容量和遍历规则不同，不应混用。

## 2. 坐标与数值

RVO 在世界水平面工作：

```text
RVO.x = world.x + 400 m
RVO.y = world.z + 400 m
```

平移不改变距离，只使原生 RVO 坐标保持在正区间。世界高度 `world.y` 不参与避让。

位置、半径、速度、时间和权重全部使用有符号 Q32.32 raw integer。单位配置中的米值先量化
到 1 mm，再进入 Q32.32。不能把中间值转成 `f32`/`f64` 后再还原，因为以下细节都会影响
逐 tick 一致性：

- 乘法遵循原生 raw 运算的截断和 wrapping 路径；
- 除法先按绝对值舍入到最近值，再恢复符号；
- 向量除法先计算一次共享倒数，再分别相乘；
- `FPoint.op_LessThan` 把相差不超过 43 raw 的值视为相等，至少相差 44 raw 才为真；
- `FPoint.Min/Max` 在等价时返回第二个参数，因此参数顺序也是行为的一部分；
- 平方根、三角函数和指数函数使用与目标 build 对齐的 `Fastest` 定点近似。

碰撞分支的 `centerMagnitude < radius` 是直接严格比较，不使用上述容差比较。

## 3. 四 tick 双缓冲流水线

普通战斗逻辑 tick 为约 `0.05 s`，RVO 每 4 tick 求解一次。一个 RVO 边界按以下顺序执行：

1. 发布上一次求解得到的目标点和速度；
2. 根据发布结果更新单位当前速度；
3. 组装建筑和存活单位的 agent 输入；
4. 用 `tree_position` 建四叉树并聚合节点最大已发布速度；
5. agent 切换到当前 `position` 后，查询已经建好的树；
6. 为邻居生成速度障碍并求解；
7. 将新目标点和速度存入 solver 缓冲，等下一个四 tick 边界发布。

因此“求解”和“对移动生效”之间还有一个 RVO 周期。战斗逻辑 tick 内，单位移动先消费当前
已发布目标和速度，随后才可能进入本 tick 的 RVO 边界。

### 3.1 首次建树

原生 sampled agent 构造时会写公开 `Position`，但不会初始化求解器内部的 position 缓冲。
首次 `BuildQuadtree` 又发生在第一次 `BufferSwitch` 之前，所以所有新 agent 的首次
`tree_position` 都是 `(0, 0)`；随后邻居查询已经使用真实 `position`。

Simulator 用 `rvo_first_tree_pending` 显式复现这一点：

```text
首次边界：tree_position = (0, 0)，position = 当前真实位置
以后边界：tree_position = 上次边界位置，position = 当前真实位置
```

这不是随机抖动，也不能通过跳过首次求解替代。它与叶容量共同产生可观察差异：12 个 agent
时根节点不分裂，仍会扫描整片叶；29 个 agent 时零坐标树发生分裂，真实位置查询可能无法
进入包含 agent 的分支。详见 [首次树的查询语义](quadtree.zh.md#8-首次零位置树)。

## 4. Agent 输入

| 字段 | 当前含义 |
| --- | --- |
| `key` | Unit 或 Building namespace 内的稳定身份 |
| `main_layer` | `Ground=1`、`Air=2`；不同主层直接排除 |
| `layer` | 由 collider priority 生成的单 bit |
| `collides_with` | 查询方接受的 candidate layer mask |
| `group` | 避让分组；当前 kernel 使用队伍 ID 填充 |
| `locked` | 不可移动 agent 为真；邻居承担全部避让责任 |
| `tree_position` | 本轮建树使用的旧内部位置 |
| `position` | BufferSwitch 后查询和 VO 使用的当前位置 |
| `current_velocity` | 上次已发布目标和速度导出的当前速度 |
| `desired_velocity` | 本 tick 目标方向和期望速度导出的速度 |
| `desired_target_delta` | 当前位置到原始移动目标点的向量 |
| `desired_speed` | 当前请求速度 |
| `max_speed` | 当前速度上限，参与树的可达范围和超速惩罚 |
| `published_calculated_speed` | 上次已发布求解速度，供四叉树节点聚合 |
| `radius_outer/inner` | RVO 外/内半径，不等同于 MCFR 碰撞半径字段 |
| `size` | 原生有序枚举 `Xs < S < M < L` |
| `priority` | 同组双方分配避让责任的 Q32.32 权重 |

单位的 `outer_radius`、`inner_radius`、`size`、`collider_priority` 和 `priority` 来自
`config/units/*.yaml` 的 `rvo` 字段。配置要求 `inner_radius <= outer_radius`、priority 在
`0..=1`、collider priority 在 `1..=10`。

### 4.1 碰撞 layer

对 priority `p`，可移动 agent 使用偶数位 `2p-2`，并接受自己的 layer 及所有更高位。
不可移动建筑使用奇数位 `2p-1` 且自身 `collides_with=0`。当前碰撞建筑固定使用 priority
10、`size=M`、内外半径均为建筑宽度的一半、`locked=true`。工事一样，只是 priority 用
自己那一行的 `pathfinding_collider_priority`（防御墙是 5）而不是 10，并标记
`passable_by_own_group`。

筛选是有方向的：只有查询方满足
`query.collides_with & candidate.layer != 0`，candidate 才成为邻居。建筑自身不会避让，
但高优先级建筑 layer 可以被单位查询命中。

## 5. 邻居选择

每个 agent 通过四叉树最多保留 20 个邻居。候选按以下顺序过滤：

1. 排除自身 key；
2. 排除不同 `main_layer`；
3. 排除未被查询方 collision mask 接受的 layer；标了 `passable_by_own_group` 的候选
   （工事）对同组的查询方也排除；
4. 要求当前坐标距离平方严格小于查询范围平方；
5. 按距离升序插入并截断到 20 个。

等距候选不会插到已有等距项之前，因此四叉树遍历顺序决定平局。输入组装、叶链表顺序和
分支访问顺序都是确定性状态，不能事后按 ID 重排。

## 6. 从邻居生成速度障碍

令 `center = other.position - self.position`。

### 6.1 不同 group

不同组使用短时间窗：

```text
offset          = (0, 0)
radius          = self.outer + other.inner
inverse_horizon = 100        # 0.01 s
```

算法只比较 group 是否相等。虽然当前 kernel 用队伍 ID 填充 group，RVO 模块本身不把该值
解释为战斗归属。

### 6.2 相同 group

同组先计算查询方承担的避让强度 `s`：

```text
other.locked 时：s = 1
否则：           s = other.priority / (self.priority + other.priority)
权重和不大于 0： s = 0.5
```

随后构造协作速度中心：

```text
other_optimal = Lerp(other.current_velocity,
                     other.desired_velocity,
                     clamp(2s - 1, 0, 1))
offset        = Lerp(self.current_velocity, other_optimal, s)
```

半径选择是非对称的：查询方 size 小于邻居时使用双方 inner radius，否则使用双方 outer
radius。时间窗固定为 12 秒，即 `inverse_horizon = 1/12`。

### 6.3 VO 几何

每个 VO 的基础权重为：

```text
weight_factor = max(1, 1 + 4 * exp(-((|center|^2 / radius^2)^2)))
```

若当前中心距离严格小于半径，构造立即碰撞的分离直线，响应系数为 `0.3`，并使用
`inverse_delta_time = 1 / (4 * logic_delta)`。恰好接触不进入碰撞分支。

未碰撞时，将相对位置和半径投影到速度空间，构造两条切线、截断线和远端圆弧。切点角度
使用定点 `Atan2Fastest`、`AcosFastest`、`SinFastest`、`CosFastest`。对一个待评估速度，
VO 返回使其离开禁区的梯度和穿入深度；有效梯度再乘
`2 * weight_factor`，正权重额外加一 raw `Q32_ONE`。

## 7. 速度求解

求解先在期望速度上施加顺时针对称偏置。所有 VO 中只取最大的正穿入量：

```text
bias = min(0.1, max_penetration / |desired_velocity|)
desired += clockwise_tangent(desired) * bias
target  += clockwise_tangent(target)  * bias
```

期望速度模小于 `0.001` 时不修改向量，但仍保留“是否位于 VO 内”的判断。

- 若偏置后的期望速度不在任何 VO 内，直接保留原始目标点增量和 `desired_speed`；
- 若位于 VO 内，分别从 `current_velocity` 和偏置后的 `desired_velocity` 启动一条 trace，
  选择得分更低的一条；得分按容差相等时选择第二条。

每条 trace 固定 50 次：

```text
step_size = max(outer_radius, 0x33333333 * desired_speed)
remaining = 1 - Q32(step_index) / Q32(50)
step      = remaining^2 * step_size
point    += normalize(gradient) * step
```

第一次评估无条件成为 incumbent；以后只有至少低 44 raw 的得分才替换。梯度评分包含：

- 所有 VO 中权重最大的一个梯度，不对多个 VO 求和；
- 到偏置期望速度的吸引项，权重 `0.1`；
- 超出最大速度的惩罚，权重 `3`；
- 超出期望速度的惩罚，权重由两个分别截断的 `0.1` 相加。

最后把最佳 point 直接作为新的目标点增量，求解速度为
`min(|point|, max_speed)`。这里不再额外乘时间步长；目标点增量和速度是原生
`CalculateVelocity` 的两个独立输出。

## 8. 验证边界

实现使用三层验证：

- `rvo.rs` 单元测试固定 build 2259 的同组 pair 解和 VO 构造 raw 值；
- kernel 测试覆盖建筑碰撞、Q32.32 距离边界、树的粗可达范围和停止移动的边界行为；
- `tests/regression/mcfr-regressions.yaml` 的 native smoke 样本比较稳定物理投影的逐 tick
  `physics_result_hash`，包括
  Steel Ball 对战样本。

常用检查命令：

```text
cargo test -p mechcore-simulation rvo
cargo test -p mechcore-simulation --test battle native_regression_smoke_hashes_match
```

局部 native RVO sidecar 只用于研究和定位，字段与范围见 [Adapter 文档](../adapter/adapter.md)；它不
替代正式 MCFR 的完整战斗 hash，也不构成 Layout 新字段。
