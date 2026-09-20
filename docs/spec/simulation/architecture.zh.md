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

build 2259 有这 35 个，模拟器为每一个都带一个模块，无论实现与否：

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
它带的每个字段都被一个**已实现**的模块认领；否则拒绝，并指名字段和模块。实现一个机制
是把它的模块填满，永远不是去改驱动它的那个循环。

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

这张表带出两件事。攻击间隔有下界钳制而射程没有，这是种类上的差别不是巧合。以及，一个
property 的输入恰好就是录像记的那些列，所以**一个机制在算术未知之前就能被检验**：
"我填的表算出的数和游戏当时存的数一样吗"是一个问题，"这个数合成出的伤害对不对"是
另一个问题。

## 镜像，以及现在哪些是空的

模拟器用同样的名字带同样的三层，靠把模块填满来生长，而不是靠改内核：

| 原生 | 模拟器 |
| --- | --- |
| `IModuleManager` + `FightModule` | 模块登记表，每个系统一条，各自声明认领哪些 layout 字段 |
| 盖在共享描述上的 `DataSet` | 两层覆盖，每 actor 一层、每技能一层，每条目带写它的机制 |
| `BuffManager` 聚合 | buff 通道，按 actor 加总 |
| `FightProperty` | 经 `stat(...)` 读到的派生值，永远不直读字段 |
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

- **覆盖层怎么合成出一个数。** 比率是 `base × (1 + add − reduce)` 还是对条目连乘，
  同一下标上的 Float 与 FloatRate 条目如何相互作用，`AdditiveDataFloat` 与
  `MultiplicativeDataFloat` 对顺序意味着什么。索引没有方法体，所以这一条要靠把录像的
  聚合值和它的结果拟合起来才能关掉。
- **模块被驱动的顺序，以及一次推进内部的工作顺序。** `FightCoreSystem.Update` 调用
  `TeamUpdate` 再调用 `GroupUpdate`，`PreCalculate` 和 `Update` 并列存在，但方法体内的
  调用顺序不在索引里。这一条靠对着录像测量关掉。
- **哪个枚举下标对应哪个字段。** `MechDataChange*` 和 `SkillDataChange*` 的名字是已知的，
  录像的字段名也是；它们背后的数字下标还没有读出来。
- **`IsStepFinish` 判的是什么。** 每个模块都有一个，而战斗现在结束所依据的终止条件是
  模拟器自己的。
