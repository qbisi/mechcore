# 目标与计划

把 mechcore 做成 agent 和人类的对战平台：输入一条原子 action，返回下一个
state。一局比赛就是一份 battle 文档，平台一边下一边把它写出来。

命令行按 [docs/spec/mechcore/cli.md](docs/spec/mechcore/cli.md) 治理。那份
文档是契约，本文件只记进度和顺序。

## 验收指标

1. **一局下得完。** 两个进程分别扮演蓝红，只通过 `match` 的操作把一局下到
   结束，产出的文档 `doc verify` 通过。
2. **部署阶段每一步都由规则决定。** 平台执行的转移就是 `step`／`predict`，
   和现有的 battle 语料指标共用一套实现：追踪集每份 battle 的非战斗叶全部
   预测一致，这条由 CI 把守，不能因为平台而退化。
3. **战斗结算范围外就拒绝。** 后端结算不了的局面明确拒绝，不给近似值。

## 已经存在

- **部署阶段的转移**：`step`／`step_placing`／`open_round`／`predict`，落位
  规则，补给与额度。追踪集上非战斗叶 67305/67305 一致。
- **发牌**：开局四组与每回合增援由种子推出并校验，放弃补给按回合取值。
- **文档**：layout／state／action／battle 四份格式与规范形式，`verify`、
  `format`、`diff`、`convert`。
- **战斗模拟**：确定性内核，覆盖 1 级、无科技技能建筑装置的基础范围。
- **游戏控制**：adapter 的 Training Ground 操作，能下发整份 layout 并录制
  一场战斗。

## 还不存在

- **对局本身**：`match` 命名空间、进行中回合的 journal、按方可见的视角、
  可行 action 枚举。
- **战斗结算到 state 的映射**：反应堆伤害和经验分配这两条规则还没有出处，
  模拟或录像给出的战斗结果也还没有转成下一回合 state 的通路。
- **编排**：`arena`，即让两个 agent 进程互相下完一局。
- **命令行的统一**：shell 用位置参数、mcscript 用 YAML 参数，同一个操作两套
  写法；离线工具、游戏控制和对局还混在同一层。

## 下一步

### 一、命令行迁移

按 cli.md 重排命名空间，一次改完代码、脚本、CI 和其它 spec 里的命令行。

| 现在 | 之后 |
| --- | --- |
| `mechcore verify <doc>...` | `mechcore doc verify` |
| `mechcore format <doc> [--write]` | `mechcore doc format` |
| `mechcore diff <l> <r>` | `mechcore doc diff` |
| `mechcore convert <grbr> <yaml>` | `mechcore replay convert` |
| `mechcore opening <seed> <map>` | 删除；开局预测留在库里供 verify／convert 用，不再是命令 |
| `mechcore sim <layout>` | `mechcore fight run` |
| `mechcore sim compare <rec>...` | `mechcore fight verify` |
| `mechcore mcfr compare <l> <r>` | `mechcore fight compare` |
| shell 里的 `status`、`apply_layout`、`record_battle` 等 | `game <op>`，名字沿用 adapter 协议 |
| shell 的位置参数解析 | 每行等同一条命令，去掉 `mechcore` 前缀 |
| mcscript 的 `sim`、`compare`、`apply_layout` 等步骤 | `fight.run`、`fight.compare`、`game.apply_layout` |

同时统一输出：结果一个 JSON 对象走 stdout，诊断走 stderr，退出码按 cli.md
的 0／1／2／3／4／5。

这一步还有三件事不属于命名空间迁移：

- **取消 `--config`。** 模拟器的配置已经全部内嵌，`mechcore sim --config <dir>`
  和 mcscript 里对应的 `config` 字段一起去掉，配置只有内嵌这一份。
- **`man`。** 把 `docs/rules` 和 `docs/spec` 编译进二进制，`mechcore man [<topic>]`
  读出来，这样分发出去的二进制能自己解释游戏规则和自己的格式与接口，不依赖仓库。
- **`doc schema`。** 给出四种文档的 JSON Schema。layout 已经有 `JsonSchema`，
  battle／state／action 还没有，要先给这些类型补上，所以它排在这一步的后半段。
- **`shell --json`。** 一行一个请求、一行一个结果，和 `arena` 跟玩家进程说的是
  同一套协议，所以它跟着对局一起做，不在这一步。

### 二、对局最小可用

`match new`／`show`／`act`／`commit`，战斗由 `--fight sim` 结算。`act --dry-run`
就是合法性判定，`commit` 等待对方并直接返回下一回合。journal 与缩并写回在这一步
定下来。

战斗那半边先把"从录像读出五个字段"的读取器写出来：反应堆伤害、装置、地形、
空投护盾都能从 MCFR 读到。经验 MCFR 没有记，模拟器覆盖不到的局面也还很多，
这两处在 `match` 里明确标成缺口并拒绝，不给近似值，不引入"由调用方给战果"
这种口子。

### 三、编排

`arena run` 起两个玩家进程，按请求协议对局。`shell --json` 是同一套请求协议的
交互端。

### 四、战斗后端

1. 经验：MCFR 现在不记经验。优先让 adapter 把它记进录像，让证据直接带上，而不是
   先补一条没有出处的分配公式。
2. 把 convert 借用的模拟与验证规则迁到 simulation，由它暴露给 convert 和
   `match` 调用。迁移不改变任何输出，追踪集和指标都不能变。
3. 扩大 `--fight sim` 的覆盖范围；再接 `--fight game`，用 adapter 在真实游戏里
   打一场，读回同一份录像。game 打的一仗不可重算，所以它的 outcome 必须落盘。
4. 战斗决定的五个字段逐项从依赖战斗改为预测，在那之前非战斗叶持续验收。

## 各阶段共同的验证方式

- 小规模 state／action 构造做局部回归，覆盖冷却重置、额度、收入、交付顺序等
  规则的边界，用能区分错误实现的反例。
- battle 语料做跨回合回归，指标由断言钉住。追踪 YAML 只由转换器生成，错误
  变体在测试临时数据里构造。
- 平台自身的回归：一局脚本化的对局从头下到尾，产出的文档能 `doc verify`，
  并且同样的输入重放出同一份文档。
- Python／mcscript 批量比较 `project(step*(state(r), actions(r)))` 与原生部署末
  layout，作为部署与投影的独立粗验证。原生一端必须来自回放或实际执行。
- 反汇编研究跟着具体字段的差异走。回放一致不能证明非法动作的拒绝正确；
  `MAP_BuyUnit`、`MAP_UpgradeUnit`、`MAP_ChooseReinforceItem` 的 `Perform`
  各支条件仍待核出。

## 顺序

先做命令行迁移（一），它决定了后面所有东西的接口形状，越晚改代价越大。再做
对局最小可用（二），这一步不依赖战斗规则就能交付一个可用平台。可行动作与
编排（三）跟着上。战斗后端（四）最后，它卡在两条还没有出处的规则上。
