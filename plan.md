# 目标与计划

把 mechcore 做成 agent 和人类的对战平台：输入一条原子 action，返回下一个
state。一局比赛就是一份 battle 文档，平台一边下一边把它写出来。

平台那一半已经能跑：`match` 发牌、开局、部署、提交、写账本，命令行按
[docs/spec/mechcore/cli.md](docs/spec/mechcore/cli.md) 治理完毕。**现在整件事卡
在一处：模拟器打不了真实对局的仗。** 所以这份计划从头到尾是关于模拟器的——它怎么
长、怎么验、怎么让几条线并行推进。

## 卡在哪里，有多远

把语料 41 份回放的每一回合投影成部署结束的 layout（`doc project`，334 个回合全部
投影并编译通过），再拿模拟器现在的闭包去量，得到的就是距离：

| 字段 | 欠哪个模块 | 挡住多少回合 |
| --- | --- | ---: |
| `officers` | Loadout | 334 (100%) |
| `constructions` | FightConstructionSystem | 322 (96%) |
| 单位 `level` > 1 | Loadout | 276 (82%) |
| `techs` | Loadout | 256 (76%) |
| `battle_skills` | CommanderSkillSystem | 176 (52%) |
| 单位 `equipment` | Loadout | 167 (50%) |
| `contraptions` | InterceptSystem | 161 (48%) |
| `tower_strengthen_levels` | BuildingSystem | 123 (36%) |
| `energy_tower_skills` | BuildingSystem | 118 (35%) |
| `travelling` | SuperDeploymentSystem | 94 (28%) |
| `terrains` | RangeItemSystem | 5 (1%) |
| `airdrop_shields` | AdvancedEnergyShieldSystem | 4 (1%) |

（`blueprints` 不在表里：编译 layout 时每条链都按它交出的军官应用，所以蓝图是以军官
的身份到达战斗、也以军官的身份被拒。）

**没有一个回合只差一样东西。** 按字段算最少差 2 样、中位数 6 样；按**模块**算差 2 到
6 个，中位数 4 个。所以"按字段实现、拿真实对局验收"这条路，早期根本走不通——真实回合
要等四五个模块齐了才第一次可用。早期验收只能靠**合成 layout**：一份只动一个字段的
布阵，拿游戏录一份原生 MCFR，模拟器必须逐 tick 对上。这正是
`tests/mcfr-regressions.yaml` 现在的 81 条在做的事。

上面那张表和下面这串数都出自
[`scripts/fight-coverage.py`](scripts/fight-coverage.py)，而且**是问二进制自己要的**：
拒绝一次把两边所有欠账一起报出来，脚本只做汇总。模块按贪心次序落地时，闭包内回合数
这样涨：

```text
+ AdvancedEnergyShieldSystem      0/334
+ CommanderSkillSystem            0/334
+ InterceptSystem                 0/334
+ SuperDeploymentSystem           0/334
+ FightConstructionSystem         0/334
+ Loadout                       166/334
+ BuildingSystem                329/334
+ RangeItemSystem               334/334
```

**`Loadout` 是最大的一根杠杆**：它一个模块认领四个字段（军官、科技、装备、等级），
自己就压着 166 个回合。脚本第一行报"模拟器现在接受几个回合"，今天是 0，这就是进度条。

## 一、先定架构，否则并行不起来

现在 `kernel.rs` 是 8500 行，`Simulation` 一个 impl 就 3200 行，单位的数值直接从
`UnitConfig` 读（186 处）。这种形状下每加一个字段都要动内核，两个人没法同时干活。
架构要先改，改的依据不是审美，是**游戏自己的结构**：
[architecture.md](docs/spec/simulation/architecture.md) 把它读了出来——35 个
`FightModule`、共享描述 + `DataSet` 覆盖层 + `BuffManager` 聚合的三层数据、以及带缓存
的 `FightProperty` 派生值层，全部由
[`scripts/fight-structure.py`](scripts/fight-structure.py) 从反编译索引重新生成。
下面三条就是照着它镜像：

### 1. 模块：35 个全部在场

原生那边是 35 个 `FightModule`，每个只认 `Init`／`OnFightStart`／`Update`／
`IsStepFinish`／`OnFightEnd` 这一组钩子；`FightCoreSystem` 只是其中一个，不是挂着
别人的主循环。模拟器照搬：**35 个模块全部在场**，没实现的是空模块，认领自己的 layout
字段然后拒绝。

闭包因此由登记表生成，不再是 `layout.rs` 里一串手写的拒绝：一份 layout 能编译，当且
仅当它带的每个字段都被一个已实现的模块认领。加机制＝填一个模块，永远不动驱动它的
循环，`fight run`、`match` 和覆盖率报告拿到的也是同一句话。

模块之间的**驱动顺序**索引里没有（没有方法体），要靠对着录像测量补上——记在
architecture.md 的 Unresolved 里。

### 2. 数据：一份共享描述，三层覆盖

这是最值得照抄的一层，因为它是"一个效果事后可归因"的来源：

- **描述共享**：一份 `ISkillData`、一份单位描述服务该类型所有实例，谁都不许写；
- **改动是带来源的覆盖条目**：`DataSet` 按下标存 Float／FloatRate／Int 三种条目，每次
  写入都带 `IDataModifier`，所以科技、装备、buff 能加上又摘掉而无需任何人重算基础值；
- **buff 单独聚合**：`BuffManager` 把生效的 buff 加总，它**不是** `DataSet`——一个 buff
  和一条数据改动即使产生同一个数字也仍然可分。

这三层正好就是录像每 tick dump 的 `buff_modifiers`、`unit_dynamic_modifiers`、
`skill_dynamic_modifiers`，[mcfr.md](docs/spec/mcfr/mcfr.md) 明说它们互相独立、可分别
归因——所以也可以分别验收。

### 3. 派生值：没有人直读数值

原生的 `FightProperty` 是带缓存的派生值：注册成所依赖覆盖条目的监听器，变了就置脏，
`Refresh()` 里重算。模拟器的 `Stats` 就是它——内核读的是 `stats.move_speed()` 而不是
`rules.move_speed()`，**收敛的范围就是录像记了的那几个数值**（移速、生命上限、伤害、
攻击间隔、射程），其余保持直读，改动量正好等于机制面。

做完这条，"军官给 +20% 伤害"是一张表里的一行，不是内核里的一段 if。至于三层**怎么
合成**出一个数（连乘还是 `base × (1 + add − reduce)`），索引答不了，要靠拿录像的聚合值
去拟合结果。在那之前，**任何非中性的修正一律拒绝而不是猜**：`resolve` 遇到非中性条目
就报错并指向 architecture.md 的未决项，所以没有哪个机制能在规则立起来之前先蒙混过去。

## 二、模块并行，按登记表分工

登记表已经把十二个字段分给了七个模块，所以分工就是模块，几条线可以同时走：

| 模块 | 认领的字段 | 验收面 |
| --- | --- | --- |
| **Loadout**（非原生模块） | `officers`、`techs`、`equipment`、单位 `level` | 录像的三条修饰符通道，逐 tick 对齐 |
| **FightConstructionSystem** | `constructions` | 录像的 `buildings`（内核已经有塔了） |
| **BuildingSystem** | `energy_tower_skills`、`tower_strengthen_levels` | 录像的 `buildings` |
| **CommanderSkillSystem** | `battle_skills` | 录像的 `terrains`、`shields` 和释放事件 |
| **RangeItemSystem** | `terrains` | 录像的 `terrains`，[terrain.md](docs/rules/terrain.md) 已有机制 |
| **AdvancedEnergyShieldSystem** | `airdrop_shields` | 录像的 `shields` |
| **InterceptSystem** | `contraptions` | 录像的 `buildings` 与拦截事件 |
| **SuperDeploymentSystem** | `travelling` | 录像的 `units.position`／`motion_state` |

`Loadout` 之所以不是原生的 35 个之一：军官、科技、装备、等级在游戏里是**开打之前**
施加到单位上的（`TechnologySystem.AddTechnologyEffect` 收的是 `PlayerController`，由
部署动作 `MAP_AddUnit` 调用），不是战斗里的系统。模拟器照样在建立战斗时一次性把它们
写进覆盖层。

每个模块各自还欠一份**配置提取**，这是研究不是工程，可以和引擎并行。军官那份已经做完：
[`config/officer_effects.yaml`](config/officer_effects.yaml) 是 79 名军官写给单位的修正，
[`docs/rules/officer_effects.md`](docs/rules/officer_effects.md) 说明编码和瞄准规则，
数字拿 build 2227 的本地化文案核对过 73 条、不一致就拒绝写表。

科技、装备、能量塔技能是**同一个接口**（`ICommonMechDataChangeDataSource`）的另外三个
实现，所以表的形状已经定了；它们的数据在别的 Unity 对象里，还要各解析一次。

## 二之半、一个机制怎么研究：循环

每个机制都是同一个循环，[`scripts/officer-composition.mcscript`](scripts/officer-composition.mcscript)
是它的范本——那一份脚本同时是实验设计、实验记录和可重跑的过程：

1. **把问题收成一个数。** 不是"军官怎么生效"，而是"`+0.3` 是乘上去还是加上去，两份
   同名军官是 1.6 还是 1.69"。收不成一个数的问题，做不成实验。
2. **先读能读的，并说清它答不了什么。** 反编译索引给结构、字段、调用边，
   [architecture.md](docs/spec/simulation/architecture.md) 就是这么读出来的；它给不出
   算术，因为没有方法体。**"索引答不了"本身要写下来**，否则下一个人会再读一遍。
3. **设计能区分的实验，并且带对照。** 三份 layout 只差 `officers` 一行；对照组录两遍
   必须逐 tick 相等，不等就停——不可复现的管线会让后面每个差异都失去意义。
4. **把预期值先算出来写进脚本。** 三个答案分别对应 13242／11845／11425 的剩余生命，
   两两相差几百，一次录制就能分开。**先写预期再录**，否则事后总能给任何数字编一个解释。
5. **测量有两半，对上了才算规则。** 录像同时给"游戏**存**了什么"（三条修饰符通道，
   `fight outcome` 会把它们报出来）和"游戏**算**出了什么"（伤害、剩余生命）。
   一条 `+0.6` 的聚合值配上 1.6 倍的伤害是同一件事看两遍；`+0.3` 出现两次则是另一件事。
6. **规则落成表和索引，并且拒绝先于猜。** 数落进 `config/`，出处和未决项落进
   `docs/rules/`，捕获没够着的情况 `crates/simulation/src/data.rs` 一律拒绝。

这一轮已经跑完，答案是 14639／13243／11845：比率在通道内**相加**再乘一次、向零截断，
而且 build 在写入时就把两名军官合并成一条 `+0.6`。规则写在
[`officer_effects.md`](docs/rules/officer_effects.md)，实现在 `data.rs`，那三行剩余生命
和两条聚合值现在是脚本里的 `expect`——**同一份脚本从实验设计变成了对 build 的回归**。
没够着的（`*_value` 怎么合成、两条通道谁先谁后）留在未决项里被拒绝着。

这个循环不需要谁在场协调：一个机制一条线，各自录各自的、拟各自的、写各自的索引。

## 三、验收分三层，覆盖率钉在 CI

1. **机制层（合成）**：一份只动一个字段的 layout，游戏录一份原生 MCFR，
   `physics_result_hash` 钉住。现有 81 条就是这层，每个机制进来加自己的几条。
2. **回放层（调试）**：`fight verify` 重跑录像自带的 layout，逐 tick 比，报第一处
   分歧。它是第 1 层失败时的定位工具。
3. **语料层（稀疏）**：334 个真实回合，投影→模拟→拿**下一回合的 state** 当稀疏
   oracle。这层不需要开游戏，覆盖的是真实字段组合而不是人造样例。

第 3 层现在就有一个**不欠任何规则**的信号：293 次回合过渡里 289 次只有一方掉血，
所以谁输是文档直接告诉我们的——**模拟出来的败方必须和文档掉血的那一方一致**。
反应堆伤害和经验两条规则要等这层能跑之后，用同样的 334 个回合去拟合和证伪
（`doc project` + `fight outcome` 已经把存活方交到手上了）。

**覆盖率是这件事的进度条**：334 个回合里有多少落进闭包，`fight-coverage.py` 现在
就在报，和文档那边的叶子覆盖率一样钉进 CI，只许涨不许跌。每个机制落地，这个数就
动一次。脚本里的字段清单现在是照 layout 格式手写的；等机制登记表落地，闭包和拒绝
原因都该由二进制自己答，脚本只做汇总。

## 四、战斗结果那两条规则

模拟器能打之后才轮得到：

- **反应堆伤害**。核心不是战斗里的对象（录像里每方只有两座 3400 血的塔），所以它是
  一条作用在战斗末局面上的公式。语料的形状已经露出来了：伤害 20..2738，最常见的
  100／200／400 正是单位的 `value`。最便宜的一条线是先查 build 的单位数据里有没有
  现成的对核心伤害字段；没有再用 Training Ground 做可控实验。
- **经验**。MCFR 根本没记，`ExpSystem` 只有名字和调用边。先让 adapter 把每个单位的
  经验（和每队的核心）录进 MCFR——分层哈希就是为这种增量观测字段设计的——证据带上
  了再谈规则。

这两条落地，`match` 的 `fight` 阶段就不再答 `unresolved`，一局才真正下得完。

## 已经做完的

- **平台**：`match new`／`show`／`act`／`commit`，账本即 battle 文档，turn 文件装锁
  和草稿；可见性按 cli.md 裁剪；超时判负写成投降。
- **命令行**：六个命名空间、`man`、`doc schema`／`project`、获取游戏是操作不是选项。
- **部署阶段**：`step`／`deployed`／`predict`／`open_round`，落位、补给、额度、
  拒绝买不起和没解锁。追踪集非战斗叶 67305/67305 一致。
- **发牌**：开局四组、每回合增援、放弃补给，全部由种子推出并校验。
- **文档**：四种格式与规范形式，`verify`／`format`／`diff`／`convert`／`project`／
  `schema`；进行中的对局也是合法 battle 文档。
- **战斗读取**：`fight outcome` 从录像读出存活编队（按文档 index 对号）和三类物体的
  残留，读不出来的逐项报缺。

## 还没做、但不在这条主线上

- **`arena run` 和 `shell --json`**：一行一个请求、一行一个结果，和 arena 跟玩家
  进程说的是同一套协议。它需要各操作**返回**结果对象而不是打印，是一次跨命名空间的
  重构。模拟器能打完一局之前，编排出来的也只是一局停在 `fight` 的对局，所以排在
  模拟器后面。
- **game 后端**：让真实游戏打这一仗。adapter 现在只能下发 layout 并录制，读不回打完
  的局面；等它具备了再定怎么进契约。game 打的一仗不可重算，届时 outcome 必须落盘。

## 顺序

1. 架构三件事（35 个模块全部在场、共享描述加三层覆盖、派生值经 `stat` 读）。这一步
   不加任何机制，三层覆盖保持中性，靠现有 81 条 physics 回归和语料 334 个回合的覆盖率
   数字证明"什么都没变"——三条修饰符通道本来就不在 physics 哈希里。
2. 四组机制并行，每组自带配置提取和合成验收；覆盖率数字一路往上走。
3. 语料层稀疏验收接上（先只判胜负方），再拟合反应堆伤害与经验两条规则。
4. `arena` 与 `shell --json`；`game` 后端最后。
