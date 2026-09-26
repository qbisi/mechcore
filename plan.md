# 目标与计划

把 mechcore 做成 agent 和人类的对战平台：输入一条原子 action，返回下一个
state。一局比赛就是一份 battle 文档，平台一边下一边把它写出来。平台那一半已经能跑，
卡在模拟器打不了真实对局的仗；这条主线没有变。

这份文件只写方向和先后：要做什么、为什么先做它、做到什么算完。量出来的数和做到了哪一步
不写在这里，写在它们各自的出处：一次改动的前后数字在那个 PR 的正文，一个课题的录像、钉子和
分叉在 `tests/<topic>/README.md`，一条规则在 `docs/rules/`。PR 只在方向或先后变了时改这份
文件。

## 离真实对局还有多远

`scripts/fight-coverage.py` 把语料的每一回合投影成它开打时的 layout 交给 `fight run`，
模拟器一次报出这份 layout 被拒的全部理由，它的输出就是这一节的答案。挡得最多的是模块
（物件 `InterceptSystem`、战场技能 `CommanderSkillSystem`、能量塔技能 `BuildingSystem`、
空投中的单位），然后是炮塔技能与军官科技的关系、装备过了第 1 回合的耐久，以及几个单独的
军官、科技、装备字段。

## 下一步：battle 转 grbr，语料成为游戏 oracle

游戏能不建场景地打一份回放的任意回合（`record_replay_round`），一份 layout 也能写成回放来打
（`replay convert <layout.yaml> <replay.grbr>`，[layout-replay.md](docs/spec/document/layout-replay.md)）。
下一个 PR 把这两件事接到语料上：battle 的每一回合写成回放，由游戏无头打完。

- **对照。** 同一回合，真实回放与由 battle 写出的回放各打一遍，两份录像应当相等；不相等的地方
  要么是投影丢了战斗读的状态，要么是写入器的错。
- **oracle。** 无头打完的结果对照 battle 下一回合记录的状态，语料的每一回合因此都有游戏给的
  答案，不依赖模拟器。
- **还没在对局里验过的**：被拦截裁剪的油区网格，只由单元测试对着回放读取器验证。
- **battle 等价表示 grbr：奖励池日志。** battle 与 grbr 已经互转幂等，但游戏重打一个回合时自己重新
  发奖励卡，奖励池由快照里的 `poolOPs` 重建；battle 不含这份日志，所以从某次选择改变了后续卡池的回合起，
  游戏发的卡与对局不同，记录的选择拿到别的卡。battle 要能表示奖励池（`ReinforcePool.SelectReinforce`、
  `OnNewRound` 怎么产生这些操作），写出的回放才在游戏里逐回合与原回放相同。

## 主线：单位的无科技模拟

一个单位 U 在 1 级、第 1 回合、没有军官、科技和装备时算有**基本支持**，当且仅当：

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
   **标准布阵要在任何一座塔倒下之前结束**；打不到对面的一方会走去拆塔，所以能打的一方
   出足够多的编队，在塔倒下之前赢，M5 的目标放得比敌方两座塔都近。
2. 反编译里 U 不带科技就有的每个技能，要么被某个布阵复现，要么按名拒绝。
3. 对手的单位——rhino、crawler、wasp、marksman——先满足这个定义。

达标的单位把全套布阵落在 [`tests/units/`](tests/units/README.md)，哪些单位达标、哪几场
在哪里分叉，那份 README 说。

### 做法：一起放行，批量录，看缺口

不再一个一个加：所有单位同时放行，内核不再按白名单挑单位，有配置就进战斗；配置表达不了的主
技能形状按名拒绝。批量录制，逐场与模拟器逐字段比较，按第一处分叉归到机制上。一个机制修好，
所有卡在它上面的单位一起前进；每修一个，重跑全部录像，对上的钉进
`tests/units/regressions.mcscript`，已有的钉子一直要过。

目标范围：除 hacker、sandworm 和三个 800 费用的单位（war_factory、abyss、mountain）之外的
所有单位。剩下的机制，由易到难（各自卡住哪几场见 `tests/units/README.md`）：

1. **overlord**：一发爬升弹丸的高度差几个 raw 单位；一个单位游戏里停下、模拟器里继续走。
2. **phantom_ray 的内容层**：战斗结束后冷却里仍点名已死的最后一个敌人。
3. **正前方目标的朝向正负号。** `FightUtility.ConvertToAngle` 的符号规则与模拟器相同，差在
   预搜索时取方向的两个位置，要一段采到预搜索时位置的录制。
4. **wraith 的分组搜索。**
5. **Raiden。** 构建按单位数据 27 给它的每件武器一个固定在机身上的变换
   （`FightWeapon` 构造器），三件分组武器齐射；子槽位在没有别的单位可选时锁敌方的塔、
   不在射程就不开火，子武器的朝向在交战时滞后机身一 tick、否则冻结。要单独研究。

### 换版本留下的尾巴

- **只读、未录的规则。** 各需一段录制：重型导弹打击的一次释放、一个编队戴两个强化模块升级、
  第 9 个物件被拒、次级装备专家用的玩家种子。
- **语料的自动核对。** 现在只在本地跑（`replay.py sync`、`export-replay-corpus.py`、
  `verify-battles.py`）。语料独立增长，不宜挡 PR；待定的做法是一个 master 推送和每日触发
  的 workflow，只核 `replays/<GAME_VERSION>/`。
- **随迁移发现的。** 模拟器让核弹、闪电风暴、离子轰炸按 `initial_cooldown` 1 入列；新的
  `SkillDataChangeInt.AttackValue` 谁写；adapter 按枚举下标读技能整数，2.0 新增了 6 到 11；
  `UpgradeExp` 取代经验条，谁写它；rapid-fire 用种子 1787720817 时第 242 tick 的分叉。
- **新机制随新单位而来。** 近战模式（`MeleeModeEffectSystem`）、副武器
  （`SideArmSearchTargetController`）、弹药池（`AmmoSkillPool`）、`IgnoreBuffEffectSystem`、
  出售单位（`PAD_SellUnit`）、塔成为 buff 目标。批量录像会指到其中哪些先要做。

## 一个机制怎么研究：循环

[`tests/modifier/composition.mcscript`](tests/modifier/composition.mcscript) 是范本：

1. **把问题收成一个数。** 收不成一个数的问题，做不成实验。
2. **先读能读的，并写下它答不了什么。**
3. **设计能区分的实验，并且带对照。** 对照录两遍必须逐 tick 相等，不等就停。
4. **先把每个假设的预期值写进脚本，再录。**
5. **测量有两半，对上了才算规则**：游戏存了什么，和游戏算出了什么。
6. **规则落成表和代码，拒绝先于猜。** 数落进 `config/`，出处落进 `docs/rules/`，
   没够着的情况模拟器按名拒绝。

## 之后的顺序

模块广度（`Modifier` 的等级与装备、`CommanderSkillSystem`、`InterceptSystem`、
`BuildingSystem`），效果表（装备、能量塔技能），语料层稀疏验收与反应堆伤害、经验两条
规则，最后 `arena`、`shell --json` 和 `game` 后端。单位基本支持之后按 fight-coverage 的
顺序重排。
