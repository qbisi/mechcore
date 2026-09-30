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
  谁打的这一仗，`game` 或 `simulator`，模拟器算的不作夹具；两者都带 `ticks` 和 hash，游戏从回放
  打和从 layout 打的哈希一致，不一致是 adapter 的错；没有
  `winner`，胜负由 `core_damage` 说明；单位 `exp: before/after/maximum`；contraption 的
  `retained` 默认 `true`；本回合释放的技能的结果写在它自己的 `battle_skills` 条目下，地形类
  技能另记 `grid_rows`。
- **命令行按动作划分。** 命名空间只给活过一条命令的东西：`match`、`arena`、`game`。对文件的
  操作是顶层动词，语义由文件内容（YAML 的 `kind`、二进制文件头）决定：`verify`、`convert`、
  `diff`、`show`、`format`、`schema`。`convert <in> --to <kind>` 必填 `--to`，分改写（无损，
  做不到就拒绝）和计算（模拟）两类，模拟是 `convert --to fight|mcfr`。fight 文档没有自己的
  扩展名。`show` 与 `match show` 同名。`game record` 按输入分派，合并现有的四个录制入口。

## 栈

空。mcscript 的执行器、`run` 与它的规格保留，不再有被跟踪的脚本。

## 停车场

- **重生进事件。** MCFR 不记重生：采集在 `FightController.CreateMech` 看得到 `isRebirth` 和
  `createType` 却丢掉，outcome 只能从"死过又活到最后"推断重生、从编队配对推断召唤。只有凤凰
  （量子重组）会重生，不改通用单位字段：新增 `unit_reborn` 事件，`unit_created` 带上
  `create_type`，outcome 改为读事件。reopen_when：研究凤凰的量子重组科技时，用一个死后重生并
  活到最后的凤凰夹具钉住 `rebirth_unit_score_rate`。
