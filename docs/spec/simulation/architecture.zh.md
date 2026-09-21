# 战斗架构

[English](architecture.md)

[TOC]

## Scope

本契约规定模拟器的形状：什么是一个机制、它住在哪里、由谁驱动、单位的数值怎么算
出来。它是各机制契约挂靠的框架，不规定任何单个机制做什么——那属于
[rvo.md](rvo.zh.md)、[quadtree.md](quadtree.zh.md)、[`docs/rules/`](../../rules)
和每个机制自己的索引。

这个形状不是设计出来的。build `1.11.1.3.2259` 本身有一套模块架构、对象模型和派生值
层，本文把三者都照着镜像过来——一个和被复现对象形状一致的模拟器，才能一块一块地对着
它验收。当 build 的结构和"好写的结构"冲突时，以 build 为准，并写下理由。

**证据覆盖到哪里。** 这里的结构读自本地反编译索引
`work/unity-index/1.11.1.3.2259/index.sqlite`：Cpp2IL 产出的 36,361 个类型、
301,615 个方法、662,014 条调用边。下面每一份清单和表格都由
[`scripts/fight-structure.py`](../../../scripts/fight-structure.py) 重新生成。
该索引**不含方法体**，所以它能确立的是归属关系和调用边——哪个类型存在、它拥有什么、
它调用了谁；它确立不了算术、分支条件，以及一个方法体内部的调用顺序。下面每一句都属于
前一类，需要后一类才能回答的都在 [Unresolved](#unresolved)。

## 原生形状，一张图

```text
              IMatch
                │  持有
                ▼
          IModuleManager ──────────────────────────────────┐
                │  GetModules()                            │
                ▼                                          │
     FightModule（共 35 个）                                │
       Init · OnActive/OnDeactive · OnEnterFight/OnExitFight
       OnFightStart/OnFightEnd · Update · IsStepFinish · Stop
                │                                          │
                │ 驱动                                      │ 拥有
                ▼                                          ▼
          FightActor                                   旁路对象
       ├── FightMech    （一个单位）            RangeItem · FightEnergyShield
       └── FightCrystal （塔、建筑）            Projectile · FightInterceptor
                │
                ├── BuffManager   ── 对生效中的 Buff 做聚合
                ├── DataSet       ── 每实例的覆盖层，每笔改动带 IDataModifier
                └── FightSkill(s) ── 各自的 DataSet，盖在共享的 ISkillData 上
                            │
                            ▼
                     FightProperty
            AttackInterval · AttackRange · Damage · MoveSpeed
            ProjectileCount · ProjectileDuration · …
            带缓存，由 DataSet 的变更监听器置脏
```

三句话就是全部：

- **凡是会跑的东西都是模块。** `FightCoreSystem` 只是 35 个里的一个，不是一个挂着
  其它东西的特权主循环。
- **凡是机制对单位的改动都走两层覆盖之一**——actor 自己的 `DataSet` 或它各技能的
  `DataSet`——或者走 `BuffManager` 对生效 Buff 的聚合。**没有人写基础数值。**
- **没有人直接读数值。** `FightProperty` 把共享描述和这些覆盖层合成出结果、缓存起来，
  由变更事件置脏。

## 代码在哪

`crates/simulation/src/fight/` 就是战斗，按每部分镜像游戏里的什么拆开，每个文件声明它拥有的数据。一个
单位是 `mod.rs` 里的 `Actor`——机甲本身：布置、位置、朝向、生命——它持有一个 `Motion`（`motion.rs`，
即 `MotionController`：运动状态、被要求去哪、RVO 求解的结果）和一个 `Skill`（`skill/mod.rs`，即
`FightSkill`：锁定和武器打的对象、技能状态、正在进行的攻击）。经过单位的路径和游戏的读法一致：
`actor.skill.lock_target`、`actor.motion.state`。

| 文件 | 对应 |
| --- | --- |
| `mod.rs` | 各对象，以及 `FightCoreSystem` 的一步推进：`step` |
| `deploy.rs` | 部署：编队、初始单位和建筑、预搜索 |
| `mech.rs` | `FightMech`：位置、朝向、生命及其快照 |
| `search.rs` | 目标四叉树和目标选择器 |
| `motion.rs` | `MotionController`：保持死掉的目标、攻击射程内的目标、离开或走近目标；RVO 提交 |
| `damage.rs` | `DamagePerformer`，见下文"伤害" |
| `projectile.rs` | `ProjectileSystem` |
| `skill/mod.rs` | 按命名步骤组织的 `FightSkill` 更新、`SkillState`、搜索计时器、`TryStartAttack`、攻击区域检查 |
| `skill/check.rs` | `SearchAttackTarget`、`SkillAttackableChecker`、`WallConstructionTargetChecker`、`SkillAttackState.Finish` |
| `skill/perform.rs` | 攻击执行器：一击、一发、一串连发 |
| `skill/group.rs` | 成组技能的各个位 |
| `rvo.rs` | `RVOSimulatorFixed`，运动提交给它的采样 RVO；见 [rvo.zh.md](rvo.zh.md) |
| `random.rs` | `GRRandom`，攻击间隔抖动取数的随机流 |
| `math.rs` | 游戏的定点数运算 |
| `run.rs` | 跑一份布阵、和录像比较 |

战斗之外，crate 里放的是搭起一场战斗的东西。`layout/` 把布阵编译成部署，`layout/constructions.rs` 算出
工事释放的建筑。`modifier/` 就是 `Modifier` 这一步：开战前写到单位身上的军官和科技。`data.rs` 是它们和
战斗共同读取的数据层，`rules.rs` 是单位描述，`module.rs` 是哪个模块认领哪个布阵字段的登记表。

## 模块

`FightModule` 是每个系统的基类，`IModuleManager.GetModules()` 持有它们。生命周期就是
一个模块和战斗之间的全部契约：

| 钩子 | 何时 |
| --- | --- |
| `Init(IModuleManager)` | 对局构建模块时 |
| `OnActive` / `OnDeactive` | 模块被打开或关闭 |
| `OnEnterFight` / `OnExitFight(isInterrupted)` | 进入和离开战斗场景 |
| `OnFightStart` / `OnFightEnd` | 战斗本身开始和结束 |
| `Update` | 一次逻辑推进 |
| `IsStepFinish` | 本次推进这个模块是否已经稳定 |
| `Stop` | 对局拆除 |

一个模块就是 `GameRiver.Fight` 里名字以 `System` 结尾、且构造函数收下对局的类型。
这 35 个全都是，而以同样方式构造的其它类型只有 `FightController`、`FightModule`
自己和两个泛型基类，所以这个集合恰好就是它们。其中 25 个至少重写了一个生命周期
钩子，10 个一个都没重写——说明那 10 个是被调进去的调用驱动的，不是被推进驱动的。

模拟器为每一个都带一个模块，无论实现与否：

```text
AdvancedEnergyShieldSystem  AutoRecoverySystem      BuffSystem
BuildingSystem              BurrowSystem            ClearRangeItemSystem
CloakSystem                 CommanderSkillSystem    DeadEffectSystem
ExpSystem                   ExtraSkillSystem        FightConstructionSystem
FightCoreSystem             FightEffectSystem       FightGroupSystem
FightTeamSystem             InterceptSystem         IterationEffectSystem
LifeChangeEffectSystem      MechGrounpSystem        MineSystem
MoveAbilityRangeItemSystem  MoveAbilitySummonSystem ProjectileSystem
RangeItemSystem             ReactiveArmorSystem     RecoveryEffectSystem
SiegeModeEffectSystem       StealthTechSystem       SummonSystem
SuperDeploymentSystem       SupportUnitSystem       TeamTranslationSystem
TechnologySystem            WreckageRecoverySystem
```

**什么都不做的模块也要存在。** 它声明自己会认领 layout 的哪些字段，然后拒绝它们——
这正是闭包成为登记表的性质、而不是一串手写拒绝的原因：一份 layout 能编译，当且仅当
它带的每个字段都被一个**看得懂它**的模块认领；否则拒绝，并指名字段和模块。实现一个机制
是把它的模块填满，永远不是去改驱动它的那个循环。

**一个模块不是全有或全无。** 它认领若干字段，其中一部分是它当下看得懂的，其余的照样
被拒绝，和空模块的认领一模一样。`Modifier` 认领军官、科技、装备、等级，今天看得懂的是
军官和科技——因为四张效果表里有两张已经提取出来了。而且即使字段本身看得懂，某一份
具体 layout 仍可能被拒：一名军官或一项科技的效果这个 build 合成不出来时，持有它的那一方
被拒绝，并指名是哪一项、哪个字段，而不是把它应用一半。

有一个模块不是 build 的。军官、科技、装备、等级是在**开打之前**施加到单位上的——
build 自己的 `TechnologySystem.AddTechnologyEffect` 收的是 `PlayerController`，由部署
动作 `MAP_AddUnit` 调用——不是战斗里的系统。所以模拟器在建立战斗时一次性把它们写进去，
这一步叫 `Modifier`。其余"哪个模块认领哪个字段"是模拟器自己的安排，名字则是 build 的；
其中两条安排也是 build 的：`RangeItemSystem` owns 地形，`SuperDeploymentSystem` owns
travelling 的单位——因为 `FightCoreSystem.PreCalculate` 问的正是它 `IsTravelling`。

一次拒绝把两边所有字段一起报出来，而不是只报第一个，因为调用方想知道的是这份部署离
能打还有多远：

```text
side blue needs modules this build has not implemented: constructions
(FightConstructionSystem), units above level one (Modifier); side red needs
modules this build has not implemented: unit equipment (Modifier)
```

来自"字段本身能过、但其中某一项不行"的拒绝，指的是那一项而不是那个字段——字段是看得懂
的，看不懂的是它里面这一个：

```text
side blue: officer 20006 (先进瞄准系统) writes attack_range_value, and how a
value composes with a description is not measured
```

## 对象

| 原生 | 是什么 |
| --- | --- |
| `FightActor` | 一切有生命、有阵营、有边界、能挂 buff 和护盾的东西 |
| `FightMech` | 一个单位；带技能、运动和编队 |
| `FightCrystal` | 塔、建筑、构筑物 |
| `FightTeam` / `FightTeamController` | 一方，以及它拥有的东西 |
| `IFightGroup` | 一个编队，群体行为的单位 |
| `FightSkill` | 拥有者的一条技能通道，盖在共享的 `ISkillData` 上 |
| `RangeItem` | 一片地形区域，按类型由 `RangeItemController` 持有 |
| `FightEnergyShield` | 独立护盾，由 `AdvancedEnergyShieldSystem` 持有 |
| `FightInterceptor` | 一个拦截源，由 `InterceptSystem` 持有 |
| `Projectile` | 飞行中的弹丸，由 `ProjectileSystem` 持有 |

这些每一个在录像里都有自己的一列，这正是一个机制能被单独验收的原因：
[mcfr.md](../mcfr/mcfr.zh.md) 把单位、建筑、护盾、地形、弹丸记成彼此独立的集合，
上面这张对象表和它们一一对应。

## 数据层

这一层最值得照抄，因为它正是"一个效果事后可归因"的来源。

**描述是共享的。** 一份 `ISkillData`、一份单位描述服务于该类型的每一个实例。
谁都不许写它们。

**一次改动是一条带来源的覆盖条目。** `DataSet` 按下标存三种条目：

| 种类 | 读法 | 装什么 |
| --- | --- | --- |
| Float | `GetDataFloat(index)` | 一个加法值 |
| FloatRate | `GetDataFloatAddRate(index)` / `GetDataFloatReduceRate(index)` | 一对，增和减 |
| Int | `GetDataInt(index)` | 一个整数 |

每一次写入都带着来源：`ChangeDataFloat(index, IDataModifier, value)`。这就是一项科技、
一件装备、一个 buff 能被加上又摘掉而无需任何人重算基础值的原因，也是录像能说出一条
修正来自哪条通道的原因。

那些下标就是 `MechDataChange{Float,FloatRate,Int}` 和
`SkillDataChange{Float,FloatRate,Int}` 这几个枚举，技能自身的改动入口是
`FightSkill.AddData(SkillDataChangeFloat, IDataModifier, value)`。这些枚举正好就是
MCFR 记作 `unit_dynamic_modifiers` 和 `skill_dynamic_modifiers` 的那两组字段。

**Buff 单独聚合。** `BuffManager` 把一个拥有者身上生效的 `Buff` 加总，通过 getter
暴露总量——`GetAmplifyDamageAddRate`、`GetAttackIntervalChangeAddRate`，以及 MCFR
记作 `buff_modifiers` 的其余那些。它**不是**一个 `DataSet`，这正是要点：一个 buff 和
一条数据改动即使产生同一个数字，也仍然可以区分。

所以一个单位的状态是**一份共享描述加三层覆盖**，而录像每一 tick 把三层都 dump 下来。

## 派生值

`FightProperty` 是一个带缓存的派生数值。它把自己注册成所依赖的覆盖条目的监听器——
`RegisterDataChangedEventFloat`、`…FloatRate`、`…Int`——在其中之一变化时置脏，然后在
`Refresh()` 里重算。`MechPropertyFloat` 挂在 `FightMech` 上，`SkillPropertyFloat`
挂在 `FightSkill` 上。

每个 property **读什么**是调用边事实，因而是确立的；把它们**合成起来的算术不是**：

| Property | 读取 |
| --- | --- |
| `AttackIntervalProperty` | 技能的 `GetDataFloatAddRate` / `GetDataFloatReduceRate`；`BuffManager` 的攻击间隔与额外攻击间隔增/减率；`FPoint.Max` |
| `AttackRangeProperty` | 技能的 `GetDataFloatAddRate` / `GetDataFloatReduceRate`；`BuffManager` 的射程与额外射程增/减率 |
| `DamageProperty` | `FightMech.GetBaseDamage`；技能的 `GetDataFloatReduceRate`；自身的 `CalculateBaseDamage` 与 `CalculateDamage` |
| `MoveSpeedProperty` | `FightMech` 的 `GetDataFloatAddRate` / `GetDataFloatReduceRate` / `GetDataInt`；某个 `DataSet` 的增减率；`BuffManager.GetMoveSpeedChangeValue` |
| `ProjectileCountProperty` | `DataSet.GetDataInt` |

### 一条修正怎么合成

build 用自己的类型名说了这件事。`DataSet` 带三张列表：

```text
List<DataInt>                 intDatas        一个普通整数
List<AdditiveDataFloat>       floatDatas      ChangeDataFloat      —— 一个 value
List<MultiplicativeDataFloat> floatRateDatas  ChangeDataFloatRate  —— 一条比率
```

`DataInt` 是抽象类，某个下标用它三个子类里的哪一个，就是那个下标的全部算术：
`DataIntGroup` 把条目求和、再钳制在构造函数收下的两个边界之间，`DataIntSingleMax` 只保留
最大的那一条，`DataIntSingleMin` 保留最小的那一条。`FightMech` 的构造函数建的是
`DataIntGroup(0x80000000, 0x7FFFFFFF, 0)`，所以一个单位的整数就是纯求和——钳制范围是整个
`Int32`，永远不生效。

`AdditiveDataFloat.Refresh` 把自己的条目**相加**——ISIL 就是一个带饱和保护的 `add` 循
环——而且这个类带着 `Min`/`Max`，调用表里有 `FPoint.Clamp`，所以一个 value 是一个可被钳
制的和。

`MultiplicativeDataFloat.Refresh` 带**两个**累加器。它把其中一个重置为 0，另一个重置为
元数据里的常量 1，然后遍历条目、用 `FPoint.op_GreaterThan` 按**符号**分流：一路求和，
一路相乘。`GetDataFloatAddRate` 返回前者，`GetDataFloatReduceRate` 返回后者——这就是为什
么录像里一条 `reduce` 能代表好几次削弱：build 早就把它们乘在一起了，而 MCFR 存的是
`1 − 那个乘积`。

`FightMech.CalculateMaxLife` 把装配顺序摆了出来：它载入 `0x100000000`（Q32.32 的 1）、
把 add 率加上去，把数据源给的整数左移 32 位变成 `FPoint`，然后相乘。于是：

```text
(base + Σ value) × (1 + Σ enhance) × Π (1 − impair)
```

在 build 把结果转回 `Int32` 的地方向零截断一次，此前不截断。**削弱不是负的增强**：两条
`0.11` 留下的是 `0.89 × 0.89`，不是 `1 − 0.22`。`tests/modifier/` 放着逐条子句
对着游戏量出来的那些 fixture，[`officer_effects.md`](../../rules/officer_effects.zh.md)
记录了它们的答案。

这张表带出两件事。攻击间隔有下界钳制而射程没有，这是种类上的差别不是巧合。以及，一个
property 的输入恰好就是录像记的那些列，所以**一个机制在算术未知之前就能被检验**：
"我填的表算出的数和游戏当时存的数一样吗"是一个问题，"这个数合成出的伤害对不对"是
另一个问题。

## 伤害

战斗里每一种造成伤害的方式，都是交给同一个执行者的一个提供者。build 的
`IDamageProvider` 描述一次命中——`GetDamage`、`GetSplashRange`、`GetMainTarget`、
`GetMaxTargetCount`、`GetEffectTargetType` 等——实现它的有技能、弹丸、指挥官技能、
击杀爆炸、地雷、支援单位和工事。`DamagePerformer` 结算其中任何一个：
`PrepareRangeTargets` 决定打到谁，`PerformSingleEffect`／`PerformRangeEffect` 扣血。
它打的对象是 `FightActor`，所以单位和建筑走的是同一段代码。

模拟器照这个分法镜像：

| 原生 | 模拟器 |
| --- | --- |
| `IDamageProvider` | `DamageHit`：来源、伤害量、瞄准的对象、是否无论远近都打中它、溅射中心和半径、能打到的域 |
| `DamagePerformer.PrepareRangeTargets` | `damage_targets`，唯一决定一次命中落到谁身上的地方 |
| 单个目标的生命变化 | `strike`，唯一扣血的地方，单位和建筑都走它 |
| `PerformRangeEffect` | `perform_damage`，按顺序打每个目标，对每个掉了血的记一条 `damage` |

**一种造成伤害的方式是一个提供者，而不是又一份结算。** 直接攻击、弹丸落地、激光都各
自描述自己这一下，然后交出去；激光没有范围，只取单个目标那一步。它们之间唯一的差别是
在哪里记录后续——弹丸先记自己被移除，再记它造成的死亡——所以死亡和倒塌交还给调用方，
不由执行者记录。

**溅射两类都打，按树里的顺序。** `damage_targets` 走每个敌方队伍的目标树，墙和单位
都在树里：射程和域允许的单位、边缘在溅射半径内且活着可被选中的建筑，都吃满额，不管
这一下瞄的是什么。一次打击造成的死亡和倒塌按被打的先后交回，在这一 tick 末尾、本步的
死亡之后记录；死掉的单位这时离开目标树，其余的顺序不变。

## 攻击目标

技能决定武器打什么只在一个地方，战斗里所有关于工事的规则都经它到达武器。游戏的
`FightSkill.SearchAttackTarget` 拿着锁定，把护盾、否则挡在线上的墙（`WallConstructionTargetChecker`，
没有别的调用者）、否则锁定本身交给武器。空闲状态在有锁定的每次更新都调它；
`SkillAttackableChecker.Check` 也调它——准备状态每次更新都跑这个检查，攻击状态在两下之间和蓄力期间
跑。`SkillAttackState.CheckAttackable` 在交给检查器之前先拒绝已死亡的建筑攻击目标；检查失败就结束
攻击：`StopAttack` 清掉锁定，技能冷却，然后带着"清空目标"进入空闲。

| 原生 | 模拟器 |
| --- | --- |
| `FightSkill.SearchAttackTarget` | `search_attack_target`，`wall_in_the_way` 唯一的调用者 |
| `SkillAttackableChecker.Check` | `check_attackable` |
| `SkillAttackState.CheckAttackable` | `attack_state_check_attackable`，在 `between_blows` 成立时问 |
| `SkillAttackState.Finish` | `finish_attack`，进入冷却保持或直接空闲 |
| `SkillPrepareState` 里检查失败 | `enter_idle_clearing_targets` |

**这里没有一处是关于墙的。** 墙和单位的差别只在于它是 `SearchAttackTarget` 可能给出的一个答案，以及
攻击状态对已死亡目标做的那个建筑类判断；两者都是游戏自己的。检查在哪些技能状态里跑，是从每个单位
`SkillStateController` 状态和 `SkillAttackController` 阶段的采集里读出来的（`target_refs_v1`
instrumentation profile），和 `tests/construction/` 下的录像一起。

**怎么对齐到游戏的。** 两份整个回归清单的采集是对照：每个单位逐 tick 的技能状态和攻击阶段
（`tests/regression/skill-state.mcscript`），以及每一次 `Check` 调用前后技能的锁定和攻击目标
（`skill_attackable_checker_v1` profile）。内核的状态现在在所有可比的单位-tick 上只有 3 处与游戏不同，
`check_attackable` 对游戏自己的 148,595 次调用有 148,466 次给出相同回答。除检查器本身外，还需要：

- 检查在两下之间、出手前的等待、以及出手那次更新（出手之前）运行；
- 后摇在下一下到来时结束：描述里的后摇被截到攻击间隔，Wasp 就是这样；
- 所有单位的攻击状态都会延续到后摇之后；
- 刚进入的状态当 tick 不更新：单位进入射程的那个 tick 技能就进入准备或攻击状态，即使朝向还在修正；
  准备结束后，第一下要等到下一个 tick。

有了这些，原来回答"锁定死了或走远了"的失效目标、快速切换出界、失效替换三条路径都删了，全部由检查器
回答。

**镜像还没接管的部分。** 成组技能的核心是 `GroupedSkillAttackBehaviour` 而不是 `FightSkill`，采集里
没有它的状态，它仍用原先校准过的准备时序。不能快速切换的技能为什么对走出射程的活锁定不重新搜，而是
保留它，这一点是量出来的，还没有在反编译里读到对应分支。

## 镜像，以及现在哪些是空的

模拟器用同样的名字带同样的三层，靠把模块填满来生长，而不是靠改内核：

| 原生 | 模拟器 |
| --- | --- |
| `IModuleManager` + `FightModule` | 模块登记表，每个系统一条，各自声明认领哪些 layout 字段 |
| 盖在共享描述上的 `DataSet` | 两层覆盖，每 actor 一层、每技能一层，每条目带写它的机制 |
| `BuffManager` 聚合 | buff 通道，按 actor 加总 |
| `FightProperty` | 经 `stat(...)` 读到的派生值，永远不直读字段 |
| `IDamageProvider` + `DamagePerformer` | 一种命中描述、一处结算，见上面"伤害"一节 |
| `SearchAttackTarget` + `SkillAttackableChecker` | 武器打什么只有一个答案，见上面"攻击目标"一节 |
| 录像的各列 | 每一层各自逐 tick 对齐的验收面 |

没实现的模块照样在场、照样认领字段、然后拒绝。语料里有多少回合的 layout 能编译，就是
进度条，由 [`scripts/fight-coverage.py`](../../../scripts/fight-coverage.py) 报出。

## Determinism invariants

本框架继承模拟器已有的确定性不变量，并自己加三条：

- **模块顺序固定且显式。** 无论以什么顺序驱动模块，那都是 build 的一个常量，而不是
  一次哈希表遍历；同一种子的两次运行驱动顺序完全一致。
- **覆盖层是一组带标签的条目，不是一个滚动的总和。** 加上再摘掉一条修饰符，必须精确
  回到原值，因为值是从条目重算出来的，从不就地增减。
- **派生值是描述与覆盖层的纯函数。** property 可以缓存、可以置脏，但任何时刻把缓存丢掉
  重算都得到同一个数，所以缓存不可能改变结果。

## Fidelity boundary

本框架复现的是 build 的**结构**，不是它的实现。这里的一个模块是一个机制的位置，而一个
机制的保真度只由它自己的契约说了算。三件事明确不作承诺：

- **本文不产生任何时序断言。** 一个模块有 `Update` 只说明它每次推进都会跑，不说明它在
  这次推进的什么位置跑。
- **本文不产生任何算术断言。** 一个 property 的输入清单是调用图给的；这些输入怎么合成
  是每个数值自己的问题。
- **空模块是一次拒绝，绝不是一次近似。** 需要未实现机制的战斗，就是不打。

## Unresolved

- **一个 value 怎么合成，以及两条通道按什么顺序作用。** 比率已经定了：
  `tests/modifier/composition.mcscript` 测出的是同一条通道内
  `base × (1 + Σ add − Σ reduce)`、向零截断，
  [`officer_effects.md`](../../rules/officer_effects.zh.md) 记录了那次捕获。但那次捕获
  把两条修正放在同一条通道里、而且两条都是比率，所以同一下标上 Float 与 FloatRate 并存
  时如何相互作用、单位/技能/buff 三条通道按什么顺序作用，仍然没有人测过——`data.rs`
  对这两种情况一律拒绝，而不是把规则外推过去。
- **模块被驱动的顺序，以及一次推进内部的工作顺序。** `FightCoreSystem.Update` 调用
  `TeamUpdate` 再调用 `GroupUpdate`，`PreCalculate` 和 `Update` 并列存在，但方法体内的
  调用顺序不在索引里。这一条靠对着录像测量关掉。
- **哪个枚举下标对应哪个字段。** `MechDataChange*` 和 `SkillDataChange*` 的名字是已知的，
  录像的字段名也是；它们背后的数字下标还没有读出来。
- **`IsStepFinish` 判的是什么。** 每个模块都有一个，而战斗现在结束所依据的终止条件是
  模拟器自己的。
