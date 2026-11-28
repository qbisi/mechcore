# 战斗文档规范（Fight）

## 范围

战斗文档（`kind: fight`）在单场战斗所起步的[布局](layout.md)之上直接叠加结算结果，完整包含起始布局、本次战斗使用的随机种子 `seed`、逐对象归位的战后结算字段、可选的录制 SQL 断言（`asserts`）以及可选的轨迹哈希钉住（`ticks` 与 `hash`）。任何模拟后端以该 `seed` 执行其起始布局，都必须得出与文档一致的结算结果。

```yaml
kind: fight
source: game
seed: 4242
round: 3
blue:
  core_damage: 0
  officers: [extended_range_marksman]
  units:
  - {name: marksman, index: 0, position: {x: 0, y: -50}, exp: 12/170/650}
  contraptions:
  - {name: interceptor, index: 0, position: {x: 5, y: -95}, retained: false}
  battle_skills:
  - {name: shield_airdrop, standing: {position: {x: -150, y: -150}}, retained: false}
red:
  core_damage: 37
  units:
  - {name: arclight, index: 0, position: {x: 0, y: -100}, exp: 0/40/750}
  battle_skills:
  - {name: sticky_oil_bomb, positions: [{x: -30, y: 150}, {x: 60, y: 150}], grid_rows: {2: [], 3: [], 4: []}}
ticks: 870
hash: 23:380d721bf2aa581622f521e4386160a0b5eedfb16ffed7b477b7e288c31534ef
```

上例表示：蓝方精准射手本场经验由 `12` 增至 `170`（满条 `650`），其拦截器被摧毁、早先回合空投的护盾被击破；红方弧光经验由 `0` 增至 `40`（满条 `750`），反应堆核心受损 `37` 点，本回合释放的黏性油弹战后仍完整留存第 `2`、`3`、`4` 号共三个圆形油斑；整场战斗历时 `870` 帧并在 23 号哈希剖面下精确钉住。

在四类核心文档体系中，战斗文档对应[对局文档](match.md#1-回合间演进与覆盖率分类)中的一次单场战斗演进边（`fight` 类迁移字段）：输入棋盘定义由 [`layout.md`](layout.md) 规定（本文不重复展开），轨迹内容哈希由 [`mcfr.md`](../mcfr/mcfr.md#the-hash) 定义。

战斗文档只声明“战斗产出了什么结果”，不记录每帧的演化过程；逐帧状态保存在 MCFR 录制文件中，本文档仅通过 `hash` 锚定该轨迹。它与 [`layout.md`](layout.md) 一同专注于**单回合战斗演算的完备性、确定性以及与游戏演算的一致性**；对于少数因纯终态投影无法与原生历史操作顺序保持一致的行为，规范统一按确定性规则做化简（详见[表达边界与未覆盖范围](layout.md#表达边界与未覆盖范围)）；至于战前核心血量存量与整局胜负判定，则归属 `state`/`match`，战斗文档不承载战前 `reactor_core`。无论 `source` 为何值，一份结构合法的战斗文档都是有效文档；但 `source: simulator` 仅代表模拟器自身的计算产物，**永远不可作为回归钉住（pin）夹具**——关于回归夹具的准入条件由 [`tests/README.md`](../../../tests/README.md) 统一规定。

---

## 与布局的投影关系

战斗文档是在布局对象上原位写入结算字段的扩展格式，存在确定的投影还原映射：

```text
layout = project(fight)
```

- **投影规则**：剥离根级 `source`、`asserts`、`ticks`、`hash`、双方阵营的 `core_damage` 以及各对象的战后结果字段（`retained`、释放条目的 `grid_rows`）；同时将每支编队的 `exp: begin/final/max` 折叠回 `begin/max`（若 `begin == 0` 则省略整个 `exp` 字段）。其余所有布局字段（包括 `seed`）原样保留。规范形式下战斗文档的投影结果仍是规范形式的布局文档。
- **双向严格校验**：若 `project(fight)` 无法通过布局合法性校验，则整份战斗文档无论结算为何一律拒绝；反过来，`kind: layout` 文档若夹带任何战斗结果字段，则直接按布局未知字段拒绝（仅凭顶层 `kind` 即可在解析前区隔两类文档）。
- **`seed` 必填语义**：在 `layout` 中 `seed` 为可选默认参数，但在 `fight` 中 **`seed` 必填**——因为每一份战斗结算结果都是特定种子下的结果。
- **场内新生对象不入结果**：战斗过程中由技能或机制临时生成的单位、护盾或地形区域在初始布局中不存在条目，因此战斗文档也不对其记录独立结果（其行为已纳入 `hash` 覆盖）。

---

## 根级字段

除沿用自布局的根级字段（`game_build`、`map_id`、`seed`、`round`、`blue`、`red`，其中 `seed` 必填）外，战斗文档额外定义以下根级字段：

| 字段 | 要求 | 说明 |
| --- | --- | --- |
| `kind` | 必填常量 | 固定写作 `fight`（区别于 `layout`）。 |
| `seed` | 必填 `i32` | 本场战斗实际使用的非零随机种子。 |
| `source` | 必填枚举 | 执行并产出本结果的一方：`game` 或 `simulator`。 |
| `asserts` | 可选列表 | 对本次战斗录制的 SQL 断言及真机回答行（无断言时省略）。 |
| `ticks` | 可选正整数 | 本场战斗的逻辑总帧数（对应 MCFR 录制的 `tick_count`，须 `>= 1`）。 |
| `hash` | 可选字符串 | `<profile>:<result>` 形式的轨迹内容哈希，即 MCFR 中的 `hash_profile` 与 64 位小写十六进制 `result_hash`（例如 `23:e4ed…`）。 |

### 执行来源 `source` 与轨迹校验

- **`game`**：由真实游戏经适配器执行并录制得到。它是游戏实际行为的**权威证据**，也是回归测试夹具的唯一合法来源。由于游戏分别从原始回放回合与从[合成原生回放](layout.md#合成原生回放grbr)跑同一场战斗时每帧完全一致（含内容哈希），这两种来源均标记为 `game`（若二者产生分歧属于适配器缺陷而非第二种“游戏”语义）。
- **`simulator`**：由本仓库模拟器计算得出，用于记录模拟输出与对齐检查，不构成关于游戏行为的证据。

`source` 与对应 MCFR 录制的元数据 `producer`（[`mcfr.md#file-metadata`](../mcfr/mcfr.md#file-metadata)）保持一致；`hash` 本身不区分两者，因为对齐的场景下双端对同一场战斗生成相同的时间线。

校验一份战斗文档时（见 [`verify`](../mechcore/cli.md#verify)），校验器以文档声明的 `seed` 运行 `project(fight)`，随后逐项比对双方 `core_damage`、每个对象的结果字段、`asserts`（若存在）以及 `ticks` 与 `hash`（若存在）。

`ticks` 与 `hash` 必须**成对出现或同时省略**：省略二者意味着仅检验战后结算与断言而不钉住逐帧轨迹；同时写出时，校验器要求 `ticks >= 1`、`<profile>` 为十进制剖面编号且 `<result>` 恰好为 64 位小写十六进制字符（剖面兼容性按 [`mcfr.md#the-hash`](../mcfr/mcfr.md#the-hash) 检查）。

---

## 阵营与对象结果字段

### 阵营级字段：`core_damage`

每个阵营（`blue` / `red`）在自身布局字段之前必须显式声明 `core_damage`：一个非负整数（**包含 `0`**），表示本场战斗中敌方对己方反应堆核心造成的生命扣减量（计算规则见 [`reactor_damage.md`](../../rules/reactor_damage.md)）。

### 对象级结果字段总表

| 所属对象 | 新增结果字段 | 含义 |
| --- | --- | --- |
| `units` | `exp: begin/final/max` | 编队战前经验、战后经验与当前等级满槽上限 |
| `contraptions` | `retained` | 装置在战斗结束时是否依然存留 |
| `battle_skills`（滞留 `shield_airdrop`） | `retained` | 早先遗留的空投护盾在战斗结束时是否依然存留 |
| `battle_skills`（本回合释放 `shield_airdrop`） | `retained` | 本回合空投的护盾在战斗结束时是否依然存留 |
| `battle_skills`（本回合释放 `sticky_oil_bomb`） | `retained`、`grid_rows` | 本回合释放的油弹战后是否仍有区域存留，以及具体存留油斑与裁切掩码 |

### 经验（`exp`）

`exp` 将布局中的 `current/maximum` 扩展为三段格式 `begin/final/max`：

- `begin` 与 `max` 来自起始布局；即使 `begin == 0`（对应投影后布局省略 `exp`），`max` 仍须严格等于[单位经验索引](../../rules/unit_experience.md)中该单位当前等级（`1..=9`）的满槽值。
- 满足不变式 **`0 <= begin <= final <= max`**：战斗只会增加不会扣除经验；经验槽已满的编队不再分摊战场经验（见 [`unit_experience.md#what-a-full-bar-means`](../../rules/unit_experience.md#what-a-full-bar-means)），因此战斗中攒满时即止步于 `max`；战后各编队经验按整数截断写入 `final`（见 [`unit_experience.md#what-a-kill-hands-out`](../../rules/unit_experience.md#what-a-kill-hands-out)）。
- 当且仅当 `begin == 0 && final == 0` 时，规范形式省略 `exp` 字段。

### 战后存留（`retained` 与 `grid_rows`）

`retained` 默认值为 `true`，仅在结果为 **`false`** 时显式写出（表示该对象在战斗中被摧毁、拦截或彻底消除）。具体承载规则如下：

- **所有装置（`contraptions`）**：无论是 `shield`、`interceptor` 还是 `missile`，均允许携带 `retained`。
- **战场技能（`battle_skills`）**：仅当技能造物有机会跨越本回合继续留存时，才允许携带存留结果；其余全部不携带结果字段：
  - **滞留 `shield_airdrop`**：携带 `retained`（空投护盾在被击破前持续跨回合存在，存留为 `true` 即作为下一回合的滞留条目）。
  - **滞留 `sticky_oil_bomb`**：**不携带任何结果字段**（油田仅持续两回合，已处于滞留态的油田这是其第二且最后一个回合，战后绝不留存至下回合）。
  - **本回合释放 `shield_airdrop`**：携带 `retained`（本回合释放的护盾若在战中被击破，记为 `retained: false`）。
  - **本回合释放 `sticky_oil_bomb`**：按滞留油田完全相同的规则记录战后存留（以 `positions` 作为 `[p0, p1]` 控制点）：
    - `grid_rows` 遵循 [`layout.md`](layout.md#滞留条目) 中的 `0..=6` 七点映射与 `12 × 12` 位掩码格式；缺省 `grid_rows` 表示 7 个圆形油斑全部完整存留；
    - `retained: false` 表示 7 个油斑无一存留（例如全部被提前清除）；**`retained: false` 与非空 `grid_rows` 严格互斥**；
    - 凡战后仍有存留的释放结果，其描述的形态即下一回合 `layout` 中对应的滞留油田条目。
  - **其余所有本回合释放的战场技能**：**不携带任何结果字段**（效果在本回合结束前即消散）。

凡在非法对象上声明 `retained`、或在“本回合释放的 `sticky_oil_bomb`”以外的条目外层声明 `grid_rows`（滞留油田自身的 `grid_rows` 留在其 `standing` 载荷内部），一律拒绝。

---

## 录制断言（`asserts`）

战后结算字段说明了战斗交接给下一回合的状态，内容哈希钉住了精确轨迹，但二者均未直白阐明“某份测试夹具究竟旨在证明哪条机制结论”。`asserts` 用于显式声明并固化对战斗录制过程的机制断言：

```yaml
asserts:
- sql: |-
    SELECT tick, target__kind, target__id, amount
    FROM events
    WHERE type = 'damage' AND source__kind IS NULL AND tick = 63
    ORDER BY ordinal
  rows:
  - [63, building, 5, 3000]
  - [63, building, 6, 1112]
```

| 字段 | 含义 |
| --- | --- |
| `sql` | 查询语句，语法与表结构同 [`mechcore query`](../mechcore/cli.md#query) 一致 |
| `rows` | 真机录制对该查询返回的结果行（逐行列表，单元格类型同 `query` 输出）；在真机实录前省略 |

校验断言时，校验器对被测战斗产生的录制执行同一条 `sql` 并严格比对返回行。断言既可按 `tick` 精确绑定（如上例），也可以只聚焦事件因果顺序或数值总和（不选入 `tick`），从而与轨迹哈希形成互补：当不相关的规则调整导致全局 `hash` 变动时，未受影响的机制断言依然屹立，具体失败的断言会直接点出哪条机制性质发生了偏移。

`rows` 只能由真机录制写入（通过 `verify --backend game --update` 采集填充），手写断言只提供 `sql` 而不预填 `rows`，在未经真机实录回填前不会标记为已通过校验。

---

## 规范形式

战斗文档处于规范形式，当且仅当其投影 `project(fight)` 处于布局规范形式，所有结果字段精确附着在其所属对象的规范位置，且满足以下排序与省略规则：

- **根级字段顺序**：`kind` → `source` → `game_build`（与 `GAME_VERSION` 不同时写入）→ `map_id` → `seed` → `round` → `blue` → `red` → `asserts` → `ticks` → `hash`；
- **阵营级顺序与默认值**：`core_damage` 必须位于阵营内所有布局字段之前，且恒显式写出（包含 `0`）；
- **编队 `exp`**：位于布局 `exp` 原本位置，当且仅当 `begin == 0 && final == 0` 时省略；
- **条目 `grid_rows` 与 `retained`**：追加于该条目的布局字段之后，`grid_rows` 先于 `retained`；`retained: true` 省略；释放条目的 `grid_rows` 若表达 7 个圆点全部完整存留则省略（与滞留油田规范一致）。

在 YAML 拼写规范上，内嵌布局对象（含其结算结果字段）沿用 [`match.md#规范形式`](match.md#规范形式) 的统一三条排版规则，使每个编队、装置或技能条目保持单行 `flow` 写法；`asserts` 保持声明顺序且先 `sql` 后 `rows`：`sql` 使用字面块（`|` / `|-`）精确保留换行与前导空格（仅当包含会被字面块规范化改写的控制字符时改用转义引号字符串），每个 `rows` 结果行写作单行 flow 序列，已录制但结果为空集时写 `rows: []`，尚未录制时省略 `rows`。

---

## 排除字段

| 排除内容 | 排除原因 |
| --- | --- |
| `winner` | 单场战斗可能平局；胜负由各阵营核心剩余血量与 `core_damage` 决定 |
| 战前起始 `reactor_core` | 战斗文档与布局专注于单回合战斗演算与 `core_damage` 结算，不独立判定对局终结；核心血量归属 `state` 与 `match` |
| 单位战后剩余生命值 | 幸存单位下一回合开局全部满血恢复，战后残血不向后传递，且完整轨迹已受 `hash` 保护 |
| 累计伤害、击杀统计等计数器 | 已受 `hash` 覆盖，且后续回合规则不读取这些统计量 |
| 录制文件中的逐帧状态流 | 战斗文档只表征结果；过程路径由录制文件与 `hash` 承载 |
| 仪表盘通道（instrument channels） | 用于解释机制调试过程而非战斗规则输出，不进入哈希（见 [`mcfr.md`](../mcfr/mcfr.md#instrument-channels)） |
| 录制本身的构建信息 | 布局自带 `game_build` 声明依据版本，读取器对版本不符者直接拒绝 |
| 场内动态生成对象的结果 | 起始布局中不存在该条目，无处附着结果字段 |

---

## 待决问题

无。
