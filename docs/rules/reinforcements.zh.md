# 增援发牌

[English](reinforcements.md)

这些规则覆盖 build **1.11.1.3.2259**，标准 1v1、无游戏规则，且在
[开局初始化](opening.zh.md)之后。种子决定一条随机流和初始池。之后的发牌还取决于双方
已上场的单位、已解锁的单位、已激活的科技和军官，以及此前的选卡。预测发牌不等于预测
这些决策或战斗结果。

## 随机流与池的生命周期

`ReinforcementSystem.OnEnterDeployment` 分派到 `GenerateReinforceItems`。第 1 回合不发
任何东西。之后的回合沿用开局留下的增援流，不重新播种。录像的
`MatchSnapshot.randomState` 是该回合生成之前的状态，`reinforceItems` 是生成出来的有序
发牌。初始化时选定的单位回合池决定这一回合用普通增援还是单位增援。单位回合会绕过普通
池的刷新和普通发牌。

普通卡的 scope 为 1，且没有场景限制或场景为 1。每个正的军官类型组最初贡献它被选中的
代表；类型为零的条目各自单独贡献。池按级别排序，再按 ID 排序。选中一张不可重复的卡会
把它从池里移除，并清掉它所属正类型组剩余的替补。放弃和选中可重复的卡都不改变池。这些
操作不消耗随机数。

## 普通增援

`ReinforcePool.OnNewRound` 检查双方已上场单位类型的并集。对于 `appearCondition = 1` 的
军官，它 `unitID` 列表里的每个类型都必须不在场。不满足的代表会被移除；在它所属组中仍
满足该条件的替补里，均匀地选一个补上——即使只剩一个也会消耗一次随机调用。没有合格替补
就不补。级别和原始 ID 按升序访问，替补只在该遍历结束之后追加，然后池重新排序。

`GetCurrentReinforceItemProbabilityData` 取回合数不大于当前回合的最后一行概率。
`GenLvProbs` 在某级别没有可用候选时把它的权重置零。候选必须满足 `earliestRound`，以及
在 `latestRound` 为正时满足它。配置的权重是：

| 回合 | 1 级 | 2 级 | 3 级 | 4 级 | 5 级 |
| --- | --- | --- | --- | --- | --- |
| 2 | 60 | 40 | 0 | 0 | 0 |
| 3 | 30 | 40 | 30 | 0 | 0 |
| 4 起 | 25 | 25 | 25 | 25 | 0 |

`RoundRand` 发四张卡。每一张先在剩余级别权重之和上均匀抽取，再由 `RandOnce` 在该级别
的候选窗口内抽取。窗口从该级别已取走的数量开始；它的索引指向完整的池，被选中的索引会
和该池当前的取用位置交换。窗口内只剩一个候选时，选择不消耗随机调用、也不做交换。某级别
被取尽会把它的权重置零。发牌之后池重新排序。

如果一次发牌里出现了 4 级的 `CommanderSkillBase`，那么下一个数值回合排除全部 4 级指挥官
技能。这取决于发了什么，与选了什么无关。单位回合不会把这个排除推迟到之后的普通回合。

## 单位增援

`GenerateUnitReinforceItems` 请求四张 `activeRound` 等于当前回合的卡。合格的行 scope
为 0，场景过滤相同。带 `preventUnitReinforcements` 的生效军官会排除它列出的单位类型。
两遍抽取都不能提供本次发牌已经选过的单位类型。

`UnitFirstRand` 还会额外排除当前已上场的单位类型，然后从按卡 ID 排序的候选里均匀抽取。
它的计数器从"请求数量与候选数量中的较小者"开始。每次抽取，计数器因为这次选择减一，
**并且为被移除的该单位的每个变体各再减一**。原生循环里两处递减都在。因此即使还有更多
合格的单位类型，这一遍也可能返回不足四张。

`UnitSecondRand` 用一个跨双方、按单位类型计算的投入分来填满剩余槽位：

- 每个已上场的单位加 `CardData.baseMoney`，不随等级缩放。
- 每个已解锁的商店单位加 `CardData.unlockPrice`。
- 对出现在这张合并表里的类型，每一方已激活的科技加上
  `UnitTechnologyManager.CalculateUpgradeCost`。

科技的计算遵循 `UnitUtility.CalculateUpgradeTechnologyCost`：基础升级价，加上此前已激活
科技的数量乘以该单位为正的 `techUpgradeIncreaseSupplyPerCount`，若不为正则乘以全局的
`Config.upgradeTechnologyCostIncreaseDelta`。为正的 `techUpgradeMaxSupplyLimit` 会给每
一项封顶。这是一个基础投入度量，不使用实付价格和军官折扣。标准单位行的步进覆盖和封顶
都是零，所以它们的步进是 200，科技的顺序不影响总和。非标准的带封顶单位类型不在本规则
已验证的范围内。

候选按分数排序、再按卡 ID 排序；缺失的单位分记为零。每个剩余槽位在分数最低的候选中均匀
抽取，并移除被选中单位的全部变体。只剩一个候选时仍然消耗一次抽取。两遍使用同一条增援流。

两遍之后，若回合数达到或超过所选计划的 `supplyRound`，最后一个选项会被替换为
`supplyReinforceID`，索引为 `round - supplyRound` 并钳制到最后一个条目。被替换的那张卡
的随机抽取已经发生过了。这个分支由原生方法和资源行确定；后期补给替换在验证语料里没有
录像覆盖。

## 放弃

每个回合在发牌之外都提供"放弃"，而放弃会给补给。普通回合给的是
`Config.noReinforcementSupply`，即 50。单位回合给的是所选计划的 `giveUpSupply` 在该回合
于 `roundGroup` 中位置上的取值：`ReinforcementSystem.m_RoundGiveUpSupply` 由
`UnitReinforceRoundPool.GetGiveUpSupply(round)` 构造，后者读的正是这个。计划 1 的单位
回合是 2、5、8、11，分别给 50、150、400、700。55 个标准计划有 55 份互不相同的列表，
数额随它所对应的回合增长：第一个单位回合是 50 到 150，之后是 150 到 350、400 到 600、
700 到 900。

没有任何一份本地录像同时给出单位回合的放弃和它之后的那一回合：本机 51 份 build 2259
录像共放弃 53 次，全部在普通回合；下载录像里唯一一次单位回合的放弃发生在对局的最后一个
回合。单位回合的数额来自配置，不是实测值。

## 输入与证据边界

[config/reinforcements.yaml](../../config/reinforcements.yaml) 由
`scripts/extract_reinforcements.py` 从配置导出和 `level0` 抽取。所有取值都是这些资源里
的整数或标志，没有拟合出来的权重、流偏移或评分系数。

| 输入 | 资源来源与单位 |
| --- | --- |
| 普通数量 4 | Config，MonoBehaviour 136，序列化体第 108 字节（`reinforceItemCount`，原生字段偏移 148）；卡 |
| 单位数量 4 | `commonParms.unit_reinforcement_quantity`；卡 |
| 级别权重 | `reinforceItemProbabilityDatas`；相对整数权重 |
| 合格性、级别、分组、回合限制、可重复性 | `ConfigDataContainer` 里的军官行；`level0` 中 MonoBehaviour 167 的指挥官技能与 188 的装备 |
| 单位候选与计划 | `unitReinforceDatas`、`unitReinforceRoundPool`；ID、整数回合，以及每个单位回合的放弃补给 |
| 单位投入 | `cardDatas.baseMoney`、`unlockPrice`、科技步进与封顶字段；补给 |
| 科技价格与全局步进 | [科技定价](unit_techs.zh.md)；补给 |

原生调用链是 `OnEnterDeployment` → `GenerateReinforceItems` →
`GenerateRoundReinforceItems` / `GenerateUnitReinforceItems`。普通发牌闭合在
`ReinforcePool.OnNewRound`、`CheckReinforeCondition`、`CheckAppear`、`GenLvProbs`、
`RoundRand`、`RandOnce` 和 `SelectReinforce` 上。单位发牌闭合在 `UnitFirstRand`、
`UnitSecondRand`、它们的排序回调，以及科技成本方法上。技能冷却的类型转换是通过二进制
元数据槽位绑定到 `CommanderSkillBase` 的，而不是从发牌吻合推断出来的。在中间反编译表示
不完整的地方，第一遍的双重递减和成本方法的尾调用由原生指令解决。

二进制与资源的身份见[开局的制品身份](opening.zh.md#证据与边界)。证据是原生控制流加上
抽取出来的资源字段，并与录像的选项数组和前后相继的原生随机状态比对。改动过的池、其它
模式、负种子和非标准的带封顶单位类型仍在受支持范围之外。最后一个被记录的回合没有后继
快照来独立检查它的出口状态。制品身份变化、范围扩大，或原生的一次发牌或流边界与本流程
不一致时，重新打开本文。
