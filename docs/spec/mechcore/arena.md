# 对战调度规范（Arena）

## 范围

本契约定义 `mechcore arena` 命名空间（`arena new` 与 `arena load`）：作为连续的 `solve` 问题分发者，在内存中驱动两名常驻 Bot 进程完成一场标准 1v1 对局，并在对局终结后向标准输出写出最终规范对局文档（`kind: match`）。

与兄弟契约的分工：

- [`rpc.md`](rpc.md)：定义底层双向 JSON-RPC 2.0 传输帧、无状态引擎反向方法（`convert`、`act`、`simulate`）、单请求取消（`$/cancel`）与 JSON-RPC 错误码；
- [`solve.md`](solve.md)：定义 Bot 子进程模型、进程组清理、`initialize` 握手、纯 JSON 完整展开态 `problem` 结构、`solve` 请求/响应契约及 `--log` 线路日志格式；
- [`match.md`](../document/match.md)、[`state.md`](../document/state.md) 与 [`action.md`](../document/action.md)：定义对局多段 YAML 流、回合间结算、规范形式与终局 `result` 段；
- [`cli.md`](cli.md)：定义通用错误对象、退出码与其余命令表面。

明确不在本契约范围内的内容：

- `mechcore arena` 全程在内存推进对局，不读取、不修改源对局文件，不创建或锁定 `.turn` 协调文件，也不落盘任何试算或官方战斗的 MCFR 录制文件；
- `mechcore arena` 不提供面向 Bot 的独立 `commit` 方法；双方每回合的决策提交统一由 [`solve.md`](solve.md#求解请求与响应solve) 的 `solve` Response 承载。

---

## 命令行入口

```sh
mechcore arena new --blue '<command>' --red '<command>' \
  [--seed <i32>] [--map <id>] [--max-round <n>] [--base-core <n>] \
  [--no-reinforce] [--full-tech] [--timeout <seconds>] [--parallel <n>] [--log <file.jsonl>]

mechcore arena load <match.yaml> [--round <n>] --blue '<command>' --red '<command>' \
  [--max-round <n>] [--base-core <n>] [--timeout <seconds>] [--parallel <n>] [--log <file.jsonl>]
```

### 1. 子命令专属参数

- **`arena new`**（新开对局）：
  - `--seed <i32>`：可选正整数对局种子（`<= 0` 按 `refused` 拒绝）；省略时随机抽取正整数种子，固定种子确定性地复现地图抽签、开局发牌与双方玩家种子；
  - `--map <id>`：可选地图 ID；省略时从支持开局初始化的地图中由种子抽取；
  - `--max-round <n>`：可选正整数（`>= 1`），覆盖地图默认最大回合数；
  - `--base-core <n>`：可选正整数（`> 0`），覆盖地图默认第 0 回合初始反应堆核心血量；
  - `--no-reinforce`：关闭增援发牌（对局头写入 `reinforce: false`；默认开启）；
  - `--full-tech`：双方均采用全科技配置（默认使用 CLI 内嵌的标准科技配置）。
- **`arena load`**（从既有对局续跑）：
  - `<match.yaml>`：必填，只读加载既有 `kind: match` 文档（非 `match` 文档按 `refused` 拒绝）；
  - `--round <n>`：可选整数，默认 `0`。保留第 `n` 回合之前的所有历史回合与第 `n` 回合开局态，并从第 `n` 回合起由两名新 Bot 接管双方决策（支持从截断对局流内任意可玩检查点起步）。目标回合不存在或该检查点已因核心归零而终局时按 `refused` 拒绝；
  - `--max-round <n>` 与 `--base-core <n>`：可选正整数覆写；对局地图、种子、`reinforce` 开关与双方 `tech_loadout` 均沿用源文档；
  - 产出对局头写入两方新 Bot 在 `initialize` 中上报的 `name`，并清除源文档中历史玩家的 `elo_score`。

### 2. 公共选项与输出

- `--blue <command>`、`--red <command>`：均为必填非空 Shell 命令，分别经 `sh -c` 启动蓝方与红方常驻 Bot 进程；
- `--timeout <seconds>`：每方单次 `initialize` 或单回合 `solve` 的墙上时钟秒数上限（含反向 RPC 耗时），默认 `30.0`，`0` 表示不限时（在线上映射为 `limits.timeout: null`）；
- `--parallel <n>`：并发执行反向 `act` 与 `simulate` 试算的最大工作线程数，默认逻辑 CPU 核心数（无法获取时为 `1`）；
- `--log <file.jsonl>`：可选新文件路径，按时间顺序交叉记录蓝红双方全对局线路日志；
- **标准输出**：对局正常终结后，以退出码 `0` 在 `stdout` 输出最终规范紧凑形式的完整 `match` 文档（含 `source: simulator` 与终局 `result` 段，符合 [match.md](../document/match.md#规范形式)）；Bot 与引擎诊断信息全部留在 `stderr`。

---

## 连续问题分发与对局调度

`mechcore arena` 按以下确定性流程运行一场对局：

1. **构建初始对局态**：按 `arena new` 或 `arena load` 在内存中初始化对局状态（含历史前缀与当前回合开局态）。
2. **启动并握手双方 Bot**：
   - 按 [solve.md](solve.md#进程模型与生命周期) 启动蓝方、红方两个独立进程组的常驻子进程；
   - 依次对蓝方、红方发起 [`initialize`](solve.md#会话握手initialize) 握手，收集双方 `name` 写入对局头。
3. **回合循环（连续 `solve` 分发）**：当对局尚未终结时重复以下步骤：
   - **构造不可变问题**：以当前回合开局态构建一份中立的完整展开态 [`problem`](solve.md#问题表达problem)（包含展开态 `tech_loadout` 与 `reinforce_pool` 的 `state`、前序已完成回合 `history` 以及指向当回合内存中只读 `Game` 的不可变 `ref`）；
   - **先蓝后红串行求解**：
     1. 向蓝方发送 `solve` 请求（`side: "blue"`）并挂载反向 [`rpc.md`](rpc.md) 服务（此时红方进程保持存活并在 `stdin` 上空闲等待）；收到蓝方响应或超时后，立即取消蓝方在途反向试算并执行裁决；
     2. 若蓝方本回合的最终有效动作包含 `concede`（无论由 Bot 主动返回，还是由部署期超时/非法提交折算而来），立即结束本回合分发，不再向红方发送本回合 `solve` 请求；
     3. 否则以同一份 `problem` 向红方发送 `solve` 请求（`side: "red"`）并执行同等反向服务与裁决；
   - **官方结算与推进**：释放本回合 `problem.ref`；`round: 0` 将双方开局选队推进至 `round: 1` 开局态；`round >= 1` 若存在 `concede` 则直接结案写入 `result`，否则将双方合法动作投影为布局并经模拟器执行官方战斗结算，推进至下一回合开局态或终局 `result`。
4. **收尾与输出**：向蓝红双方依次关闭 `stdin`（发送 EOF）并执行最多 2 秒优雅等待与超时进程组 `SIGKILL` 清理，最后将完整规范 `match` 文档写至 `stdout`。

---

## 裁决与错误分类

Bot 在回复 `solve` 之前随时可通过同一管道上的反向无状态 `act` 干跑校验候选序列合法性。为保证连续对战在常规回合的稳健收敛，同时避免掩盖开局失败与底层协议故障，`arena` 按下表区分「判负终局（折算 `concede`）」与「命令异常中止（非零退出）」：

| 场景 | 发生阶段 | `arena` 裁决行为 | 退出码与输出 |
| --- | --- | --- | --- |
| Bot 返回合法 `result.actions` | 任意回合 | 采纳该序列并继续调度 | 继续对局 |
| Bot 超过 `limits.timeout` 未回复 | 部署回合（`round >= 1`） | 取消在途反向试算，向 `stderr` 记录告警，将该阵营本回合有效动作折算为 `[{"type": "concede"}]` 并结束对局 | `0`；`stdout` 输出含终局 `result` 的规范 `match` 文档 |
| Bot 返回非法 `result.actions`（动作解析错误或规则校验被拒） | 部署回合（`round >= 1`） | 在 `--log` 中保留原始响应（若启用），向 `stderr` 输出具体违规原因与 `action_index` 告警，将该阵营本回合有效动作折算为 `[{"type": "concede"}]` 并结束对局 | `0`；`stdout` 输出含终局 `result` 的规范 `match` 文档 |
| Bot 超时或返回非法 `result.actions` | 开局回合（`round == 0`） | 无法生成合法的首回合局面，中止对局并清理双方进程组 | `5`（`failed`）；`stderr` 输出错误对象，`stdout` 为空 |
| 子进程启动失败、`initialize` 失败/超时/非法名称、Bot 进程崩溃或提前 EOF、畸形 JSON/非 UTF-8/协议错误、Bot 返回 JSON-RPC `error` | 任意阶段 | 中止对局并向双方进程组发送 `SIGKILL` | `5`（`failed`）；`stderr` 输出错误对象，`stdout` 为空 |
| 官方战斗结算被模拟器拒绝（未建模机制） | 回合结算 | 中止对局并报告未解原因与 `sites` | `5`（`failed`）；`stderr` 输出错误对象，`stdout` 为空 |
| 未知子命令（非 `new`/`load`）、缺失 `--blue`/`--red`、非法 `--timeout`/`--parallel` | 启动解析 | 拒绝启动 | `2`（`usage`）；`stderr` 输出错误对象，`stdout` 为空 |
| 非正数 `--seed`/`--max-round`/`--base-core`、`arena load` 输入非 `match`、目标回合不存在或已终局 | 启动校验 | 拒绝启动 | `3`（`refused`）；`stderr` 输出错误对象，`stdout` 为空 |

---

## 线路日志（`--log`）

`arena` 复用 [`solve.md`](solve.md#校验错误分类与线路日志--log) 定义的 JSON Lines 日志格式，将蓝红双方在 `initialize` 及各回合 `solve`（含全部反向 `act`、`simulate` 与 `$/cancel` 报文）的收发记录按真实发生时序交叉写入同一文件。

日志与对局文档的职责分工：

- Bot 在 `solve` 响应中附带的 `result.meta` 完整保留于 `--log`，永不写入 `stdout` 的 `match` 文档；
- 当某方在部署回合因提交非法 `actions` 被折算为 `concede` 时，`--log` 原样保留 Bot 实际发出的非法响应以便事后复盘，而 `stdout` 输出的 `match` 文档按规则记录该方当回合动作为 `[{"type": "concede"}]`。

---

## 待决问题

1. **双边并行求解调度**：当前规范规定每回合按蓝方优先、红方其次的顺序串行分发 `solve` 请求，使思考方独占全部 `--parallel` 计算槽位且计时互不干扰；未来在独立算力预算场景下是否开放同回合双边并行 `solve` 模式尚未决定。
