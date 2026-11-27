# 状态规范（State）

## 范围

状态（State）描述一场对局在某一回合特定时刻的完整局面，由[决策段规范](action.md)定义的**资源态**与**战斗态**共同构成。在四种核心文档（`match`、`state`、`action`、`layout`）中，`state` 是信息最完备的形态，其余文档均可由它组合或投影得到。

本文档定义：

- 两种载体形态（**段内状态**与**独立状态**）及各自携带的字段边界；
- 两种回合时点（**开局态**与**部署态**）的约束与合法字段；
- 独立状态作为单步部署及跨回合演化 MDP 求解局面的完备性要求；
- 根级共享字段、第 0 回合开局状态 `state(0)` 与常规回合阵营状态结构；
- 实体命名规则、到 [layout.md](layout.md) 的投影关系、规范形式、原生回放重建规则及排除字段。

主要消费入口与姊妹文档分工：

- [`match.md`](match.md)：定义整场对局 YAML 流、`kind: match` 头文件、回合间结算与端到端覆盖率校验；
- [`action.md`](action.md)：定义 14 种决策、单步状态转移边界与净决策折叠规则；
- [`layout.md`](layout.md)：定义战斗引擎可见的棋盘投影 `layout` 及其规范形式。

---

## 两种载体形态与两种回合时点

格式从两个正交维度区分状态文档：**载体形态**决定文档是否依赖外部 `match` 头文件，**回合时点**决定状态处于回合开始前还是部署过程中。两者均统一使用 `kind: state`，不额外引入新的文档 `kind`。

| 维度 | 分类 | 定义与用途 |
| --- | --- | --- |
| 载体形态 | **段内状态** | 嵌在 `match` 文档流中的 `kind: state` 片段。紧随前置 `kind: match` 头文件之后，只记录每回合开局检查点；全局不变量与双方 `tech_loadout` 由头文件声明，段内不重复。 |
| 载体形态 | **独立状态** | 脱离 `match` 流的独立 `kind: state` 文档。在根级与阵营级补齐必要的公共上下文，可单独承载任意时刻的局面，统一服务于求解器/Bot 对弈接口及逐步转移验证。 |
| 回合时点 | **开局态** | 某一回合尚未执行任何 `action` 前的状态（含 `round: 0` 选队开局与 `round >= 1` 回合开局）。面板无 `release`、编队无 `travelling` 或 `recovered`、本回合 `energy_tower_skills` 为空、`shop` 限额满额；段内只允许此形态。 |
| 回合时点 | **部署态** | 应用了部分或全部本回合决策后的状态（含**部署中间态**与**部署终态**）。在开局态基础上可携带 `reinforce_choice`、剩余 `shop` 限额、`energy_tower_skills`、`battle_skills[].release`、`units[].travelling` / `recovered` / `recordPosition` / `recordRotated` 等部署期字段。 |

### 段内状态 vs 独立状态字段边界

| 所属层级 | 字段 | 段内状态 | 独立状态 |
| --- | --- | --- | --- |
| 根级共享 | `kind`, `round` | 必填 | 必填 |
| 根级公共头 | `build`, `reinforce`, `max_round`, `map_id`, `seed` | **省略**（由前置 `match` 头文件声明；段内出现即拒绝） | **允许/推荐**（不带 `match` 头文件时用于补全规则与目录上下文） |
| 根级演化态 | `random_state`, `reinforce_pool`, `reinforce_offers` | `round >= 1` 出现（`offers` 仅发牌回合存在） | 同段内（`round >= 1`） |
| 阵营公共配置 | `blue/red.tech_loadout` | **省略**（由前置 `match` 头文件声明） | **允许/推荐**（用于校验 `upgrade_technology` 合法动作集） |
| 阵营第 0 回合 | `reactor_core`, `seed`, `offers`, `constructions` | `round: 0` 出现（`seed` 可选） | 同段内（`round: 0`） |
| 阵营资源与战斗态 | `random_state`, `reactor_core`, `supply`, `unlocked_units`, `next_index`, `units`, ... | `round >= 1` 出现 | 同段内（`round >= 1`） |
| 阵营部署期状态 | `reinforce_choice` | **省略**（存在即拒绝；读入有牌面回合时自动恢复 `-1`） | 开局 `-1` 或已选下标；无牌面回合省略 |
| 阵营部署期状态 | `shop`（`buys_remaining`, `unlocks_remaining`, `contraptions_remaining`） | **省略**（由 `officers` 确定性初始化；段内出现即拒绝） | 开局可省略；**部署中间态建议显式写出**以保持 Markov 完备 |
| 阵营部署期状态 | `energy_tower_skills`, `battle_skills[].release`, `units[].travelling/recovered/record*` | **禁止出现**（开局前必为空） | 部署中间态 / 部署终态按实际决策结果写入 |

### 独立状态的 MDP 完备性

`match` 头文件中只有 `build`、`reinforce`、`max_round`、`map_id`、`seed` 以及双方 `tech_loadout` 属于规则与动作合法性前提；`source`、`name`、`elo_score` 仅用于对局溯源与展示，不参与任何规则判定。将这组公共规则上下文直接并入独立状态，并使第 0 回合起始 `reactor_core` 与玩家 `seed` 原生归位于 `state(0)` 后，**独立状态（无论 `round: 0` 还是 `round >= 1`）无需外挂 `match` 头文件或历史决策即可自足闭环**：

1. **单步部署 MDP 完备**：结合 `reinforce`、`reinforce_offers`、`reinforce_choice`、`tech_loadout`、`shop`、`supply`、`next_index` 与棋盘状态，任意独立状态加上一条 `action` 即可判定合法性并算出下一独立状态；
2. **战斗模拟完备**：`map_id`、`seed` 与棋盘投影 `project(state) -> layout` 直接构成战斗模拟输入；
3. **跨回合演化完备**：根级 `random_state`、`reinforce_pool` 与 `seed` 支持推演下回合增援发牌，阵营 `random_state`（或 `round: 0` 玩家 `seed`）支持军官随机交付，`max_round` 确定终局视野。

### 完整 YAML 示例

#### 1. 第 0 回合开局状态（`round: 0`，段内形态）

```yaml
kind: state
round: 0
blue:
  reactor_core: 4500
  seed: 134917337
  offers:
  - {team: void_eye-sledgehammer, specialist: cost_control_specialist, core_modify: 200}
  - {team: vortex-fire_badger, specialist: giant_specialist, core_modify: -200}
  - {team: marksman-sledgehammer, specialist: quick_supply_specialist, core_modify: 100}
  - {team: crawler-steel_ball, specialist: aerial_specialist, core_modify: -200}
  constructions:
  - {name: defensive_wall, index: 0, position: {x: 140, y: -55}}
  - {name: rapid_fire_turret, index: 1, position: {x: -140, y: -100}}
red:
  reactor_core: 4500
  seed: 134917338
  offers:
  - {team: fang-steel_ball, specialist: fortified_specialist, core_modify: 1000}
  - {team: crawler-sabertooth, specialist: supply_specialist, core_modify: -1000}
  - {team: crawler-fire_badger, specialist: training_specialist, core_modify: 200}
  - {team: arclight-steel_ball, specialist: sabertooth_specialist, core_modify: 300}
  constructions:
  - {name: defensive_wall, index: 0, position: {x: 140, y: -55}}
  - {name: rapid_fire_turret, index: 1, position: {x: -140, y: -100}}
```

> 若作为独立 `round: 0` 状态，则在根级补齐 `build`、`reinforce`、`max_round`、`map_id`、`seed`，并在 `blue`/`red` 下补齐 `tech_loadout`。

#### 2. 常规回合段内开局状态（`round >= 1`，省略公共头与部署期字段）

```yaml
kind: state
round: 7
random_state: [12917254667871916365, 9250091961005733138, 14288931841979358149, 16091592835658840205]
reinforce_pool:
  removed: [mass_produced_fortress]
  added: [extended_range_fortress]
reinforce_offers:
- {type: unit, name: phoenix_2x_lv2, unit: phoenix, level: 2, squads: 2, cost: 200}
- {type: unit, name: sabertooth_2x_lv2, unit: sabertooth, level: 2, squads: 2, cost: 200}
- {type: unit, name: steel_ball_2x_lv2, unit: steel_ball, level: 2, squads: 2, cost: 200}
- {type: unit, name: sledgehammer_2x_lv2, unit: sledgehammer, level: 2, squads: 2, cost: 200}
- {type: supply, name: refund_400, cost: -400}
blue:
  random_state: [11398329854704076253, 2588188838420805615, 8713068559080739648, 8599063887646576635]
  reactor_core: 3197
  supply: 850
  unlocked_units: [fang, marksman, crawler, void_eye]
  blueprints: [sticky_oil_bomb, field_recovery, attack_enhancement_ii]
  tower_strengthen_levels: [1, 1]
  equipments: [improved_firepower_control_system]
  battle_skills:
  - {index: 0, name: intensive_training, cooldown: 0}
  - {index: 1, name: lightning_storm, cooldown: 1}
  next_index: {unit: 7, contraption: 1}
  officers: [cost_control_specialist, efficient_light_manufacturing]
  techs:
    fang: [grenade_launcher]
  units:
  - {name: crawler, index: 0, value: 100, exp: 450/450, position: {x: -140, y: -135}}
  - {name: marksman, index: 2, value: 0, position: {x: 0, y: -160}, source: delivered}
  constructions:
  - {name: defensive_wall, index: 0, position: {x: 140, y: -55}}
red: {...}
```

#### 3. 常规回合独立部署态（含公共头、`tech_loadout`、`shop` 与部署期动态字段）

```yaml
kind: state
build: <GAME_VERSION>
reinforce: true
max_round: 13
map_id: 1012
seed: 2038621361
round: 7
random_state: [12917254667871916365, 9250091961005733138, 14288931841979358149, 16091592835658840205]
reinforce_offers:
- {type: unit, name: phoenix_2x_lv2, unit: phoenix, level: 2, squads: 2, cost: 200}
- {type: unit, name: sabertooth_2x_lv2, unit: sabertooth, level: 2, squads: 2, cost: 200}
- {type: unit, name: steel_ball_2x_lv2, unit: steel_ball, level: 2, squads: 2, cost: 200}
- {type: unit, name: sledgehammer_2x_lv2, unit: sledgehammer, level: 2, squads: 2, cost: 200}
- {type: supply, name: refund_400, cost: -400}
blue:
  tech_loadout: full
  shop: {buys_remaining: 1, unlocks_remaining: 1, contraptions_remaining: 7}
  random_state: [11398329854704076253, 2588188838420805615, 8713068559080739648, 8599063887646576635]
  reactor_core: 3197
  supply: 1050
  reinforce_choice: 4
  unlocked_units: [fang, marksman, crawler, void_eye]
  energy_tower_skills: [mass_recruitment]
  tower_strengthen_levels: [1, 1]
  battle_skills:
  - index: 0
    name: field_recovery
    cooldown: 0
    release:
      target: {unit: 2}
  - index: 1
    name: shield_airdrop
    cooldown: 1
    standing:
    - {position: {x: -285, y: -22}}
  - index: 2
    name: lightning_storm
    cooldown: 0
    release:
      order: 0
      target: {area: [{x: 50, y: 44}, {x: 189, y: 38}]}
  next_index: {unit: 8, contraption: 2}
  officers: [cost_control_specialist]
  units:
  - {name: crawler, index: 0, value: 100, position: {x: -220, y: -60}, travelling: true, recordPosition: {x: -140, y: -135}}
  - {name: marksman, index: 2, value: 0, source: delivered, recovered: true}
  - {name: void_eye, index: 7, value: 100, source: joined, position: {x: 20, y: -160}, equipments: [improved_firepower_control_system]}
  contraptions:
  - {name: shield, index: 1, position: {x: -130, y: -153}}
red: {...}
```

---

## 根级共享字段

`round: 0` 的状态段仅包含 `kind`、`round`（为 `0`）、`blue`、`red`（独立状态还可在根级携带下方公共头）。`round >= 1` 时根级还承载双方共享的增援随机流与牌面。

### 1. 文档头与独立状态公共头

- `kind`：固定为 `state`。
- `round`：非负整数，`0` 表示开局选队态，`>= 1` 表示对应常规部署回合。
- **独立状态公共头（段内状态省略）**：
  - `build`：可选字符串（非空），标识数据表版本；省略时默认取当前程序绑定的 `GAME_VERSION`。
  - `reinforce`：可选布尔值，默认 `true`，意义同 [match.md](match.md)。
  - `max_round`：可选正整数（`>= 1`），决定终局回合上限；省略时按 `map_id` 对应地图预设取值。
  - `map_id` 与 `seed`：可选整数，语义与可选性与 [layout.md](layout.md#seed-and-map) 完全一致。两者置于根级而非阵营下，因为整场战斗与初始增援池由双方共享。

### 2. 共享随机流与普通增援池

游戏内实际存在 4 条伪随机流：

| 随机流 | 种子来源 | 是否随状态记录 | 用途 |
| --- | --- | --- | --- |
| `Match.random` | `BattleInfo.SystemSeed`（根级 `seed`） | 是（根级 `random_state`） | 回合间抽取普通与单位增援牌面 |
| `Player.random` | 玩家 `seed`（`state(0)` 各阵营 `seed`） | 是（阵营级 `random_state`） | 军官按周期随机抽发装备 |
| `Match.roundRandom` | `SystemSeed + round` | 否 | 战斗阶段确定性逻辑；由 `seed` 与 `round` 直接导出 |
| `FightTeam.random` | `(round + teamIndex) * 4444` | 否 | 战斗阶段单队逻辑；由 `round` 与索引直接导出 |

每个 `round >= 1` 状态的根级与双边阵营下各带一个长度固定为 4 的无符号 64 位整数数组 `random_state`（对应 Lua 5.4 xoshiro256 状态字），**且不允许全为 `0`**（全零是该生成器的吸收不动点）。在回合开局态下，`random_state` 记录本回合完成增援发牌与军官交付**之后**、双方开始决策**之前**的断点；只有触发抽牌或发物的规则才会推进相应流状态。

`reinforce_pool` 记录当前普通增援池相对于“由根级 `seed` 初始化的原始池”之间的**集合净差值**：

```yaml
reinforce_pool:
  removed: [mass_produced_fortress]
  added: [extended_range_fortress]
```

- `removed`：仅列初始池中已被移除的卡牌；`added`：仅列非初始池中新增的卡牌。两者严格互斥、去重且按卡牌 ID 升序排列。
- 为空的列表省略；若 `removed` 与 `added` 均空，整个 `reinforce_pool` 键省略。
- 这是集合差值而非操作历史日志；当头文件或公共头 `reinforce: false` 时，`reinforce_pool` 必须保持空且省略，根级 `random_state` 也停留在开局初始位置。
- 回合开局状态已包含本回合的条件池刷新；本回合选用不可重复卡牌后的移池发生在下一轮发牌前；4 级战场技能在相邻下一回合的临时互斥直接由本回合 `reinforce_offers` 推导，不单独记入 `reinforce_pool`。

> **安装器约束**：向真机游戏写入缺少随机流的局部局面时，应跳过 `randomStateData` 写入以保持游戏默认初始化，绝不可写入 `[0, 0, 0, 0]`，否则会导致解析长度/全零异常及后续所有随机抽取锁死在零值。

### 3. 本回合增援牌面 `reinforce_offers`

`reinforce_offers` 在根级列出本回合发放给双方的同一组增援卡牌（按发牌顺序排列），末尾固定附带一项 `supply` 拒绝项，供双方使用同一 0-based 下标分别作出选择。

每项均为显式标注 `type`、`name` 与 `cost` 的结构化映射（`cost: 0` 显式写出；负 `cost` 表示拒绝后获得补给）：

- `{type: officer, name: <card_name>, cost: <amount>}`
- `{type: equipment, name: <card_name>, cost: <amount>}`
- `{type: commander_skill, name: <card_name>, cost: <amount>}`
- `{type: unit, name: <unit>_<squads>x_lv<level>, unit: <unit>, level: <level>, squads: <squads>, cost: <amount>}`
- `{type: supply, name: refund_<amount>, cost: -<amount>}`

规范约束：

- `round: 0`、`round: 1` 或 `reinforce: false` 时本回合不发增援牌，**整个字段省略**（不写空数组 `[]`）。
- 每张卡牌均须通过当前构建目录对 `type`、`cost` 及单位展开字段的校验；末尾 `supply` 条目必须且只能出现一次，其 `cost` 须等于 `-<amount>`。直接写出 `cost` 与解码后的单位参数，使下游消费方无需重放发牌或反查目录即可计价与评估。

---

## 阵营状态

`blue` 与 `red` 各自持有一份独立的阵营状态。`round: 0` 仅有开局选队与地图初始建筑；从 `round: 1` 起遵循 [`action.md`](action.md) 定义的**资源态**与**战斗态**两分法。

### 1. 第 0 回合开局状态（`round: 0`）

`state(0)` 尚无棋盘编队、商店或技能面板，每个阵营包含以下字段：

| 字段 | 要求 | 说明 |
| --- | --- | --- |
| `reactor_core` | 必填，正整数 | 第 0 回合选队前的初始核心血量（标准 1v1 地图默认 4500）。选队后按所选 offer 的 `core_modify` 加减并写入 `round: 1` 的 `reactor_core`。 |
| `seed` | 可选，`i32` | 玩家个人随机流 `PlayerRecord.seed`。用于第 0 → 第 1 回合初始化玩家 `random_state`（支持无损导出回原生 GRBR）；省略时无法预测会随机抽取物品的军官交付。 |
| `offers` | 必填列表（4 项） | 本局开局发给该阵营的 4 组先遣队与专家组合 `{team, specialist, core_modify}`，详见 [match.md](match.md#the-opening-is-round-zero)。 |
| `constructions` | 必填列表 | 地图在第 0 回合发牌前就位的基础建筑（如 `defensive_wall`、`rapid_fire_turret`）。 |
| `tech_loadout` | 独立状态允许/推荐 | 与 `match` 头文件中的定义相同，声明各单位允许研发的科技集合。 |

### 2. 资源态（`round >= 1`）

资源态描述决定性经济、随机断点与科技/能力集合：

- **`random_state`**：阵营个人随机流 4 个 `u64` 状态字（非全零），记录本回合军官交付后的位置。
- **`reactor_core`**：当前反应堆核心血量。开局后仅在 `round: 0` 受选队 `core_modify` 增减，此后每个常规回合只在战斗结算扣减，不能由回合内决策改动。
- **`supply`**：该时刻阵营**实际可花费的补给余额**——已经加上本回合收入、扣减/计入了所有已执行购买、升级、回收退款与能量塔技能费用。所有决策合法性校验均直接检查该值。
  - **与原生回放的差异**：原生回放各回合快照中的补给字段记录的是“本回合收入尚未发放前”的结余，转换器按 [match.md](match.md) 重建为本字段。将状态注入真机游戏时，因为 `supply` 已含本回合收入，安装器须同步置位游戏内部的一次性 `IsSpecialSupply` 锁存标志，防止游戏在进入回合时重复发放当回合收入。
- **`reinforce_choice`**：
  - 在没有 `reinforce_offers` 的回合**必须省略**；
  - 在有 `reinforce_offers` 的回合，**段内开局状态省略且禁止显式写出**（读取器打开回合开局时会自动恢复为 `-1`）；**独立状态 / 部署态**下未作出选择时写 `-1`（或省略），选择后写所选条目在 `reinforce_offers` 中的 0-based 下标（选择末尾退款项则等于已发卡牌数）。
- **`tech_loadout`（仅独立状态）**：声明本阵营可研科技白名单（`"full"`、42 字符 Base64url 正向位图或按单位分组的展开映射）；在段内状态中由 `match` 头文件统一承载。
- **`unlocked_units`**：商店中已解锁可购买的单位类型列表，使用蛇形英文名，按单位 ID 升序排列。互补的“未解锁列表”及无标准变动的 `MaxUnlockCount` 均不写入。
- **`shop`（仅独立部署态可选）**：包含 `buys_remaining`、`unlocks_remaining`、`contraptions_remaining` 三个非负整数字段，记录本回合当前剩余限额。
  - 回合开启时，所有限额完全由 `officers` 确定性决定：**2 次购买 + 每名 Additional Deployment Slot（`10004`）加 1 次、1 次解锁、8 次装置释放**（见 [rules/contraptions.md](../../rules/contraptions.md)）。因此**段内开局状态不保存 `shop`**（写即拒绝）；独立状态处于部署过程中时写出 `shop`，使下游可在不重放前置决策的情况下继续校验下一动作的次数限制。
- **`next_index`**：单调递增索引分配器 `{unit: <i32>, contraption: <i32>}`，无论列表是否为空始终显式写出。
  - 每购买、发牌或军官交付一队单位即推进 `next_index.unit`，每放置一件装置即推进 `next_index.contraption`。已阵亡、消耗或回收的索引永不复用。
  - 分配器不仅给出下一个可用编号，也承载了对象列表无法体现的**状态差异**（例如在历史回放比较或状态 diff 时，识别曾经购买或消耗过哪些槽位）。
  - 不记录 `constructionIndex`：标准 1v1 赛制中开局建筑在第 1 回合就位后从不新增，只会随回收/摧毁减少，每个建筑各自携带稳定的 `index` 即可。
- **`officers`**：阵营当前持有的军官名称列表（按 ID 升序排列的多重集，重复获取出现多次）。注意由二阶连锁蓝图（`4`/`401`、`5`/`501`）赋予的产物军官（`20310`/`20311`/`20300`/`20301`）**不得重复写入 `officers`**，其所有权由 `blueprints` 唯一声明。
- **`techs`**：已激活的单位科技，按单位类型分组（与 `tech_loadout` 结构一致）；尚未研究任何科技的单位不列键，空映射省略。标准 1v1 下不存在“已解锁但未启用”的中间科技态。
- **`blueprints`**：研发中心已激活蓝图列表（空列表省略），按 ID 升序排列。
  - 标准 1v1 蓝图目录仅含 `[1, 2, 3, 4, 5]`（以及链式升级后的二阶 `401`、`501`）：`1`/`2`/`3` 发放战场技能（`sticky_oil_bomb`、`field_recovery`、`mobile_beacon`），`4->401` 与 `5->501` 为增强连锁。连锁二阶激活后直接**替换**一阶条目，同一条链不可同时出现两个等级。
  - **为何保留该字段而不从 `battle_skills` 与 `officers` 反推**：战场技能面板同时汇集开局专家、增援卡牌与研发中心蓝图三路来源且共用技能 ID 空间，蓝图与卡牌的互斥仅靠游戏数据规则人工维持，单独看状态无从可靠拆解来源；而增强连锁由 `blueprints` 唯一持有则避免了与 `officers` 的双重真值冲突。
  - 不包含 `research_queue`：仅在启用特殊游戏规则「战场技能研发」（`999917`，见 [match.md](match.md#game-rules-are-the-premise-so-the-match-carries-them)）时才会启用蓝图异步研发队列；标准 1v1 赛制中所有蓝图均为即时生效。
- **`energy_tower_skills`**：**本回合内已激活**的能量塔技能列表（空列表省略），按 ID 升序排列。
  - 能量塔技能均仅限当回合有效，因此所有回合开局态均为空并省略，随后在本回合依次执行 `active_energy_tower_skill` 时填入。
  - 状态完整保留标准 1v1 全部五项技能（不同于 `layout` 只保留战斗可见的 `5`、`6`）：其中 `rapid_resupply`（`1`）虽属纯经济技能，却自带“当回合提前支取、下一回合开启时从回合收入抵扣”的跨回合债务，状态若丢弃它就无法正确结算下一回合的开局 `supply`。原生快照会在下一回合延迟一行记录该标志，只能用于辅助反推上一回合补给，绝不可直接抄入新回合的 `energy_tower_skills`。
- **`tower_strengthen_levels`**：固定长度为 2 的数组，记录两座基础设施的强化等级（各取值 `0..=4`）。
  - **统一规范下标**：对红蓝双方一律规定 **`0` 为能量塔（`energy_tower`），`1` 为研发中心（`research_center`）**。
  - 游戏原生 `BuildingManager.buildings` 对红蓝双方的内部排序相反（蓝方 `[EnergyTower, ResearchCenter]`，红方 `[ResearchCenter, EnergyTower]`）；文档格式在转换、导出、编译与采集边界自动完成红方的 `0 <-> 1` 映射（依据 [`config/maps.yaml`](../../../config/maps.yaml)）。
- **`equipments`**：本阵营当前持有、尚未被任何存活编队穿戴的装备库存（按装备 ID 升序排列的多重集；空列表省略）。
  - 状态采用**未穿戴库存 + 各编队穿戴列表**的差分化存储，两者互斥且共同完备所有装备资产（不同于原生 `equipmentDatas` 把已穿戴和未穿戴全部写在一起）。
  - 回收带装备编队时，其佩戴装备必须同时全部退回本字段；空列表省略。

### 3. 战斗态（`round >= 1`）

战斗态描述放置在棋盘上或等待战斗释放的实体与面板：

#### 编队列表 `units`

```yaml
units:
- {name: crawler, index: 0, value: 100, exp: 450/450, position: {x: -140, y: -135}}
- {name: marksman, index: 2, value: 0, position: {x: 0, y: -160}, source: delivered}
- {name: fang, index: 3, value: 50, level: 2, source: joined, recovered: true}
```

每条编队记录以下属性：

- **`name`** / **`index`**：单位蛇形英文类型名与分配器授予的终身索引。
- **`value`**：购买该编队时按当时军官价格折扣实际支付的补给，也是使用 `field_recovery` 回收该编队时返还的补给额；同型号同等级的两支编队 `value` 可以不同。
- **`level`** / **`exp`**：等级与经验条（写成 `current/maximum` 形式，当前经验为 `0` 时省略 `exp` 字段）。存活编队在反序列化后若省略 `exp`，默认视作当前等级的 `0/maximum`。
- **`position`** / **`rotated`**：编队在本方局部坐标系中的位置及朝向（`rotated: false` 时省略）。
- **`equipments`**：该编队当前穿戴的装备列表（空列表省略），**严格保持装配先后顺序**（槽位 0 在前，不按 ID 重排）。
- **`source`**：取值 `legacy`（默认值，省略）、`joined`、`delivered`，描述本回合该编队是如何入场的：
  - 第 1 回合的开局先遣队在回合开局为 `joined`；
  - 军官在常规回合开启时交付的小队在当回合开局为 `delivered`；
  - 幸存至下一回合开局的所有编队（含上一回合的 `joined` / `delivered` / 新购编队）统一转为 `legacy`；
  - 部署阶段通过增援卡或商店购买加入的编队在部署态中标记为 `joined`。
- **部署移动权限不落盘**：`movable` 不是文档字段，而是由 `source` 与其他状态推导的派生性质——当回合 `joined`/`delivered` 的编队可移动；`legacy` 编队仅当装备 Deployment Module、所属单位已研究 Jump Drive、或本回合被 `redeployment` 技能指定为目标时才可移动（规则见 [rules/mobility.md](../../rules/mobility.md)）。CLI 交互部署视图（见 [cli.md#match-act](../mechcore/cli.md#match-act)）额外展示的 `movable` 与 `unpinned` 属于临时界面字段，不写入 `state`。
- **部署期专属字段**（仅部署态允许出现，段内开局态禁止）：
  - `travelling`：`true` 表示跨区域（进入侧翼）待传送标记，回合战斗结算后清空；
  - `recovered`：`true` 表示本回合已被 `field_recovery` 回收的墓碑。墓碑**仅保留** `name`、`index`、`value`、`level`、`source` 与 `recovered: true`，**必须清空且禁止携带** `position`、`exp`、`rotated`、`equipments`、`travelling`、`recordPosition`、`recordRotated`。该墓碑在下一回合开局时被清理，而它占用的编号在 `next_index.unit` 中保持已消耗。
  - `recordPosition` / `recordRotated`：从部署终态向 `layout` 投影时，用于记录在部署前后位置或朝向发生变化的遗留编队其开局原始位姿。

#### 建筑 `constructions` 与装置 `contraptions`

格式严格区分这两类实体，空列表均省略：

- **`constructions`（建筑）**：地图静态防御物（如城墙 `defensive_wall`、炮台 `rapid_fire_turret` 等）。开局由地图在 `state(0)` 发牌生成，局内仅可通过 `release_commander_skill` 新增（标准 1v1 赛制无直接从商店购买建筑的动作）。建筑被 `field_recovery` 回收或被摧毁后**直接从列表移除**，不留墓碑。
- **`contraptions`（装置）**：玩家通过 `release_contraption` 消耗商店限额（每回合 8 次）并扣除补给购买的一次性棋盘设备（如护盾、导弹、拦截器），每件携带由 `next_index.contraption` 分配的 `index` 与单点坐标 `position`。

#### 战场技能面板 `battle_skills`

```yaml
battle_skills:
- index: 0
  name: intensive_training
  cooldown: 1
- index: 1
  name: shield_airdrop
  cooldown: 2
  standing:
  - {position: {x: -285, y: -22}}
- index: 2
  name: lightning_storm
  cooldown: 0
  release:
    order: 0
    target: {area: [{x: 50, y: 44}, {x: 189, y: 38}]}
- index: 3
  name: field_recovery
  cooldown: 0
  release:
    target: {unit: 12}
```

- **`index`**：技能加入面板时分配的终身槽位号，`release_commander_skill` 通过该索引寻址。面板只追加不缩减、不重排，支持同一技能 ID 在面板中拥有多个独立槽位。
- **`name`** / **`cooldown`**：技能蛇形名称与当前剩余冷却回合数。
  - 新技能加入面板时取该技能表的 `initial_cooldown`（[rules/commander_skills.md](../../rules/commander_skills.md) 说明部分强力技能如核弹、雷暴、离子轰炸的 `initial_cooldown` 为 `1`）；
  - 每个新回合开启时，上一回合曾释放（含部署期释放）的槽位重置为该技能的 `cooldown`，其余未释放槽位递减 `1`（下限 `0`）。
- **`standing`**：先前回合遗留在场上的技能造物列表（无滞留物时省略）。标准 1v1 中仅 `shield_airdrop` 与 `sticky_oil_bomb` 允许携带 `standing`，其余技能若带该字段按名字拒绝；同一槽位在先后回合可留下多个滞留物。
- **`release`（仅部署态）**：记录本回合对该槽位执行 `release_commander_skill` 后的释放信息；开局态下必须省略。
  - 战斗释放（投放战场技能）：包含 0-based 释放顺序号 `order` 与 `target`；
  - 部署期释放（`field_recovery`、`redeployment`、`intensive_training`）：已即时改写状态，只保留 `target` 而省略 `order`。
  - **`target` 三态互斥**：采用 `{area: [...]}`、`{unit: <index>}`、`{construction: <index>}` 三种纯 YAML 单键映射之一（见 [action.md](action.md)）。指向单位/建筑时状态只保存已解析的对象 `index`，不保留原生回放里的鼠标点击坐标 `Positions`；`area` 点列长度须匹配 [rules/battle_skill.md](../../rules/battle_skill.md) 定义。

---

## 命名约定

文档统一使用游戏实体对应的蛇形英文名替代数字 ID：

- 单位类型名与装置名沿用 [layout.md](layout.md) 约定；
- 军官、科技、蓝图、能量塔技能、战场技能与装备名均来自 [`config/names.yaml`](../../../config/names.yaml)（对应游戏英文名去撇号转蛇形，例如 `supply_specialist`、`grenade_launcher`、`field_recovery`、`rapid_resupply`、`intensive_training`、`photon_coating`）；
- 同名冲突由规则显式消歧：增援卡授予的移动信标写为 `mobile_beacon_card`，研发中心蓝图授予的写为 `mobile_beacon`；
- **单位增援卡按回合上下文消歧**：卡面名称按 `<unit>_<squads>x_lv<level>` 拼写，不包含回合号；解析 ID 时结合所在状态段或独立状态的 `round` 上下文定位；
- 虽然文档按名称拼写，但所有集合排序仍以底层**数字 ID 升序**为准；出现当前版本目录未知的名称时一律拒绝。

---

## 到布局的投影

布局不是独立概念，而是状态向战斗模拟可见面的确定性投影：

```text
layout = project(state)
```

任意合法独立状态（包括部署中任意一步的中间态）都可投影为 `layout` 送入训练场或模拟器，从而实现逐动作粒度的检查与诊断。

1. **直接丢弃（战斗不可见）**：`supply`、`unlocked_units`、`reinforce_offers`、`reinforce_choice`、`reinforce_pool`、根级与双方 `random_state`、`next_index`、`shop`、`recovered` 墓碑（其中被回收的 `delivered` 编队索引收集写入 `Layout.recovered`）、以及本回合既无滞留物也未释放在场上的技能槽位（含所有仅作用于部署期的 `field_recovery`、`redeployment`、`intensive_training` 释放与冷却计数值）。
2. **直接透传**：根级 `build`、`map_id`、`seed`、`round`；双边 `officers`、`techs`、`constructions`、`contraptions`、`tower_strengthen_levels`（全 `[0, 0]` 时 `layout` 省略）；存活编队的位姿、等级、经验条、`source` 与按槽位序穿戴的 `equipments`（若由起止状态投影完整部署，对比开局与终态还会为发生位姿变化的遗留编队生成 `recordPosition`/`recordRotated`）。
3. **过滤转换**：

| 状态来源 | 布局目标 | 投影规则 |
| --- | --- | --- |
| `blueprints` | `blueprints` | 仅保留增强连锁（`4`、`401`、`5`、`501`）；技能类蓝图（`1`、`2`、`3`）仅经面板释放影响战斗 |
| `energy_tower_skills` | `energy_tower_skills` | 仅保留战斗可见项 `5`、`6`；经济与招募技能（`1`、`3`、`4`）已分别由资源与新购编队体现 |
| `battle_skills` | `battle_skills` | 展平为两段：先列各槽位 `standing` 滞留项（按布局规范序），再列带 `order` 的战斗 `release`（转换为 `{name, positions}` 并按 `order` 升序排列） |

---

## 规范形式

### 1. 集合排序与省略规则

| 集合 / 字段 | 规范排序与形式 |
| --- | --- |
| 根级字段顺序 | `kind` → `[build, reinforce, max_round, map_id, seed]`（仅独立态）→ `round` → `random_state` → `reinforce_pool` → `reinforce_offers` → `blue` → `red` |
| `units`、`constructions`、`contraptions`、`battle_skills` | 按 `index` 升序 |
| `equipments`（阵营未穿戴库存） | 按装备 ID 升序（多重集） |
| `units[].equipments` | **按穿戴先后槽位序**（槽位 0 在前） |
| `officers`、`unlocked_units`、`blueprints`、`energy_tower_skills` | 按各自目录 ID 升序（`officers` 为多重集） |
| `techs` | 按单位 ID 升序排列键，各单位名下按科技 ID 升序 |
| `reinforce_pool.removed` / `.added` | 按卡牌 ID 升序集合 |
| `battle_skills[].standing` | 护盾按 `(x, y)` 升序；油田按控制点序 |
| `reinforce_offers` | 保持发牌原序，末尾接 `supply` 退款项 |
| `tower_strengthen_levels` | `[energy_tower, research_center]`（下标 0 为能量塔，下标 1 为研发中心） |

- 单位经验 `exp` 写作 `current/maximum`，当前经验为 `0` 时省略；
- 所有可选空列表（`blueprints`、`energy_tower_skills`、`equipments`、`battle_skills`、`techs`、`constructions`、`contraptions`、`standing`、`reinforce_pool`、`record*`）均在为空时省略；
- 数值格式与行内折叠遵循 [match.md](match.md#normal-form) 的统一三条拼写规则（列表项与全标量映射写作单行 flow 样式）。

### 2. 跨字段不变式

- `next_index.unit` 必须严格大于 `units` 中的最大 `index`（无冲突且不复用历史槽位）；`next_index.contraption` 必须严格大于 `contraptions` 中的最大 `index`；
- `state(0)` 中双方 `reactor_core` 必须 `> 0`；独立状态 `max_round` 若写出必须 `>= 1`；
- 所有出现的 `random_state` 均须恰好为 4 个 `u64` 且不可全为 `0`；
- `tower_strengthen_levels` 存在时长度必须为 2，且每项在 `0..=4`；
- `reinforce_choice` 与 `reinforce_offers` 严格一致：无牌面回合不得有 `reinforce_choice`，段内开局不得显式写 `reinforce_choice`，已选下标须落在 `0..offers.len()`；
- `recovered: true` 的墓碑编队仅限部署态出现，且不得残留位置、朝向、经验、穿戴装备或 `travelling` 标记。

---

## 从原生回放重建状态

原生 GRBR 中绝大部分回合状态可直接读取，但有两类差异需要转换器结合 [match.md#what-conversion-rebuilds](match.md#what-conversion-rebuilds) 重建：

1. **三项不可直接照抄的字段**：原生每回合快照采集于游戏“回合开启重置（发收入、冷却倒计时、清除过回合标志）”**之前**，因此：
   - `supply` 需在旧结余上叠加当回合收入、军官收益与上一回合 `rapid_resupply` 欠款后再减去本回合已执行动作开销；
   - `battle_skills[].cooldown` 需依据上一回合槽位释放记录执行重设或减 1；
   - `energy_tower_skills` 在开局强制清零，仅依据该回合自身动作重新填充（不照抄原生快照延迟记录的旧标志）。
2. **其他边界重建**：
   - 第 1、2 回合在原生战报前发生，因此编队花名册一律从逐回合玩家数据读取，而不依赖场次级战报；
   - 原生快照同样早于当回合军官交付，转换器按军官时间表补齐交付的小队（依 [rules/landing.md](../../rules/landing.md) 确定性落点）、技能、装备与商店解锁；
   - 战场技能滞留物 `standing` 从原生面板槽位的 `rangeItems` 提取，再投影归位到对应槽位下；
   - 原生未穿戴 + 已穿戴一并写入的 `equipmentDatas` 在导入时扣除编队佩戴后得到净库存 `equipments`。

---

## 排除字段

| 原生字段 | 排除原因 |
| --- | --- |
| `playerData.preRoundFightResult` | 既非玩家决策也非下一回合输入 |
| `playerData.IsSpecialSupply` | 仅在安装状态时防止游戏重复发收入的一次性内部锁存器，标准 1v1 正常对局不产生 |
| `playerData.researchQueue` | 仅在启用特殊游戏规则「战场技能研发」（`999917`，见 [match.md](match.md#game-rules-are-the-premise-so-the-match-carries-them)）时使用，标准 1v1 下永不可达 |
| `matchDatas.deadCount` | 无任何状态校验或战斗规则读取 |
| `NewUnitData.EquipmentID` | 恒为 `0`，实际佩戴已记录于编队 `equipments` 列表 |
| `NewUnitData.Durability` | 当前版本对单位无生效语义 |
| `EquipmentData.durability` | 标准 1v1 下装备耐久恒为 `-1`（永久有效） |
| `NewUnitData.RoundCount`, `SellSupply` | 可完全由购入时机与 `value` 推导 |
| `ConstructionSnapshotData.durability` | 各段惰性冗余项，不参与状态转移 |
| `matchDatas.teamRanks` | 席位显示序，无规则读取 |
| `matchDatas.poolOPs` | 已由集合净差值 `reinforce_pool` 替代 |
| `matchDatas.RoundExcludeReinforce` | 下一回合 4 级技能临时互斥可由本回合 `reinforce_offers` 直接推知 |

---

## 待决问题

无。
