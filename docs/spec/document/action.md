# 决策段规范（Action）

## 范围

决策（Action）是玩家在一个回合中执行的一条净操作。本文档定义：

- 14 种决策的类型标签、操作数及前置校验；
- `kind: action` 文档段格式及其在 [对局流](match.md) 中的位置；
- 单步状态转移对资源态与战斗态的影响边界；
- 原生回放的净决策折叠、12 阶段规范化排序与排除字段。

文档分工：

- [match.md](match.md) 定义整场对局流、头文件规则开关及回合端到端覆盖率校验；
- [state.md](state.md) 定义各回合状态段的完整字段结构（包括第 0 回合开局状态段 `state(0)`）；
- [layout.md](layout.md) 定义战斗开始前的阵型投影与[坐标系约定](layout.md#coordinate-system)（不含决策）。

所有坐标与朝向均采用阵营局部坐标系；单条决策本身不携带 `blue`/`red` 字段，其所属阵营由所在的列表键决定。

---

## 文档结构与公共约定

每个决策段承载同一回合内双方的净决策序列，按执行顺序排列：

```yaml
kind: action
round: 7
blue:
- {type: choose_reinforce_item, index: 3, name: sledgehammer_2x_lv2}
- {type: buy_unit, name: void_eye, position: {x: 0, y: -160}}
- {type: release_commander_skill, index: 0, name: intensive_training, target: {unit: 4}}
red: []
```

- `round`：与紧邻在前的状态段保持一致；开局为 `round: 0`（对应前面的开局状态段 `state(0)`）。
- `blue` / `red`：二者均为必填键，若该阵营本回合未作任何决策则写为空序列 `[]`。
- `type`：固定以 `snake_case` 作为标签键；除 `concede` 外，每种决策均有固定操作数。
- **名称字段**：`name` 与 `upgrade_technology` 中的 `unit`/`tech` 一律使用 [state.md](state.md#names) 定义的蛇形英文名称，不写数字 ID。单位科技名仅在本单位命名空间内唯一，因此 `upgrade_technology` 必须同时指明 `unit` 与 `tech`。
- **索引字段**：
  - `index` 在编队操作中指代分配器授予的编队索引，在 `release_commander_skill` 中指代技能面板槽位索引，在开局/增援选择中指代对应列表的 0-based 位置；
  - `tower` 在文档中对红蓝双方统一采用规范塔索引：`0` 表示 `energy_tower`，`1` 表示 `research_center`（原生回放转换与导出会自动处理红方的 `0 <-> 1` 映射，详见 [state.md](state.md)）。
- **省略约定**：`move_unit` 与 `buy_unit` 在 `rotated: false` 时省略 `rotated`；`buy_unit` 采用默认落点时省略 `position`。

### 14 种决策速查总表

| `type` | 所属域 | 操作数 | 影响的状态域 | 补给 / 限额消耗 |
| --- | --- | --- | --- | --- |
| `choose_advance_team` | 开局与增援 | `index`, `name`, `specialist` | 资源态（开局队在 round 1 交付） | 无补给消耗；修改核心基线 `core_modify` |
| `choose_reinforce_item` | 开局与增援 | `index`, `name` | 资源态（+ 局内发牌小队进战斗态） | 按卡牌价格扣补给（或拒绝返还补给） |
| `unlock_unit` | 商店与科技 | `name` | 资源态（`unlocked_units`） | 消耗单位解锁价；消耗解锁次数 |
| `buy_unit` | 商店与科技 | `name`, `[position]`, `[rotated]` | 资源态（分配器）+ 战斗态（`units`） | 消耗购买价（含商店等级加价）；消耗购买次数 |
| `upgrade_unit` | 商店与科技 | `index` | 战斗态（`units[].level/exp`） | 消耗单位升级价（减装备折扣，下限为 0） |
| `upgrade_technology` | 商店与科技 | `unit`, `tech` | 资源态（`techs`） | 消耗基础科技价 + 同单位已研数量递增惩罚 |
| `use_equipment` | 装备与布置 | `name`, `index` | 资源态（库存 `equipments`）+ 战斗态（`units[].equipments`） | 免费装配 |
| `move_unit` | 装备与布置 | `index`, `position`, `[rotated]` | 战斗态（`units[].position/rotated/travelling`） | 免费 |
| `release_contraption` | 装备与布置 | `name`, `position` | 资源态（分配器）+ 战斗态（`contraptions`） | 消耗装置价格；消耗本回合装置部署次数（每回合 8 次） |
| `active_blueprint` | 塔与技能 | `name` | 资源态（`blueprints`）+ 资源/面板（`battle_skills`） | 消耗蓝图价格；随后可能开放商店购买或技能槽 |
| `active_energy_tower_skill` | 塔与技能 | `name` | 资源态（`energy_tower_skills`） | 按技能净价扣补给（或记录下回合欠款） |
| `strengthen_tower` | 塔与技能 | `tower` | 资源态（`tower_strengthen_levels`） | 消耗达到目标等级的价格 |
| `release_commander_skill` | 塔与技能 | `index`, `name`, `target` | 资源态 + 战斗态（回收/训级/重部署或战斗释放） | 免费（回收类操作反还对应补给） |
| `concede` | 终局 | 无 | 无（直接终止对局，不进战斗） | 无 |

---

## 状态转移模型

本格式将一回合内随决策演化的局面抽象为**资源态**与**战斗态**两个部分（各字段的完整结构、默认值与拼写由 [state.md](state.md) 定义）：

- **资源态**：包含补给（`supply`）、单调递增的分配器（`next_index.unit`、`next_index.contraption`）、增援选择、已解锁单位、科技、军官、蓝图、能量塔激活与塔强化等级、未穿戴装备库存，以及核心血量基线等。除战斗直接扣减核心血量与发放单位经验外，资源态完全由决策与回合开启规则确定性决定。
- **战斗态**：包含棋盘上存活及待传送编队（位置、朝向、经验条、穿戴装备、`travelling` 标记）、建筑（`constructions`，如城墙、炮台）、装置（`contraptions`，如护盾、导弹、拦截器）以及战场技能滞留物和本回合释放目标。部署阶段的决策排布战斗态，随后由战斗阶段消费并结算下一回合的幸存局面。

### 单步可定义性与隐式转移规则

状态在**每执行一条决策后**均须保持良定义：对位置 $S$ 依次应用每条决策必须能够推导出合法中间态（这也是逐动作校验比回合端到端对比更严格的原因）。同时，以下四项规则不体现在动作名中，却直接决定转移结果：

1. **确定性落点与分配器占位**：开局先遣队、增援小队卡牌及军官按周期交付的小队，均通过地图主部署区中心对齐 10 米网格寻找最近空位落盘（见 [rules/landing.md](../../rules/landing.md)），并依次占用 `next_index.unit`。若忽略军官在回合开启时的前置交付，后续所有购买与回收的 `index` 均会错位。
2. **军官的延迟交付与多重集叠加**：军官不一定在获取当回合交付物资，其小队、战场技能或装备分别在 `active_round` 发放，单位则在 `unlock_round` 进入商店；`officers` 列表按多重集存储，可重复获取并叠加。
3. **蓝图链替换**：高阶连锁蓝图激活时直接替换 `blueprints` 中同链前驱条目。
4. **回合次数限额与核心/经验特殊入口**：商店每回合存在购买、解锁与装置部署次数限额（其中能源塔技能 `3` 与军官 `10004` 各永久或本回合增加 1 次购买机会；`10004` 作为军官会持续至后续回合）。`reactor_core` 仅在开局决策按 `core_modify` 移动一次；`units[].exp` 仅在升级清空、或被 `intensive_training` 填满时由决策改动。

---

## 决策类型详述

### 1. 开局与增援

#### `choose_advance_team`

```yaml
- {type: choose_advance_team, index: 1, name: vortex-fire_badger, specialist: giant_specialist}
```

- 仅允许出现在 `round: 0`；任何其他回合出现或缺失均拒绝。
- `index` 必须指向 `state(0)` 中该阵营 `offers` 的合法下标，`name` 与 `specialist` 须与该 offer 一致。
- 效果：在开启 `round: 1` 时交付对应先遣队编队与 `specialist` 军官（解锁对应单位并推进 `next_index.unit`），同时将 `state(0)` 中该阵营的 `reactor_core` 按该 offer 的 `core_modify` 调整后写入 `round: 1` 的 `reactor_core`。

#### `choose_reinforce_item`

```yaml
- {type: choose_reinforce_item, index: 3, name: photon_coating}
- {type: choose_reinforce_item, index: 4, name: refund_50}
```

- 仅在头文件 `reinforce: true` 且该回合存在 `reinforce_offers`（普通回合第 2 回合起）时合法；当 `reinforce: false` 或第 1 回合出现此动作时直接拒绝。
- 必须在除 `concede` 外的首条决策执行，每回合最多一次；`index` 与 `name` 必须同时匹配 `reinforce_offers` 对应槽位（卡牌名或末尾的 `refund_<amount>` 拒绝项）。
- 效果：写入 `reinforce_choice`；拿取卡牌时按卡牌价格扣补给（叠加军官即时返利），分别向 `officers`、`battle_skills`、`equipments` 发放对象或向棋盘与 `unlocked_units` 发放小队；选择末尾拒绝项则按该回合发牌给出的退款额增加 `supply`（见 [rules/reinforcements.md#declining](../../rules/reinforcements.md#declining)）。

### 2. 商店、编队与科技

#### `unlock_unit`

```yaml
- {type: unlock_unit, name: void_eye}
```

- 扣除单位解锁价，消耗 1 次解锁限额，将单位类型加入 `unlocked_units`；重复解锁已持有的单位类型会被拒绝。

#### `buy_unit`

```yaml
- {type: buy_unit, name: void_eye, position: {x: 0, y: -160}}
```

- 从商店购买一队已解锁单位并按 `next_index.unit` 分配索引。
- **不携带 `level` 参数**：购买等级完全由当前已激活的军官与能量塔技能动态决定（例如 Elite Recruitment 激活后抬升商店等级）；价格等于单位基础价加上超出 1 级部分的基础升级加价，并叠加购买价格修正（下限为 0；编队升级折扣军官不减免商店购买等级加价）。
- **落点与规范形式**：省略 `position`（且不允许设 `rotated: true`）时由棋盘确定性落点放置；显式给出 `position` 时目标足迹不得冲突，若位于侧翼则直接设置 `travelling`。规范化形式统一将购买写成无位置无旋转的默认落点购买，并在移动阶段用 `move_unit` 送至终态位置。

#### `upgrade_unit`

```yaml
- {type: upgrade_unit, index: 5}
```

- 目标编队 `index` 的 `exp` 必须已满（省略 `exp` 视为 0 经验，不可升级）；升级后等级加 1、经验条清零，扣除该单位升级价减去佩戴装备折扣后的补给（下限为 0）。

#### `upgrade_technology`

```yaml
- {type: upgrade_technology, unit: fang, tech: grenade_launcher}
```

- 为单位 `unit` 研究科技 `tech` 并写入 `techs`：
  - 必须在场且属于头文件该侧 `tech_loadout` 允许的范围（`"full"` 表示全目录允许，否则须在 42 字符 Base64url 正向位图中置 `1`）；
  - 须遵守单单位研发上限（`war_factory`、`abyss`、`mountain` 最多 6 项，其余单位最多 4 项），不可重复研究；
  - 价格随该单位**已研发科技数**递增，与场上其他单位无关。
- 研究各单位的 Jump Drive（Wasp `1606`、Overlord `1611`、Phoenix `1616`）后，该类型所有编队从本回合起（含后续回合）均获部署移动权限。

### 3. 装备与棋盘布置

#### `use_equipment`

```yaml
- {type: use_equipment, name: photon_coating, index: 3}
```

- 从本方未穿戴库存 `equipments` 消耗一件装备并追加至编队 `index` 的 `equipments` 末尾（装备槽位已满或库存不足均拒绝）。
- 装备在装配时免费；未穿戴库存跨回合满足多重集守恒：

```text
stock(R+1) = stock(R) + 本回合增援卡获得 + 本回合军官交付 + 回收编队退回 − 本回合装配消耗
```

- 所有操作按顺序生效，因此同一回合获得或经回收退回的装备可立即再次装配。为编队装配 Deployment Module（`13040001`）可授予该编队本回合及此后所有回合的部署移动权限。

#### `move_unit`

```yaml
- {type: move_unit, index: 0, position: {x: 85, y: -80}, rotated: true}
```

- 免费且仅修改棋盘上编队 `index` 的 `position`、`rotated` 与 `travelling`。
- 编队必须拥有本回合移动权限（当回合到达、佩戴 Deployment Module、研究 Jump Drive，或本回合对该编队施放过 `redeployment`），且目标足迹须完整落入所处部署区而不与其他已占足迹碰撞（见 [rules/landing.md#moving-and-placing](../../rules/landing.md#moving-and-placing)）。
- **跨区传送语义**：两侧翼部署区从 `round: 2` 起开放。若移动起点与终点在同一区域（主部署区或同一侧翼）内，`travelling` 保持不变；只要跨区域到达任一侧翼（含从一侧翼跨至另一侧翼），即置 `travelling: true`，返回主部署区则清除 `travelling`。战斗结算清空所有 `travelling`。

#### `release_contraption`

```yaml
- {type: release_contraption, name: shield, position: {x: -130, y: -153}}
```

- 扣除装置价格并消耗 1 次本回合部署限额（每回合 8 次），分配 `next_index.contraption` 后置入 `contraptions`。
- 本动作仅用于从商店购买并放置**装置**（`contraptions`，如护盾、导弹、拦截器），不同于**建筑**（`constructions`，如城墙、炮台；后者由地图开局发牌或经 `release_commander_skill` 生成，不通过本动作放置）。
- 装置通常须避开足迹碰撞，但护盾与导弹不参与部署碰撞检测。

### 4. 塔、蓝图与战场技能

#### `active_blueprint`

```yaml
- {type: active_blueprint, name: field_recovery}
```

- 激活 `blueprints` 中的一项研发中心蓝图并扣除费用；若提供战场技能则同步写入 `battle_skills`。不可重复激活，连锁蓝图须先激活前驱，且二阶激活后会替换一阶条目。
- 文档特意区分状态层的 `blueprints` 与投影到 `layout` 中的 `officers`（见 [state.md](state.md)）。

#### `active_energy_tower_skill`

```yaml
- {type: active_energy_tower_skill, name: mass_recruitment}
```

- 激活单回合能量塔技能并写入 `energy_tower_skills`（同一技能每回合不可重复）。扣除净价格，部分技能（如 `rapid_resupply`）会在下一回合开启时从回合收入中抵扣相应补给，另有技能会提升本回合后续购买的商店等级加价。

#### `strengthen_tower`

```yaml
- {type: strengthen_tower, tower: 0}
```

- 提升 `tower`（`0 = energy_tower`，`1 = research_center`）对应强化等级 1 级并扣除对应费用。

#### `release_commander_skill`

```yaml
- {type: release_commander_skill, index: 0, name: intensive_training, target: {unit: 4}}
```

- 释放技能面板槽位 `index` 中当前持有的技能 `name`（槽位实际技能不匹配、`cooldown > 0` 或指向类槽位已释放过均拒绝）。
- `target` 必须为恰好包含一个键的纯 YAML 映射，不使用 `!unit` 等自定义标签：

| 形式 | 含义 |
| --- | --- |
| `{area: [{x, y}, ...]}` | 按顺序给出技能覆盖坐标点 |
| `{unit: <index>}` | 指向本方现存编队 |
| `{construction: <index>}` | 指向本方现存建筑（`constructions`，如城墙、炮台；不含装置） |

- **部署期效果 vs 战斗释放**：
  - `field_recovery` 回收编队或建筑（`constructions`）并返还补给：编队转为恢复墓碑留在 `units`，同时将 `units[].equipments` 全部退回库存；
  - `intensive_training`（`1100001`）须以非满级（< 9 级）且经验未满的本方编队为目标，将 `exp` 设为 `maximum/maximum`（详见 [rules/unit_experience.md](../../rules/unit_experience.md)）；
  - `redeployment`（`1000001`）赋予目标编队本回合移动权限；
  - 仅战斗技能的释放会在面板写入带 `order` 的 `release` 并投射至战斗 `layout`。

### 5. 终局操作

#### `concede`

```yaml
- {type: concede}
```

- 无操作数、不修改盘面，立即结束整场对局（不再运行本回合战斗，后续不再出现新状态段）。该阵营同回合此前执行的决策及对手序列予以保留。

---

## 序列、净决策折叠与规范化

### 1. 顺序与保密语义

- 序列只记录逻辑执行顺序，不保留时间戳；
- `blue` 与 `red` 在同回合部署中互相保密、彼此独立，回放无法观测到双方的交叉时序，也不进行跨阵营交错合并。

### 2. 原生回放的净决策折叠

原生回放录入了玩家的思考与撤回历史，转换器在写入 `action` 前通过栈模型折叠为**净有效决策**：

- **撤回与重做（`Undo` / `Redo`）**：按回放动作压栈/弹栈，被撤回的决策从净序列中移除；
- **取消战场技能释放（`CancelReleaseCommanderSkill`）**：仅按面板槽位 `SkillIndex` 匹配并作废最近一次尚未失效的释放；原释放与取消操作在栈中**保留占位**以保证 `Undo` 计步准确，最终仅输出未被作废的净决策。
- **转换同步剥离的原生细节**：每回合固定唯一的 `FinishDeploy`；`BuyUnit` 未赋值的 `UIDX`（改由分配器推导）；批量 `MoveUnit` 拆分为单编队动作，且仅根据最终位置与旋转重构必要移动。

### 3. 标准 12 阶段规范化（`convert` 与 `format`）

基于起止状态重构常规部署序列时，规范化生成器按以下 12 个固定阶段排出确定性决策序列，同阶段内依次以编队索引、科技 ID、面板槽位索引破平局：

1. 选择部署终态 `reinforce_choice` 对应的增援条目；
2. 若本回合启用了 `rapid_resupply`，最先激活以固定预算基线；
3. 回收目标在开局即存在且终态为恢复墓碑的编队/建筑（若升级价格能因库存增强模块降低，回收/升级前先行装配可用模块，回收后模块退回库存）；
4. 按分配器顺序执行购买（默认落点），并按分配器位置就地处理“购买后即刻回收”的新建编队（每个已分配索引须以存活编队或墓碑收尾）；
5. 在全部对应编队登场后激活其余单位科技；
6. 按终态每支编队的穿戴顺序装配其余装备（保证价格折扣早于升级生效）；
7. 统一移动与旋转所有存活旧编队及新购编队（此时 Jump Drive 与 Deployment Module 已生效；必要时自动插入 `redeployment`、先退回主区再进侧翼以保 `travelling` 的路线，以及互换占位所需的让路中转点）；
8. 执行 `intensive_training` 与 `upgrade_unit`；
9. 按分配器顺序放置 `contraptions`；
10. 执行其余能量塔激活、塔强化与研发中心蓝图；
11. 执行其余 `unlock_unit`；
12. 按相对释放顺序执行战斗类 `release_commander_skill`。

- **精英征召（Elite Recruitment）切点搜索**：若终态激活了该能量塔技能，规范化会在所有购买切点中搜索能精确复现全部终态字段的方案，优先选取动作数最少且切点最早者。
- **顺序敏感回退**：若标准 12 阶段序列无法精确复现部署终态（如涉及 Stealth Cloak / 幽灵光线等顺序敏感初始化），`convert` 与 `format` 会保留能够合法复现终态的原始有效动作序（并在合法且等价的前提下折叠同区连续移动），确保重格式化幂等。

---

## 规范形式

| 集合 | 顺序约定 |
| --- | --- |
| 决策段字段 | `kind` → `round` → `blue` → `red` |
| `blue` / `red` 序列 | 按[标准 12 阶段规范化](#3-标准-12-阶段规范化convert-与-format)排序 |

每条决策均按 [match.md#normal-form](match.md#normal-form) 的拼写规则写为单行 flow mapping。

---

## 排除字段

| 原生字段 / 历史设计 | 排除原因 |
| --- | --- |
| `Time`, `LocalTime` | 决策序列只记录执行顺序，无规则读取时间 |
| `PAD_Undo`, `PAD_Redo`, `PAD_CancelReleaseCommanderSkill` | 已由净决策折叠消去；格式明确只记录净决策而非推敲撤回历史 |
| `PAD_FinishDeploy` | 每回合每阵营恒出现一次，不携带决策信息 |
| `PAD_MoveUnit.positionRecord`, `rotateRecord`, `superDeployRecord` | 仅重申动作前的位置快照 |
| `PAD_BuyUnit.UIDX` | 原生为未设置值；编队索引由状态分配器确定 |
| 指向对象时的 `PAD_ReleaseCommanderSkill.Positions` | 仅代表玩家鼠标点击坐标，不决定任何状态 |
| `PAD_ReleaseContraption.ExtraPosition` | 标准 1v1 装置均仅需单点坐标，原生字段恒为零点，不纳入文档格式 |
| `PAD_ReleaseConstruction` | 区分于商店购买的**装置**（`contraptions`，如护盾、导弹、拦截器，由 `release_contraption` 放置），**建筑**（`constructions`，如城墙、炮台）在标准 1v1 赛制中开局由地图发牌给出、局内新增全部经由 `release_commander_skill` 触发，原生 `PAD_ReleaseConstruction` 不可达，故不单列第 15 种动作 |

---

## 待决问题

无。
