# 对局文档规范（Match）

## 范围

对局文档（`kind: match`）以 `---` 分隔的多段 YAML 流记录一场标准 1v1 对局：流首由对局头文件声明整场共享的规则设置与双方元信息，其后写入第 0 回合开局与选队决策，再按回合交替记录每回合开局状态与双方净决策，并可在末段附带终局结果声明胜负与核心血量。

```yaml
kind: match
build: <GAME_VERSION>
source: game
reinforce: true
max_round: 40
map_id: 1001
seed: 2038621361
blue: {name: blue, tech_loadout: full}
red: {name: red, tech_loadout: full}
---
kind: state
round: 0
blue:
  reactor_core: 4500
  seed: 134917337
  offers: [...]
  constructions: [...]
red:
  reactor_core: 4500
  seed: 134917338
  offers: [...]
  constructions: [...]
---
kind: action
round: 0
blue:
- {type: choose_advance_team, index: 1, name: vortex-fire_badger, specialist: giant_specialist}
red:
- {type: choose_advance_team, index: 0, name: crawler-tarantula, specialist: supply_specialist}
---
kind: state
round: 1
blue: {...}
red: {...}
```

流中的每份独立 YAML 文档称作一个**段（segment）**，首行均由 `kind` 标明段类型。本文档定义 `kind: match` 头文件、`kind: result` 终局段、流语法、基于种子的发牌核验、跨回合不变式，以及原生 `.grbr` 回放与 `match` 文档的双向转换（`convert --to match` 与 `convert --to grbr`）；单段载荷与投影分工如下：

- [state.md](state.md)：定义 `kind: state` 段内状态与独立状态的完整结构；
- [action.md](action.md)：定义 `kind: action` 双方决策序列的 14 种动作与单步转移语义；
- [layout.md](layout.md)：定义状态朝战斗模拟输入投影而成的纯战斗布局（`layout` 永不出现在对局流中）。

每个回合具备自足的切片可读性：任意工具若只需处理第 $R$ 回合，只需读取对局头文件、该回合的 `state(R)` 与 `action(R)`，无需重放或反序列化中间相隔回合的历史数据（见 [state.md](state.md#独立状态的-mdp-完备性)）。同时，相邻两回合之间的战斗演化仅通过两段 `state` 之间的差异体现。

---

## 多段流语法与终局结果

### 1. 完整对局与截断对局

- **完整前缀**：从第 0 回合开局起始，固定遵循交替序列：

```text
match, state(0), action(0), state(1), action(1), state(2), action(2), ... [, result]
```

- **截断对局（Partial Match）**：省略开局与早先缺失回合，直接从首个捕获到的检查点回合 `N`（`N >= 1`，含 `N = 1`）起步：

```text
match, state(N), action(N), state(N+1), action(N+1), ... [, result]
```

  首个状态 `state(N)` 作为既定检查点读入，不重塑也不声称验证缺失的开局决策与历史回合，故截断对局不包含 `state(0)` 与 `action(0)`。

- **回合号与追加能力**：`state` 的 `round` 从起始回合起每次严格加 `1`，紧随其后的 `action` 与前一段 `state` 同属同一回合。YAML 流允许在**任意合法段之后终止**——进行中的对局只需不断追加下一段而无需重写全文；若流停止于某回合的 `action` 段，表示该回合战斗尚未写入结果（并不蕴含该场战斗是否已经打完）。
- **流结束约束**：除主动终止外，以下两类情况确立对局已达终点，其后仅允许可选的 `result` 段，不允许出现任何新 `state` 或 `action`：
  1. 某回合 `action` 段中有一方执行 `concede`（该回合不进行战斗，流不再产生下一回合状态）；
  2. 某回合 `state` 段中任一方 `reactor_core <= 0`（上一场战斗已摧毁该核心，该段不开新决策）。
- 凡出现段次乱序、`round` 跳跃/重复、在终局信号后继续写入状态或决策、或在开局状态段携带 `release` 等违规情况，读取器一律拒绝。

### 2. 终局判定与 `result` 段

```yaml
---
kind: result
round: 6
winner: blue
blue: {reactor_core: 1995}
red: {reactor_core: -240}
```

- `result` 为可选收尾段，整场至多出现一次且必居流末；其 `round` 指明最后一次发生战斗的回合、或发生 `concede` 的回合，须紧接该回合的 `action` 段之后，或紧接由该回合打出的终局 `state(round + 1)` 之后（不能跳过决策直接接在普通非终局 `state` 后）。截断对局同样可以携带 `result`。
- `winner` 取值为 `blue`、`red` 或 `draw`；双方均须写出带符号的最终核心血量 `reactor_core`（允许负值）：
  - **常规战斗终局**：若未发生认输且 `round < max_round`，至少一方 `reactor_core <= 0`（唯一存活方胜；双方同时归零或为负则判 `draw`）。若打满至 `round == max_round`，允许双方核心血量均为正，血量高者胜；若双方正核心血量相等，因原生排名还会比较小队评分奖励而本段不存该项，格式按文本显式保留声明的 `winner`。
  - **认输终局**：发生 `concede` 时另一方获胜且本回合不发生战斗，双方终局核心维持该回合开局时的 `reactor_core`；若流同时附带了终局 `state`，`result` 中的核心数值必须与之一致。
- **缺失含义与验证**：缺席 `result` 仅表示终局比分尚未记入文档（不代表对局一定还在进行），仅有胜者而无双方终局核心不得写 `result`。在未产生下一回合状态的末局上，`mechcore verify` 直接将末回合战斗模拟结算后的核心扣血（开局部署核心减去战斗 `core_damage`）与 `result` 核心对比校验，无需伪造下一回合 `state`；认输终局则直接核对未变动的开局核心。无论来源是锦标赛赛果拉取还是本地对局结算，`result` 结构完全一致，产出后端由头文件 `source` 区分。

---

## 对局头文件（`kind: match`）

头文件描述跨全对局不随单回合决策改变的信息，由**规则不变量**与**纯溯源/展示元信息**两类字段构成（对比见 [state.md](state.md#独立状态的-mdp-完备性)）：

### 1. 字段总览

| 层级 | 字段 | 归属分类 | 含义与来源 |
| --- | --- | --- | --- |
| 根级 | `kind` | 格式定位 | 恒为 `match` |
| 根级 | `build` | 规则不变量 | 匹配表所依据的游戏版本 `Application.version`（由 `GAME_VERSION` 钉住） |
| 根级 | `source` | 溯源元信息 | 产生对局战斗结果的后端：`game` 或 `simulator` |
| 根级 | `reinforce` | 规则不变量 | 是否启用增援发牌与选择，默认 `true` |
| 根级 | `max_round` | 规则不变量 | 最大回合数上限（正整数），默认取 `map_id` 在 [config/opening.yaml](../../../config/opening.yaml) 中的预设（地图 `1012` 为 `13`，其余为 `40`） |
| 根级 | `map_id` | 规则不变量 | 地图标识，来源同 `BattleInfo.MapID` |
| 根级 | `seed` | 规则不变量 | 共享随机种子，来源同 `BattleInfo.SystemSeed` |
| 阵营级 | `blue`/`red.name` | 溯源元信息 | 玩家或 Bot 展示名（来自 `PlayerRecord.name`，省略默认 `blue`/`red`，规范输出恒写出） |
| 阵营级 | `blue`/`red.elo_score` | 溯源元信息 | 可选整数，来自原始服务端开局快照 `PlayerBaseInfo.eloScore` |
| 阵营级 | `blue`/`red.tech_loadout` | 规则不变量 | 该阵营可研单位科技白名单 |

> 其中 `build`、`reinforce`、`max_round`、`map_id`、`seed` 以及双方 `tech_loadout` 正好构成 [state.md](state.md) 中**独立状态**所需的完整公共头；开局初始核心与玩家个体 `seed` 属于第 0 回合初始态本身，统一记录于 `state(0)`，不在头文件存放。

`blue` 与 `red` 严格由原生记录中的位置决定：`blue` 固定为 `playerRecords[0]`，`red` 固定为 `playerRecords[1]`（原生 `PlayerRecord.data.team` 对双方可能相同，`Seat` 仅标记录制客户端席位，均不决定阵营归属）。玩家展示名无唯一性要求、不参与任何游戏规则，且文档不记录玩家账号 ID。`elo_score` 仅标识历史 1v1 段位水平（不等于 `rankPoint`），省略表示未知、`0` 仍保有明确含义，在读入、格式化与视图投影中无损透传，不参与任何合法性校验或战斗结算。

### 2. `source` 溯源与 `build` 版本兼容

- **`source`（必填）**：只能为 `game` 或 `simulator`，记录产生该对局战斗结果的执行后端——它只是**来源事实标记**，绝不用于反向决定后续命令的执行后端：
  - 原生游戏录像转换写 `source: game`；由当前执行产生的新战斗（如 `arena` 或本地推进）写入实际承担战斗计算的后端；格式化、截断与 `to GRBR` 往返在未重新执行战斗时保留原 `source`（合成 GRBR 在 `<MechcoreMatch><Source>` 中保存）；
  - 未标注来源的原生录像默认视为 `game`；缺少 `source`、未知取值或畸形 `<MechcoreMatch>` 元数据一律拒绝。若基于异后端前缀继续模拟后续新回合，产出文档的 `source` 改写为本次执行战斗的后端，不单独逐回合认证来源。
- **`build`（规范输出必填）**：与 [layout.md](layout.md#document-shape) 的 `game_build` 同义，新生成与原生转换对局均从仓库唯一版本钉 `GAME_VERSION` 读取，不在代码或文档中多处硬编码。若输入头文件省略 `build` 则视作当前程序版本；若输入 `build` 与当前程序不同（例如历史版本对局），所有 CLI 操作**告警后继续读入并按当前二进制的目录与规则运行**（格式化保留其声明的版本号，但投影与模拟可能与原历史版本产生差异）。

### 3. `tech_loadout` 编码规范

双方在局前选定的单位可选科技列表声明在 `blue`/`red.tech_loadout`（同一场对局双方可各自不同）。对局内任何 `upgrade_technology` 若指定了 `tech_loadout` 之外的科技，一律在校验时拒绝：

```yaml
# 紧凑形态（当阵营全为标量字段时折叠为单行 flow mapping）：
  blue: {name: blue, elo_score: 1850, tech_loadout: full}

# 展开形态（`mechcore format --expanded`）：
  blue:
    name: blue
    elo_score: 1850
    tech_loadout:
      fortress: [anti_air_barrage, launcher_overload, solid_shot, elite_marksman]
      marksman: [doubleshot, electromagnetic_shot, aerial_specialization, range_enhancement]
```

- **规范紧凑编码**：当本构建支持的标准 1v1 全科技均开启时写为字面量 `"full"`；任意真子集（含空集）编码为无 `=` 填充的 RFC 4648 §5 **Base64url 字符串**（共 `42` 个字符，对应按 `tech_id` 升序排列的 `241` 条标准 `(tech_id, unit_id)` 目录项，采用字节内低位优先 `i / 8`、`1 << (i % 8)` 的小端位序；末尾多余填充位必须为 `0`，全 `1` 位图必须写作 `"full"` 而禁止写字符串位图）。
- **展开形态**：`mechcore format --expanded` 按单位 ID 升序、每单位下按科技 ID 升序写为嵌套 YAML 映射以便审阅；读取器兼容上述两种形态。原生记录中不属于标准 1v1 赛制的单位（如 Death Knell `2001`、Experimental Death Knell `4001`）在导入时过滤丢弃。

### 4. 标准 1v1 赛制前提

除头文件显式写出的 `reinforce` 与 `max_round` 外，标准 1v1 赛制的其余原生对局常量属于本格式本身的前提，不在每场对局中重复罗列：

- 对局模式为 `VS_1_1`、`GameMode: Normal`、`ScoreMode: ReduceScore`，启用先遣队与建筑；
- 战斗时长 `FightTime = 120` 秒（含战前准备，逻辑上至多 `2360` 帧强制超时结算；MCFR 结算与尾帧快照解耦）；
- 部署倒计时 `DeployTime`（默认 `100` 秒）属于运行时对局协调配置，保存在旁路 [turn 文件](../mechcore/turn.md) 中，不属于对局头文件（原生导出使用标准 `100` 秒）。

此外，头文件**不设 `game_rules` 键**，本格式要求原生 `BattleInfo.gameRules` 为空——无特殊规则才能成立其余规范的全部裁剪前提（例如未启用「战场技能研发」`999917` 时 `research_queue` 永不可达且蓝图池恒为 `[1, 2, 3, 4, 5]`；未启用 `999903` 时装备耐久恒为 `-1`）。一旦原生回放携带任意非空游戏规则，转换器与校验器直接拒绝，避免在被打破的前提下误读局面。

---

## 第 0 回合开局与发牌核验

### 1. 第 0 回合语义（`state(0)` + `action(0)`）

- 在完整对局流中，第 0 回合由 [state.md](state.md#1-第-0-回合开局状态round-0) 定义的 `state(0)` 与 [action.md](action.md#choose_advance_team) 定义的 `action(0)` 构成：`state(0)` 记录双方开局 `reactor_core`、可选玩家 `seed`、各 4 组带显式 `core_modify`（须精确等于 [config/advance_teams.yaml](../../../config/advance_teams.yaml) 先遣队与专家修饰之和）的开局备选 `offers`，以及地图生成的对称初始 `constructions`。
- `action(0)` 中各方只能包含唯一一条 `choose_advance_team`，其 `index`、`name` 与 `specialist` 必须匹配 `state(0)` 对应阵营的 `offers[index]`；应用该决策后，按 `core_modify` 调整第 0 回合 `reactor_core` 并交付首回合编队与专家，生成 `state(1)`（标准 1v1 在开局前不解锁其他单位、不授予战场技能、双塔强化等级均为 `0`）。

### 2. 基于种子的发牌核验（`mechcore verify`）

```bash
mechcore verify <match.yaml>
```

对局中的开局发牌与每回合增援发牌均由头文件 `seed` 与 `map_id` 确定性生成，因此校验器无需原 GRBR 回放即可独立验证：

1. **全量结构预读**：首先用完整文档类型解析每一段的全部状态字段与动作操作数（包含发牌不直接使用的字段）；缺失必填键、类型错位、未知动作/状态字段、未知技能目标形式均拒绝。
2. **第 0 回合开局核验**：基于 `seed` 与 `map_id` 直接推算开局（含地图建筑分组与翻转标志），逐项核对 `state(0)` 中双方 4 组 `offers` 顺序及 `constructions` 坐标，并核对 `action(0)` 所选索引与组合一致（不搜寻替代流偏移，支持半途仅单方选队场景）。
3. **增援发牌核验**：
   - 当 `reinforce: true` 时：完整对局从第 1 回合起沿共享随机流逐回合连续前进（第 1 回合不发牌），比对后续每回合根级 `random_state`、净池变化 `reinforce_pool`、有序 `reinforce_offers` 及末尾 `refund` 数额；截断对局从首个捕获状态 `state(N)` 携带的检查点开始向后校验（不计入首段 `N` 本身从头派发的偏移，其相对偏移以该检查点为基准）。除流末尾尚无动作的回合外，凡存在发牌的已完成回合均须在对应 `action` 中存在双方的合法 `choose_reinforce_item` 选择（下标与卡名/退款名一致）。
   - 当 `reinforce: false` 时：校验全流不存在任何 `reinforce_offers`、非空 `reinforce_pool` 与 `choose_reinforce_item`，且共享 `random_state` 保持在开局初始断点不变（报告 0 增援回合、0 检查项）；开局选队与玩家自身随机流校验仍照常执行。

> 种子发牌校验属于条件一致性检查：在给定每回合前序场面与玩家选择下，确认对局未篡改发牌流与池结果，并不等价于验证回合内部署操作合法性或战斗过程。

---

## 跨回合演进、不变式与端到端验证

### 1. 回合间演进与覆盖率分类

相邻两个状态段并非紧邻帧，中间隔着当回合部署决策与一场完整战斗：

```text
state(R) ──[action(R)]──▶ 部署终态 ──[project → fight]──▶ state(R+1)
```

只有 `match` 文档能够检验“跨回合非战斗状态精确复现 + 战斗投影端到端吻合”的整场一致性。在无战斗的回合间（第 0 → 第 1 回合），所有差异直接视作确定性错误（无 `fight` 类别）；在常规回合间，`verify` 对 `state(R+1)` 展开逐叶对比并划为四类：

| 覆盖率类别 | 含义 |
| --- | --- |
| `equal` | 确定性规则预测与下一回合开局记录完全一致 |
| `fight` | 仅能由战斗或玩家抽发决定的字段（如 `reactor_core` 战损、战后存活 `units` 及 `units[].exp`、`constructions` 战毁、`standing` 中的护盾摧毁、部分依赖玩家随机流的交付），按当前规则仅做范围或非战斗基线验证 |
| `unequal` | 确定性字段预测与记录不一致（一旦出现即导致校验失败） |
| `unimplemented` | 尚无规则覆盖的叶子字段（即使数值恰好相同也显式标出并视为未过验） |

此外，`verify` 会将每一回合的开局态及部署终态各自投影为 `layout` 并经由布局编译器检验合法性（每常规回合共 2 份 `layout`），并同步执行战斗对比与终局结果一致性校验。

### 2. 顺序敏感初始化的投影告警边界

标准规范化序列与 12 阶段重排由 [action.md#3-标准-12-阶段规范化convert-与-format](action.md#3-标准-12-阶段规范化convert-与-format) 完整定义。因 `layout` 本身只表现部署终态而不记录回合内研究与侧翼移动的相对先后，若出现以下四类对原生对象初始化顺序敏感的场景，转换器与格式化器会保留可合法复现终态的历史动作序（仅合并等价的同区连续移动）并输出投影告警——`verify` 会显式**跳过该回合的战斗结果比对**，同时照常检验所有非战斗状态转移与其余回合：

1. **Phantom Ray 侧翼部署后研发 Stealth Cloak**：保留“先侧翼移动后研科技”的历史顺序，不将其提前为常规研究序，附带告警并跳过当回合战斗比对。
2. **携带 Aerial Specialist 且 Void Eye 处于侧翼传送（`travelling`）时研发 Aerial Mode**：原生逻辑在传送前或后研发会对生命修正留存不同结果，因状态不记录该内部修正，无论动作顺序如何均附带告警并跳过当回合战斗比对（若回合开启前已拥有 Aerial Mode 则无此告警）。
3. **Hound 处于侧翼传送时研发 Fire Extinguisher**：同理，先移或先研影响原生清地形控制器初始化状态，均附带告警并跳过当回合战斗比对（先前回合已拥有者除外）。
4. **Wraith 同时持有 Land Cruiser 与 Floating Artillery Array 且拥有 Aerial Specialist**：多武器槽位会沿用早先回合残留的军官修正而状态不保存该中间量；若回合开启前已持有 Land Cruiser，或同回合先研 Land Cruiser 再研 Floating Artillery Array，均附带告警并跳过当回合战斗比对（同回合先研 Floating Artillery Array 再研 Land Cruiser 则不触发该同回合告警）。

### 3. 跨回合不变式与补给守恒

单段自身合规外，任何良构 `match` 在跨越每一条回合接缝时均须满足以下单调性与守恒不变式：

| 跨回合不变式 |
| --- |
| `next_index.unit` 单调不减 |
| `next_index.contraption` 单调不减 |
| 已研发科技 `techs` 持续保留，且恒不越过头文件 `tech_loadout` 与单单位上限（4 项；War Factory / Abyss / Mountain 为 6 项） |
| 战场技能面板 `battle_skills` 仅在尾部追加扩展，永不重排或缩短 |
| `unlocked_units` 持续保留 |
| `tower_strengthen_levels` 各塔等级单调不减 |
| `blueprints` 持续保留，或被同链更高阶替换（产物军官随同替换） |
| 其余 `officers` 持续保留（军官多重集不丢失） |
| `constructions` 只减不增或保留，第 1 回合初值承自 `state(0)`（技能新建除外） |
| 除第 0 → 第 1 回合开局按 `core_modify` 增减外，`reactor_core` 只减不增 |

同时，`supply` 在任意两回合间严格服从**补给守恒恒等式**（标准 1v1 无战斗击杀赏金，故 `supply` 全程属确定性非 `fight` 字段）：

$$\text{supply}(R + 1) = \text{supply}(R) - \text{spent}(R) + \text{income}(R + 1)$$

其中 $\text{spent}(R)$ 为该回合所有 `action` 按 [action.md](action.md) 产生的净收支之和（第 0 回合起步为 `0`，开局无消费），$\text{income}(R + 1)$ 为通用回合基础收入加上第 $R+1$ 回合开启时的军官与存活穿戴装备收益、并扣除第 $R$ 回合 `rapid_resupply` 产生的跨回合欠款。

---

## 原生回放双向转换（GRBR 与 Match 互转）

`match` 与游戏原生 `.grbr` 回放之间支持双向无损转换：从原生录像提取对局文档（`convert --to match`），以及将完整对局写回为可直接在真机游戏或模拟器入口回放的合成 `.grbr`（`convert --to grbr`，Adapter `record_replay_round` 可直接拉起其中任意回合开战）。对于标准动作规范形式下的完整对局，满足 `match → grbr → match` 逐字节幂等（历史非规范序列写回再转回后会折叠为 `format` 标准形式）。

### 1. 原生回放转对局（`convert --to match`）

```bash
mechcore convert <replay.grbr> --to match <match.yaml> [--force]
```

- **纯静态脱机转换**：转换器仅解析单个原生 `.rep.grbr` 文件（支持客户端录制与在游戏内打开服务端回放转存的 `.rep.grbr`，不支持原始服务端 `.rep.bak`），不启动游戏进程也不运行战斗模拟。未携带合成源标记的原生录像默认记为 `source: game`。
- **拒绝原则**：以下情形一律明确报错拒绝而非猜测：非当前匹配版本、非 `VS_1_1` 模式、`Test` 测试局、携带任意非空 `BattleInfo.gameRules`、双边回合序列不共享或非连续、普通增援与单位增援开关彼此不一致（单布尔键 `reinforce` 无法表达）、出现本构建目录未定义实体或本格式不支持的动作。
- **完整前缀 vs 观战截断缓存**：
  - 完整对局从第 0 回合重放读出各阵营选队（原录只记小队，专家 ID 由第 1 回合 `officers` 反推）并验证随机流重建的 4 组备选；仅含开局而无任何常规部署回合的录像直接拒绝。
  - 观战缓存若只含第 0 回合占位与一段自第 $N$ 回合起的连续后缀，转换省略第 0 回合而以截断对局输出：直接采纳首段快照的预发牌共享随机流与累计增援池日志、验证首段发牌并还原检查点与玩家流；截断对局不再导回完整原生回放。
- **流吻合检查与原生终局**：原生中每回合随机状态均须与 `seed`（及各阵营 `state(0).seed`）逐步推进的流状态及增援池日志严格一致。因原生回放只在“下一回合开启快照”中记入上一场战斗结果，最后一回合打完后不会生成后续快照，故**从原生回放转换得到的 `match` 总是止于最后一回合的 `action` 段**（不含 `result` 段；锦标赛拉取流程会在转换后结合官方赛果接口补充并互验 `result` 段）。若原生存在 `concede`（`PAD_GiveUp`），无论录制中该阵营在该回合前后是否夹带多余操作，规范转换统一写为该阵营当回合唯一动作 `concede` 并立即收尾（存在后续回合或双方同时认输均拒绝）。

#### 转换重算项与服务端重建快照差异

各单段字段的重算细节已由 [state.md](state.md#从原生回放重建状态) 给出，此处汇总整场对局级的一致性重算与对账边界：

| 转换重建项 | 对局级核对逻辑 |
| --- | --- |
| 共享与玩家 `random_state` | 原生快照处于发牌与军官交付**之前**；转换核对所有前序随机字精确吻合后，在 `state` 中写入发牌/交付**之后**的断点 |
| `reinforce_pool` | 用初始池 + 真实发牌与选择记录校验并等价压缩原生 `poolOPs` 历史日志 |
| `supply` | 客户端快照在旧结余上叠加地图回合收入、装备/军官收益并扣减上一回合 `rapid_resupply` 欠款；服务端重建快照若已置 `IsSpecialSupply=true` 则直接采用其已含收入的补给 |
| `reinforce_offers` 末尾退款 | 原生列表不含拒绝项；转换依据地图预设或单位增援表在末尾补入 `{type: supply, name: refund_<amount>, cost: -<amount>}` |
| `battle_skills[].cooldown` & `standing` | 按上一回合实际释放与面板 `rangeItems` 重算；油田按 `round` 倒计时归零丢弃，空投护盾持续存在直到实体消亡，无关其他技能出现 `rangeItems` 一律拒绝 |
| `energy_tower_skills` 双源对账 | 客户端快照在后一回合滞后记录带延迟扣款的技能 `1` 标志；转换器强校验「第 $R$ 回合动作激活」与「第 $R+1$ 回合快照标志」严格一致（仅末回合无后继快照不对比），随后在每回合开局态中置空 |
| `equipments` & 军官交付 & `travelling` | `equipments` 扣除各编队佩戴后入库；军官按周期交付编队依 [rules/landing.md](../../rules/landing.md) 确定性落盘并标记 `delivered`（第 1 回合先遣队标 `joined`，其余标 `legacy`）；战斗前已清空所有 `travelling`，故转换后状态不含该标记 |

针对**服务端下载转换的重构快照**（`IsSpecialSupply = true`），还包含以下特定重建规则：
1. 其第 0 回合 `Match.random` 位于增援池初始化**之前**（客户端录像位于之后），转换器分别按对应节点核对；
2. 服务端第 0 回合动作省略小队 ID、增援选择省略下标（非零卡 ID 须落入该回合牌面，零视作拒绝），转换器结合牌面与首回合军官反推对应索引；
3. 开局若编队坐标写为 `(0, 0)` 或 `(int.MinValue, int.MinValue)`，代表按创建顺序使用游戏自动落点；二阶研发链在服务端快照中若同时残留高低两级，仅保留高阶；地图 `1012` 复用地图 `1011` 的棋盘与部署规则。

### 2. 对局写回原生回放（`convert --to grbr`）

```bash
mechcore convert <match.yaml> --to grbr <replay.grbr> [--force]
```

写回要求输入为**包含第 0 回合开局与选队决策的完整前缀**（截断对局不补造历史，直接拒绝）。对同一份 `match` 与当前构建版本，输出具备严格字节确定性。

#### 文件封装、`BattleRecord` 与 `<MechcoreMatch>` 元数据

- **封装格式**：沿用 [layout-replay.md](layout-replay.md#the-file) 的 `.NET BinaryFormatter` 流（根节点 `GameRiver.Replay` + UTF-8 BOM `BattleRecord` XML），其中 `battleID` 固定为 `battle`、`mapID` 为对局 `map_id`、`Seat` 固定为 `0`（本地录制回放）、`Version` 为 `GAME_VERSION` 末段构建号、`playerDatas` 为 `{id: 1, name: blue}` 与 `{id: 2, name: red}`。
- **`<MechcoreMatch>` 帧内元数据**：合成 `.grbr` 在 `<BattleRecord>` 下紧跟写入唯一一个元数据节点：

```xml
<MechcoreMatch>
  <Source>simulator</Source>
  <Reinforcements>true</Reinforcements>
</MechcoreMatch>
```

  `<Source>` 与 `<Reinforcements>` 均为必填：分别无损保留头文件 `source`（写回不篡改实际产出后端）与 `reinforce` 布尔值，本身并非原生 `gameRules` ID。反向读取时先读取该元数据再赋值规则：若缺失、不完整、重复、畸形或 `<Reinforcements>` 与原生双增援开关冲突，一律**直接报错拒绝**而不降级猜测。

- **`BattleInfo` 与 `PlayerRecord`**：`BattleInfo` 写入根级 `seed`（`SystemSeed`）、`map_id`、`MaxRound = max_round`、`VS_1_1`、标准 1v1 常量（开启先遣队与建筑、`DeployTime = 100`）、无 `gameRules` 与 `matchType`，且 `EnableReinforcement` 与 `EnableUnitReinforcement` 同时设为 `reinforce`。`PlayerRecord` 按先蓝后红顺序写入双方 `name`、来自 `state(0)` 的 `seed` 与 `reactor_core`（同时作为初始与上限核心）、全局统一回合补给参数，以及将 `tech_loadout` 展开到 `data.unitDatas`（`"full"` 展开全部标准科技，写回决策仍严格遵守每单位 4/6 项研发上限）。

#### 随机流断点与增援池日志重建

写回时根据种子重演并回填每一回合原生快照应处断点（原生快照位于当回合发牌与交付**之前**，区别于 `state` 位于**之后**）：

- **共享随机流（`Match.random`）**：第 0 回合写入根级 `seed` 初始化状态（发开局四选前）；对发牌回合写入该轮发牌**前**的流状态（并将牌面写入 `reinforceItems`），其紧邻下一回合写入发牌**后**状态，不发牌回合沿用此前流状态。当 `reinforce: false` 时无牌面、无刷新日志，共享流停在开局初始位置。
- **玩家随机流（`Player.random`）**：各自从 `state(0).blue/red.seed` 起步，每当早先回合有需要随机抽发装备的军官实际抽取时向前推进。
- **历史增援池日志（`poolOPs` 与 `RoundExcludeReinforce`）**：完整对局按 [rules/reinforcements.md](../../rules/reinforcements.md) 重建自开局起每一轮条件刷新移除与替补条目及被选走的不可重复卡牌日志（同回合双选固定按先蓝后红写入——因还原回合时会重新整理池集合，顺序不影响语义），并将 4 级战场技能在紧邻下一回合的排除表写入 `RoundExcludeReinforce`。

#### 回合快照逆向回退

因为 `state` 记录回合开启**之后**的状态而原生快照要求开启**之前**的状态，导出每回合快照时需做精确的“回合开启逆向回退”，随后立刻用转换器规则验证其能无差重开回原 `state`：

- **第 0 回合**：仅持初始核心与建筑、无部署前状态；动作仅包含单条 `PAD_ChooseAdvanceTeam`（写所选 offer 下标与小队，专家身份在转回时从第 1 回合 `officers` 还原），**不写 `PAD_FinishDeploy`**（真机第 0 回合无结束部署动作，写出会提前误结束第 1 回合部署）。
- **第 $R \ge 1$ 回合快照回退**：
  1. 撤回本回合开局军官交付的战场技能、未穿戴装备（含按玩家流抽取的那一件）与小队，并按交付队数回退 `unitIndex` 分配器；
  2. 从 `supply` 中扣回本回合基础收入与存活装备收益，并加上上一回合激活 `rapid_resupply` 产生的欠款（同时把上一回合激活过的 `rapid_resupply` 写进当前快照的 `energyTowerSkills`）；
  3. 还原面板冷却：上一回合释放过（含部署释放）的槽位标为 `active`（让开局重启冷却），其余未释放但冷却未归零的槽位将 `cooldown` 加 1；
  4. 合并穿戴与库存装备写入全量 `equipmentDatas`，二阶连锁蓝图并列写入对应产物军官，并将各槽位 `standing` 滞留物写回对应槽位的 `rangeItems`。

#### 决策映射、顺序与虚拟时钟

从第 1 回合起按原序列顺序逐方写出（非 `concede` 回合末尾追加 `PAD_FinishDeploy`），并遵守 [action.md](action.md#3-标准-12-阶段规范化convert-与-format) 的阶段与依赖顺序：

| 文档决策 | 原生写入动作 |
| --- | --- |
| `choose_reinforce_item` | `PAD_ChooseReinforceItem`（拒绝项写下标 `-1`） |
| `buy_unit` | `PAD_BuyUnit`（游戏先自动落地；若决策自带显式 `position`/`rotated`，紧随补一条 `PAD_MoveUnit` 移至目标位姿；买级由当前军官与能量塔动态决定，不作动作参数） |
| `move_unit` | 单编队 `PAD_MoveUnit` |
| `release_commander_skill` | `PAD_ReleaseCommanderSkill`（带槽位号与区域/对象目标） |
| `concede` | `PAD_GiveUp`（不附加 `PAD_FinishDeploy`） |
| 其余 9 类决策 | 各自同名 `PAD_*` 原生动作（`PAD_UpgradeUnit`、`PAD_UnlockUnit`、`PAD_UpgradeTechnology`、`PAD_ActiveBlueprint`、`PAD_ActiveEnergyTowerSkill`、`PAD_StrengthenTower`、`PAD_UseEquipment`、`PAD_ReleaseContraption`） |

原生时钟记录为 `Time = 0`，`LocalTime` 按步长 2 秒（游戏 `0.05` 秒精度刻度）从第 2 秒起递增；若操作数较多超出 `DeployTime`（100 秒）或第 0 回合 `PrepareTime`（30 秒），则等距压缩步长以容纳全部动作（转换回读完全忽略时间字段）。

#### 写回拒绝条件

以下情形均明确报错拒绝写出：
- 末回合决策之后仍带有下一回合 `state`（原生回放无法记录末回合之后的独立开启快照）；
- 缺少第 0 回合开局或任一方缺省玩家 `seed`（无法重建玩家随机流与 `<PlayerRecord><seed>`）；
- 发牌核验未通过（如启用增援却未选牌或随机断点不匹配）；
- 逆向回退后的快照无法通过转换开启规则精确复现原 `state`、决策落地发生碰撞或非法步骤、或存在面板无对应槽位的 `standing` 滞留物。

---

## 规范形式

### 1. 流序与段内字段顺序

| 对象 / 集合 | 规范顺序 |
| --- | --- |
| 文档流段序 | `match` →（完整局）`state(0)` → `action(0)` → `state(R)` → `action(R)` 升序递增 → 可选 `result` |
| 头文件根级键序 | `kind` → `build` → `source` → `reinforce` → `max_round` → `map_id` → `seed` → `blue` → `red` |
| 其余段前两个键 | 固定先写 `kind`，再写 `round`（方便流扫描快速定位） |
| `offers` | 保持发牌原始顺序（`choose_advance_team.index` 直接对其 0-based 寻址） |
| `constructions` | 按 `index` 升序 |
| `tech_loadout` | 紧凑形态 `"full"` 或 42 字符 Base64url 字符串；`--expanded` 按单位 ID 升序，每单位内部按科技 ID 升序 |

### 2. 统一 YAML 拼写规则（三条）

本格式族（`match`、`state`、`action`、`layout`）全系共用完全相同的三条排版规则，规则仅依据值的数据形状而非字段名，保证任意一份规范文档拥有**唯一确定字节序列**（反序列化时对块式或行内风格均等价兼容）：

1. **序列元素单行化**：任意序列中的元素以单行 flow 样式（`{...}` 或 `[...]`）写出；
2. **全标量容器行内化**：当一个映射或序列的直接子成员全为标量（字符串、数字、布尔、null）时，在所属键同一行以 flow 样式写出（如紧凑模式下的 `blue: {name: blue, tech_loadout: full}`）；
3. **混杂容器维持块式**：凡包含非标量嵌套子元素的映射或序列，均保持标准的 block 多行缩进样式（如开启 `--expanded` 展开 `tech_loadout` 时的 `blue` 块）。

> 对数以千计的动作条目、编队条目与 ID 列表，上述规则确保每个对象独立占一行，使行级 `diff` 清晰可控且便于整场浏览。

---

## 排除字段

| 原生字段 | 排除原因 |
| --- | --- |
| 完整单场战后结算表 | 各回合变动已由下一回合 `state` 体现；`result` 仅记最终胜负与核心血量（终局经验、残余建筑与装置无需结转） |
| `BattleInfo.BattleID` | 仅标识服务端记录主键，无任何规则读取 |
| `BattleRecord.Seat` | 仅记录采集回放的客户端席位，非对局客观属性 |
| `PlayerRecord.id`, `ad` | 账号身份字段，无任何机制读取 |
| `PlayerRecord.data.styleData` | 皮肤外观数据 |
| `PlayerRecord.data.team`, `isLeader`, `type` | 阵营已由数组位置唯一确定 |
| `PlayerRecord.data` 局内回合基础补给参数 | 所有标准对战地图共用同一套基础回合收入表 |
| `BattleRecord.reinforceItems` | 顶层占位空列表，实际牌面在逐回合 `matchDatas` 中 |
| `PAD_GiveUp.Time`, `LocalTime` | 认输顺序已由所处段与列表决定，无规则读取时间戳（写回均填合成步长） |
| `BattleRecord.Version`, `CreateTime` | 原生帧号与墙上时间；对局级语义事实由 `build` 与 `source` 表达 |
| `PAD_Undo`, `PAD_Redo`, `PAD_CancelReleaseCommanderSkill` | 文档只保留折叠后的净决策，写回原生不再回填撤回操作历史 |
| `PAD_MoveUnit.positionRecord`, `rotateRecord`, `superDeployRecord` | 仅重申移动前位姿，写回不填历史快照位姿 |
| `NewUnitData.RoundCount` | 读取或写回均无规则消费 |
| 标准 1v1 固定模式常量与非空 `BattleInfo.gameRules` | 模式常量属本格式隐含前提；非空自定义规则（如 `999917`、`999903`）破坏既有前提，直接整场拒绝而不进文档 |

---

## 待决问题

无。
