# 目标与计划

两件事：从录像离线生成完整的 battle 文档，以及让部署的转移方程成立并可验证。
转移有两个粒度：`step(state, action)` 推一个决策，`apply(state, actions)` 推一个
回合。战斗那一段由模拟器负责，部署那一段由文档和目录负责。

本文件只写两样东西：什么存在、什么不存在，以及接下来做什么。

数字不写在这里。追踪集的指标由 `crates/document` 里的断言钉住，一变就红；
本机语料的指标跑 `mechcore convert` 和 `mechcore verify` 当场得到。写进文件的
数字只会变旧，而且没有人会发现。

设计上还没定的选择也不写在这里，它们写在对应 spec 的 Unresolved 一节，
挨着它们影响的那份契约。

执行中撞见、但还没决定要不要处理的问题也不写在这里，它们开成 GitHub issue。
什么算一条 issue、它怎么离开，由 `.github/CONTRIBUTING.md` 规定。

## 已经存在

- 五份文档契约互相引用：layout、state、turn、action、battle，外加 mcfr、
  mcscript、session、adapter、unit-rules、rvo、quadtree 七份。全部按
  `docs/README.md` 的 spec 规范写成，主文档一律英文，由 `scripts/check-docs.py` 把住。
- 离线转换器 `mechcore convert`。别的 build、下载录像、试验场对局、带游戏规则
  的对局和回合不连续的录像都按名拒绝，不猜。能量塔的债有两种读法，对不上也拒绝。
  追踪集没有一份被拒。
- 留存物体从释放它的技能自己的 `rangeItems` 读出来：空投护盾 800001 和粘性油弹
  400002。护盾没有寿命，油弹的寿命逐回合减到零就消失，两者由 `round` 分开。
- 七份配置表进入版本库，覆盖单位价、科技价、卡价、军官效果、开局、蓝图、
  建筑回收和每回合收入，由 `scripts/extract_prices.py` 从 build 2259 生成。
- 补给账本，接进转换并全闭合。可判定的回合没有留下无法定价的。
- 转移函数 `transition::apply`，产出下一回合 state 中战斗不决定的九个字段，
  含两个分配器和装备清单。
- 回合内部署转移 `transition::step`：把一个决策作用到它被做出的那个局面上，
  产出完整的下一个局面，棋盘也在内。两个不由任何表决定的输入单独报出，不猜：
  卡牌召唤的编队落在哪里，以及经验技能给多少经验。
- observation 读取器 `observe` 与 oracle `oracle`，由 `mechcore verify` 驱动。
  把原生录像采集的每个局面映射成文档 state，把每条原生动作映射成文档 action，
  再两种方式核对：逐决策比对全部字段，和把折叠后的整回合序列从回合初推到部署末。
  `verify` 按文件自报的 schema 分派，批量靠管道，不自己展开目录。
- 投影 `project(state) → layout`。
- 认输写在 battle 根上：它结束一局而不是移动一个位置，所以不在动作序列里。
- 适配器能装载一份 layout 并把同一份采集回来，录像与试验场的往返对得上。
- battle 级部署状态采集 `record_replay_battle`：输入原样录像和新 JSONL 路径，
  记录开局选择、所有回合的初始化、双方动作和结束部署边界。
  回合通过原生录像跳转串联，跨回合的 `round_jump` 和跳转中的 `replay_reset`
  与部署动作明确分开，不把快照差异冒充动作效果或已观测的战斗过程。
  逐回合核对源录像动作覆盖和状态连续性，末尾写明源录像耗尽；结束部署后的
  认输另列为未实机观测的源事件，不推断末回合战斗结果。校验和清理完成后才
  发布整场文件，单回合只作为内部步骤，没有旧公开入口或兼容别名。
- 批量 oracle 语料器 `scripts/export-replay-corpus.py`。`tests/grbr` 的 build 2259
  标准 1v1 原样录像逐份独立转换和采集。JSONL 完整输出不覆盖，源/输出身份、拒绝
  和失败逐步原子写入本机 `work/replay-corpus/manifest.json`，中断后可续跑。

## 还不存在

**回合开局的转移。** 原生 observation 到文档 state 的映射已经完成，录像动作
参数也逐字段核对过，缺的是动作之外的那一段：军官交付、增援发牌、补给收入、
开局编队落位都发生在 `OnEnterDeploy`，不由任何一个决策产生。`exp` 和
`reactor_core` 在战斗里的变化同样不在部署转移的职责内。

**执行一个 turn 的能力。** 适配器能真正做出的动作是移动、装备、强化塔、
释放装置、释放技能和能量塔技能；购买、解锁、升级、科技、蓝图和选卡只能装载
结果。装载一个初始条件和做出一个合法决策是两种能力，后者还没有。

**一个字段转换器填不出来。** `opening_offers` 要复现增援池的概率表与抽取顺序，
而录像只存了随机状态与池操作日志。

`terrains` 不在这里了：读取端按技能查目录取地形类型，任何留下战场区域的技能都能
装进文档。编译端另说——点半径和点数量属于产生它的技能，随包只量到了粘性油弹的，
其余类型按名拒绝进 plan。标准 1v1 里这条拒绝从没触发过，因为只有粘油弹持续两
回合，别的都在下一个快照之前就没了。

**一个数量出来而不是读出来。** "这一回合多买一个"的 1：能量塔技能 3 和增援卡
10004 各加一次购买额度，能量塔那张表根本没有额度字段，`OfficerData.IsAddExtraUnit`
是个布尔而不是数。成立，只是出处不明。

科技步长 200 已经读出来了，出处在 `Config.upgradeTechnologyCostIncreaseDelta`：
单位自己的 `techUpgradeIncreaseSupplyPerCount` 为零或更小时，
`UnitUtility.CalculateUpgradeTechnologyCost` 回落到这个全局字段，而标准单位随包
全写 0。放弃增援的 50 是 `Config.noReinforcementSupply`，名字和值都对得上，但从
这个字段到增援项的调用链没追通，只能算按名对上。

## 下一步

录像 oracle 已经建立，回合内的部署转移在语料走过的路径上逐决策对齐到全字段。
剩下的是回合开局那一段、整份 layout 的对比，以及把转移闭到反汇编上。主动执行
turn 是另外的能力，不作为前置条件。

### 一、回合开局与 layout 验收

1. 把回合开局本身写成转移：军官交付、增援发牌、补给收入、开局编队落位都发生在
   `OnEnterDeploy` 而不是某个动作上，语料里每回合最后一次 initialization 的
   after 就是它的验收目标。做完之后 `apply` 的九字段可以由 `step` 折叠推出，
   两条路径不再各算各的。
2. 验收 `project(step*(state, actions))` 与实机 layout 的相等。跨战斗的反应堆、
   经验、存活和留存效果另由模拟器负责。
3. 补上两个未决输入：卡牌召唤的落位由 `MechPositionManager` 决定，经验技能的
   给予量没有任何随包的表给出，两者都要实机或反汇编确认。

### 二、把转移闭到反汇编上

`step` 现在只由语料支撑。语料验证的是已经发生过的动作效果，非法动作的拒绝行为
和语料没走过的分支不能由回放相等宣称覆盖。

按 `MAP_MoveUnit.Perform` 的做法读下 `MAP_BuyUnit`、`MAP_UpgradeUnit`、
`MAP_ChooseReinforceItem` 这几个 `Perform`，把每一支的条件核出来。

语料里计数为零的三处已经查过，都不是缺口：`PAD_Redo` 在实战界面里没有按钮，
只挂在 `DevelopmentTool` 的快捷键上；`PAD_ReleaseConstruction` 要游戏规则
`IsConstructionBoughtEnabled` 才会出现在面板里，标准 1v1 不开任何规则，转换器
也拒绝带规则的录像；塔强化第 4 级的价是从 build 的 `towerStrengthenDatas` 读出
来的，查表按等级取值没有分支，语料到没到过第 4 级不影响它。

### 三、实机相关

按验证结果补充：

- 补齐原生 turn 执行能力，验证正常合法性检查与录像播放路径的差别。适配器现在
  能真正做出的是六个动作，其余只能装载结果。

## 顺序

先补回合开局转移，再做 layout 验收。二和三都不阻塞一。
