# 文档与命令行

回归夹具是一份自己说清楚结果的文档，命令行按动作划分、按文件内容决定语义。做完的判据：
`tests/` 里的钉子都是 fight 文档；命令行只剩对文件的顶层动词，和 `match`、`arena`、`game`
三个有状态的命名空间。

已经定下的设计：

- **场上遗留物是战场技能。** layout 和 state 里不再有 `airdrop_shields`、`terrains` 两个集合：
  前几回合释放、开战时还在场的护盾和油区是 `battle_skills` 的 `standing` 条目，和本回合的释放
  （`positions`）字段互斥；标准型里 `standing` 条目排在前面并排序，释放保持原序。state 记在
  面板槽位上，和原生回放把它们记在释放技能的 `rangeItems` 里一致。
- **fight 文档是写进了结果的 layout。** `layout = project(fight)`；`seed` 必填；`source` 是
  `recording`、`replay` 或 `simulator`，模拟器算的不作夹具；录像来源带 `ticks` 和 hash；没有
  `winner`，胜负由 `core_damage` 说明；单位 `exp: before/after/maximum`；contraption 的
  `retained` 默认 `true`；本回合释放的技能的结果写在它自己的 `battle_skills` 条目下，地形类
  技能另记 `grid_rows`。
- **命令行按动作划分。** 命名空间只给活过一条命令的东西：`match`、`arena`、`game`。对文件的
  操作是顶层动词，语义由文件内容（YAML 的 `kind`、二进制文件头）决定：`verify`、`convert`、
  `diff`、`show`、`format`、`schema`。`convert <in> --to <kind>` 必填 `--to`，分改写（无损，
  做不到就拒绝）和计算（模拟）两类，模拟是 `convert --to fight|mcfr`。fight 文档没有自己的
  扩展名。`show` 与 `match show` 同名。`game record` 按输入分派，合并现有的四个录制入口。

## 栈

1. **standing 重构。** layout、state、投影、回放互转、状态转移预测、Adapter 的应用与采集、
   模拟器的拒绝、`fight outcome` 与规格一起改。
2. **命令行改版。** `docs/spec/mechcore/cli.md` 与 `mcscript.md` 按上面的划分重写，代码、
   脚本里的操作名、测试一起迁；`doc`、`fight`、`replay` 三个命名空间消失。
3. **fight 文档。** `docs/spec/document/fight.md`，`convert --to fight`（从录像读、从 layout
   模拟），`verify` 读 fight 文档。
4. **钉子迁成 fight 夹具。** `tests/<topic>/regressions.mcscript` 里的布阵、种子和 hash 变成
   一份份 fight 文档，脚本只剩对目录跑 `verify`。

## 停车场

- **语料按回合出 fight 夹具。** 转换器从原生回放的每一回合写出 `source: replay` 的 fight 文档，
  只有结转字段、没有 hash，放在 `work/` 下不进仓库。reopen_when：栈 3 合并。
