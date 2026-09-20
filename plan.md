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

- **战斗结算**：`match` 走到 `fight` 阶段就停在那里，因为经验没有出处（见下）。
  一局现在能下完开局和第一回合的部署，往后卡在这一条。
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

这一步还有几件事不属于命名空间迁移，都已经做完：

- **取消 `--config`。** 配置只剩内嵌这一份。
- **`man`。** `docs/rules` 和 `docs/spec` 编译进二进制，分发出去的二进制能自己
  解释游戏规则和自己的格式与接口。
- **`doc schema`。** 四种文档的 JSON Schema。battle／state／action 的类型都补上了
  `JsonSchema`，按写出来的形态（名字而不是 ID）描述。
- **`doc project`。** 导出某一回合部署结束的 layout。语料 334 个回合全部投影并编译
  通过——为此修掉一条自己的错规则：一回合可以释放两次同一种战斗技能（导弹专家发两
  张导弹打击），layout 本来就按释放顺序记，编译器却拒绝重复。
- **获取游戏是操作不是选项。** `shell` 不再有 `--launch`／`--attach`／`--level`，
  prompt 里用 `game launch --level 3`；一次性命令也去掉了必填的 `--attach`。

`shell --json`（一行一个请求、一行一个结果）跟 `arena` 说的是同一套协议，所以它
跟着编排一起做，在第三步。

### 二、对局最小可用 —— 已完成，战斗除外

`match new`／`show`／`act`／`commit` 已经实现，账本就是 battle 文档，旁边一个
turn 文件装锁、本回合开始时刻和两方未提交的决策。两个进程指定同一个文件就能
把开局和第一回合下完，产出的文档 `doc verify` 通过。

做出来的还有几条当时没写进计划的：

- **决策要过两道规则**：转移说它花多少、留下什么，布阵编译器说它能不能站在那里。
  买不起、没解锁就买、挪出格点、压在别的单位上，现在都拒绝。前三条是这次给
  `transition::step` 补的（`afford` 与解锁检查），语料 41 份回放不受影响。
- **可见性**按 cli.md 实现：对手只给回合开始时的位置，去掉 supply 和 shop，
  loadout 只给它上过场的机型，种子两边都不给，`--omniscient` 给全。
- **battle 文档可以是进行中的**：一方还没开局、一回合只有一方写了决策，都是
  合法文档，`doc verify` 随时读得动。

增援发牌也接上了：回合开的时候由 `deal_last_round` 从表头种子的流里发出来，
写进该回合的 state，`verify` 再去核对它。

还差战斗。`match` 现在会真的打：把双方提交后的局面投影成 layout、交给模拟器、
再用 `fight outcome` 读录像。走不通的地方都由这条路自己报出来——第一仗卡在
"side blue officers are outside the current baseline simulator slice"，因为每
个开局都发一名专家军官。

### 三、编排

`arena run` 起两个玩家进程，按请求协议对局。`shell --json` 是同一套请求协议的
交互端。

### 四、战斗后端

读取器（`fight outcome`）已经在了：录像里能读到谁活下来、活了几个、剩多少血，
按文档的 index 对上号；一回合没带进装置／地形／护盾的，也就没有留下来的。剩下
的按下面的顺序补。

0. 模拟器收不下真实部署。军官（每局必有）和建筑（每张图必发）先补，否则前面几条
   都无从验证。语料里的证据：293 次回合过渡有 289 次只有一方掉血，伤害 20..2738，
   最常见的 100／200／400 正是单位的 `value`，所以"核心伤害是存活方 value 的函数"
   是第一个要证伪的猜想。最便宜的一条线是先查 build 的单位数据里有没有现成的对核
   心伤害字段。
1. 经验：MCFR 现在不记经验。优先让 adapter 把它记进录像，让证据直接带上，而不是
   先补一条没有出处的分配公式。反应堆核心同理：它不是战斗里的对象（录像里每方只
   有两座 3400 血的塔），所以也要 adapter 把它记下来才谈得上量。
2. 把 convert 借用的模拟与验证规则迁到 simulation，由它暴露给 convert 和
   `match` 调用。迁移不改变任何输出，追踪集和指标都不能变。
3. 扩大模拟器的覆盖范围。再往后才是"让真实游戏打这一仗"：adapter 现在只能下发
   整份 layout 并录制，读不回打完之后的局面，所以这条还不成立，等 adapter 具备
   了再定它怎么进契约。game 打的一仗不可重算，届时 outcome 必须落盘。
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
