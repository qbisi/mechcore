# 单题求解规范（Solve）

## 范围

本契约定义 `mechcore` 面向 Bot 的单题求解协议与命令行接口：

- `mechcore solve` 命令行入口、输入边界与标准输出结构；
- 宿主（`mechcore solve` 与 `mechcore arena`）管理 Bot 子进程的进程模型与生命周期；
- 宿主向 Bot 发起的双向 JSON-RPC 2.0 会话握手（`initialize`）与单回合求解请求/响应（`solve`）；
- 纯 JSON 完整展开态的 `problem` 数据结构；
- 单题求解的动作校验、错误分类及线路日志（`--log`）格式。

与兄弟契约的分工：

- [`rpc.md`](rpc.md)：定义底层双向 JSON-RPC 2.0 帧协议、并发队列、取消通知（`$/cancel`）、无状态引擎反向方法（`convert`、`act`、`simulate`）以及 JSON-RPC 错误码分类；
- [`arena.md`](arena.md)：将本契约的 `solve` 请求连续分发到整场对局的两个常驻 Bot，并规定对局级超时/违规裁决与终局文档输出；
- [`state.md`](../document/state.md)、[`action.md`](../document/action.md) 与 [`match.md`](../document/match.md)：定义状态文档、14 种决策、对局多段流及完整展开格式（`tech_loadout` 与 `reinforce_pool`）；
- [`cli.md`](cli.md)：定义通用错误分类、退出码与其余命令表面。

明确边界：

- **仅求解回合开局态**：本契约中的 `problem.state` 严格限定为某一回合的**开局态**（`round: 0` 开局选队态，或 `round >= 1` 尚未执行本回合任何决策的开局态）。从已执行部分本回合决策的部署中间态出发、携带动作前缀或额外约束的求解问题不在本契约范围内（见[待决问题](#待决问题)）；
- **无独立提交方法**：Bot 通过回复 `solve` 请求对应的 JSON-RPC Response 提交最终决策序列，协议不设单独的 `commit` RPC 方法。

---

## 命令行入口

```sh
mechcore solve <match.yaml|state.yaml> --side blue|red --bot '<command>' \
  [--round <n>] [--timeout <seconds>] [--parallel <n>] [--log <file.jsonl>]
```

各操作数与选项语义：

| 参数 / 选项 | 要求 | 说明 |
| --- | --- | --- |
| `<file>` | 必填 | 只读输入文件，按内容识别文档类型，仅接受 `kind: match` 或独立 `kind: state`；其他类型按 `refused` 拒绝。命令不修改 `<file>`，也不读取或生成 `.turn` 文件。 |
| `--side blue\|red` | 必填 | 指定为哪一阵营求解本回合决策。 |
| `--bot <command>` | 必填 | 非空 Shell 命令字符串，经 `sh -c` 启动单个 Bot 子进程。 |
| `--round <n>` | 可选整数 | 仅在输入为 `kind: match` 时合法（对 `kind: state` 传入按 `usage` 拒绝）。指定提取第 `n` 回合的开局态（完整对局支持 `0`，截断对局支持流内存在的任意可玩回合）；省略时取流尾尚待决策的回合开局态。缺席回合、已终局状态（任一方 `reactor_core <= 0`、已认输或已带 `result` 闭合）均按 `refused` 拒绝。 |
| `--timeout <seconds>` | 可选浮点数 | 有限非负墙上时钟秒数，默认 `30.0`；`0` 表示不限时（在线上映射为 `limits.timeout: null`）。分别约束 `initialize` 握手与本次 `solve` 求解（含期间全部反向 RPC 试算耗时）。 |
| `--parallel <n>` | 可选正整数 | 本次求解期间允许 Bot 反向并发执行 `act` 与 `simulate` 的最大计算槽位数；默认为进程可用逻辑 CPU 核心数（无法获取时为 `1`）。 |
| `--log <file.jsonl>` | 可选路径 | 新建 JSON Lines 线路日志文件（目标文件已存在时拒绝），按时间顺序记录宿主与 Bot 之间的全部报文。 |

**标准输出**：求解成功且返回的 `actions` 通过规则校验后，以退出码 `0` 在 `stdout` 输出单个结果对象（支持 `--format json|yaml`，默认 `json`）：

```json
{
  "schema": "mechcore.solve-result",
  "bot": "game-ai",
  "side": "blue",
  "round": 3,
  "actions": [
    { "type": "choose_reinforce_item", "index": 4, "name": "refund_400" },
    { "type": "buy_unit", "name": "crawler", "position": { "x": -100, "y": -150 } }
  ],
  "meta": {
    "value": 0.62,
    "trials": 48
  }
}
```

其中 `meta` 仅在 Bot 响应中携带非空 `meta` 时写出，否则省略；Bot 的运行日志与诊断输出始终走 `stderr`。

---

## 进程模型与生命周期

`mechcore solve` 与 `mechcore arena` 使用同一套子进程管理模型：

1. **进程组隔离启动**：宿主以 `sh -c "<command>"` 启动单个常驻子进程；在 Unix 平台上调用 `process_group(0)` 使子进程成为独立进程组组长，以管道接管 `stdin`/`stdout` 作为双向 JSON-RPC 2.0 通道，`stderr` 继承宿主标准错误输出。
2. **单进程全生命周期**：不再为查询 Bot 名称单独派生短生命周期进程；进程启动后先完成一次 `initialize` 握手，再接收一次（`mechcore solve`）或连续多回合（`mechcore arena`）的 `solve` 请求，期间可通过同一管道反向调用 [`rpc.md`](rpc.md) 定义的 `act`、`simulate` 与 `$/cancel`。
3. **退出与进程组清理**：
   - 正常结束时，宿主关闭 Bot 的 `stdin`（发送 EOF）并最多等待 **2 秒**让其自行退出；
   - 若超过 2 秒仍未退出，或宿主因超时、断连、协议错误提前终止，宿主向该 Bot 的整个进程组发送 `SIGKILL`（`kill(-pgid, SIGKILL)`），一并回收 Shell 及 Bot 派生的所有后代进程。

---

## 会话握手（`initialize`）

子进程启动后，宿主首先发送 `initialize` 请求，向 Bot 宣告引擎构建版本与可用的反向 RPC 能力，并获取 Bot 名称：

```json
// Host -> Bot (Request)
{
  "jsonrpc": "2.0",
  "id": "init",
  "method": "initialize",
  "params": {
    "protocol": "mechcore-rpc",
    "game_build": "<GAME_VERSION>",
    "capabilities": {
      "methods": ["convert", "act", "simulate"],
      "parallel": 8
    }
  }
}

// Bot -> Host (Response)
{
  "jsonrpc": "2.0",
  "id": "init",
  "result": {
    "name": "game-ai"
  }
}
```

校验规则：

- `result.name` 必须为非空 UTF-8 字符串（首尾空白去除后不为空）且不得包含任何控制字符；
- 握手阶段不允许 Bot 发起 `act` 或 `simulate` 反向调用；
- 若握手超时、子进程提前退出、返回非法 JSON / JSON-RPC `error` 或名称非法，宿主立即终止会话并以 `failed`（退出码 `5`）失败。

---

## 问题表达（`problem`）

`problem` 是中立于具体阵营的不可变局面描述（同一回合的蓝红双方收到的 `problem` 完全一致），全部以**纯 JSON 完整展开格式**表达，不在 JSON 字符串中嵌套多文档 YAML：

```json
{
  "state": {
    "kind": "state",
    "build": "<GAME_VERSION>",
    "reinforce": true,
    "max_round": 40,
    "map_id": 1001,
    "seed": 2038621361,
    "round": 2,
    "random_state": [12917254667871916365, 9250091961005733138, 14288931841979358149, 16091592835658840205],
    "reinforce_pool": ["supply_specialist", "laser_sights", "extended_range_marksman"],
    "reinforce_offers": [
      { "type": "officer", "name": "cost_control_specialist", "cost": 0 },
      { "type": "equipment", "name": "photon_coating", "cost": 50 },
      { "type": "supply", "name": "refund_50", "cost": -50 }
    ],
    "blue": {
      "tech_loadout": {
        "arclight": ["armor_enhancement", "electromagnetic_shot", "elite_marksman", "range_enhancement"],
        "crawler": ["acid_explosion", "loose_formation", "mechanical_replication", "subterranean_blitz"]
      },
      "random_state": [11398329854704076253, 2588188838420805615, 8713068559080739648, 8599063887646576635],
      "reactor_core": 4120,
      "supply": 400,
      "unlocked_units": ["crawler", "arclight"],
      "next_index": { "unit": 5, "contraption": 0 },
      "officers": ["supply_specialist"],
      "towers": [
        { "name": "energy_tower", "index": 0, "position": { "x": -140, "y": -170 }, "size": { "width": 20, "depth": 20 }, "level": 0 },
        { "name": "research_center", "index": 1, "position": { "x": 140, "y": -170 }, "size": { "width": 20, "depth": 20 }, "level": 0 }
      ],
      "units": [
        { "name": "crawler", "index": 0, "value": 100, "position": { "x": -140, "y": -135 }, "rotated": true, "size": { "width": 20, "depth": 50 } }
      ]
    },
    "red": {
      "tech_loadout": { ... },
      "reactor_core": 4500,
      "supply": 400,
      ...
    }
  },
  "history": [
    {
      "round": 0,
      "state": { "kind": "state", "round": 0, "blue": { ... }, "red": { ... } },
      "actions": {
        "blue": [{ "type": "choose_advance_team", "index": 1, "name": "vortex-fire_badger", "specialist": "giant_specialist" }],
        "red":  [{ "type": "choose_advance_team", "index": 0, "name": "crawler-tarantula", "specialist": "supply_specialist" }]
      }
    },
    {
      "round": 1,
      "state": { "kind": "state", "round": 1, "reinforce_pool": [...], "blue": { ... }, "red": { ... } },
      "actions": { "blue": [...], "red": [...] }
    }
  ],
  "ref": "solve:r2"
}
```

### 字段语义与展开约定

| 字段 | 要求 | 说明 |
| --- | --- | --- |
| `state` | 必填 | 待求解回合的**独立开局态** JSON 对象（遵循 [state.md](../document/state.md#独立状态的-mdp-完备性)），包含根级规则头（`build`、`reinforce`、`max_round`、`map_id`、`seed`）与双边 `tech_loadout`，不包含仅用于溯源展示的 `source`、`name`、`elo_score`。`round: 0` 时包含双方 `reactor_core`、可选玩家 `seed`、`offers` 与 `constructions`。 |
| `history` | 可选 | 当输入来自 `match` 文档（含 `arena`）且存在早于当前回合的已完成回合时提供；按 `round` 升序排列，每项为 `{"round", "state", "actions"}`，其中 `state` 为该回合的段内开局态（省略已在 `problem.state` 中声明的公共规则头与 `tech_loadout`），`actions` 为该回合双方已结算的规范净决策（`{"blue": [...], "red": [...]}`）。当输入为独立 `state` 文档或尚无前序回合时省略。 |
| `ref` | 可选 | 指向宿主内存中已预解析不可变局面的只读句柄，仅在本次 `solve` 请求完成或超时前有效。在本次 `solve` 期间反向调用 `act` 或 `simulate` 时，传入 `{"ref": problem.ref}` 与内联传入 `{"state": problem.state}` 语义严格等价。 |

**完整展开格式约束**：

- **`tech_loadout` 恒展开**：`problem.state.blue.tech_loadout` 与 `problem.state.red.tech_loadout` 一律写为 `{unit: [tech, ...]}` 映射（外层按单位 ID 升序、内层按科技 ID 升序），即使属于全科技配置也完整展开，不使用 `"full"` 或 Base64url 位图；
- **`reinforce_pool` 恒展开**：当 `reinforce: true` 时，`problem.state.reinforce_pool`（含 `round: 0` 开局态的初始普通增援池与 `round >= 1` 常规回合的当前活跃池）以及 `problem.history` 中各回合 `state.reinforce_pool` 一律写为完整卡牌名数组 `[card_name, ...]`（按卡牌 ID 升序，对应 [state.md](../document/state.md#2-共享随机流与普通增援池) 定义的展开形态），不使用 `{removed, added}` 差值；当 `reinforce: false` 时省略；
- **空间实体恒展开**：`problem.state`（与 `problem.history`）中的 `round >= 1` 阵营改用 `towers` 实体列表替代 `tower_strengthen_levels`，且所有具占位体积的存活实体（`round: 0`/`round >= 1` 的 `constructions[]`、存活 `units[]`、`contraptions[]` 中的 `interceptor` 与 `towers[]`）均直接附带当前坐标系下的实际占宽占深 `size: {width, depth}`（参见 [state.md](../document/state.md#4-展开态空间实体与-size-字段)）。
- **信息隔离不变量**：`problem` 仅包含当前回合开局态与已完成的历史回合，绝不暴露对手在本回合的任何未结算或已提交决策。

---

## 求解请求与响应（`solve`）

### 1. 请求（Host → Bot）

```json
{
  "jsonrpc": "2.0",
  "id": "solve-2-blue",
  "method": "solve",
  "params": {
    "side": "blue",
    "problem": { "state": { ... }, "history": [ ... ], "ref": "solve:r2" },
    "limits": {
      "timeout": 30.0,
      "parallel": 8
    }
  }
}
```

- `params.side`：必填，`"blue"` 或 `"red"`，声明 Bot 本次求解的己方阵营；
- `params.problem`：必填，符合[问题表达](#问题表达problem)的不可变展开态问题对象（回合号由 `problem.state.round` 唯一确定，不在外层重复声明）；
- `params.limits`：必填对象：
  - `timeout`：正浮点数墙上时钟预算（秒，含反向 RPC 耗时），或 `null` 表示不限时；
  - `parallel`：正整数，宿主为本轮分配的反向 `act`/`simulate` 最大并发槽位数。

### 2. 求解期间的反向 RPC 调用

在 Bot 写回对应 `id` 的 `solve` Response 之前，Bot 可在同一对 `stdin`/`stdout` 管道上任意发起单条或批量 [`act`](rpc.md#act请求)、[`simulate`](rpc.md#simulate请求) 请求及 `$/cancel` 通知（例如使用 `act` 干跑校验候选动作序列合法性，或使用 `simulate` 评估备选布局）。

宿主一旦收到匹配的 `solve` Response 或触发 `limits.timeout` 截止时间：

1. 立即取消该次 `solve` 尚未完成的全部反向计算任务（排队项直接丢弃，运行中线程置位取消标志协作式退出）；
2. 使本次的 `problem.ref` 失效；
3. 不再为已结束的 `solve` 写回过期的反向试算响应。

### 3. 响应（Bot → Host）

```json
{
  "jsonrpc": "2.0",
  "id": "solve-2-blue",
  "result": {
    "actions": [
      { "type": "choose_reinforce_item", "index": 2, "name": "refund_50" },
      { "type": "buy_unit", "name": "crawler", "position": { "x": -100, "y": -150 } }
    ],
    "meta": {
      "value": 0.62,
      "trials": 48
    }
  }
}
```

- `result.actions`（必填数组）：本方在本回合执行的决策序列，严格遵循 [action.md](../document/action.md) 语法与顺序校验规则（`round: 0` 须为恰好一条匹配的 `choose_advance_team`；`round >= 1` 允许为空序列 `[]`、合法部署序列或包含 `{"type": "concede"}`）。宿主使用与无状态 `rpc` `act` 完全相同的校验路径核验 `result.actions`；
- `result.meta`（可选 JSON 对象）：由 Bot 自由附加的诊断或评估元数据。宿主不读取其内容进行任何规则判定，也不将其写入对局文档，但会原样记录到 `--log` 并在 `mechcore solve` 的 `stdout` 结果中透传；若 `meta` 既非省略/`null` 也不是 JSON 对象，按协议错误处理。

---

## 校验、错误分类与线路日志（`--log`）

### 1. `mechcore solve` 错误分类

`mechcore solve` 遵循 [`cli.md`](cli.md#error-taxonomy) 的标准错误对象与退出码，所有失败均向 `stderr` 输出一条 `mechcore.error` JSON 且保持 `stdout` 为空：

| 失败场景 | 错误 `kind` | 退出码 | 说明 |
| --- | --- | --- | --- |
| 非法命令行参数、空 `--bot`、非法 `--timeout`/`--parallel`、对 `state` 传入 `--round` | `usage` | `2` | 调用方修正命令行 |
| 输入文件不是 `match`/`state`、目标回合不存在或已终局、输入 `state` 为部署中间态 | `refused` | `3` | 输入超出本契约接纳范围 |
| Bot 返回的 `result.actions` 未通过动作解析或规则校验 | `refused` | `3` | `details` 携带 `{"side", "action_index"?}` 指出首个违规步骤 |
| 日志文件已存在、子进程启动失败、`initialize` 失败或超时、`solve` 超时、Bot 断连/提前退出、畸形 JSON/非 UTF-8/协议错误、Bot 返回 JSON-RPC `error` | `failed` | `5` | 环境、进程或协议级失败 |

> `mechcore arena` 在部署回合遇到超时或非法 `actions` 时采用按 `concede` 裁决策略，详见 [`arena.md`](arena.md#裁决与错误分类)。

### 2. 线路日志格式（`--log <file.jsonl>`）

启用 `--log` 时，宿主新建目标文件（若已存在则以 `failed` 拒绝，绝不覆盖），按事件发生的时间顺序每行写入一条 JSON 对象：

```jsonl
{"side":"blue","direction":"host_to_bot","phase":"initialize","message":{"jsonrpc":"2.0","id":"init","method":"initialize","params":{"protocol":"mechcore-rpc","game_build":"<GAME_VERSION>","capabilities":{"methods":["convert","act","simulate"],"parallel":8}}}}
{"side":"blue","direction":"bot_to_host","phase":"initialize","message":{"jsonrpc":"2.0","id":"init","result":{"name":"game-ai"}}}
{"side":"blue","direction":"host_to_bot","phase":"solve","message":{"jsonrpc":"2.0","id":"solve-1-blue","method":"solve","params":{...}}}
{"side":"blue","direction":"bot_to_host","phase":"solve","message":{"jsonrpc":"2.0","id":"solve-1-blue","result":{"actions":[]}}}
```

记录规则：

- 每行包含 `side`（`"blue"` | `"red"`）、`direction`（`"host_to_bot"` | `"bot_to_host"`）、`phase`（`"initialize"` | `"solve"`）与 `message`；
- 合法 JSON 报文（含批量数组）以结构化 JSON 直接写入 `message`；畸形 JSON 文本以原始字符串写入；非 UTF-8 字节流写作 `{"bytes": [<u8>, ...]}`；
- 每次收发即时落盘；若求解或对局中途失败，此前已记录的全部日志行均完整保留。

---

## 待决问题

1. **带限制与动作前缀的部署中间态 `problem` 定义**：当需要从已执行部分本回合决策的部署中间态 $s_1$ 继续求解时，如何定义 `problem` 中的既有动作前缀、约束条件（如禁止撤回或锁定已定编队）与期望返回语义（增量动作序列抑或全回合序列），以及任意中间态 $s_1$ 到终态 $s_2$ 的净决策规范化如何与 [action.md#3-标准-12-阶段规范化convert-与-format](../document/action.md#3-标准-12-阶段规范化convert-与-format) 衔接。
2. **随机流信息隐藏边界**：`problem.state` 完整携带 `seed`、`random_state` 与展开态 `reinforce_pool`，与独立 `state` 的 MDP 完备性一致；在严格隐藏未来抽牌随机源的竞技场景下，是否提供裁剪随机流状态字的受限视图模式尚未决定。
