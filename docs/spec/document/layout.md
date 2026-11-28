# 布局规范（Layout）

## 范围

布局（`kind: layout`）描述单场战斗开始时双方在棋盘上**对战斗可见的完整期望态**，是[状态](state.md#到布局的投影)向战斗模拟输入的确定性投影（`layout = project(state)`）。它只声明棋盘最终所处的状态，不包含构造该状态的决策历史，也不携带 `reactor_core`、`supply`、`unlocked_units`、`shop`、`next_index` 或 `random_state` 等经济与跨回合字段。

同一份布局同时服务于双后端执行：

- **确定性模拟器**：经共享的 `mechcore-document` 静态编译器校验并编译为执行计划；单位数值按[单位配置契约](../simulation/unit-rules.md)解析，而非写入布局本身。模拟器的可建模闭包窄于真机执行器，遇到尚未覆盖的机制时会按名拒绝而非近似模拟（边界详见[表达边界与未覆盖范围](#表达边界与未覆盖范围)）。
- **原生游戏执行**：既可通过[合成原生回放](#合成原生回放grbr)（`convert <layout.yaml> --to grbr` 或 `convert --to mcfr --backend game`）交由真机按单回合回放直跑录制，也可由实时训练场[执行器](../adapter/adapter.md)分阶段构建并校验。

与本格式族其余文档的分工：

- [`state.md`](state.md)：定义完整回合态（含资源态与战斗态）以及 `project(state) -> layout` 投影映射；
- [`match.md`](match.md)：定义整场对局多段 YAML 流、`kind: match` 头文件及真实 `.grbr` 回放双向转换；
- [`action.md`](action.md)：定义 14 种部署期决策与跨区域进入侧翼（`travelling`）的转移规则；
- [`fight.md`](fight.md)：在 `layout` 之上叠加战斗结果与内容哈希钉住（pin）契约。

读取与校验遵循严格拒绝（fail-closed）原则：未知字段、缺失或非 `layout` 的 `kind`、未录入当前构建目录的实体名称、以及与 `GAME_VERSION` 不匹配的 `game_build` 一律报错拒绝。

### 表达边界与未覆盖范围

1. **纯终态投影的顺序敏感边界**：布局只描述战前部署终态、不携带回合内或跨回合的动作先后历史，因此无法表达原生客户端中依赖“研发与侧翼移动或同回合多科技研究先后次序”的 4 类初始化差异（见 [`action.md#3-标准-12-阶段规范化convert-与-format`](action.md#3-标准-12-阶段规范化convert-与-format) 与 [`match.md#2-顺序敏感初始化的投影告警边界`](match.md#2-顺序敏感初始化的投影告警边界)）：
   - Phantom Ray 侧翼部署后研发 Stealth Cloak；
   - 携带 Aerial Specialist 且 Void Eye 处于侧翼传送（`travelling`）时研发 Aerial Mode；
   - Hound 处于侧翼传送时研发 Fire Extinguisher；
   - Wraith 同时持有 Land Cruiser、Floating Artillery Array 与 Aerial Specialist。
   布局与战斗文档专注于单回合战斗演算的完备性、确定性与游戏一致性，对上述少数无法由终态与原生历史顺序保持一致的行为统一做确定性化简（按科技与军官在部署前就绪结算），不增设字段编码历史初始化副作用；对局验证遇到上述情形时发出投影告警并跳过当回合战斗比对。
2. **赛制与地图范围边界**：仅覆盖标准 1v1 地图与位置指向型战斗战场技能（见 [`map.md`](../../rules/map.md) 与 [`battle_skill.md`](../../rules/battle_skill.md)）；携带特殊规则的地图（如 `1012`、`1022`）及非标准 1v1 技能配置不在此契约范围内。
3. **模拟器模块化闭合边界**：模拟器按[战斗架构](../simulation/architecture.md)显式声明其支持的战斗模块与机制闭包；对于处于该闭包之外的机制、地图障碍或实体组合，模拟器在开战前严格按名拒绝（fail-closed）而不做近似。具体支持范围以模拟器模块注册及拒绝结果为准。

---

## 文档结构与根级字段

布局文档顶层包含且仅包含以下字段：

| 字段 | 要求 | 说明 |
| --- | --- | --- |
| `kind` | 必填常量 | 固定写作 `layout`。仅作为文档根节点判别符（子树不得重复）；读取器最先解析该字段，缺失或指向其他类型时直接拒绝。 |
| `game_build` | 可选字符串 | 文档所依据的游戏构建版本（即仓库 `GAME_VERSION` 钉住、二进制内嵌的版本号）。缺省等于读取器自身版本；写入规范形式时若与当前版本一致则省略；若显式声明另一版本则严格拒绝，绝不用错误配置表读取。 |
| `map_id` | 可选正整数 | 原生 `MatchSetting` 地图 ID（非玩法模式 ID），缺省使用基准地图 `1021`。 |
| `seed` | 可选 `i32` | 战斗默认随机种子，禁止为 `0`。 |
| `round` | 必填整数 | 回合号，须 `>= 1`，决定侧翼开放与传送判定规则。 |
| `blue` | 必填映射 | 蓝方阵营定义。 |
| `red` | 必填映射 | 红方阵营定义。 |

完整规范 YAML 示例：

```yaml
kind: layout
round: 3
blue:
  officers: [supply_specialist, efficient_tech_research, extended_range_marksman]
  techs:
    marksman: [range_enhancement, quick_reload]
  blueprints: [attack_enhancement]
  energy_tower_skills: [enhanced_range, high_mobility]
  tower_strengthen_levels: [1, 2]
  units:
  - {name: marksman, index: 0, position: {x: 0, y: -50}, exp: 12/650, equipments: [laser_sights]}
  - {name: arclight, index: 1, position: {x: -310, y: 20}, travelling: true, source: joined}
  constructions:
  - {name: defensive_wall, index: 0, position: {x: 140, y: -105}}
  contraptions:
  - {name: interceptor, index: 0, position: {x: 5, y: -95}}
  - {name: shield, index: 3, position: {x: 100, y: -150}}
  battle_skills:
  - {name: shield_airdrop, standing: {position: {x: -285, y: -22}}}
  - {name: mobile_beacon, positions: [{x: -100, y: -150}, {x: 0, y: -100}, {x: 100, y: -50}]}
red:
  units:
  - {name: marksman, index: 0, position: {x: 0, y: -100}}
```

字段书写顺序不影响反序列化语义，但示例与工具输出应采用上述顺序；规范形式中为空或等于默认值的可选字段一律省略（如上例 `red` 侧）。

### 种子与地图

- **`map_id`**：选择原生 `MatchSetting` 地图（例如 `1001` 为铁路镇 `MainSceneDesert`，`1021` 为训练基地 `MainSceneMilitaryBase`；详见 [1v1 地图规则](../../rules/map.md)）。缺省时固定选取 `1021`，未知 ID 由原生适配器拒绝。所选地图决定了中立水晶（`FightCrystal`）及对应的 RVO 避障环境，并不蕴含模拟器支持每张地图特有障碍物的语义。
- **`seed`**：可选有符号 32 位整数（`i32`）。**明确拒绝 `0`**——在原生 `set_SystemSeed` 中传入 `0` 代表请求系统随机生成种子，若允许写入文档则同一文件将不再对应唯一确定场景；缺省 `seed` 本身即表达“由执行器生成非零种子”。
- **`seed` 的调用点覆盖优先级**：`seed` 本质上是战斗参数而非状态属性，布局只为其提供默认值，实际执行时由调用点决定最终取值：
  - `convert --to mcfr` 按「命令行 `--seed` → `layout.seed` → 生成非零随机种子」顺序解析（`--seed 0` 同样拒绝，自动生成也不得取 `0`）；
  - 实时执行器 `apply_layout` 按「调用参数 `seed` → `layout.seed` → 游戏自动生成」顺序解析；
  - MCFR 录制文件内嵌的 `layout.yaml` 必须写入最终解析得到的非零 `seed`，缺失 `seed` 的内嵌布局一律拒绝；
  - 导出为单回合[合成原生回放](#合成原生回放grbr)时必须具有明确的 `seed`（缺省则拒绝写出）。

### 回合与侧翼规则

`round`（`>= 1`）对应游戏 `Match.get_RoundCount()`，标志着本布局所属的回合节点，并决定侧翼（埋伏区）与 `travelling` 的静态合法性：

- **第 1 回合**：原生侧翼尚未开启，禁止出现任何位于侧翼的单位；
- **第 2 回合**：所有位于侧翼的单位**必须**声明 `travelling: true`（因为第 1 回合尚无单位可进入侧翼，第 2 回合出现的侧翼单位必为本回合初入）；
- **第 3 回合及以后**：侧翼单位同时允许 `travelling: true`（本回合新进）与 `travelling: false`（前序回合遗留定居）。

单位仅在“从其他区域移动进入侧翼”时进入 `travelling` 状态，并在经过一场战斗后于下一回合开局解除该标记（转移规则见 [`action.md`](action.md#move_unit)）。第 3 回合起无需从上一回合推演即可描述一个定居在侧翼的 `legacy` 编队——布局服务于**单场战斗的可复现性**而非整场对局的可达性；需要核对历史可达性的工具应在调用本契约之前对照前一回合 `state` 检查。

Schema 本身对 `round` **不设上限**（训练场与排位赛均可超过第 15 回合）。`MAX_STAGED_ROUND = 15` 仅是实时训练场执行器 `apply_layout` 因 55 秒总超时预算而在单局内从第 1 回合逐步推进时的操作上限，不约束静态校验、模拟器或合成原生回放（`record_replay_round`）。

---

## 坐标系与部署空间规则

布局使用自包含的二维 `(x, y)` 部署平面坐标系（沿用游戏原生二维部署接口命名；布局中的 `y` **不是** Unity 世界的垂直高度轴）。每个坐标均表示在**所属阵营局部坐标系**下的位置：

- 战场中心为 `(0, 0)`；
- `+y` 始终从己方指向对手方向；
- `+x` 始终指向己方右侧；
- 己方主部署半区恒为负 `y` 区间。

Schema 中所有坐标对均写作单个 `{x, y}` 映射（`x`、`y` 为精确有符号 `i32` 整数；拒绝 `10.0` 等小数形式）。

### 阵营变换与世界坐标映射

编译器与适配器在向原生或模拟内核提交前，按以下规则将阵营局部坐标映射至棋盘基准坐标（即蓝方坐标系）：

```text
blue: native(x, y) = local( x,  y)
red:  native(x, y) = local(-x, -y)
```

进入模拟器内核或 MCFR 录制后，该二维平面映射到 Unity 世界水平轴：`layout.x -> world.x`、`layout.y -> world.z`（世界 `y` 为垂直高度）。这一变换独立应用于所有单位、建筑、装置的 `position`、`recordPosition`，所有战场技能释放坐标 `positions`，所有滞留护盾 `position` 与滞留油田 `control_points`；对于红方的滞留油田 `grid_rows`，原生执行器同步将其行列方向各做 180° 翻转。**180° 阵营变换不会改变单位自身的 `rotated` 布尔值。**

### 部署矩形区域

在所属阵营局部坐标系下，地图（全图边界为 `x ∈ [-360, 360], y ∈ [-310, 310]`，尺寸 `720 × 620`）划分出三个互不重叠的部署矩形：

| 部署区域 | 局部 `x` 范围 | 局部 `y` 范围 | 适用对象与规则 |
| --- | --- | --- | --- |
| 己方主部署区 | `[-300, 300]` | `[-310, -10]` | 所有建筑、拦截器，以及中心 `y < 10` 的普通单位（导弹装置中心同样限此区域） |
| 左侧翼（左埋伏区） | `[-360, -300]` | `[10, 310]` | 第 2 回合起开放，容纳中心 `y >= 10, x < 0` 的普通单位 |
| 右侧翼（右埋伏区） | `[300, 360]` | `[10, 310]` | 第 2 回合起开放，容纳中心 `y >= 10, x >= 0` 的普通单位 |

左、右两侧翼在游戏内属于各自独立的 `MapRegion`（彼此间或主区与侧翼之间跨越均属于跨区移动）。除滞留物和导弹/护盾适用专用边界外，所有部署实体的**完整足迹矩形**必须全部落入所属区域边界以内（允许边缘精确相切，中心在界内但足迹探出界外时拒绝）。

### 区域感知旋转、碰撞空间与网格对齐

1. **区域感知旋转与有效世界足迹**
   单位的 `rotated` 标志相对于**所在区域**定义，而非绝对世界朝向：主部署区内 `rotated: true` 交换宽与高；而左右侧翼自身相对主区已有原生 90° 朝向。因此，基础足迹 `width × height` 在有效部署平面上发生宽高转置的条件是且仅是：**`rotated` 与“是否位于侧翼”二者相异（异或关系）**。例如：基础足迹 `50 × 20` 的爬虫，在主区 `rotated: false` 与侧翼 `rotated: true` 下有效占位均为 `50 × 20`；在主区 `rotated: true` 与侧翼 `rotated: false` 下有效占位均为 `20 × 50`。
2. **共享碰撞空间**
   蓝红双方各自的所有**单位、建筑与拦截器（`interceptor`）**在同一平面内共享同一碰撞空间（红方先做 180° 旋转变换再检查）：任意两个实体的有效足迹矩形不得出现正面积重叠（边缘精确相贴合法）。双方各两座固定基础设施塔各占 `20 × 20` 固定空间（位于世界坐标 `(±140, ±170)`），任何与四塔重叠的放置一律按区域限制（`RegionLimit`）拒绝。`shield` 与 `missile` 不参与此部署碰撞检查。
3. **`10 × 10` 网格对齐同余规则**
   所有单位、建筑与拦截器足迹的四个顶点必须全部落在 `10 × 10` 部署网格交点上。等价地，对每个坐标轴独立检验中心同余条件：
   - 当该轴有效尺寸 `size ≡ 0 (mod 20)` 时，要求 `center ≡ 0 (mod 10)`；
   - 当该轴有效尺寸 `size ≡ 10 (mod 20)` 时，要求 `center ≡ 5 (mod 10)`。
   常见示例（主部署区）：`20 × 20` 须 `x ≡ 0, y ≡ 0`；`30 × 30` 须 `x ≡ 5, y ≡ 5`；未旋转 `50 × 20` 须 `x ≡ 5, y ≡ 0`；旋转后 `20 × 50` 须 `x ≡ 0, y ≡ 5`（以上模均指 `mod 10`）。侧翼先按规则 (1) 转置有效宽高，再应用同一同余条件（例如侧翼中 `rotated: true` 的 `50 × 20` 爬虫同样要求 `x ≡ 5, y ≡ 0 (mod 10)`）。
4. **护盾与导弹的独立区域规则**
   `shield` 与 `missile` 不使用足迹和 `mod 10` 对齐规则：
   - `missile` 中心须位于己方主部署区 `x ∈ [-300, 300], y ∈ [-310, -10]`；
   - `shield` 半径为 70 米，其圆形范围允许边缘与己方主部署区边界相切，因此整数中心坐标须满足 `x ∈ [-230, 230], y ∈ [-240, -80]`。

### 实体足迹表

所有 32 种标准单位、3 种建筑与拦截器的基础足迹尺寸如下（未录入目录的类型严格拒绝，不从机甲碰撞半径反推）：

| 基础足迹（`width × height`） | 涵盖实体 |
| --- | --- |
| `20 × 20` | 单位：`arclight`、`marksman`、`vortex`；建筑：`anti_armor_turret`、`rapid_fire_turret` |
| `30 × 30` | 单位：`farseer`、`hacker`、`rhino`、`sabertooth`、`scorpion`、`tarantula`、`wraith`；装置：`interceptor` |
| `40 × 20` | 单位：`hound`、`phoenix`、`typhoon`、`void_eye` |
| `40 × 40` | 单位：`fortress`、`melting_point`、`raiden`、`sandworm`、`vulcan` |
| `50 × 20` | 单位：`crawler`、`fang`、`fire_badger`、`mustang`、`phantom_ray`、`sledgehammer`、`steel_ball`、`stormcaller`、`wasp` |
| `50 × 50` | 单位：`overlord` |
| `60 × 10` | 建筑：`defensive_wall` |
| `70 × 70` | 单位：`abyss`、`mountain`、`war_factory` |

---

## 阵营定义（`blue` / `red`）

双方阵营结构完全相同。`units` 为必填且至少包含 1 支有效编队；其余所有阵营级与编队级可选字段在缺省时均取下表基准值：

| 层级 | 字段 | 缺省默认值 |
| --- | --- | --- |
| 阵营级 | `officers`、`blueprints`、`energy_tower_skills`、`tower_strengthen_levels`、`recovered`、`constructions`、`contraptions`、`battle_skills` | `[]`（其中 `tower_strengthen_levels` 全为 `0` 级） |
| 阵营级 | `techs` | `{}`（无已研发科技） |
| 编队级 | `index` | **无默认值（必填）** |
| 编队级 | `level` / `exp` / `rotated` / `travelling` | `1` / `0` / `false` / `false` |
| 编队级 | `equipments` / `source` | `[]` / `legacy` |
| 编队级 | `recordPosition` / `recordRotated` | 分别缺省等于该编队当前 `position` / `rotated` |

### 命名约定与封闭词表

布局遵循 [`state.md#命名约定`](state.md#命名约定)，统一使用蛇形英文名替代数字 ID（写数字 ID 直接拒绝；所有名称经 [`config/names.yaml`](../../../config/names.yaml) 解析并在排序时按底层数字 ID 升序排列）：

- **单位封闭词表（32 种）**：`abyss`、`arclight`、`crawler`、`fang`、`farseer`、`fire_badger`、`fortress`、`hacker`、`hound`、`marksman`、`melting_point`、`mountain`、`mustang`、`overlord`、`phantom_ray`、`phoenix`、`raiden`、`rhino`、`sabertooth`、`sandworm`、`scorpion`、`sledgehammer`、`steel_ball`、`stormcaller`、`tarantula`、`typhoon`、`void_eye`、`vortex`、`vulcan`、`war_factory`、`wasp`、`wraith`。
- **建筑封闭词表（3 种）**：`defensive_wall`、`anti_armor_turret`、`rapid_fire_turret`（标准 1v1 开局所放置的三类建筑）。
- **装置封闭词表（3 种）**：`shield`、`interceptor`、`missile`。

各配置表与效果索引参见：[军官索引](../../rules/officers.md)、[单位科技索引](../../rules/unit_techs.md)、[装备索引](../../rules/equipment.md)、[战场技能索引](../../rules/battle_skill.md)、[单位经验索引](../../rules/unit_experience.md)。

### 军官（`officers`）

`officers` 为当前生效于该阵营的持久军官列表（多重集），统一包含开局专家、常规增援军官与单位专项改造军官（原生 `typeID != 0` 的单位改造同样属于军官实体，如 `extended_range_marksman` 即 `30201`，无需单设字段）：

- **重复与唯一性约束**：仅在配置表中标记 `canRepeated` 的军官可重复出现，且重复获取会线形叠加属性（如两份 Advanced Offensive Tactics 叠加 +60% 攻击、两份 Advanced Targeting System 叠加 +20 米射程）；执行器按列表个数逐一添加并校验计数。其余不可重复军官至多出现一次，开局专家（[`config/advance_teams.yaml`](../../../config/advance_teams.yaml) 所列）整阵营至多出现一名，违规重复一并拒绝。
- **排他边界**：由研发中心攻防强化连锁授予的四名隐形产物军官（`20300`、`20301`、`20310`、`20311`）**禁止写入 `officers`**，须统一在 `blueprints` 中声明。军官随附发放的战场技能、装备与交付小队，分别进入 `battle_skills`、编队 `equipments` 与 `units`，不在此处表示。
- **确定性语义**：规范形式按军官 ID 升序排列，该序列即执行器按原生 ID 确定性逐一添加并核对 `OfficerManager` 计数的顺序。

### 单位科技（`techs`）

`techs` 按所属单位名称分组声明已激活的单位科技，每项名称仅在其归属单位名下唯一。布局中写出的所有科技均视为**已购买并处于激活状态**（标准 1v1 无已购未启用的中间态），不允许同单位下重复声明同一科技。编译器将其解析为原生 `TechnologyData.ID` 并对照运行时目录校验归属与版本一致性；规范序下外层按单位 ID 升序、内层按科技 ID 升序排列，该拍平序即为执行器添加与激活的确定性顺序。

### 研发蓝图（`blueprints`）

`blueprints` 在布局中**仅承载研发中心的攻防强化连锁蓝图**（分别对应 [`state.md`](state.md#2-资源态round-1) 中的蓝图 ID `4`、`401`、`5`、`501`）：

| 蓝图名称 | 对应产物军官 ID | 战斗效果 |
| --- | --- | --- |
| `attack_enhancement` | `20310` | 攻击 +12% |
| `attack_enhancement_ii` | `20311` | 攻击 +36% |
| `defense_enhancement` | `20300` | 生命 +15% |
| `defense_enhancement_ii` | `20301` | 生命 +45% |

- 同条连锁的二阶强化替换一阶，因此每条连锁在 `blueprints` 中至多出现一项。执行器直接将其作为对应产物军官安装（游戏将其存入 `OfficerManager.GetInvisibleOfficers` 列表，捕获与回读会同时检查两份军官列表）。
- 授予战场技能的蓝图（`sticky_oil_bomb`、`field_recovery`、`mobile_beacon`）**在此严格拒绝**——战斗只能看见这些技能经由 `battle_skills` 释放产生的效果。

### 能量塔技能（`energy_tower_skills`）

`energy_tower_skills` 列出本回合已激活且对战斗有直接影响的能量塔技能（按 ID 升序）：

| 技能名称 | 原生 ID | 战斗效果 |
| --- | --- | --- |
| `enhanced_range` | `5` | 远程单位攻击射程 +15 米 |
| `high_mobility` | `6` | 全体单位移速 +3 米/秒 |

仅允许上述两项出现（购买补给或减免购置费用的经济技能仅影响 `state`，投影至 `layout` 时丢弃，在此出现即拒绝）。与 `tower_strengthen_levels` 不同，能量塔技能每回合部署后清空；其结构与 `state.energy_tower_skills` 同形，投影时直接筛选保留战斗可见项即可。

### 塔强化等级（`tower_strengthen_levels`）

记录双方两座固定基础设施的强化等级（各取值 `0..=4`，对应 [`config/economy.yaml`](../../../config/economy.yaml) 中的定价等级）：

- 合法形式为**空列表 `[]`**（代表两座塔均为 `0` 级；规范形式直接省略整个字段）或**长度固定为 2 的数组 `[能量塔等级, 研究中心等级]`**。
- **固定文档下标顺序**：无论蓝方还是红方，下标 `0` 恒为能量塔（`energy_tower`），下标 `1` 恒为研究中心（`research_center`），与 [`state.md`](state.md#2-资源态round-1) 完全一致。
- 因原生 `BuildingManager.buildings` 与 MCFR 在红方将两塔按逆序 `[ResearchCenter, EnergyTower]` 排列，编译、采集与回放导出均会按 [`config/maps.yaml`](../../../config/maps.yaml) 在边界层自动执行红方 `0 <-> 1` 索引映射。

### 编队来源与回收（`source` 与 `recovered`）

```yaml
units:
- {name: marksman, index: 3, position: {x: 0, y: -50}}
- {name: sabertooth, index: 8, position: {x: -105, y: -155}, source: delivered}
- {name: rhino, index: 9, position: {x: 40, y: -60}, source: joined}
recovered: [8]
```

每支编队的 `source` 说明其在本回合的入场身份，战斗在开局一刻对三者加以区分：

| `source` 取值 | 含义 |
| --- | --- |
| `legacy` | 前序回合遗留至今的旧编队（默认值，规范形式省略）。对局第 1 回合的先遣队属于 `joined`；人工布局可在任意回合建模 `legacy` 编队，合成回放会将其写入回合快照。 |
| `delivered` | 本回合开启时由军官（如 `opening_unit` 周期机制）自动交付的小队：须符合交付军官指定的单位类型、等级不低于交付等级，且交付时自身无经验积累。 |
| `joined` | 本回合新入场编队：含第 1 回合开局先遣队、商店新购入编队与增援卡发入编队（须显式写出）。 |

来源、索引区间与战斗规则约束：

- **索引单调分段不变式**：原生分配器按创建时间顺次发号，因此全局编队索引必严格分属三个连续递进区间：**所有 `legacy` 索引 `<` 所有 `delivered` 与 `recovered` 索引 `<` 所有 `joined` 索引**。任何破坏区间顺序的声明一律拒绝。
- **非传送侧翼约束**：单位只能在回合中跨区移动进入侧翼并处于 `travelling`；因此任何位于侧翼且 `travelling: false` 的编队，其 `source` 必为 `legacy`。
- **战斗索敌差异（`IsOldUnit`）**：开战瞬间拥有武器且不阻挡的防御建筑选择初始朝向时，只从对方塔、炮台与 `IsOldUnit` 为真的目标（即 `legacy` 编队与 `delivered` 小队，**绝不含 `joined` 编队**）中按评分择优转向，且 `delivered` 小队按位于棋盘原点 `(0, 0)` 参与评分与测角（详见[炮台初始朝向规则](../../rules/turrets.md#as-the-fight-starts-it-faces-the-target-its-selector-scores-best)）。
- **`recovered`（战地回收索引集）**：按升序记录那些“在回合开启时由军官交付、随即在部署期被战地回收（Field Recovery）收回”的小队 `index`。这些小队在开战时已不在场上，不写入 `units`、索引与 `units[].index` 严格互斥且须落在 `delivered` 索引区间内，供[合成原生回放](#合成原生回放grbr)在回合决策中精确复现。
- **捕获与实时训练场差异**：从回放回合开局采集时，第 1 回合依 `MechTeam.IsOldUnit` 区分（真实先遣队为 `joined`，合成快照可含 `legacy`），第 2 回合起依开局分配器、军官交付事件与新购操作区分。实时训练场执行器 `apply_layout` 只能在当前回合动态添加单位，故拒绝包含 `legacy` 或 `delivered` 的布局（请改用合成原生回放执行）。

### 编队（`units`）

`units` 列表列出本阵营在战前依然存活在棋盘上的所有编队，每项包含：

- **`name`**：32 种单位蛇形英文名之一。
- **`index`**：必填非负整数，为单位获分配的终身唯一身份编号（一经出售或摧毁永不复用，允许存在空洞缺号）；在 `units` 中须随声明顺序严格单调递增。
- **`position`**：所属阵营局部坐标系中的精确整数中心 `{x, y}`。
- **`source`**：`legacy`（默认省略）、`delivered` 或 `joined`。
- **`recordPosition` / `recordRotated`**：仅限 `source: legacy` 编队声明的可选回合开局位姿，分别缺省等于当前 `position` 与 `rotated`（规范形式在相等时省略）；`delivered` 与 `joined` 禁止携带。两者共同构成该遗留编队的战前准备期成员坐标，须整体满足部署区域与 `10 × 10` 网格对齐校验，但不作为当前占位参与部署碰撞检查；开战时防御炮台按此准备位姿对其评分定角，正式交战则使用当前 `position` 与 `rotated`。从对局起止状态投影时，若遗留编队未移动/未转向则省略。
- **`level`**：可选等级，范围 `1..=9`，缺省为 `1`。
- **`exp`**：当前等级内经验槽位进度，写作 `"<current>/<maximum>"` 字符串（如 `12/650`）。`current` 为非负整数，`maximum` 须严格匹配[单位经验索引](../../rules/unit_experience.md)中该单位该等级（`1..=9`）对应的满槽上限（满经验条件为 `current >= maximum`，规范形式在 `current == 0` 时省略整个 `exp` 字段）。
- **`rotated`**：可选布尔值，缺省 `false`，含义见上文[区域感知旋转](#区域感知旋转碰撞空间与网格对齐)。
- **`equipments`**：该编队穿戴的装备蛇形英文名列表（缺省 `[]`），**严格按装配先后顺序排列（不可重排）**。每支编队基础拥有 1 个装备槽，每名阵营持有的军官叠加其在 [`config/officers.yaml`](../../../config/officers.yaml) 中定义的 `equipment_slots`（对应原生 `CardElement.GetEquipmentSlotCount`）。列表长度超过可用槽位上限、为不可佩戴装备的单位（`wears_equipment: false`，见 [`config/unit_prices.yaml`](../../../config/unit_prices.yaml)：`war_factory`、`mountain`、`abyss`）佩戴装备、或装备 `mech_type` 与目标单位不匹配，一律拒绝。
- **`travelling`**：可选布尔值，缺省 `false`。仅允许位于侧翼的单位声明 `true`（主区单位写 `true` 非法；第 2 回合侧翼单位写 `false` 或省略同样非法）。

### 建筑（`constructions`）

`constructions` 描述棋盘上静止的建筑（缺省 `[]`），每条须含 `name`、`index` 与 `position`。`index` 由 `ConstructionManager` 按释放顺序分配，生命周期内恒定且永不复用（已出售或摧毁的建筑留下永久空号；测试模式快照里的 `10000` 偏移量会在读取时自动剥离），须为非负整数且在列表内**严格单调递增**。建筑与装置使用彼此独立的索引分配器，因此同号在两边可独立存在。

注意一段长墙（`defensive_wall`）在 MCFR 中可能对应多行建筑实体（由 `index` 与 `FightConstruction.GetConstructionChildIndex` 联合区分）；布局中的 `index` 是阵营内全局稳定的部署对象编号，而非 MCFR 里的单行 `building_id`。

### 装置（`contraptions`）

`contraptions` 描述部署终态时棋盘上依然存在的所有装置（含早先回合保留的以及下回合已重置能量待恢复的护盾；不含已触发移除的导弹与已摧毁的拦截器；缺省 `[]`），每条包含 `name`、`index` 与 `position`（三类装置均不带额外位置字段）。

`index` 由 `ContraptionManager` 独立分配器按释放顺序赋予，永不复用、允许缺号、须非负且在列表中严格递增。规范采集时会校验护盾半径与最大能量是否匹配选定原生配置，并对拦截器/导弹投影的平面整数坐标严格拒绝对小数做四舍五入。

### 战场技能（`battle_skills`）

`battle_skills` 同时描述两类与战场技能相关的棋盘状态：早先回合释放遗留至今的**滞留条目（`standing`）**，以及**本回合释放的战斗条目（`positions`）**。每条元素必居其一（同时出现或同时缺失均拒绝）。标准 1v1 仅覆盖以位置为目标且能影响战斗的技能（[战场技能索引](../../rules/battle_skill.md)）；需要指定单位/建筑作为目标的辅助技能（如 `field_recovery`、`redeployment`、`intensive_training`）已即时改写对应属性，不进入布局 `battle_skills`。

#### 滞留条目

```yaml
battle_skills:
  - name: shield_airdrop
    standing: {position: {x: -285, y: -22}}
  - name: sticky_oil_bomb
    standing:
      control_points:
        - {x: -24, y: 11}
        - {x: 80, y: 1}
      grid_rows:
        0: [240, 1020, 2046, 2046, 4095, 4095, 1023, 511, 254, 126, 60, 48]
        1: [240, 1020, 1022, 510, 255, 127, 63, 63, 30, 30, 12, 0]
        5: [16, 28, 30, 30, 63, 63, 127, 127, 254, 510, 1020, 240]
        6: [48, 124, 126, 254, 255, 511, 1023, 2047, 2046, 2046, 1020, 240]
```

滞留条目对应游戏存储在面板技能槽 `rangeItems` 中跨越回合存活的场地对象，在开战前直接重建而非作为本回合释放，既不占用技能库存也不占用释放顺序。标准 1v1 下**仅限 `shield_airdrop` 与 `sticky_oil_bomb` 两种技能允许携带 `standing`**（其他任何技能出现 `standing` 即按名拒绝）：

- **`shield_airdrop`**：写作 `standing: {position: {x, y}}`。空投护盾在被击破前可跨多回合持续存在（回合间自动恢复满能量），同一阵营可同时持有多个（每座盾一条）。它通过技能途径而非装置途径创建，没有装置 `index`，只需中心落在全图范围 `x ∈ [-360, 360], y ∈ [-310, 310]` 内即可（70 米圆盘可探出部署半区）。同一回合新释放的 `shield_airdrop` 记为释放条目而非滞留条目。
- **`sticky_oil_bomb`**：写作 `standing: {control_points: [p0, p1], grid_rows}`，按原始释放次序每发一条（油田持续 2 个回合，其他场地技能持续 1 个回合故绝不会跨回合遗留）：
  - `control_points`：恰好两个阵营局部整数点 `[起点, 终点]`，两者确定直线路径；经 30 米半径外扩后必须与全图 `[-360, 360] × [-310, 310]` 有交叠（含边缘相切）。原生 `FixedMath` 的线式散射函数据此精确展开出下标 `0..=6` 共 7 个定点油斑圆心（由于步长与定点数开方精度原因，中间 5 点与末点均会落在带小数的 Q32.32 定点值上，无法以整数位置表达，必须由控制点重算恢复）。
  - `grid_rows`：可选映射，键为存活圆点下标 `0..=6`。若省略或为空映射 `{}`，表示 7 个直径 60 米（半径 30 米）的完整圆形全部完整存活；若非空，则其键集合即为**存活圆点全集**（缺席键代表该油斑已被提前清除或完全消除）：映射值若为空列表 `[]` 表示该圆完整未受裁切，否则必须为恰好 12 个 `0..=4095` 无符号整数（至少 1 个活跃格，禁止第 12 位及以上置位），构成被护盾裁切后的 `12 × 12` 占格位掩码——第 `i` 项对应局部 `+x` 方向的列 `i`，低 12 位对应局部 `+y` 方向的行格（红方整体按局部系翻转 180°）。
  - 由于标准 1v1 中油田总寿命仅 2 回合，跨回合进入下一战时必然仅剩 1 回合且单场战斗过程本身不消耗剩余回合数，因此滞留条目不设剩余回合字段（执行器与模拟器重建时固定按剩余 1 回合恢复）；存活圆点集合已由 `grid_rows` 的键集唯一表达，也不再另设独立存活下标列表。

执行器在部署阶段安装完装置之后、开始任何本回合释放之前，按规范形式顺序先重建所有 `shield_airdrop`、再重建所有 `sticky_oil_bomb`，从而确保源文档与规范形式打出完全一致的战斗。

#### 本回合释放条目

本回合释放条目的数据结构为 `{name, positions}`，其中 `positions` 为精确 `i32` 局部坐标列表（即使单点技能也必须写作单元素数组 `[{x, y}]`，不使用 `position_1` 等拆分键；且战场技能位置不适用单位足迹或 `mod 10` 网格规则）：

- **位置元组长度**：单点技能取 1 个目标中心；两点线式技能取 `[起点, 终点]`（第二点为实际坐标而非方向向量）；`mobile_beacon`（含 `mobile_beacon_card`）取 3 个有序点：`[选中/起始点, 中间路径点, 终点]`。各技能所需静态与运行时 `GetEffectPositionCount()` 须严格一致（禁止截断或补齐）。
- **Mobile Beacon 200 米截断规则**：每对相邻点之间必须能被客户端 200 米放置圆盘后的向零整数截断（`MapVector`）所覆盖——校验时检测对应整型截断像元矩形是否与 200 米圆盘相交（允许对角方向相切像元，但排除精确轴向延拓出的整米外切像元）。
- **重复释放与语义不可交换性**：同技能可在一回合中合法释放多次（如导弹专家提供两枚导弹轰炸），故 `battle_skills` 中本回合释放段落代表严格有序的释放历史而非集合。**规范化器绝不可对本回合释放条目重排序**：同一阵营的散射类战场技能按释放顺序依次从该阵营唯一共享的 `FightTeam.random` 流中抽取散布坐标（蓝红两流相互独立，且均在每回合 `BattleSystem.OnEnterDeployment` 时重置）。相比之下，`units` 声明顺序不消耗共享随机流，因而允许按 `index` 规范重排。
- **地图与敌塔几何校验**：编译器依据全图 `720 × 620` 边界（[地图规则](../../rules/map.md#the-board)）与敌方双塔固定局部中心 `(-140, 170)`、`(140, 170)` 在变更前做静态几何预检，原生 `CanReleaseCommanderSkill` 与 `PAD_ReleaseCommanderSkill` 回读构成最终权威校验。

---

## 合成原生回放（GRBR）

`mechcore convert <layout.yaml> --to grbr <replay.grbr>` 与 `convert <layout.yaml> --to mcfr --backend game` 将单份布局编写为真机可直接运行的 `.grbr` 文件，由适配器 [`record_replay_round`](../adapter/adapter.md#record_replay_round) 载入指定回合开打并录制 MCFR；`convert <synthetic.grbr> --to layout` 则可将该规范合成回放无损还原回 `layout`。

该机制利用了原生回放的核心特性：游戏回放某一回合时，直接从该回合记录的 `PlayerRoundRecord` 快照开场，再顺序执行该回合的动作指令，而不重跑此前所有回合。因此合成回放只需在目标回合构建出与布局等效的快照与还原决策（`source`、经验、`travelling` 状态、装备、能源塔激活与战场技能释放），**不伪造历史购置或部署轨迹**。真实对局 `.grbr` 读取规则与投影边界顺序警示（如 Stealth Cloak / Aerial Mode）见 [`match.md`](match.md#1-原生回放转对局convert---to-match)。

### 1. 文件封装（`BinaryFormatter` 与 `GameRiver.Replay`）

一个 `.grbr` 文件本质上是包含 `GRCore` 库 `GameRiver.Replay` 根对象的 .NET `BinaryFormatter` 二进制流（[对局写回原生回放](match.md#2-对局写回原生回放convert---to-grbr)共用同一外层封装）。其成员严格按以下顺序写出：

| 成员 | 值（合成布局回放） |
| --- | --- |
| `battleID` | `"layout"` |
| `version` | 构建号（`GAME_VERSION` 末段数字） |
| `seat` | `0` |
| `mapID` | `layout.map_id`，缺省为 `1021` |
| `playerDatas` | 固定含两项的 `List<Replay+PlayerData>`：`{id: 1, name: blue}` 与 `{id: 2, name: red}` |
| `realBattleRecordData` | 以 UTF-8 BOM 开头的 XML 字符串，对应 `XmlSerializer` 序列化的 `<BattleRecord>` |
| `<CreateTime>k__BackingField` | 零值 `DateTime` |

### 2. 对局头与前置回合占位

- **`BattleInfo`**：写入 `SystemSeed = layout.seed`、`mapID` 与标准 1v1 固定参数（见 [`match.md#对局头文件kind-match`](match.md#对局头文件kind-match)：`VS_1_1`、阶段时长、最大回合数），关闭先遣队与两类增援、开启建筑、`Version` 取构建号、`Seat = 0`。
- **回放主体与空前置占位**：`playerRecords` 依序写入蓝、红两阵营（`data.unitDatas` 仅写双方拥有科技的单位清单，以保证科技研发校验通过），`matchDatas` 为第 `0` 至 `layout.round` 回合各写一个无随机流状态的 `MatchSnapshotData`。双方在 `0 .. layout.round` 的每个回合都拥有一份 `PlayerRoundRecord`；目标回合之前的全部回合均为占位空回合，不带任何决策。
- **坐标系转换**：`<BattleRecord>` 中所有位置均按全局棋盘坐标系（即蓝方坐标系）写入，红方坐标与油田位掩码在写入前做 180° 取反。

### 3. 目标回合快照（`playerData`）

目标回合开局 `playerData` 装载状态如下：

| 成员 | 内容说明 |
| --- | --- |
| `units` | 全部 `source: legacy` 编队的 `NewUnitData`（含 `Index`、`Position`、`IsRotate`、`Level`（0-based，即 `layout.level - 1`）、`Exp` 与穿戴装备；其余源编队不在快照创建） |
| `unitIndex` | 回合开局时的分配器起始值（见下节分配器规则） |
| `officers` / `bluepints` | 阵营全部军官（含强化连锁对应产物军官）及对应强化连锁蓝图 ID |
| `commanderSkills` | 技能面板槽位列表（见下述精确顺序） |
| `activeTechnologies` | 双方已激活单位科技，按单位分组 |
| `equipmentDatas` | 编队穿戴的所有装备实例，每件耐久写 `-1` |
| `contraptions` / `contraptionIndex` | 所有装置 `index`、类型、位置，及 `最大 index + 1` |
| `towerStrengthenLevels` | 双方塔强化等级（红方先转为原生逆序；未声明则全 `0`） |
| `constructionSnapshotDatas` / `constructionIndex` | 建筑 `Index`、类型、位置与逐段耐久（`defensive_wall` 为 5 段，其余为 1 段），及 `最大 index + 1` |
| `shop.unlockedUnits` / `supply` | 空列表；若本回合需升级 `delivered` 小队，则 `supply` 写 `10000`，否则 `0` |

- **第 1 回合 `legacy` `RoundCount=1` 机制**：在合成第 1 回合布局的快照中，`legacy` 单位的 `RoundCount` 写入 `1`。原因是第 1 回合进入时不触发跨回合老化，而 `MechTeam.IsOldUnit` 判定要求 `RoundCount > 0`；第 2 回合及以上则写入 `0`（进入部署期时引擎自动将其加 1）。为保持 `joined` 单位正确的新入场身份，所有 `joined` 编队均通过回合动作创建而不放入快照。
- **`commanderSkills` 面板槽位排布顺序**：
  1. 本回合所有战斗释放条目（按 `layout.battle_skills` 中释放原序，均置本回合可用）；
  2. 所有滞留条目（先 `shield_airdrop` 后 `sticky_oil_bomb`）：将 `rangeItems` 重建至对应技能槽下，油田含控制点、存活点 `ByteMask`、`12 × 12` 格子（红方旋转 180°）及剩余 `1` 回合；
  3. 当 `recovered` 非空时追加 1 个就绪的战地回收槽位（`900001`）；
  4. 按 `units` 索引顺序，为每一支需要恢复满槽经验（且等级低于 9 级）的 `joined`/`delivered` 编队追加 1 个就绪的强化训练（Intensive Training）槽位（`isActive = false, coolingRound = 0`）。
  这些辅助槽位均不改变战斗释放时的技能下标引用。

### 4. 回合还原指令（`actionRecords`）

目标回合按以下严格顺序写入决策序列：

1. 对 `recovered` 中每个索引：通过战地回收槽位发出回收指令；
2. 对每个 `delivered` 小队：依次升级至目标等级、穿戴装备、按需强化训练恢复满槽经验、并移动至布局目标位姿；
3. **遗留编队状态还原**：先为每个 `travelling: true` 的 `legacy` 编队发出基于 `PAD_TestCommand` 的强制移动 `MAD_MoveUnit`（置位 `IsForce = true`、`IsUndo = true` 及期望 `superDeployRecord` 标志：原生 Undo 通道在目标处于不同区域时直接恢复其传送成员资格；若该传送遗留编队的 `recordPosition` 与当前 `position` 恰在同一区域，则先将其送往对侧翼再拉回当前位姿，同时保持快照开局位姿不变）；随后将所有当前位姿不同于 `recordPosition`/`recordRotated` 的普通遗留编队移动到位。该强制路径不受单位自身跨区机动能力限制，布局无需额外声明“哪件装备或技能赋予了其移动权”；
4. 对每支 `joined` 编队按 `index` 升序：以 `PAD_TestCommand` 携带 `MAD_AddUnit` 创建在己方主部署区（蓝方区域 1 / 红方区域 4；seat 分别为 0 / 1；若序号连续紧接当前分配器则传 `-1` 交由分配器自增，否则传明确 `index`），随后穿戴装备、按需训练满槽经验、再移动至目标位置与朝向（从主区移入侧翼时由引擎自然赋予 `travelling` 标记）；
5. 激活本回合所有 `energy_tower_skills`；
6. 严格按布局声明顺序从面板对应槽位释放每条 `battle_skills` 释放条目（维持团队散布随机流的精确消耗序）；
7. 当此前写入过至少一条上述决策时，末尾补入 `PAD_FinishDeploy`。

**分配器、军官交付与经验边界细则**：

- **单位索引分配器 `unitIndex`**：若本回合有军官开启交付小队，则快照 `unitIndex` 设为首个 `delivered` 或 `recovered` 的索引；否则设为所有 `legacy` 最大 `index + 1`。当有军官将在下一回合继续交付小队时，`joined` 编队禁止留有跳号空洞（因为显式指定 `index` 不会推进分配器，会导致下回合交付冲突）。
- **军官开局交付**：每名在本回合触发 `opening_unit` 的军官在快照之上额外发出一支小队，必须在布局中恰好作为 1 支 `delivered` 编队或 1 个 `recovered` 索引被完整记账；标准 1v1 不会让同一阵营同回合触发两名军官各自发队，出现即拒绝。
- **经验约束**：`legacy` 编队直接从快照读取任意 `current` 经验；`joined` 与 `delivered` 编队新创建时经验为 0，仅当 `current == maximum` 且 `level < 9` 时可通过强化训练槽瞬间补满（非零不满槽或 9 级满级携带经验无法以此法还原）。

### 5. 逆向读取（`convert --to layout`）

`convert <synthetic.grbr> --to layout` 读取合成回放最后一个正数回合的快照与还原指令，完整恢复各方状态、`source`、`recovered`、经验、`travelling`、装备、能源塔技能与战场技能（前置占位空回合不会产出布局）。读取器**只接纳本格式编写器为当前固定构建版本生成的规范 XML**：它在还原出 `layout` 后会重新正向编码回 `.grbr` 并校验两者内嵌 XML 完全一致，拒绝任何无法识别的原生字段或通用测试指令。

### 6. 写回拒绝清单

即使布局静态编译合法，下列情况因合成原生回放无法无损表达而报错拒绝并点名具体原因：

1. 布局未指定 `seed`；
2. 同一阵营在同一回合开启时触发了两名及以上军官交付小队；
3. `delivered` 编队的单位类型不符交付军官定义，或其等级低于交付军官规定等级；
4. 本回合有军官交付却缺少对应的 `delivered`/`recovered` 条目，或相反出现无交付军官对应的 `delivered`/`recovered`；
5. 下一回合即将触发军官交付小队，但本回合 `joined` 编队存在跳号空洞（导致分配器滞后冲突）；
6. `joined` 或 `delivered` 编队携带了既非 0、又不是 `level < 9` 满经验条的 `exp` 值；
7. 某项单位科技在当前构建下查不到对应归属单位。

---

## 执行器应用保证

任何向真机执行布局的模块（具体协议见 [`adapter.md`](../adapter/adapter.md)）必须满足下列闭环不变量：

1. **初始前置校验**：实时训练场入口必须处于干净确定的第 1 回合开局部署阶段，`round` 满足侧翼/传送规则，且禁止向该仅支持回合内加兵的路径提交含 `legacy` 或 `delivered` 的布局。
2. **明确双边选边**：双方均须成功建立并可被适配器显式选边操控。
3. **变更前静态全检**：在发送任何写动作前，完成所有类型、专属字段、局部坐标、技能点列长度、足迹网格对齐与碰撞校验；释放战场技能前须先通过运行时 `GetEffectPositionCount()` 与原生 `CanReleaseCommanderSkill` 核查。
4. **有序突变**：每个数组内部严格按文档声明序推进变更；所有滞留战场技能均在装置之后、任何本回合技能释放之前安装（滞留护盾先于滞留油田）。
5. **逐操作权威回读**：每一步写操作后均执行原生只读回读核验。
6. **零重试快速失败**：任何拒收、目录缺失、双塔歧义、传输报错或回读偏差立即中止并报告精确上下文（单位报 `name` 与 `position`，战场技能报 `name` 与有序点列或滞留物），绝不静默重试。
7. **完整期望态语义**：布局定义完整目标局面而非增量补丁；执行器须从空白对局起步或证明未声明状态处于基准零位，绝不悄然保留上一份布局残留的军官、科技、塔等级或能量塔技能。

---

## 规范形式

同一个游戏初始战斗状态在规范形式下仅对应唯一一份文档。规范形式要求：**省略所有与公有默认值等价的语法**，并按以下规则重排各集合：

| 字段 / 集合 | 规范顺序与省略条件 |
| --- | --- |
| 根级与阵营字段序 | 根级：`kind` → `game_build`（不同构建时）→ `map_id` → `seed` → `round` → `blue` → `red`；阵营：`officers` → `techs` → `blueprints` → `energy_tower_skills` → `tower_strengthen_levels` → `units` → `recovered` → `constructions` → `contraptions` → `battle_skills` |
| `units`、`constructions`、`contraptions`、`recovered` | 按 `index` 升序 |
| `officers`、`blueprints`、`energy_tower_skills` | 按各自底层数字 ID 升序（`officers` 可包含重复元素） |
| `techs` | 外层按单位 ID 升序，单位内部按科技 ID 升序 |
| `tower_strengthen_levels` | 顺序固定为 `[energy_tower, research_center]`；若全零（`[0, 0]`）或空列表则**省略整个字段** |
| `battle_skills` | **所有滞留条目排在最前**，先按 `name` 升序（`shield_airdrop` 先于 `sticky_oil_bomb`），同类内部护盾按 `(x, y)` 升序、油田按 `control_points` 升序；**其后接本回合释放条目且严格保持原有释放顺序（绝不重排）** |
| 默认值省略 | `game_build == GAME_VERSION`、空列表/空映射、单位 `level: 1`、`exp` 的 `current == 0`、`rotated: false`、`travelling: false`、`source: legacy`、`recordPosition == position`、`recordRotated == rotated`，以及全 7 点完整无裁切的油田 `grid_rows` 均省略 |

> **合成原生回放的规范确定性**：在给定的 `(layout, seed, build)` 三元组下，导出的合成 `.grbr` 文件具备严格字节级确定性（玩家固定先蓝后红、回合升序、实体沿用布局序、还原指令严格按前述 7 步顺序写出）。

在文本排版层面，规范布局文档严格共用 [`match.md#规范形式`](match.md#规范形式) 所规定的**三条统一 YAML 拼写规则**（序列元素单行 flow 化、全标量键值单行 flow 化、其余保留块式），从而使每一支编队、建筑、装置、坐标对与技能条目占据单行，并与 [MCFR 规范](../mcfr/mcfr.md) 中内嵌 `layout.yaml` 成员的字节规范保持一致；反序列化读取器同时兼容等价的块式写法。对同一份已规范布局再次执行规范化不会产生任何字节变动。

---

## 排除字段

以下字段出于设计边界有意排除在本规范之外：

| 排除字段 / 概念 | 排除原因 |
| --- | --- |
| `unit_modifications` | 单位专项改造在底层就是常规军官实体，统一写入 `officers`。 |
| `research_blueprints` | 攻防强化连锁写入 `blueprints`；技能蓝图仅通过 `battle_skills` 释放影响战斗。 |
| `reactor_core`、`supply` 与商店额度 | 属于经济与跨回合决胜资源（归属 `state`），与单场战斗棋盘无关；合成回放仅在内部按需填充临时 `supply`。 |
| 独立于 `battle_skills` 的滞留物列表 | 游戏将滞留护盾与油田保存在面板技能槽的 `rangeItems` 内，归入 `battle_skills[].standing` 更忠实于真实结构。 |
| 油田等地形材质名称 | 材质类型唯一取决于留下它的 `battle_skills[].name`。 |
| `standing` 中的 `active_sub_effects` 与 `remaining_rounds` | 存活油斑圆点已由 `grid_rows` 键集唯一表达；标准 1v1 油田跨回合仅可能剩 1 回合且单场战斗不依赖剩余回合数（对比 MCFR `terrains.remaining_rounds` 仅记录当时余寿）。 |
| `opening_techs`、`reinforcement_techs` 与候选下标 | 布局表达结果状态而非获取路径，且候选卡片下标为瞬态值不具备静态稳定性。 |
| `randomStateData`（原生回放快照字段） | 战斗仅消耗由 `SystemSeed`（`seed`）和 `round` 派生的 `Match.roundRandom` 与 `FightTeam.random`（见 [`state.md`](state.md#2-共享随机流与普通增援池)）。 |
| `PlayerRecord.data.style` 与玩家外观/账号信息 | 皮肤、`seed`、`id`、`name`、`ad` 均不影响战斗模拟判定。 |
| `MatchActionData.Time` / `LocalTime` | 动作仅按排列顺序生效，战斗模拟不读取操作时钟。 |

---

## 待决问题

无。
