# 目标与计划

把 mechcore 做成 agent 和人类的对战平台：输入一条原子 action，返回下一个
state。一局比赛就是一份 battle 文档，平台一边下一边把它写出来。平台那一半已经能跑，
卡在模拟器打不了真实对局的仗；这条主线没有变。

仓库已经整体换到 2.0（build `2.0.0.1.2324`，提交 `0e3891d`），不再兼容 1.11。换版本的
计划全文和做完的状态在那个提交的 `plan.md`。

## 离真实对局还有多远

`scripts/fight-coverage.py` 把语料的每一回合投影成它开打时的 layout 交给 `fight run`，
模拟器一次报出这份 layout 被拒的全部理由。2.0 语料 287 回合，接受 12 回合。挡得最多的是
模块（物件 `InterceptSystem` 142 回合、战场技能 `CommanderSkillSystem` 116、能量塔技能
`BuildingSystem` 84、空投中的单位 43）、vortex 的分组齐射（112）、炮塔技能与军官科技的关系、
装备过了第 1 回合的耐久，以及几个单独的军官、科技、装备字段。按清除后放行最多的顺序，第一个
是 vortex 的分组齐射。

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

达标的单位把全套布阵落在 [`tests/units/`](tests/units/README.md)。

### 做法：一起放行，批量录，看缺口

不再一个一个加：所有单位同时放行，内核不再按白名单挑单位，有配置就进战斗；配置表达不了的主
技能形状按名拒绝。批量录制，逐场与模拟器逐字段比较，按第一处分叉归到机制上。一个机制修好，
所有卡在它上面的单位一起前进；每修一个，重跑全部录像，对上的钉进
`tests/units/regressions.mcscript`，已有的钉子一直要过。

### 现状

分支目标：除 hacker、sandworm 和三个 800 费用的单位（war_factory、abyss、mountain）之外，
所有单位达到无科技基本支持。

2.0 上 29 个单位有全套布阵，271 场钉住（`tests/units/regressions.mcscript`）：

- 四个参照单位 44 场全部钉住；arclight、fang、mustang、steel_ball、wraith、stormcaller、
  phoenix 84 场钉住 76 场。
- 批量放行的 18 个单位 216 场：hacker（控制光束）、raiden 和 vortex（分组齐射）、farseer
  和 overlord（先爬升再飞的弹丸）按名拒绝；其余 156 场钉住 151 场。centurion、fortress、
  melting_point、sabertooth、scorpion、sledgehammer、tarantula、typhoon、void_eye、vulcan
  12 场全对。
- 修好的七条机制写在 `docs/rules/combat.md`：武器按原生下标命名；跟随目标的弹丸保留随机
  偏移；单武器的偏移后抽先落；模拟运动的弹丸落在已死单位上什么也不做；带溅射的光束打溅射
  范围内所有目标；离开空闲状态的出手等一 tick；空闲技能只在能打到时保留锁定。后两条靠
  adapter 新增的 `mech_body_rotation`（`target_refs` sidecar 里机甲本体的朝向）读出。

剩下的，由易到难：

1. **phantom_ray m3 一场**只差内容层（第 176 tick 一件武器的攻击目标），未读。
2. **正前方目标的朝向正负号。** hound 两场、fire_badger 一场、phantom_ray 一场，加上
   steel_ball、stormcaller 各一场。`FightUtility.ConvertToAngle` 的符号规则与模拟器相同，
   差在预搜索时取方向的两个位置。
3. **wraith 的分组搜索**（6 场）。2.0 录像不在本机，要重录。
4. **被拒的主技能形状**：分组齐射（raiden、vortex）、先爬升的弹丸（farseer、overlord）。
   hacker 的控制光束不在本分支目标内。

### 换版本留下的尾巴

- **只读、未录的规则。** 各需一段训练场录制：重型导弹打击的一次释放、一个编队戴两个强化
  模块升级、第 9 个物件被拒、训练场里次级装备专家用的玩家种子。
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
