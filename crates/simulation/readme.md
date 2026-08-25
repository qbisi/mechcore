# mechcore-simulation

该 crate 根据布局、配置和随机种子执行确定性战斗模拟，并输出与 Adapter
相同格式的 MCFR。Simulator 与真实录像对齐时采用“大胆假设、严格求证”的
对抗式研究流程，但能够复现录像不等于已经证明游戏机制。

## 机制与数值的证据门禁

机制推演和证据审查由两个相互对抗的 Agent 承担：

- 机制推演 Agent 可以根据首个分歧 tick 提出并实现可检验的临时假设，用于继续
  模拟、生成预测和设计区分性场景。
- 证据审查 Agent 独立检查假设是否得到目标游戏版本反编译代码的支持，并检查
  Simulator 是否按该代码在 Adapter 录像中复现了可观察结果。

一项机制或具体数值必须依次通过以下门禁：

1. **反编译代码门禁**：机制必须能追溯到反编译代码中的分支、状态转换、公式或
   调用链。具体数值必须能追溯到反编译代码中的常量、原生字段来源或确定性计算
   过程，并记录单位与精度转换。缺少这项支持的机制或数值不予通过。
2. **录像对齐门禁**：在代码证据成立后，通过 Adapter 原生采集的 S/E 和 MCFR
   首个分歧 tick 检查 Simulator 是否正确复现该机制。

Adapter 录像、边界实验以及 MCFR tick/result hash 可以用于定位分歧、排除错误
假设和验证已有代码解释，但不能单独证明机制或确定具体数值。单个或多个场景的
hash 一致，也只表示这些场景在当前 S/E 契约下输出一致。

未通过反编译代码门禁的实现只能标记为临时假设，不得作为已确认机制、已确认
数值或模拟对齐结论通过审查。配置文件中的既有数值和旧项目资料本身不构成当前
目标版本的反编译证据。

## 最小证据记录

通过审查的机制至少应记录：

- 目标游戏版本；
- 反编译函数地址或稳定标识、相关调用链及关键控制流；
- 数值的原始来源、计算过程、单位和精度转换；
- 用于验证实现的录像场景、seed、MCFR hash 和首个分歧结果。

## Agent 自动化研究管线

### 闭合单元与目标

最小研究单元是固定的 `(game_build, layout, seed, MCFR schema)`。同一单元内只研究
一个具体 layout，并从 tick 0 开始反复执行“假设—对比—审查”，不得跳过尚未解释的
首个分歧 tick 去拟合后续结果。

一个单元只有同时满足以下条件才是**完整闭合**：

1. Adapter 与 Simulator 的 `scenario_hash` 相同；
2. 两者 tick 数量、`terminal_tick`、每个 `tick_hash` 和 `result_hash` 全部相同；
3. 该 layout 实际经过的机制分支和使用的具体数值均通过反编译代码门禁；
4. 审查 Agent 明确接受结论及其适用范围。

Hash 全等但仍使用未证实机制或数值时，只能标记为**输出已对齐**，不得推进外层
layout 队列。诊断时可以使用字段投影定位问题，但忽略某类实体或字段后的相等不能
作为完整闭合标准。

若某候选机制有唯一、充分的反编译代码支持，当前 layout 的输入和运行路径却无法区分
该机制与竞争实现，同时现有逐 tick 输出完全一致，审查 Agent 可以给出
`accepted_pending_scenario`。这表示候选实现可以通过**当前 layout** 的审查，不表示该
机制已经获得录像区分性验证。调度 Agent 必须立即设计一个能触发差异预测的最小合法
layout，并将其插入研究队列最前；该机制在前置验证 case 闭合前不得提升为公共已闭合
机制。

### Agent 分工

为保持假设与证据审查相互对抗，管线只设三个职责角色，不增加游戏领域实体：

| 角色 | 职责 | 禁止事项 |
| --- | --- | --- |
| 调度 Agent | 管理内外循环；冻结 build/layout/seed；调用 Adapter 和 Simulator；验证 MCFR；生成首个分歧；维护本地队列 | 不提出或批准机制解释；不越过首个分歧；不自行修改公共 MCFR |
| 假设 Agent | 根据首个分歧和反编译调用链提出一个可证伪解释；给出预测；在 Simulation/config 的最小范围内实现候选修正 | 不用录像拟合具体数值；不把配置、旧项目或相邻状态差分当作代码证据；一次不混入多个机制修正 |
| 审查 Agent | 独立复核反编译来源、单位/精度换算、实现等价性和复验结果；给出接受、拒绝或阻断结论 | 不修改候选实现；不以 hash 相等替代反编译证据；不接受自己提出的假设 |

调度 Agent 可以复用已经闭合的公共机制，但必须检查其 game build 和适用分支。假设
Agent 与审查 Agent 必须使用不同上下文分别工作；审查输入包含候选 diff 和证据，不
包含要求其证明候选正确的引导性结论。

### 内层：单 layout 逐 tick 闭环

调度 Agent 按以下状态机执行，每轮只允许推进一个状态：

1. **冻结输入**：保存 layout 原文及 hash、目标 build、unit config、MCFR schema 和当前
   提交；确认 Adapter 与 Simulator 使用同一逻辑步和身份契约。已有原生录像的 case
   同时冻结其 seed；新 case 在下一步由原生录像确定 seed。
2. **原生采集**：通过 MCP 在全新试验场中应用 layout 并记录 Adapter MCFR。相同
   build/layout/seed 且 Adapter 与 MCFR schema 未改变时可以复用已验证录像；否则必须
   重新采集。新 case 必须从已验证 MCFR 的 `D.match_seed` 读取并冻结 seed，不假设
   Adapter 接受外部 seed。
3. **模拟输出**：执行 `mechcore sim layout.yaml --seed ... --config ...`，保存 Simulator
   MCFR 和结构化战斗结果。
4. **规范对比**：结构化打开两个 MCFR，直接比较持久化的 tick hash。若
   `scenario_hash` 不同，先逐字段比较 `D` 与 `S(0)`，不得直接比较后续 tick。若相同，
   使用 `first_divergence` 找到首个不同 tick，只读取并比较该 tick 的 `S` 与有序 `E`。
5. **单一假设**：假设 Agent 只针对这个首个分歧提出一个机制或数值解释，并记录：
   反编译 selector/地址、调用链、关键分支或字段、原始数值、换算过程、预计改变的
   首个 tick/字段，以及能够排除的竞争解释。若当前 layout 无法产生区分性预测，必须
   明确说明未触发的条件，并同时给出最小验证 layout 的字段变化和预测。
6. **候选实现**：只修改使该预测可检验的最小 Simulation 或 unit config 范围。未经
   代码证据支持的数值不得进入候选实现；研究性 `I` 只能使用 Adapter 可直接采集的
   原生 probe，且不进入正式 hash。
7. **独立审查**：审查 Agent 检查静态证据是否唯一支持候选解释、实现是否逐步等价、
   是否夹带其它修正，以及录像是否真的把首个分歧向后推进或消除。若当前 layout 无法
   区分机制，只能在代码证据充分且逐 tick 全等时裁定 `accepted_pending_scenario`。
8. **裁决**：拒绝则回滚候选并记录反例；接受则保留实现，重新从步骤 3 对比；
   `accepted_pending_scenario` 还必须先将验证 layout 插入队首。首个分歧消失后仍须审计
   本场景实际使用的所有机制和数值，满足完整闭合条件后才结束。

每个候选必须给出可判定预测，例如“tick N 的第一个 `Damage.amount` 应由 A 变为 B”
或“首个分歧应从 tick N 后移到 tick M”。“整体更接近”不是可接受预测。Native 录像
不是每次候选修正都重跑；只有输入、Adapter、MCFR schema、原生 probe 或目标 build
改变时才重新采集，避免把墙钟噪声引入循环。

### MCFR 承载不足时的中断分支

当反编译代码证明某一事实参与下一 tick 的状态转换，但当前 `S/E` 无法区分竞争机制
时，三个 Agent 必须先判断它是否为 MCFR 承载不足，而不是继续给 Simulator 增加隐藏
状态。只有同时满足以下条件才能进入该分支：

- 该事实会影响正式模拟或事件顺序；
- 当前 `S/E` 没有直接表达它，且不能把相邻快照差分当作替代；
- Adapter 存在同一逻辑边界或原生操作上的直接采集路径；
- 新字段是来源中立的基本游戏事实，不是为当前 layout 定制的分析结果。

“当前 layout 没有触发区分条件”本身不是 MCFR 承载不足。只要现有 `S/E` 在触发该
机制的合法 layout 中能够显示差异，就应先建立前置验证 case；只有换用区分性 layout
后仍缺少必要的原生事实，才进入 MCFR 中断分支。

满足条件时，将当前单元标记为 `mcfr_blocked`，立即暂停内层和外层循环，并向用户说明
首个分歧、缺失事实、原生采集路径、预期字段以及对 hash/写入器/读取器/播放的影响。
在得到修改公共 MCFR 的明确许可后，才同步修改中英文规范、类型模型、HDF5 投影、
Writer/Reader/hash、Adapter producer、Simulator producer 和测试。MCFR 改动完成后，
所有旧录像作废，必须从同一 layout 的步骤 1 重新开始。

若该事实无法由 Adapter 直接采集，则不得加入 `S`、`E` 或 `I`，当前单元标记为
`evidence_blocked` 并停止；不得用 Simulator 私有状态、配置常量或差分推导补齐。

### 外层：由简到难枚举 layout

外层循环以 [layout 规范](../../docs/layout.md) 的字段和封闭可选值为搜索空间。每个新
layout 必须以一个已完整闭合的父 layout 为基础，只改变一个字段、一个离散选项或一个
边界维度；当前 layout 未闭合时不得生成下一个。合法性始终由 Adapter layout compiler
判定，Agent 不研究非法组合。

`accepted_pending_scenario` 生成的验证 layout 是唯一的队列抢占项：它绕过正常 P0–P6
顺序插入队首，以产生反编译机制所预测的最小可观察差异。它仍应以当前已闭合 layout
为父项，并只改变触发该机制所必需的最少字段。验证 case 结束后才恢复原外层顺序。

优先级固定如下：

1. **P0：全部普通非超巨型 Formation Unit 基线**。`round: 1`，从每队一个 level-1、
   未旋转、无装备、非 travelling 的 Formation 开始；Officer、单位科技、Research Center、
   Energy Tower 和 `battle_skills` 均保持基线。每队一个 Formation 是最小起点而不是
   场景上限：目标失效与重获、范围目标选择、攻击方向切换或本 Unit 的原生 Formation
   成员生成需要额外对象时，必须加入关闭相应 obligation 所需的最少基线 Formation，
   仍计入 P0。按冻结的 coverage manifest 逐 Unit 闭合完整 Formation 生成、其原生接敌
   与基础攻击路径、结果和配置证据。
2. **P1：已闭合单位的基础边界扩展**。在 P0 的已闭合 Unit 上，通过位置变化依次
   触发静止射程内、直线接敌、斜向接敌、射程边界、额外转向、不同 target domain
   和已经存在但 P0 最小 layout 未经过的攻击路径边界。
3. **P2：额外多 Formation 交互**。P0 已经为单位自身的基础攻击路径加入必要的最少
   Formation；P2 只在这些 obligation 之外增加 Formation，研究组合性的队内更新顺序、
   目标选择、范围伤害、同 tick 事件顺序、随机流消费和身份分配。不得把 P0 必需的
   多目标分支推迟到 P2，也不得用多单位场景反向拟合尚未闭合的单单位基础数值。
4. **P3：Unit 可选字段**。依次改变 `level`、`rotated`、`round`/ambush 位置与
   `travelling`、`equipment`；每次只启用一个非基线值，并为离散边界建立最小区分性
   layout。
5. **P4：其它 Formation 类型**。依次加入 Construction、shield、interceptor 和
   missile，先闭合其显性状态和生命周期，再研究与 Unit 的交互。
6. **P5：Side 与战场修饰**。依次研究 `research_center`、`energy_tower`、Officer、单位
   technology 和 `battle_skills`。纯数值修饰先于改变实体、事件顺序或生命周期的机制。
7. **P6：组合回归**。仅组合已经单独闭合的字段，用 pairwise 和已知机制交互选择场景，
   不对全部 layout 值做无界笛卡尔积。

同一优先级内按“默认值 → 单个非默认值 → 边界值 → 两个已闭合机制的交互”推进。
双方状态先保持镜像，只在研究目标明确需要时改变一侧，避免一次引入阵营与机制两个
变量。
新增 Unit config 必须一单位一 YAML，并先追溯其基础生命、移动、旋转、碰撞、攻击类型、
目标域、伤害、射程、间隔、释放延迟和投射物字段的原生来源。当前阶段不因科技需求提前
扩展 Unit 公共描述。

### P0 coverage manifest 门禁

P0 不是“找到一个能够闭合的无科技 layout”，而是目标 build 中全部普通、可部署、
非 `UnitType.Huge` Formation Unit 的有限覆盖任务。开始逐 layout 研究前，调度 Agent
必须建立并冻结 coverage manifest。单位集合必须同时由目标 build 的原生
`ConfigDataContainer`、原生 `UnitType` 判定和 Adapter 可部署目录证明，不能从现有
Simulator config、名称或占地尺寸反推。

Manifest 为每个 Unit 固定以下公共 obligation：

1. 完整 Formation 的生成、成员数量、成员身份和初始快照；不得把多成员 Formation
   缩减为一个代表单位；
2. 目标获取、目标失效后的重新索敌，以及该 Unit 实际需要的移动、转向和进入原生交战条件；
3. 完成一次原生基础攻击路径；
4. 该路径产生的伤害或其它原生基础效果、相关死亡和战斗结束；
5. 本 Unit config 中每个字段的目标版本代码或序列化数据来源；
6. 所有实际经过分支的逐 tick hash 全等和独立审查结论。

基础攻击 obligation 必须由目标 build 的原生技能类型和分支生成。投射物单位验证其
实际单发/多发、飞行和命中路径；射线、控制束和直接效果单位只验证其实际路径。近战
Unit 的最小 layout 不得从已满足攻击条件的位置开始，而必须从目标 build 所定义的原生
交战判定范围外开始，并实际经过有效目标下的追击、非零转向、首次进入交战边界、移动到
攻击的状态转换、无投射物伤害，以及连续攻击直至相关死亡和战斗结束。攻击准备、攻击点、
生效、后摇和冷却必须作为独立 obligation；配置与代码使某阶段为零且不可进入时，经独立
审查后标记该阶段 `not_applicable`，不得把五个阶段合并成一个无法判定的 timing obligation。
若代码证明交战边界是距离或技能范围而非物理碰撞，就只称为“近战交战边界”；
不得因 `isMeleeAttack=true` 字段或单位名称预设“碰撞触发攻击”。

攻击路径、武器拓扑与效果拓扑是三个正交门禁。带范围效果的 Unit 除完成其原生投射物
或直接效果路径外，还必须验证原生作用中心、范围内目标选择、范围外排除和确定性目标
顺序；只命中一个主目标不能关闭范围效果 obligation。
目标死亡/失效后的重新索敌与活目标周期切换是两项独立 obligation。目标版本代码表明，
死亡或失效目标会绕过 `quick_switch_target` 比较直接重获；该字段必须在旧目标仍存活、
周期搜索选出不同目标的分支验证。值为 `false` 时该次 attackability 检查失败并按当前
技能状态退出 Prepare 或结束 Attack；只有 `coolingTime > 0` 的 Attack Finish 才进入
CoolingState，不能把“禁止活目标快速切换”解释为“死亡后先冷却再换目标”。

每个 path-specific obligation 必须在 Unit 条目下单独记录状态。不得用一个聚合的
`open`/`closed` 覆盖整条攻击路径；未列出 resolution 的 obligation 默认为 `open`。
某项只有在反编译证据证明该 Unit 不可能经过时，才可由独立审查标记为
`not_applicable`；未实现、未触发或 MCFR 不可见都不是 `not_applicable`。

“最少合法 layout”只允许一个 layout 同时关闭它真实经过的多个 obligation，不允许
删减 obligation，也不允许以“基线每队一个 Formation”为理由把需要额外目标的
obligation 标记为 `not_applicable` 或推迟到 P2。每个 layout 仍是独立闭合单元和独立提交。只有 manifest 中全部 Unit
的全部 obligation 都为 `closed` 或经独立审查确认的 `not_applicable`，P0 才完成；队列
为空、一个参考对局闭合或一种攻击路径闭合均不构成完成。

P0 campaign 的 `game_build`、MCFR schema 和完整 config snapshot 必须在首个正式 case
前冻结。任一项改变都会使 campaign 内既有 case 失效并要求重新采集、模拟和审查。
冻结前取得的旧 case 只能作为候选机制证据，不能直接计入 manifest 的最终完成状态。

### 本地状态、产物与恢复

所有自动化状态和研究产物都位于 Git 忽略的 `work/research/`：

```text
work/research/
  pipeline.yaml
  <case-id>/
    layout.yaml
    native.mcfr
    simulated.mcfr
    comparison.md
    hypotheses.md
    review.md
    evidence/
```

`pipeline.yaml` 维护 campaign/coverage manifest 指针、当前冻结状态、`current`、
`queue`、`closed` 和 `blocked`。每个 case 至少记录
`id`、父 case、build、layout 路径/hash、seed、唯一变化字段、状态、首个分歧和下一动作；
前置验证 case 还要记录 `verification_for` 及待验证的 tick/字段预测。
允许的状态为 `queued`、`running`、`output_aligned`、`closed`、`rejected`、
`mcfr_blocked` 和 `evidence_blocked`。Agent 每完成一个状态转换就原子更新本地文件，
使下一次会话从最后一个已验证边界继续，而不是依赖对话历史。

当用户要求继续 Simulation 研究时，调度 Agent 必须先读取本 README 和
`work/research/pipeline.yaml`：存在 `current` 就从 `next_action` 恢复；不存在则从
`queue` 取首项；队列为空时才依据 `layout.md` 和上述优先级生成一个子 layout。内层
循环持续到 `closed`、`mcfr_blocked` 或 `evidence_blocked`；`closed` 后立即生成并进入
下一个外层 case。除公共 MCFR 改动、新原生 Hook、缺少反编译输入或真实游戏不可用等
需要新权限/外部状态的阻断外，不因单次假设失败或会话结束放弃当前 case。原生游戏操作
只能由调度 Agent 串行执行；离线反编译搜索和独立审查可以并行，但不得共享结论上下文。

代码和 config 的候选修改在审查接受前不得提交。研究日志、录像、反编译产物和临时
`I` sidecar 永不进入 Git。经审查接受的通用实现及测试可以进入仓库；提交仍需用户明确
许可。

### 已闭合机制的提升

一个机制或数值只有在通过反编译与录像两道门、覆盖其声明适用分支、无未解释首个分歧，
并获得审查 Agent 接受后，才可以从本地研究日志提升到本 README。提升内容只描述稳定
机制、数值、目标 build、适用范围和实现位置；场景过程、临时假设、hash 和反编译临时
路径仍留在 `work/research/`。

`accepted_pending_scenario` 只允许当前 layout 继续闭合和实现暂存；在其队首验证 case
完成前，相关机制或数值不得出现在下面的已闭合表中。

当前已接受的 build `1.11.1.3.2259` 基础机制范围：

| 机制 | 已闭合范围 | 尚未覆盖 |
| --- | --- | --- |
| 攻击间隔随机流 | `FightTeam.RefreshRandomData` 为每队建立 `GRRandom`，seed 为 `(round + teamIndex) * 4444`；当前单成员场景消费结果已对齐 | 多成员、多个技能的队内刷新顺序 |
| 攻击调度 | `RefreshAttackInterval` 的逻辑步换算、至少一 tick 下界，以及首次进入 Attack 后下一次更新才可释放 | 其它技能状态机分支 |
| 基础方向 | 不存在额外的 `aim_tolerance: 20` 转向死区；当前场景部署方向已对齐 | `Normalize -> Angle -> RawAcos` 全方向和边界舍入 |
| Normal 目标评分与开战前索敌 | build2259 在可见、全旋转、未分裂四叉树且唯一最优的当前普通地面目标集合中，以 Q32 边缘距离、最小射程严格排除、角度因子、射程外惩罚和严格最小分数选择目标；`FightPrepareState` 在 S(0) 前完成首次索敌并同步初始朝向 | 分裂四叉树、同分候选、建筑胜出、移动候选重插入和其它 selector mode |
| 普通投射物 | `Init/Update/Move` 的 Q32.32 移动、活动时 `released=false`、默认 rotation 和实际 transform 移除位置；build2259 长弓/弧光普通单投射物在 `isLockTarget=true`、目标存活且移动、`randomTargetRange=0`/offset=0 分支逐 tick 刷新目标 root Q32 位置 | 非锁定、目标死亡、非零随机 offset、拦截及其它投射物类型 |
| 伤害与死亡 | `ReduceLife` 将实际扣血裁剪为 `min(currentLife, incomingDamage)`；Adapter 的 Damage hook 直接记录原生 performer 返回值。除已闭合的 Projectile provider 外，build2259 犀牛主技能 5001 在 level-1、无科技/装备/动态 buff/护盾干预的当前单目标直接效果中，经 `SkillDamageProvider -> FightSkill.GetDamage -> DamageProperty` 得到 nominal 3560，末击按剩余生命裁剪 | 多目标范围伤害、修正链、护盾和其它 provider/目标域 |
| 个人护盾基线 | 无护盾单位的 `EnergyShieldController.enabled=true` 初始状态 | 实际护盾激活、吸收和销毁生命周期 |
| 普通首发延迟 | 当前 P0 普通首发分支分别将 `prepareTime` 与 `attackPoint` 除以逻辑步并截断；长弓为 10+2 tick，弧光为 0+0 tick | 重复攻击、grouped/loading、后摇及其它阶段调整；不得推广为两字段相加的通式 |
| 普通同步直接攻击后摇 | build2259 犀牛普通直接攻击在 effect tick 进入 9-tick backswing；wait controller 每 tick 先递增再以 `counter >= duration` 完成，因此第九次 wait update 所在 tick 仍不能重新发起攻击，下一 tick 才重新进入 `TryPerformAttack` | 前摇中目标失效、第三方击杀、快速换目标及其它 controller |
| 普通同步直接击杀后的死亡目标重获 | build2259 犀牛由自身同步直接攻击击杀当前目标后，在该次 after-wait 结束前保留死亡目标并保持 Idle；已闭合录像的公开边界为 tick 223 击杀、224–232 保留、233 清除、234 已有新目标并重新进入 Move | 新目标私有引用在 tick 233 或 234 的精确写入点不可由 S/E 观察；第三方击杀、活目标周期切换及其它技能状态仍未覆盖 |
| 主技能瞄准命令 | build2259 长弓、弧光在 `isHaveBody=true` 的普通主技能攻击转向分支中，将同一次 `CalculateTargetDirection` 生成的同一 direction 局部值分别传给 mech body 与主武器，因此 `independent_aim=false`；mech body 不是 MCFR unit root | 无 mech body、其它 weapon mode、特殊技能及独立方向源 |
| 一对一直接击杀的终局顺序 | build2259 中 `FightCoreSystem.TeamUpdate` 在更新本队单位前缓存 alive count；较早 Team 的同步直接击杀可被较晚 Team 在同 tick 重建为零，随后 `TryDstroyTower` 销毁建筑。塔死亡晚于该 tick 的 `DeadEffectSystem.Update`，因此还需一个 drain tick；战斗已满足 finish gate 时不再执行后续 RVO | 多成员/多 Group、召唤、重生、Construction、护盾、同 tick 混合伤害和任意 drain 长度 |

下表是旧 Marksman/Arclight 参考场景已闭合的 level-1、无科技、无装备、无动态 buff
字段证据；它们不计入当前 23 Unit campaign 的 manifest completion：

| Unit | life | radius | move | rotate | damage | range | interval | offset | prepare | attack point | backswing | cooling | projectile speed | splash |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Marksman | 1622 | 8 | 8 | 70 | 2329 | 140 | 3.1 | 0.6 | 0.5 | 0.1 | 0 | 0.2 | 500 | 0 |
| Arclight | 4813 | 9 | 7 | 80 | 365 | 95 | 0.9 | 0.3 | 0 | 0 | 0 | 0 | 300 | 7 |

其中时间小数是目标 build Q32.32 原始字段的 YAML 表示；原生逻辑按字段分别换算为
tick。Marksman 的目标域为 ground+air，Arclight 为 ground；两者均为 ground Unit、
`lock_target=true`、`quick_switch_target=true`、`target_offset_radius=0`；两者均为
Normal 拓扑的单投射物路径。零/非零 `splash_radius` 只表示效果拓扑，不是游戏原生
攻击类型枚举。

## 研究日志

具体场景、游戏 build、反编译位置、临时假设、录像 hash、首个分歧和审查结果属于
研究过程数据，统一写入仓库根目录下 Git 忽略的 `work/research/`，不进入本 README
或 Git 历史。日志文件应至少以日期、场景和目标 build 区分，保留上述最小证据记录。
