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

## 2259 长弓 vs 弧光对齐审查（2026-08-24）

本轮目标为 `1.11.1.3.2259`。反编译输入为 SHA-256
`9d2e3f163f74728da73b5dac474f76a0ebdaa3852718fbeb45ad8dec6b4360e5`
的 `GameAssembly`，对应 Cpp2IL ISIL 位于本轮审查产物
`/tmp/mechcore-2259-cpp2il-isil/IsilDump`。

通过反编译代码门禁并由录像复验的实现包括：

- `FightTeam.RefreshRandomData` 的每队单一 `GRRandom`，seed 为
  `(round + teamIndex) * 4444`；攻击间隔 offset 按技能刷新顺序消费该队流。
- `MotionController.CalculateTargetDirection`、`FightUtility.ConvertToAngle`、
  `FVector3.Angle` 和 `FPCSMath.SqrtFastest/AcosFastest` 证明不存在额外的 20 度
  转向死区。当前实现对本场景横向抖动输出部署朝向并通过录像，但尚未复现原生
  `Normalize -> Angle -> RawAcos` 的全部中间舍入，因此只按场景结论通过。
- `FightSkill.RefreshAttackInterval` 的逻辑步换算、至少一 tick 的下界，以及首次进入
  Attack 状态后到下一次更新才可发起攻击的更新顺序。
- `FightProjectile.Init/Update/Move` 的 Q32.32 移动、活动投射物
  `released=false`、普通投射物默认 rotation，以及从实际 transform 采集移除位置。
- `FightActor.ReduceLife` 返回的实际扣血量、投射物作为 `DamagePerformer` provider，
  以及伤害/销毁同 tick 的事件顺序。
- `FightMech.OnDead/ExitFight` 与 `FightTeam.ExitFight` 到
  `MotionController.ExitFight` 的即时 Idle 状态；无护盾单位初始
  `EnergyShieldController.enabled=true`。

本轮拒绝并删除了三个录像驱动但缺少代码支持的实现：

- `aim_tolerance: 20` 转向死区；
- Simulator 内硬编码的训练场建筑坐标、生命和碰撞状态；
- 将所有 `ProjectileRemoved.position` 强制写为缓存目标中心。

复验录像使用 seed `1787544888`，Adapter scenario/result hash 分别为
`ec0cb9554a1c17b6879092b40bd2285e4f93dfeba119555bd4040c62a0106f58` 和
`225f8a9acb3a75a681d934558eddf0c927da8e521214db859ef803748a1206c0`；Simulator
scenario/result hash 分别为
`66351a22c695be79284ec5135f750beb4973466b54f9a628534672e8cbec12e0` 和
`f1f68aec07063c34a889ea25aa4f65f2708a96e28049d82ddb9052b36c5cafb2`。
双方均为 92 个 tick；排除 Simulator 尚未获得反编译数值支持的四座训练场建筑后，
全部 S/E tick 均一致，没有首个分歧帧。完整 hash 尚不一致，因此不能宣称场景已完成
全量对齐。

仍未通过的具体数值包括训练场建筑属性、普通投射物当前硬编码的 `life/max_life=1/1`，
以及单位 YAML 中尚未逐项追溯到 2259 反编译字段来源的配置值。它们可以继续作为
待审配置输入，但不能记录为已确认的游戏数值。投射物 `IsLockTarget`、多单位场景下
队内技能刷新顺序、方向算法在其他方向和边界值上的完整定点等价性也仍需区分性
录像和反编译路径验证。

公共身份契约现为 `team_zx_sequential_v1`：Adapter 与 Simulator 都按统一世界坐标
`z`、`x` 的升序为队内初始 Unit 编号；MCFR `y` 仅表示高度。当前单单位对齐场景不能
独立验证多单位编号顺序，后续仍需用原生多单位录像复验该契约。
