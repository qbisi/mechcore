# Mechcore RPC 协议规范（RPC）

## 范围

本契约定义 `mechcore` 的无状态引擎 RPC 服务接口与底层双向传输帧协议：

- `mechcore rpc` 命令行入口与并发工作池；
- 基于标准输入输出（stdio）JSON Lines 的双向 JSON-RPC 2.0 帧协议、流水线并发与批量请求规则；
- 不可变上下文 `context` 的两种形态及版本告警行为；
- 无状态引擎方法（`initialize`、`convert`、`act`、`simulate`）与单请求取消通知（`$/cancel`）；
- 有限战斗模拟的停止谓词 `until` 与采集投影 `collect`；
- JSON-RPC 错误码与 `mechcore.error` 的统一映射。

与兄弟契约的分工：

- [`solve.md`](solve.md)：复用同一双向 JSON-RPC 2.0 传输层，定义面向单题求解的宿主-Bot 会话握手、纯 JSON 展开态 `problem` 格式与 `solve` 请求/响应契约；
- [`arena.md`](arena.md)：定义 `mechcore arena` 作为连续 `solve` 问题分发者的整场对局调度与裁决规则；
- [`state.md`](../document/state.md)、[`action.md`](../document/action.md)、[`layout.md`](../document/layout.md)、[`match.md`](../document/match.md) 与 [`fight.md`](../document/fight.md)：定义各文档形态、14 种决策转移、布局投影与战斗结算语义；
- [`cli.md`](cli.md)：定义文件级命令行操作及有状态的 `match`、`game`、`shell` 命令。

明确不在本契约范围内的内容：

- **无对局会话与无回合提交**：`mechcore rpc` 不持有正在进行的对局草稿，不提供提交回合动作或推进对局文件的接口，也不会写入任何对局文件、回合缓存（`.turn`）或 MCFR 录制文件；
- **战前初始化预览合并于模拟接口**：开战边界的单位初始化属性与首轮索敌/攻击关系预览通过 `simulate` 搭配 `until: {"tick": 0}` 与 `collect` 中的 `"preview"` 预设请求获取，不另设独立预览方法；
- **无静态目录或候选动作枚举方法**：静态目录、几何掩码与候选列表扩展属于本契约待决项（见[待决问题](#待决问题)）。

---

## 命令行入口

```sh
mechcore rpc [--parallel <N>]
```

- `--parallel <N>`：可选正整数，限制服务端并发执行 `convert`、`act` 与 `simulate` 计算请求的最大工作线程数；省略时默认为当前进程可用的逻辑 CPU 核心数（无法获取时为 `1`）。传入 `0`、负数、非整数或多余参数均返回 `usage` 错误并退出 `2`。
- **标准流分工**：`stdin` 专用于接收 JSON-RPC 2.0 请求与通知，`stdout` 专用于写回 JSON-RPC 2.0 响应，诊断信息与版本兼容告警统一输出到 `stderr`。
- **生命周期**：进程启动后持续读取并处理 `stdin`，当 `stdin` 收到 EOF 且所有已接收的非取消请求处理完毕并刷出响应后以退出码 `0` 正常结束。

---

## 传输与并发帧协议

本协议采用 **JSON-RPC 2.0 over UTF-8 JSON Lines（JSONL）**，既用于独立运行的 `mechcore rpc` 服务，也用于 `mechcore solve` 与 `mechcore arena` 宿主和 Bot 子进程之间的全双工通信（双方在同一对 `stdin`/`stdout` 管道上互为对等端）。

### 1. 消息封装与帧边界

- 每条消息占单行 UTF-8 文本，以 `\n`（LF）结尾并立即刷新输出流；空行在读取端忽略。
- 所有请求、响应与通知均携带 `"jsonrpc": "2.0"`：
  - **Request（请求）**：`{"jsonrpc": "2.0", "id": <u64 | string>, "method": "<name>", "params"?: <object>}`；
  - **Response（响应）**：`{"jsonrpc": "2.0", "id": <u64 | string | null>, "result": <value>}` 或 `{"jsonrpc": "2.0", "id": <u64 | string | null>, "error": <JsonRpcError>}`；
  - **Notification（通知）**：不含 `id` 字段，接收方不返回任何响应。
- **请求 `id` 约束**：请求允许使用非负整数或字符串作为 `id`，禁止传入 `null`（`null` 仅出现在无法解析出有效请求 `id` 时的错误响应中）。同一作用域内尚未完成响应的并发请求 `id` 必须唯一；重复复用仍在途的 `id` 直接返回 `-32602`（`usage`）错误。

### 2. 流水线并发与批量请求（Batch）

- **流水线与乱序响应**：调用方可在等待前序响应前连续写入多条单行请求，或将多条请求组成非空 JSON 数组写入单行作为批量请求（Batch）。服务端的每条请求独立调度执行，**按完成顺序即时写出各自独立的单行 Response**（不按输入顺序串行阻塞，也不将批量响应拼回 JSON 数组），调用方通过 `id` 关联结果。
- **批量边界规则**：
  - 传入空数组 `[]` 返回一条 `id: null` 的 `-32600`（`usage`）错误响应；
  - 批量中的单条解析失败或规则拒绝仅向该条目对应的 `id` 返回错误响应，不影响同批其余兄弟请求独立执行。
- **计算线程池与非阻塞调度**：
  - `convert`、`act` 与 `simulate` 共享上限为 `--parallel`（或宿主下发的 `limits.parallel`）的阻塞计算槽位；超出上限的请求在服务端进入 FIFO 队列等待槽位释放；
  - `initialize`、`$/cancel` 与协议校验在主异步事件循环中即时处理，不占用计算槽位；
  - 当正在执行 `simulate` 的工作线程因 `$/cancel` 或宿主超时被标记取消时，线程在下一逻辑帧边界协作式退出并返回取消错误，释放计算槽位。

---

## 不可变上下文（`context`）

所有基于局面的计算方法（`act`、`simulate`）均不依赖服务端隐式状态，必须通过 `params.context` 显式指明所依据的不可变状态。`context` 为 JSON 对象，且必须在以下两种互斥形态中**恰好选择一种**（缺少或同时提供多种形态键均按 `-32602` `usage` 拒绝）：

| 形态 | 写法 | 适用方法 | 语义与约束 |
| --- | --- | --- | --- |
| 独立状态 | `{"state": <JSON object>}` | `act`、`simulate` | 独立 `kind: state` JSON 对象（**仅接受 JSON 对象，不接受 YAML 字符串**），同时覆盖 `round: 0` 与 `round >= 1`；兼容紧凑与展开形态的 `tech_loadout` 与 `reinforce_pool`（见 [state.md](../document/state.md#两种载体形态与两种回合时点)）。 |
| 不可变引用 | `{"ref": "<string>"}` | `act`、`simulate` | 指向服务端内存中已预解析的只读不可变状态句柄（例如宿主在 `solve` 中提供的 `problem.ref`）。语义上与内联传入对应 `state` JSON 对象完全等价；引用不存在或已失效时按 `refused` 拒绝。 |

原始 YAML 文档字符串（`match` 或 `state` 文本）不直接进入 `context`，统一通过 [`convert`](#convert请求) 导出为标准 JSON `state` 对象，使 Bot 与上层工具无需依赖 YAML 解析库即可直接消费与构造 `context`。

**版本处理规则**：若 `state` 声明了与当前程序 `GAME_VERSION` 不一致的 `build`，服务端向 `stderr` 输出版本告警并按当前二进制内嵌的目录与规则继续计算（见 [match.md](../document/match.md#2-source-溯源与-build-版本兼容)）。

---

## 方法定义

### `initialize`（请求）

建立协议握手并交换服务端版本与能力。独立 `mechcore rpc` 模式下可由客户端主动发起（在无状态模式下允许省略握手直接调用 `convert`/`act`/`simulate`）；在 `solve` 与 `arena` 宿主模式下由宿主主动向 Bot 发起（详见 [solve.md](solve.md#会话握手initialize)）。

- **Params**（独立模式，可选对象）：
  ```json
  { "client": { "name": "eval-pipeline" } }
  ```
- **Result**：
  ```json
  {
    "protocol": "mechcore-rpc",
    "game_build": "<GAME_VERSION>",
    "parallel": 8,
    "capabilities": {
      "methods": ["convert", "act", "simulate"]
    }
  }
  ```

### `convert`（请求）

纯内存静态文档转换，从给定文档内容解析、提取并导出可直接送入 `act`/`simulate` 的独立 `kind: state` JSON 对象。服务端按内容自动识别源文档 `kind`，不读写本地磁盘文件，也不执行战斗模拟。

- **Params**：
  - `input`（必填，`string | object`）：源文档内容。通常为 UTF-8 YAML/JSON 文档字符串（如直接读入的 `match.yaml` 或 `state.yaml` 文本），也接受已解析的 `kind: state` JSON 对象（用于校验及紧凑/展开形态互转）。不接受本地文件路径；
  - `to`（必填，常量 `"state"`）：目标导出形态；
  - `round`（可选 `i32`）：仅当 `input` 为 `kind: match` 时合法（对 `kind: state` 传入按 `-32602` `usage` 拒绝）。指定提取第 `round` 回合（完整对局支持 `0`，截断对局支持流内存在的任意回合）；省略时选取流中最后一个 `state` 段（无论流止于 `state`、`action` 还是 `result`）；
  - `apply_actions`（可选布尔值，默认 `false`）：仅当 `input` 为 `kind: match` 时合法（对 `kind: state` 传入按 `-32602` `usage` 拒绝）。`false` 导出所选回合的**开局态**；`true` 将所选回合已记录的 `action` 序列应用于该回合开局态，导出**部署终态**（若所选回合尚无 `action` 段或决策不合法按 `refused` 拒绝）；
  - `expanded`（可选布尔值，默认 `true`）：`true` 时完整展开 `tech_loadout`（`{unit: [tech, ...]}`，含全科技展开）与常规回合 `reinforce_pool`（活跃普通增援池完整卡牌 ID 升序数组 `[card_name, ...]`）；`false` 时输出规范紧凑形态。
- **转换与展开规则**：
  - **`match` → `state`**：将对局头文件中的规则不变量（`build`、`reinforce`、`max_round`、`map_id`、`seed`、双边 `tech_loadout`）并入所选回合的 `state`，丢弃展示溯源字段（`source`、`name`、`elo_score`）。当 `round == 0` 且 `apply_actions == false` 时产出第 0 回合独立开局态；当 `round >= 1`、`reinforce == true` 且 `expanded == true` 时，依据头文件 `seed` 初始池与当回合 `reinforce_pool` 差值展开完整活跃列表 `initial \ removed ∪ added`；当 `round == 0` 或 `reinforce == false` 时省略 `reinforce_pool`。允许导出流内任意合法存在的回合（包括末回合核心已归零的终局状态）。
  - **`state` → `state`**：解析并校验独立 `kind: state`。`expanded == true` 时若存在 `tech_loadout` 则完整展开（原文档省略时保持省略）；对于 `round >= 1` 且 `reinforce == true` 的 `reinforce_pool`，若源文档已写为展开数组则校验排序后输出，若为紧凑差值映射或省略则依据 `state.seed` 展开全量列表（缺失 `seed` 时无法推算初始池，按 `-32001` `refused` 拒绝）。
  - **版本告警**：输入声明的 `build` 与当前程序不一致时，向 `stderr` 输出版本告警并按当前目录继续转换（同 [match.md](../document/match.md#2-source-溯源与-build-版本兼容)）。
- **Result**：
  ```json
  {
    "source_kind": "match",
    "state": {
      "kind": "state",
      "build": "<GAME_VERSION>",
      "reinforce": true,
      "max_round": 40,
      "map_id": 1001,
      "seed": 2038621361,
      "round": 3,
      "random_state": [1, 2, 3, 4],
      "reinforce_pool": ["supply_specialist", "laser_sights"],
      "blue": {
        "tech_loadout": { "arclight": ["armor_enhancement", "range_enhancement"] },
        "reactor_core": 3800,
        "supply": 600,
        "units": [...]
      },
      "red": { ... }
    }
  }
  ```
  - `source_kind`：服务端识别出的源文档类型（`"match"` 或 `"state"`）；
  - `state`：可直接填入 `act`/`simulate` 的 `params.context.state` 的独立 `kind: state` JSON 对象。
- **请求示例**：
  ```json
  {"jsonrpc":"2.0","id":1,"method":"convert","params":{"input":"kind: match\nbuild: <GAME_VERSION>\n...","to":"state","round":3,"apply_actions":false,"expanded":true}}
  ```
- **拒绝与错误**：
  - 未知 `to` 目标、对 `kind: state` 传入 `round` 或 `apply_actions`、参数类型错误：返回 `-32602`（`usage`）；
  - 源文档类型不是 `match`/`state`（如 `layout`/`fight`/`action`）、YAML/结构解析失败、指定 `round` 不存在、`apply_actions: true` 但目标回合缺少 `action` 段或决策非法、以及 `expanded: true` 下紧凑 `reinforce_pool` 缺少 `seed`：返回 `-32001`（`refused`）。

### `act`（请求）

无状态地校验一回合决策序列并计算执行后的状态，用于干跑（dry-run）合法性检查与部署中间态推演。不修改任何服务端或 `ref` 指向的状态。

- **Params**：
  - `context`（必填）：`{"state": <JSON object>}` 或 `{"ref": "<string>"}`；
  - `actions`（可选，默认 `{}`）：`{"blue"?: [Action, ...], "red"?: [Action, ...]}`。
    - 对 `actions` 中**显式出现**的阵营键，从 `context` 对应回合状态出发依次应用该阵营的每条决策并执行回合末序列完备性检查（如 `round: 0` 必须恰好包含一条匹配的 `choose_advance_team`，存在 `reinforce_offers` 时除非 `concede` 首条必须选择增援项或退款项，详见 [action.md](../document/action.md)）；
    - 对**省略**的阵营键，保持 `context` 中的原有状态不变且不对其执行空序列末检；若显式传入 `"red": []` 则按空序列校验红方（例如在有增援牌面回合传入空数组会被拒绝）；
    - 当双边同时出现且双方均包含 `concede` 时返回 `refused`（一回合在首个认输处终止）；
  - `expanded`（可选布尔值，默认 `true`）：控制返回的 `result.state` 中 `tech_loadout` 与 `reinforce_pool` 是否以展开形式序列化（`true` 输出展开映射与完整增援卡列表，`false` 输出紧凑编码）。
- **Result**：
  ```json
  {
    "state": {
      "kind": "state",
      "build": "<GAME_VERSION>",
      "round": 1,
      "blue": {
        "supply": 50,
        "reinforce_chosen": false,
        "shop": { "buys_remaining": 1, "unlocks_remaining": 1, "contraptions_remaining": 8 },
        "units": [...]
      },
      "red": { ... }
    }
  }
  ```
  `result.state` 为完整独立 `kind: state` 对象，已按 `actions` 更新各阵营的资源态、战斗态、`shop` 剩余限额（`buys_remaining`、`unlocks_remaining`、`contraptions_remaining`）、`reinforce_chosen` 布尔标识以及部署期字段（`units[].travelling`、`units[].recovered`、`energy_tower_skills`、`battle_skills[].release`）。
- **拒绝与错误**：
  - 决策 JSON 格式或操作数类型错误：返回 `-32602`（`usage`），`data.details` 携带 `{"side": "blue"|"red", "action_index": <usize>}`；
  - 单步规则拒绝（补给不足、越界、碰撞、未解锁、缺少移动权限等）：返回 `-32001`（`refused`），`data.details` 携带 `{"side": "blue"|"red", "action_index": <usize>}`；
  - 序列末校验拒绝（如缺失开局选队或缺失增援选择）：返回 `-32001`（`refused`），`data.details` 携带 `{"side": "blue"|"red"}`。

### `simulate`（请求）

无状态地执行一次完整或有界战斗模拟，并按请求采集观察结果。从不写入 MCFR 录制文件。

- **Params**：
  - `context`（必填）：`{"state": <JSON object>}` 或 `{"ref": "<string>"}`；
  - `actions`（可选，默认 `{}`）：`{"blue"?: [Action, ...], "red"?: [Action, ...]}`；省略的阵营视作空序列 `[]`，服务端按 [action.md](../document/action.md) 同时校验双方序列合法性；
  - `seed`（可选 `i32`）：显式指定战斗随机种子（禁止传入 `0`，`0` 返回 `-32602` `usage`）。覆盖优先级为 `params.seed` → 上下文 `state.seed`，若最终仍缺失可用种子则按 `refused` 拒绝；
  - `until`（可选，默认 `"fight_end"`）：停止条件谓词（见[有限战斗模拟](#有限战斗模拟until-与-collect)）；
  - `collect`（可选，默认 `{"settlement": "settlement"}`）：命名采集投影映射（见[有限战斗模拟](#有限战斗模拟until-与-collect)）。
- **执行与投影语义**：
  - 若上下文为 `round: 0`，或双边合法动作中任一方包含 `concede`：不启动战斗模拟，直接返回 `{"tick": 0, "stop": "no_fight", "settled": false, "collect": {}}`；
  - 若上下文可推进战斗：先应用双边 `actions` 并通过 `project(state)` 投影为规范战斗布局，再由模拟器执行至 `until` 条件满足或战斗结算；若投影触及 [layout.md](../document/layout.md#表达边界与未覆盖范围) 定义的 4 类顺序敏感初始化边界，结果额外包含 `warnings` 字符串数组；
  - 当 `until` 为 `{"tick": 0}` 且 `collect` 仅请求 `"preview"` 预设时，只执行战前准备并导出开战边界预览，不推进逻辑战斗帧。
- **Result**：
  ```json
  {
    "tick": 870,
    "stop": "fight_end",
    "settled": true,
    "collect": {
      "settlement": { "blue": { ... }, "red": { ... } }
    }
  }
  ```
  其中：
  - `tick`：停止时的逻辑帧号（`0` 为已准备完成的战前部署态，后续已关闭的战斗逻辑帧从 `1` 起算）；
  - `stop`：触发停止的条件描述（正常或超时结算时为 `"fight_end"`；`until` 提前命中时返回命中的子谓词，对 `any` 返回请求顺序下首个命中的分支；未开战时为 `"no_fight"`）；
  - `settled`：布尔值，表示战斗是否已到达终局结算；
  - `collect`：按请求键名返回对应采集值；提前停止时仅结算可用的字段（如 `"settlement"`、`"core_damage"`）不会出现在 `collect` 中；
  - `warnings`（可选字符串数组）：存在顺序敏感投影告警时写出，无告警时省略。
- **拒绝与错误**：
  - `until` / `collect` 结构非法、`nth: 0`、对 `at_stop` 使用 `group_by: "object"` 或 `seed: 0`：返回 `-32602`（`usage`）；
  - `actions` 校验失败、投影布局编译校验失败或缺少可用 `seed`：返回 `-32001`（`refused`）；
  - 模拟器遇到未建模的战斗机制：返回 `-32001`（`refused`），`data.sites` 列出拒绝源码位置，`data.details` 为 `{"status": "unresolved"}`；
  - 请求被 `$/cancel` 或宿主超时取消：返回 `-32800`（`refused`）。

#### 有限战斗模拟（`until` 与 `collect`）

每一战斗逻辑帧先处理完当帧全部模拟事件并更新状态，再检验 `until` 谓词；若自然战斗结束或到达最大战斗帧数上限与 `until` 在同一帧同时满足，终局结算优先于观测停止条件。

**`until` 停止谓词**：

| 谓词写法 | 停止条件 |
| --- | --- |
| `"fight_end"` | 战斗自然结束或到达强制结算帧上限（默认）。 |
| `{"tick": <u32>}` | 已关闭第 `tick` 帧（`{"tick": 0}` 表示战前准备完成、尚未进入第 `1` 帧）。 |
| `{"event": "<event_kind>"}` | 当前帧产生至少一条指定 `EventKind` 事件。 |
| `{"count": {"where": <Filter>, "at_least": <u64>}}` | 从开战起累计满足 `where` 过滤条件的事件数达到 `at_least`。 |
| `{"metric": {"field": <Metric>, "side"?: "blue"\|"red", "formation"?: <u64>, "at_least": <i64>}}` | 停止检查时满足过滤条件的指标之和达到 `at_least`。 |
| `{"any": [<Until>, ...]}` | 任一子谓词在同一帧成立（空数组按 `usage` 拒绝）。 |
| `{"all": [<Until>, ...]}` | 全部子谓词在同一帧同时成立（空数组按 `usage` 拒绝）。 |

**`Filter` 过滤条件与 `Metric` 指标**：

- `Filter` 各可选字段按逻辑与（AND）组合：`type`（单个事件类型名或 `{"in": [<event_kind>, ...]}`，亦接受别名 `event`）、`side`（`"blue"` | `"red"`）、`formation`（MCFR 编队编号 `u64`）、`object`（`{"kind": ..., "id": ...}`）。
- 支持的 `Metric` 指标：`alive_members`（存活机甲数）、`damage_real`（累计真实伤害）、`damage_taken`（累计承受伤害）、`kills`（累计击杀数）。

**`collect` 采集投影**：

- **空对象 `{}`**：不采集任何额外输出，返回 `collect: {}`。省略 `collect` 字段等价于 `{"settlement": "settlement"}`。
- **内置预设**：
  - `"settlement"`：仅在 `settled: true` 时产出，返回与 `convert --to fight`（[fight.md](../document/fight.md)）一致的 `{"blue": ..., "red": ...}` 双方战斗结算结构（含 `core_damage`、编队战后 `exp`、存留 `contraptions` 与 `battle_skills`）；
  - `"core_damage"`：仅在 `settled: true` 时产出，返回 `{"blue": <i64>, "red": <i64>}`；
  - `"preview"`：在战前初始化完成边界（`tick: 0`）采集完整部署预览（含单位经军官/科技/装备修正后的成员属性、预搜索锁定关系 `locks`/`locked_by` 与护盾/城墙重定向后的攻击关系 `attacks`/`attacked_by`，定义同 [cli.md](cli.md#preview)）；
  - `"opening_deaths"`：返回开局每支编队成员对象及其观测到的首次阵亡帧号列表 `[{"object": <ObjectRef>, "first_death_tick": <u32 | null>}, ...]`。
- **事件行投影**：`{"from": "events", "where"?: <Filter>, "select": [<Field>, ...]}`。
  - 按 MCFR 事件顺序返回匹配事件列表，可选列包括 `tick`、`ordinal`（该帧内包含被过滤事件在内的 0-based 全局序号）、`type`、`object`（事件主体引用）、`side`、`formation`、`position`（原始定点坐标 `{x, y, z}`）、`source`、`target`、`last_hit`（同帧内在此之前针对该主体的最近一次伤害事件 `{"source": ..., "amount": ...}`）；缺失字段取 `null`。
- **聚合投影**：`{"count": <Filter>}`、`{"first": <Filter>}`、`{"last": <Filter>}`、`{"nth": <usize >= 1>, "where": <Filter>}`。
  - 可附加 `"group_by": "side" | "formation" | "object"`；不分组时 `count` 返回整数、其余返回匹配帧号或 `null`；分组时返回 `[{"key": <group_key>, "value": <count | tick>}, ...]`。
- **停帧指标投影**：`{"at_stop": <Metric>, "group_by"?: "side" | "formation"}`（不支持按 `object` 分组）。不分组时返回总和整数，分组时返回按组汇总数组。

**请求示例**：

```json
{"jsonrpc":"2.0","id":10,"method":"simulate","params":{"context":{"state":{"kind":"state","round":1,"seed":7,"blue":{"reactor_core":4500,"supply":200,"next_index":{"unit":1,"contraption":0},"units":[{"name":"marksman","index":0,"value":100,"position":{"x":0,"y":-50}}]},"red":{"reactor_core":4500,"supply":200,"next_index":{"unit":1,"contraption":0},"units":[{"name":"arclight","index":0,"value":100,"position":{"x":0,"y":-100}}]}}},"until":{"tick":0},"collect":{"board":"preview"}}}
{"jsonrpc":"2.0","id":11,"method":"simulate","params":{"context":{"ref":"arena:r1"},"actions":{"blue":[{"type":"buy_unit","name":"crawler"}]},"until":"fight_end","collect":{"deaths":{"from":"events","where":{"type":{"in":["unit_died","building_destroyed"]}},"select":["tick","ordinal","type","object","side","formation","position","last_hit"]},"alive":{"at_stop":"alive_members","group_by":"side"},"settlement":"settlement"}}}
```

### `$/cancel`（通知）

请求取消一条仍在队列或工作线程中执行的计算请求（不携带 `id`，服务端不回复响应）。

- **Params**：`{"id": <u64 | string>}`。
- **行为**：若目标 `id` 对应仍在排队的请求，立即将其移出队列并返回 `-32800` 取消错误；若目标正在工作线程运行，则置位该请求专属的取消标志促使其尽早退出并返回 `-32800`；若目标 `id` 不存在或已完成响应，静默忽略。

---

## 错误分类

所有失败均以 JSON-RPC 2.0 `error` 响应返回，其中 `error.data` 嵌入符合 [`cli.md`](cli.md#error-taxonomy) 契约的 `mechcore.error` 对象，使调用方能精确区分可修正重试的拒绝与不可恢复的错误：

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "error": {
    "code": -32001,
    "message": "insufficient supply",
    "data": {
      "schema": "mechcore.error",
      "kind": "refused",
      "operation": "act",
      "reason": "insufficient supply",
      "details": { "side": "blue", "action_index": 2 }
    }
  }
}
```

| JSON-RPC `code` | `error.data.kind` | 含义 | 调用方处理 |
| --- | --- | --- | --- |
| `-32700` | `usage` | 行内容不是合法 JSON（响应 `id` 为 `null`） | 修正编码格式 |
| `-32600` | `usage` | 缺少 `"jsonrpc": "2.0"`、非法 `id`（如 `null`）或空批量 `[]` | 修正帧结构 |
| `-32601` | `usage` | 未知方法名（或当前会话能力未启用该方法） | 检查 `initialize` 能力列表 |
| `-32602` | `usage` | 参数类型错误、重复在途 `id`、多态 `context` 歧义、无效谓词或 `seed: 0` | 修正请求参数 |
| `-32001` | `refused` | 规则不允许：非法决策、布局编译失败、未建模战斗机制（附带 `sites`）、缺失/失效 `ref`、`convert` 不支持的文档类型/缺席回合/缺少展开所需 `seed` | 依据 `reason`/`details` 修改决策或跳过不支持机制 |
| `-32800` | `refused` | 请求被 `$/cancel` 或宿主超时取消 | 按需重新发起 |
| `-32002` | `unavailable` | 运行依赖不可用或过载 | 退避或等待环境恢复 |
| `-32603` | `failed` | 内部序列化错误、配置损坏或未预期 I/O 失败 | 中止并检查环境 |

---

## 待决问题

1. **静态目录、几何掩码、候选动作与跨回合推进方法**：是否在 `mechcore rpc` 中增设无状态的目录导出（`catalog`）、部署合法格位掩码（`mask`）、合法动作及依存费用枚举（`candidates`）以及给定双方终态动作结算下一回合开局的转移函数（`advance`）。
2. **`convert` 扩展目标与 `layout` 直通**：`convert` 未来是否增设 `"problem"`（一次导出 `state` 与 `history`）或 `"layout"` 目标，以及 `simulate` 是否支持直接传入 `context: {"layout": <JSON object>}` 以便无需构造经济字段即可模拟孤立布局。
3. **独立服务端的只读快照缓存（`context/pin` 与 `context/unpin`）**：当独立调用方在同一局面上发起数百次反事实推演时，是否提供显式内容寻址句柄注册接口以省去重复解析开销。
4. **`simulate` 编队标识对齐**：`simulate` 中的 `group_by: "formation"` 与 `opening_deaths` 是否应统一或并行提供与 `preview` 一致的布局编队标识（如 `"blue/unit/<index>"`），避免消费方在 MCFR 内部编号与部署索引之间二次映射。
