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

1. **fight 从内存读出。** `verify` 与 `convert --to fight` 每一仗都先把模拟器的时间线写成临时
   MCFR 再读回来给读取器，读取器改为直接读模拟器内存里的时间线，录像的读取仍走 MCFR。只省
   这一写一读，结果不变：全部夹具照样有效。
2. **用游戏打一份文档。** `convert <fight|layout> --to mcfr --backend game [--instrument ...]` 用
   游戏无头打完并写出录像，`game record` 的这一用法并进来；`--backend` 默认模拟器。管道逐份
   重录与一个 run 内连续重录的耗时差多少，量出来写进 PR。
3. **回归与重录的脚本退休。** CI 用管道对全部 fight 文档跑 `verify`，各 topic 的
   `regressions.mcscript` 删掉；重录改为 `scripts/` 下对夹具目录跑 `convert --backend game`
   的脚本，`units/record`、`tower/*`、各 `slots`、`skill-state`、`regression/refresh` 与两份
   重复录制比对的脚本删掉；`tests/regression/fights/` 只留 smoke 那 16 场。
4. **校验脚本退休。** 只断言 hash 以外数值的脚本（construction、`equipment/stats`、
   `level/stats`、modifier 的各探针、`interval/stagger`）连同 `adapter/smoke`、
   `layout-replay/equivalence` 与 `simulate.mcscript` 最后那一场模拟器自钉的仗一起删掉：
   它们的仗已是 fight 夹具，丢掉的只是 hash 之外的断言。
5. **观战录制改成 Python。** `replay/record-standard-1v1.mcscript` 改写为 `scripts/` 下的脚本，
   观战录制的命令行入口保留。

mcscript 的执行器、`run` 与它的规格保留，只是不再有被跟踪的脚本。

## 停车场

- **语料按回合出 fight 夹具。** 转换器从原生回放的每一回合写出 `source: replay` 的 fight 文档，
  只有结转字段、没有 hash，放在 `work/` 下不进仓库。reopen_when：outcome 补完合并。
- **重生进事件。** MCFR 不记重生：采集在 `FightController.CreateMech` 看得到 `isRebirth` 和
  `createType` 却丢掉，outcome 只能从"死过又活到最后"推断重生、从编队配对推断召唤。只有凤凰
  （量子重组）会重生，不改通用单位字段：新增 `unit_reborn` 事件，`unit_created` 带上
  `create_type`，outcome 改为读事件。reopen_when：研究凤凰的量子重组科技时，用一个死后重生并
  活到最后的凤凰夹具钉住 `rebirth_unit_score_rate`。
