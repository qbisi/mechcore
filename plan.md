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
| ~~`officers`~~ | ~~Modifier~~ | **已落地**（曾是 334，100%） |
| ~~`techs`~~ | ~~Modifier~~ | **已落地**（曾是 256，76%） |
| ~~`constructions`~~ | ~~FightConstructionSystem~~ | **防御墙已落地**（曾是 322，96%；炮台仍拒绝，30 个回合） |
| ~~单位 `level` > 1~~ | ~~单位自身~~ | **已落地**（曾是 276，82%；等级是独立乘区，不是修正） |
| `battle_skills` | CommanderSkillSystem | 176 (52%) |
| ~~单位 `equipment`~~ | ~~Modifier~~ | **第一回合的普通装备已落地**（曾是 167，50%；之后的回合和其它装备类仍拒绝） |
| `contraptions` | InterceptSystem | 161 (48%) |
| ~~`tower_strengthen_levels`~~ | ~~BuildingSystem~~ | **已落地**（曾是 123，36%；输塔的减益一并落地，见 [towers.md](docs/rules/towers.md)） |
| `energy_tower_skills` | BuildingSystem | 118 (35%) |
| `travelling` | SuperDeploymentSystem | 94 (28%) |
| `terrains` | RangeItemSystem | 5 (1%) |
| `airdrop_shields` | AdvancedEnergyShieldSystem | 4 (1%) |

（`blueprints` 不在表里：编译 layout 时每条链都按它交出的军官应用，所以蓝图是以军官
的身份到达战斗、也以军官的身份被拒。）

**军官落地之前，没有一个回合只差一样东西**——那时按字段算最少差 2 样。军官是每个回合
都带的那一样，它关掉之后有 28 个回合只差 `constructions` 一件了。按模块算现在差 1 到
6 个，中位数 4 个。所以"按字段实现、拿真实对局验收"这条路，早期根本走不通——真实回合
要等四五个模块齐了才第一次可用。早期验收只能靠**合成 layout**：一份只动一个字段的
布阵，拿游戏录一份原生 MCFR，模拟器必须逐 tick 对上。这正是
`tests/regression/mcfr-regressions.yaml` 现在的 81 条在做的事。

**一个机制的回归表跟着它的 fixture 走。** 修饰符那一组不在上面那张表里：
`tests/modifier/` 下同时放着 fixture、录制脚本（要游戏）和
`regressions.mcscript`（不要游戏，CI 跑的就是它）。fixture、它量出来的数、守着它的回归，
是一起读、一起搬的一件东西。攻击间隔随机流是另一个问题，所以它在
`tests/interval/`，工事在 `tests/construction/`——**按问题归类，不按
工具**。

上面那张表和下面这串数都出自
[`scripts/fight-coverage.py`](scripts/fight-coverage.py)，而且**是问二进制自己要的**：
拒绝一次把两边所有欠账一起报出来，脚本只做汇总。模块按贪心次序落地时，闭包内回合数
这样涨：

```text
+ CommanderSkillSystem            29/334
+ InterceptSystem                 73/334
+ BuildingSystem                 145/334
+ SuperDeploymentSystem          237/334
+ RangeItemSystem                242/334
+ AdvancedEnergyShieldSystem     246/334
```

（这是塔的强化等级落地之后的数。现在有 88 个回合的拒绝不点名任何字段——炮台、会写溅射
半径的科技之类，是已实现模块自己按名拒绝的一半——所以天花板是 246 而不是 334。）

军官那一份已经装上：`crates/simulation/src/modifier/officers.rs` 把 79 行里的 **47** 行应用到目标
单位上，其余指名拒绝。合成规则三条子句全部对着游戏量过：

```text
(base + Σ value) × (1 + Σ enhance) × Π (1 − impair)
```

增强相加、削弱相乘、value 按本单位相加、普通整数也相加——形状先从 `DataSet` 的**三张
列表**读出来（`AdditiveDataFloat`、`MultiplicativeDataFloat`、`DataIntGroup`），再用
`tests/modifier/` 下的 fixture 逐条对着游戏验证。

**剩下 18 行已经不是"怎么合成"的问题**：11 行修的是塔/护盾/地雷/部署时钟/经验，3 行按
击杀数算，3 行是溅射半径，1 行是没人枚举过的"远程"类别——每一条欠的都是一个机制或一份
枚举，不是一条算术。

脚本第一行报"模拟器现在接受几个回合"，今天是 0，这就是进度条。

## 单位：无科技基本支持

layout 字段之外还有一条独立的缺口：单位本身。游戏有 32 个单位，`config/units/` 配了
23 个，模拟器只接受其中 11 个——`crates/simulation/src/rules.rs` 的
`CURRENT_KERNEL_SUPPORTED_UNIT_CONFIGS`，最早的内核提交时手写的一份名单。另外 12 个
有配置、被拒；8 个巨型单位（fortress、vulcan、melting_point、overlord、war_factory、
sandworm、raiden、abyss）连配置都没有。名单里的 11 个证据也不均：marksman、arclight、
rhino、crawler 各有几十场钉住的对局，mustang、wasp、phoenix 各只有一场镜像战，而且
只钉了物理哈希。语料帮不上忙：334 个回合全被模块挡住，一个单位支持到哪一步，只能拿
合成布阵来量。

**定义。** 一个单位 U 在 1 级、第 1 回合、没有军官、科技和装备时算有**基本支持**，
当且仅当下面三条都成立：

1. 六个标准布阵，每个用两个种子由游戏录制，模拟器逐 tick 复现，物理和内容两层都钉住
   游戏录出来的哈希：

   | 布阵 | 布置 | 量的是什么 |
   | --- | --- | --- |
   | M1 镜像 | U 对 U，各一个编队，开局在射程外 | 编队生成、移动、接近、开火时机、弹道或伤害、死亡 |
   | M2 单体地面目标 | U 对 rhino | 锁定、转向、被近身，对大型单体的伤害 |
   | M3 群体地面目标 | U 对 crawler | 换目标、溅射、被包围时的避让 |
   | M4 空地关系 | 地面 U 对 wasp；空中 U 对 marksman | 目标筛选，没有可打目标时的行为，被对面兵种打 |
   | M5 侧向 | U 旋转放置，目标偏离正前方（地面 U 对 rhino，空中 U 对 marksman） | 攻击角度、车身或武器转向 |
   | M6 多编队 | 两个 U 编队对 crawler 加 marksman | 目标分配、友军之间的避让 |

   U 本身就是对手的那一格（rhino 的 M2、crawler 的 M3）与 M1 重合，不另录。

   **标准布阵要在任何一座塔倒下之前结束。** 塔被摧毁会给那一方挂减益，这是另一个机制，
   不属于单位。打不到对面的一方会走去拆塔，所以这种格子里能打的一方出足够多的编队，
   在塔倒下之前赢（rhino 或 crawler 对三个 wasp 编队、三个 wasp 编队对 rhino 或 crawler、
   五个 wraith 编队对 rhino——三个时塔先倒了），M5 的目标放得比敌方两座塔都近。只用一个 wasp 编队、或者目标放远时，参照单位的这 16 场录像全都在塔倒下
   那一 tick 与模拟器分开。
2. 反编译里 U 不带科技就有的每个技能，要么被某个布阵复现，要么按名拒绝；U 身上有
   按名拒绝的技能，就不算。
3. 当对手的单位——rhino、crawler、wasp、marksman——先满足这个定义，否则对不上时分不清
   是谁的错。

达标的单位把全套布阵落在 `tests/units/<unit>/`。等参照单位达标之后，支持名单改为由
钉住的全套布阵决定，而不是手写。

**次序。** 参照单位 rhino、crawler、wasp、marksman 先；然后名单里其余 7 个；然后 12 个
被拒的单位，按语料里不带它的科技出场的回合数排：sabertooth 133、hound 117、
sledgehammer 79、fire_badger 77、vortex 69、void_eye 67、tarantula 67、phantom_ray 31、
farseer 19、typhoon 17、scorpion 12、hacker 7。问题按机制切，不按单位切：双武器、
组模式的直接伤害、控制光束各是一个问题。巨型单位最后，各自立项。

一批单位怎么推进：先把它们的全套布阵录下来、拿模拟器逐场核对，对得上的直接钉住；只有
对不上的那几场，才按第一处分叉的机制切成研究问题。

**参照单位的进度。** 四个参照单位的 44 场录像全部逐 tick 对上，钉在
[`tests/units/`](tests/units/README.md)，rhino、crawler、wasp、marksman 都已达标。最后一场是
rhino 的 M6（种子 1787720817）：后摇期间被挤出射程、下一 tick 回来的爬虫，要在回来的那一
tick 就开始下一击，模拟器原先推迟了一 tick。不带科技时单位只有主技能（`FightMech` 构造时只加
`mechData.GetMainSkillID()`，额外技能只经由科技），所以第 2 条由主攻击本身满足。

**其余 7 个的进度。** arclight、fang、mustang、steel_ball、wraith、stormcaller、phoenix
的 84 场录像里 73 场逐 tick 对上并钉住；arclight、mustang、phoenix 已达标。按构建改正过的：
旋转编队的行列数（fang 的 2 场 M5）；钢球全歼对面时塔的拆除事件（6 场），`TryDstroyTower`
不看最后一击的攻击方式。另外 11 场分在四个机制上，都不是单位自己的伤害或移动：目标正前方
时出生朝向的正负号（2 场）、最后一 tick 胜方单位的转向（2 场）、fang M6 一处 0.0015 m/s
的速度差（1 场）、wraith 的分组武器搜索（5 场，外加 1 场被模拟器按名拒绝）。逐场的 tick
与分叉点见 [`tests/units/`](tests/units/README.md)。

## 一、先定架构，否则并行不起来

原来的 `kernel.rs` 长到近一万行，`Simulation` 一个 impl 就三千多行，单位的数值直接从
`UnitConfig` 读（186 处）。它现在已按游戏的结构拆成 `crates/simulation/src/fight/`
下的文件（见 architecture.md 的"代码在哪"），数据和技能状态机的拆分见下文的"统一索敌更新"。这种形状下每加一个字段都要动内核，两个人没法同时干活。
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

闭包因此由登记表生成，不再是 `layout/mod.rs` 里一串手写的拒绝：一份 layout 能编译，当且
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

做完这条，"军官给 +20% 伤害"是一张表里的一行，不是内核里的一段 if——这一条已经成立：
`officers.rs` 和 `technologies.rs` 读各自的效果表，内核里没有一行认识军官或科技。

**合成规则四条子句全部量完了**（`docs/rules/officer_effects.md`）：

```text
(base + Σ value) × (1 + Σ enhance) × Π (1 − impair)
```

增强相加、削弱相乘、value 按本单位相加、普通整数也相加，value 在 rate 之前——最后那条
顺序是用铁锤的攻击间隔量出来的。还没量的只剩**同一个数被两条通道同时修正时 property
怎么合**，`resolve` 对它照旧拒绝而不是猜。

## 收在这里的：工事机制研究

防御墙落地了，它带出来的问题收在这儿（`docs/rules/constructions.md` 有全部读数）：

- ~~**什么让单位去打墙。**~~ 量完也实现了，规则在 `docs/rules/constructions.md`：
  射程按**边到边**算（攻击距离 + 攻击者半径 + 墙块半径，早先写的"+7"只对爬虫成
  立），线宽 11.5 米是常数（钢球把区间收到 `[11.447, 11.507)`），**技能每个空闲 tick、以及
  准备攻击的每个 tick 都在问**（不是锁定那十 tick 的搜索）；准备中问到墙就放弃这次攻击。
  物理层逐 tick 对上：`wall-line-of-fire` 20/20、`wall-line-tolerance` 19/19、
  `wall-aside` 91/91。
- ~~**本体和武器拆开。**~~ 做了：`lock_target` 永远是本体目标（朝它走、有身体的朝着
  它），武器的目标由 `Actor::attack_target()` 派生（有墙挡着就是墙，否则就是锁定）。
  字段定义写进了 `docs/spec/mcfr/mcfr.md` 的"What a unit is directed at"，测量写进了
  `docs/rules/combat.md`。物理层和拆开前逐位相同。
- **物理对上不等于锁定对上。** `mech_lock_target`、`weapon_aims`、`motion_state` 是
  内容层字段，物理哈希不看，所以离线回归**钉不住**这些字段。`fight compare` 现在按
  字段组逐 tick 报差异，`content_equal` 在每份录像上都是 false 的原因也就看清了：
  墙战斗里只剩 `derived.current_attack_interval`（`tests/interval/` 那个错开），
  跟墙无关。
- ~~**墙块倒下那一 tick 之后怎么办。**~~ 做了：单位打倒正在打的那块墙之后，下一 tick
  进 Idle、锁定清空、武器还报着倒掉的那块，之后才重新找目标；武器组照旧整组清空。
  模拟器以前当场就去打下一块。现在三场能跑的墙战斗，锁定、武器目标、运动状态逐 tick
  全对（`fight compare --fields` 看的），`line-of-fire.mcscript` 录完就拿模拟器比这三项。
- ~~**近战和激光打墙。**~~ 做了：犀牛（`wall-rhino`）189 tick 物理和内容**全等**，
  进了 `regressions.mcscript`；钢球（`wall-laser`）到第 167 tick 全对。墙块倒下后：
  攻击结束（犀牛要等挥击收完）才空闲一 tick，有身体的武器还报倒掉的那块、没身体的不报。
- ~~**墙块挡不挡走路。**~~ 量完了：对放墙的一方不存在（墙"沉下去"），对另一方是不可
  移动的 RVO 障碍——墙的 `pathfinding_collider_priority`（5）那一层、半径 4 米、按对
  敌避让。`wall-laser` 因此 243 tick 物理和内容**全等**（钢球的 `derived.attack_damage`
  报光束第一档，2），进了 `regressions.mcscript`。
- ~~**爬虫编队第 8 tick 就分叉。**~~ 做了：不是编队，是单位往目标走时"已经够近了吗"这
  个比较——游戏拿精确的距离平方去比快速开方得到的停止距离的平方；爬虫的射程加半径正好
  抵消射手的半径，比较落在最后几位上，前排 8 只被派往射手中心、后面 16 只被派往算出来
  的点。修好后无墙的 `crawlers-vs-marksman` 161 tick 全对，进了回归清单；`wall-block`
  推到第 96 tick（对面爬虫挤进墙块 0.711 米那个读数也复现了）。
- ~~**`wall-block` 第 96 tick。**~~ 做了，209 tick 物理和内容**全等**，进了
  `regressions.mcscript`。一路补上的五条：两下之间（挥击收尾结束后、下一下蓄力期间）
  也会问挡路，换了一块就空闲一 tick 再打新的；够不够得着用游戏的快速开方（中心 12.00023
  米照样开打）；近战打倒的墙也在本 tick 所有命中之后才倒；只有打过这块墙的单位倒墙后
  才空闲一 tick，还没挥出第一下的直接转向；挥击收尾期间继续转向倒掉的墙块。至此能跑的
  8 场墙战斗全部逐 tick 对上。
- ~~**射手击杀后的锁定（内容层）。**~~ 做了：射手在攻击点内打死目标、而下一个目标不在
  攻击范围里时，会在冷却期间空闲、武器先对着选择器的答案，冷却后下一 tick 清空武器、再
  下一 tick 才锁定。`crawlers-vs-marksman` 和 `wall-passage` 因此内容层也全等。至此所有
  能跑的录像内容层只剩 `derived.current_attack_interval`（攻击间隔的逐周期错开）。
- ~~**`derived.current_attack_interval`。**~~ 做了：武器组读核心位的间隔（不是最后一个
  抽样的子位）；没有敌人剩下的单位从最后一个敌人死后的下一 tick（或最后一 tick）起读不带
  错开的合成间隔。至此**模拟器能跑的 25 份录像物理和内容逐 tick 全等**；8 场墙战斗在
  `regressions.mcscript` 里连内容哈希一起钉住。
- ~~**`wall-passage` 第 44 tick。**~~ 做了：原生 sidecar 显示，爬虫穿自己的墙时 20 个
  邻居里有 3 块墙、速度障碍只有 17 个——自己的墙照样占邻居名额，只在生成速度障碍时跳
  过。改完 `wall-passage` 341 tick 物理全对，进了 `regressions.mcscript`（内容层只在
  最后 327–331 tick 的锁定上有差）。
- **墙块倒了之后空闲多久。** 能看出长度的两份录像（4 tick、10 tick）带着 Farseer 和
  堡垒，模拟器跑不了，所以长度由什么决定还没分开。
- ~~**成组武器的其余武器位打墙。**~~ 做了：每个位分配到的仍是单位，挡路检查在分配之
  后按位替换"这个位打什么"；锁定丢掉时整个武器组一起清空。`wall-weapon-group` 从第
  40 tick 推到第 93 tick。之前以为"同一 tick 打两块墙＝各个位各自判断"，其实是**溅射**。
- ~~**伤害提成一条管线。**~~ 做了：`DamageHit`（对应 `IDamageProvider`）交给
  `damage_targets` 决定打到谁、`strike` 扣血、`perform_damage` 串起来，单位和建筑同一
  段代码。直接攻击、弹丸、激光都改成提供者。新旧模拟器在所有被跟踪布阵 × 三个种子上
  106 场战斗物理和内容哈希逐位相同。后面的指挥官技能、地雷、炮台、死亡爆炸都应该是
  新的提供者，而不是新的结算。
- ~~**打墙的溅射。**~~ 做了：瞄准建筑的一枪溅到边缘在半径内的其他敌方建筑，满额。
  `wall-weapon-group` 242 tick 全对（内容层也全对），进了 `regressions.mcscript`。
- ~~**跨类溅射。**~~ 做了：溅射不管瞄的是什么，按目标树顺序打半径内所有敌方单位和
  建筑（墙进了目标树，选目标时跳过不可搜索的建筑）；死亡和倒塌都放到 tick 末尾按被打
  先后记录，死掉的单位离开目标树。三个 `wall-splash*.yaml` 物理和内容全对，进了
  `regressions.mcscript`。
- ~~**打墙走通用的索敌更新（第 1 步）。**~~ 做了：墙只在 `search_attack_target`
  （`FightSkill.SearchAttackTarget`）里出现；准备期间每 tick、攻击期间两下之间和蓄力期间跑
  `check_attackable`（`SkillAttackableChecker.Check`），失败走 `finish_attack`
  （`SkillAttackState.Finish`：清锁定、冷却、清空进空闲）。原来墙块倒下、锁定死在墙后、
  准备/两下之间被打断、快速切换射击间隙重检这四条墙专用分支全删了。依据是反编译加上新的
  技能状态采集（`skill-state.mcscript`）。82 个回归用例和 11 场墙战斗物理、内容全对。
- ~~**统一索敌更新（第 2 步）。**~~ 做了（C 阶段）：技能状态收成一个 `SkillState`；游戏的技能状态和
  逐次 `Check` 调用两份采集当对照，内核状态在所有可比的单位-tick 上只差 3 处，`check_attackable` 对游戏
  149,829 次调用有 149,695 次同答，其余都是成组技能的槽位。快速切换出界、失效目标、失效替换三条旧路径删了。
  成组技能（Wraith）随后由它自己的研究问题收口：四个槽位各自过检查器，搜索时排除兄弟槽位的锁定、
  允许共享时退回到已占用的目标，子技能射程比父技能多 10 米；`tests/wraith/regressions.mcscript` 把两场
  oracle 物理、内容都钉住，规则在 `docs/rules/combat.md`。**还剩：** 成组核心的准备偏移（`step + 1`）
  仍是校准值；"不能快速切换的技能保留走远的活锁定"是量出来的。路径已读到，是同步搜索答回了锁定，
  不是快速切换分支；评分为什么偏向它还没有读到。
- **炮台怎么开火。** 已由研究问题收口：炮台是一座跑单位技能机的建筑，
  单位和建筑的差别只经 `fight/attacker.rs`（游戏的 `ISkillOwner`／`IAttacker`）进入共享的
  `update_skill`；装填是共享状态机的一部分。两场速射炮和反装甲炮打弧光那场物理、内容都钉在
  `tests/turret/regressions.mcscript`，规则在 `docs/rules/turrets.md`。**还剩：** 反装甲炮打爬虫
  那场第 509 tick 起是塔被毁的减益（排在 `BuildingSystem`，研究问题已开）；单位的开始攻击仍从运动里
  问，建筑的从技能更新之后问，游戏里两者都在 `SkillIdleState`。
- **释放到底需要什么。** 这个 build 只放得下"开局自带"的那些工事：同一份布阵换
  个种子、或者把墙挪到 `(-140, -105)`，释放就被拒绝。磁力路障因此也一次都没放
  成——所以多行工事的几何还是空的。
- **多行工事的几何。** 墙那一排 12 米是量出来的，`block_width`／`space` 解释不
  了它，唯一能把公式和"凑上一次读数"分开的磁力路障放不下去。

## 二、模块并行，按登记表分工

登记表已经把十二个字段分给了七个模块，所以分工就是模块，几条线可以同时走：

| 模块 | 认领的字段 | 验收面 |
| --- | --- | --- |
| **Modifier**（非原生模块） | ~~`officers`~~、~~`techs`~~（已落地）、~~`equipment`~~（第一回合已落地） | `fight modifiers` 的三条通道 + 原生 MCFR 逐 tick 对齐 |
| ~~**FightConstructionSystem**~~ | `constructions` | 防御墙已落地，炮台指名拒绝 |
| **BuildingSystem** | `energy_tower_skills`、`tower_strengthen_levels` | 录像的 `buildings` |
| **CommanderSkillSystem** | `battle_skills` | 录像的 `terrains`、`shields` 和释放事件 |
| **RangeItemSystem** | `terrains` | 录像的 `terrains`，[terrain.md](docs/rules/terrain.md) 已有机制 |
| **AdvancedEnergyShieldSystem** | `airdrop_shields` | 录像的 `shields` |
| **InterceptSystem** | `contraptions` | 录像的 `buildings` 与拦截事件 |
| **SuperDeploymentSystem** | `travelling` | 录像的 `units.position`／`motion_state` |

单位等级不归任何模块：`FightMech` 构造时就带着它的 `IMechLevelData`，`GetBaseLife`、
`GetBaseDamage` 先乘等级评级、再过各个 `DataSet`，build 2259 的评级就是等级本身。所以等级
随单位进入 `Stats`，是一个独立乘区（[`docs/rules/unit_levels.md`](docs/rules/unit_levels.md)）。

`Modifier` 之所以不是原生的 35 个之一：军官、科技、装备在游戏里是**开打之前**
施加到单位上的（`TechnologySystem.AddTechnologyEffect` 收的是 `PlayerController`，由
部署动作 `MAP_AddUnit` 调用），不是战斗里的系统。模拟器照样在建立战斗时一次性把它们
写进覆盖层。

每个模块各自还欠一份**配置提取**，这是研究不是工程，可以和引擎并行。军官那份已经做完：
[`config/officer_effects.yaml`](config/officer_effects.yaml) 是 79 名军官写给单位的修正，
[`docs/rules/officer_effects.md`](docs/rules/officer_effects.md) 说明编码和瞄准规则，
数字拿 build 2227 的本地化文案核对过 73 条、不一致就拒绝写表。

科技那份也做完了：[`config/technology_effects.yaml`](config/technology_effects.yaml) 是
233 项科技里 137 项写给单位的修正，
[`docs/rules/technology_effects.md`](docs/rules/technology_effects.md) 说明编码和等级索引。
它不在 `ConfigDataContainer` 里，而在 `level0` 的 `TechnologyGroupData`（path id 184），
也就是 `extract_prices.py` 已经在读的那个对象——这次只是沿着同一条记录往后读过 `supply`。

**科技带来了军官带不来的两样东西**：一是效果按**等级**索引（`List<FPoint>`，精英射手九
条），二是同一个数上可能同时出现 value 和 rate——那正是合成公式里唯一还只靠类结构、没被
测量过的顺序。

科技通道也接上了：`crates/simulation/src/modifier/technologies.rs` 把 137 行里的 **125** 行应用到
对应单位上（43 行落在内核打得动的 11 个单位），其余 12 行指名拒绝——7 行修的数这个模拟器
不推导，5 行随等级增长而哪一级读哪一条还没确立。

装备和能量塔技能是同一个接口的另外两个实现，所以表的形状已经定了；它们的数据在别的
Unity 对象里，还要各解析一次。

## 二之半、一个机制怎么研究：循环

每个机制都是同一个循环，[`tests/modifier/composition.mcscript`](tests/modifier/composition.mcscript)
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

这个循环现在拆成两半并行：读反编译、录像、发布 oracle 由持有游戏的那个会话做完、开成 issue；拟合、实现、钉住、写规则由认领的 agent 离线做，一个问题一条线。怎么开、怎么认领、怎么要新录像、怎么验收，在 [`.github/CONTRIBUTING.md`](.github/CONTRIBUTING.md) 的 Research 一节。

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
  残留，读不出来的逐项报缺；`fight stats` 读一个单位两半的数——写进去的修正、build 算出
  的派生值，以及科技是否被禁用。
- **修饰符（`Modifier` 模块）**：军官 61/79、科技 125/137 落地，合成规则四条子句全部
  对着游戏量完。`tests/modifier/` 十二条离线回归钉住物理和内容两层哈希。
- **工事（`FightConstructionSystem`）**：一条 `constructions` 落到战斗里是 `count` 个
  对象——防御墙五块、每块 1112 命、相隔 12 米，都是对着游戏量的。墙摆得对、谁也挡不
  住，**挡路的那块会挨打**：单位不换锁定，打的是自己到目标那条线够得着的最近一块敌方
  工事。两条离线回归逐 tick 钉住（摆着不动的一场 91 tick、打墙的一场 20 tick）；炮台
  会开火，指名拒绝。`fight buildings` 把录像里的 building 行匹配回布阵的落点。
- **MCFR 0.6.0**：录像带上派生值（移速、射程、伤害、当前攻击间隔），两个后端逐周期
  一致；物理层一位没动。

## 收在这里的：科技机制研究

下面这些是**科技这条线自己的深度问题**，不挡广度，谁接着做谁从这里开始：

- **一次电磁禁用持续多久。** 现有录像里雷霆每 92 tick 补一发，犀牛直到死都没解除过，
  所以测不出。要测得让载体只打一发。
- **另外 96 项科技做什么。** 召唤、改技能自己的数值、给敌人上减益——每一项欠它所属的
  那个机制，不欠算术。
- **等级索引。** 效果列表按等级索引，而"施加那一刻单位是几级"没人确立过；会增长的
  5 行因此被拒绝而不是读第 0 条。
- **战斗中还有什么在消耗攻击间隔随机流。** 部署时那次错开已经量完
  （`tests/interval/`），流的其余部分没有。

## 还没做、但不在这条主线上

- **`arena run` 和 `shell --json`**：一行一个请求、一行一个结果，和 arena 跟玩家
  进程说的是同一套协议。它需要各操作**返回**结果对象而不是打印，是一次跨命名空间的
  重构。模拟器能打完一局之前，编排出来的也只是一局停在 `fight` 的对局，所以排在
  模拟器后面。
- **game 后端**：让真实游戏打这一仗。adapter 现在只能下发 layout 并录制，读不回打完
  的局面；等它具备了再定怎么进契约。game 打的一仗不可重算，届时 outcome 必须落盘。

## 顺序

1. ~~架构三件事~~ 和 ~~修饰符~~ 都做完了。
2. **回到广度**：`FightConstructionSystem` 的防御墙那一半做完了，292 个回合的
   `constructions` 因此满足。按贪心序下一个是 `Modifier` 欠的那三个字段（等级、
   装备），然后 `CommanderSkillSystem`、`InterceptSystem`、`BuildingSystem`。
3. 与之并行的是剩下两张效果表（装备、能量塔技能）：同一个
   `ICommonMechDataChangeDataSource` 接口，表的形状已经定了，`Modifier` 接上就是
   改一行 `understood`。
4. 与模块广度并行的是**单位的无科技基本支持**（见上文那一节）：参照单位先行，
   然后按语料出场回合数补齐其余单位。
5. 语料层稀疏验收接上（先只判胜负方），再拟合反应堆伤害与经验两条规则。
6. `arena` 与 `shell --json`；`game` 后端最后。
