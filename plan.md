# 目标与计划

目标只有一个验收指标：**battle 文件内的 state 转移预测覆盖**。对 battle 里每一对
相邻开局，从 `state(r)` 和 `actions(r)` 出发，只用 header 与此前决策提供的
上下文，预测 `state(r+1)` 的每个叶字段，再与文件记录的值逐字段比较。第 0 回合
的开局选择到 `state(1)` 也是一条转移，中间没有战斗。

预测链是三段：`step*` 顺序折叠本回合动作得到部署末 state，战斗改写一部分字段，
下一回合初始化得到下一开局。部署末 state 的投影是本回合战斗开始时的 layout；
它与下一开局是不同的端点，不能混用。

入口是 `mechcore verify <battle.yaml>`，输入只有 battle 文件，不读 GRBR，
也不要求实机。下一 state 只是比较目标：从它取值填回预测，
这个字段就不算覆盖。

本文件只写两样东西：什么存在、什么不存在，以及接下来做什么。

数字不写在这里。覆盖指标由 `crates/document` 里对追踪集的断言钉住，一变就红；
单个文件的指标跑 `mechcore verify` 当场得到。写进文件的数字只会变旧，而且
没有人会发现。

设计上还没定的选择也不写在这里，它们写在对应 spec 的 Unresolved 一节，
挨着它们影响的那份契约。

执行中撞见、但还没决定要不要处理的问题也不写在这里，它们开成 GitHub issue。
什么算一条 issue、它怎么离开，由 `.github/CONTRIBUTING.md` 规定。

## 指标的定义

- **单位是叶字段。** 把预测与记录的 `state(r+1)` 按阵营展开到叶：标量按路径，
  编队按 `index` 对齐后逐字段，技能面板按 `index` 对齐，只含 ID 的集合与
  含重复项的库存作为一个叶整体比较。一侧有、另一侧没有的叶同样计数，
  不能因为对不齐而消失。
- **每个叶归入四类之一。** 一致；不一致；未实现（没有规则产出它，或本回合某个
  动作无法执行，这条转移剩下的叶全部归此类）；依赖战斗。
- **依赖战斗是一张按字段写死的白名单，**写在 battle spec 里：`reactor_core`、
  编队 `exp`、`contraptions`、`terrains`、`airdrop_shields` 的战后留存。
  `supply` 不在上面：当前版本的标准 1v1 没有战斗阶段收益，补给完全由部署与
  初始化决定。白名单只能靠核实机制来改，不能为了消掉差异扩大；
  不在白名单上的叶只能是一致、不一致或未实现。
- **验收线。** 追踪集上非战斗叶全部一致，不一致与未实现都为零；依赖战斗的叶
  逐项列出，不计入通过。以 action 结尾的 battle，末尾那段没有比较目标，只报告，
  不计作一条成功转移。
- **报告按回合、阵营、字段组给出四类计数，**并给出每个不一致叶的预测值与记录值。
  有不一致或未实现时 `verify` 不能报告成功，部分覆盖不能表述为转移通过。

## 已经存在

- 四份文档契约互相引用：layout、state、action、battle，外加 mcfr、
  mcscript、session、adapter、unit-rules、rvo、quadtree 七份。全部按
  `docs/README.md` 的 spec 规范写成，主文档一律英文，由 `scripts/check-docs.py` 把住。
- 离线转换器 `mechcore convert`。别的 build、下载录像、试验场对局、带游戏规则
  的对局和回合不连续的录像都按名拒绝，不猜。能量塔的债有两种读法，对不上也拒绝。
  追踪集没有一份被拒。
- 开局 `offers` 由录像随机状态离线还原，选择仅写为 `choose`，所选队伍与专家
  从该索引读取，并在转换时与源录像核对。`mechcore verify` 能从 seed 核验双方
  完整选项、初始防御建筑和选择索引范围。`mechcore opening <seed> <map_id>`
  直接预测双方开局；按规则推进初始化消耗，不搜索随机偏移。
- 增援发牌预测接入 `mechcore verify`：从开局后的同一随机流连续推进，结合各轮
  双方状态和此前选卡，核对普通卡与单位增援的完整顺序、选择索引和 ID；失败报
  首个不一致回合。候选刷新、级别权重、指挥技能隔轮排除、单位两阶段抽取都按原生
  流程推进。回合状态是预测输入，战斗结果和玩家决策不由 seed 决定。
- 留存物体从释放它的技能自己的 `rangeItems` 读出来：空投护盾 800001 和粘性油弹
  400002。护盾没有寿命，油弹的寿命逐回合减到零就消失，两者由 `round` 分开。
- 配置表进入版本库，覆盖单位价、科技价、卡价、军官效果、开局、增援、蓝图、
  建筑回收和每回合收入，由 `scripts/extract_*.py` 从 build 2259 生成。
- 跨回合转移只有一份：`transition::predict`。原先只产出九个字段的 `apply`、
  以它为准的 `check`、另算一遍 `travelling` 的函数，以及读后继 state 的补给账本
  都已删除；`ledger` 只剩定价与回合收入，由 `step` 与 `open_round` 调用。
  `mechcore convert` 对写出的 battle 报告覆盖。实采的
  `tests/layouts/tuff-replay-round-7.yaml` 整份等于 `project(step*(state, actions))`，
  双方棋盘在内。
- 回合内部署转移 `transition::step`：把一个决策作用到它被做出的那个局面上，
  产出完整的下一个局面，棋盘也在内。卡牌、先遣队和军官交付的编队落位按游戏规则
  计算（`landing`），不再从录像借用。经验写成 `current/maximum`（如 `124/450`），满条上限
  来自 `config/unit_experience.yaml`，强化训练把 `current` 填到 `maximum`。
  移动合法性由编队的 `movable` 判断：本回合到场的编队可移动，部署模块、高速引擎
  和再部署解除固定，移动固定编队的决策被拒绝。
- `verify` 按文件自报的 kind 分派 layout 与 battle，批量靠管道，不自己展开目录。
- 转移覆盖指标。battle 的 state 与 action 按完整文档类型读取；
  `transition::predict` 把 `step*` 与 `open_round` 串成相邻开局的预测，
  `coverage` 按叶归入一致、不一致、未实现、依赖战斗四类，接进
  `mechcore verify <battle.yaml>`，契约在 battle spec 的 Transition coverage 一节。
  尚无规则的字段组列在 `coverage::UNIMPLEMENTED`，不论取值一律计作未实现；
  追踪集按字段组的四类计数由断言钉住。发牌只有在它读取的字段两侧都预测一致时
  才计作一致。第 0 回合的转移从 header 构造的局面出发，不计战斗类。
  `scripts/verify-battles.py` 对 `tests/battle` 跑 `verify` 并按字段组与文件汇总，
  CI 要求它通过。
- 跨回合的非战斗转移已经闭合。第 0 回合由 header 构造选择前的局面（地图反应堆、
  建筑、塔等级），走 `step`、队伍交付与 `open_round(1)` 同一条链；此后每回合的
  开局重置、回合收入与能量塔债务、军官交付都是 `open_round`。标准 1v1 没有战斗
  阶段收益，补给完全由这条链预测。convert 读取开局前的快照后调用同一个
  `open_round`，只有装备库存还由它自己的规则重建。追踪集每份 battle 都通过
  `verify`：不属于战斗的叶全部预测一致。
- 投影 `project(state) → layout`。
- 认输是结束对局的 `concede` 动作，其后没有下一回合 state。
- 适配器能装载一份 layout 并把同一份采集回来，录像与试验场的往返对得上。
- `scripts/export-replay-corpus.py` 把 `tests/grbr` 的每份录像离线转换成
  `tests/battle` 的 battle，并重写两边的 `SHA256SUMS`；有录像被拒绝即失败。
- 原生部署观测 JSONL（`record_replay_battle`、`observe`、`oracle`）已经移除：
  battle 覆盖指标取代了它的验证作用，指标只需要 battle 文件。

## 还不存在

**系统性的部署末 layout 实机验收、执行一个 turn 的能力。**
适配器能真正做出的动作是移动、装备、强化塔、释放装置、释放技能和能量塔技能，
其余只能装载结果；`project(step*(state(r), actions(r)))` 还没有与原生部署末
layout 批量比较过。

## 下一步

### 一、新增技能的初始冷却

面板新增的技能按 `initial_cooldown` 起算，本 build 全为零，现在由 `panel_add`
写死为零。规则不应依赖这一点，读表并在缺行时拒绝。

### 二、把规则从 convert 迁到 simulation

convert 现在借用了一些模拟规则来合成 battle 内容，例如增援发牌的预测、开局
重建与落位。这些模拟和验证规则之后迁到 simulation，由它暴露给 convert 调用；
迁移之前仍留在 document。迁移不改变任何输出，追踪集和指标都不能变。

### 三、战斗字段

模拟器接入后，把白名单上的字段逐项从依赖战斗改为预测。在那之前，非战斗叶
持续验收，不等待完整战斗实现。

## 各阶段共同的验证方式

- 小规模 state／action 构造做局部回归，覆盖冷却重置、额度、收入、交付顺序等
  规则的边界，用能区分错误实现的反例；不要求每例配一整份原生采集文件。
- battle 语料做跨回合回归，指标由断言钉住。追踪 YAML 只由转换器生成，错误
  变体在测试临时数据里构造。
- Python／mcscript 批量比较 `project(step*(state(r), actions(r)))` 与原生部署末
  layout，作为部署与投影的独立粗验证。原生一端必须来自回放或实际执行。它不
  覆盖战斗与初始化，不阻塞指标。
- 反汇编研究跟着具体字段的差异走。回放一致不能证明非法动作的拒绝正确；
  `MAP_BuyUnit`、`MAP_UpgradeUnit`、`MAP_ChooseReinforceItem` 的 `Perform`
  各支条件仍待核出。`PAD_Redo`、`PAD_ReleaseConstruction` 和塔强化第 4 级
  已经查过，不是缺口。

## 顺序

指标已经达标并由 CI 把守：追踪集每份 battle 的非战斗叶全部预测一致，跨回合转移
只剩 `predict` 一份。接着读表取初始冷却（一），再把规则迁到 simulation（二）。
部署末 layout 的实机批量验证、反汇编核对和原生 turn 执行按需穿插。战斗字段（三）
等模拟器。
