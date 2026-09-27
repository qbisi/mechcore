# 录像成为可分析的对象

一份录像直接读出谁在哪个 tick 用哪个技能对谁造成多少伤害、谁杀了谁、经验怎么分，读得到战斗的
内部过程，agent 只用 `mechcore` 和 Python 标准库就能对整个语料统计。

研究一直卡在"录不到"，而不是"查不动"。录像之外的观测只有 instrument sidecar：每个新问题要
一个新 profile，profile 互斥，同一场仗换 profile 录好几遍；观测先进 sidecar、再升格进格式，
每次升格进 physics 层都要重录全部钉子；谁打了谁、哪个技能造成的伤害、谁杀了谁、经验、buff
的施加与到期，都不在录像里。把录像做成 agent 能直接统计的对象，也是找战斗奖励信号的前提。

- **instrument 是 MCFR 的成员。** 按通道组织，每个通道一张有类型的 parquet 表，不进 physics
  与 content 两层哈希；通道可以任意组合，一场仗录一次；边打边写，不攒到战斗结束；模拟器写同样
  的通道，游戏与模拟器的内部过程因此能逐 tick 对照。
- **instrument 的量不超过实体数的线性倍。** 两两配对的数据不录，只录每个实体有上界的那部分。
  RVO 一次求解只看 agent 自己的邻居列表，上界是 `maxNeighbours`：每个 agent 一行求解（输入、
  两条梯度追踪、起决定作用的那个 VO、输出），加至多 `maxNeighbours` 行邻居，完整 VO 几何放在
  开关后面。选择器录选中的目标与前几名候选。
- **伤害、击杀和经验读游戏自己的，进 physics 层。** 游戏在逻辑 tick 里按编队累加造成的伤害、
  实际扣掉的生命、击杀和承受的伤害（`BattleStatisticManager`），编队经验由 `ExpSystem` 分配；
  两者都进 physics 哈希，模拟器照样复现。每一下伤害是哪个技能、哪颗投射物打的，这些计数里没有，
  仍由伤害事件带着。
- **可推导的量存进文件。** `derived/` 成员放每回合摘要、每单位汇总、双方随时间的序列，不进
  哈希，由状态和事件重算，旧录像也能补。
- **查询只做导出。** `fight export` 出 JSONL 或 CSV，或把 parquet 成员原样解出来，跨文件时拼
  `derived/` 的摘要。
- **模型按需改，改了就重录。** content 或 physics 模型一变，全部钉子在有游戏的机器上重录。

## 栈

1. **content 与 physics 模型。** 伤害带来源技能，死亡带击杀者，补上 buff 施加与到期、单位生成、
   治疗和移除原因；游戏的伤害击杀统计和编队经验进 physics 层，模拟器复现；修饰器改成稀疏的
   `(通道, 字段, 值)` 列表，删从不写入的字段（`velocity.y`、投射物的 `orientation` 与
   `released`、采集里写死为空的事件键）；修采集读错的技能修饰器下标和漏读的枚举成员（军官的
   经验倍率在其中）；`spawn_containing_shields` 的规格改成它实际的意思（出生时已包住弹丸、它
   自己的命中检测会看的那些护盾，整个寿命都豁免）。重录全部钉子。
2. **`derived/` 与导出。**

## 停车场

- **模拟器写 instrument 通道。** RVO 求解与目标搜索由模拟器写成同样的行，游戏与模拟器的内部过程
  逐 tick 对照。reopen_when：单位 lane 的一个研究卡在 RVO 或选目标的分歧上，而逐 tick 的状态
  对照定位不到是哪一步。

- **实体表。** 每个实体不变的字段从逐 tick 表挪到一张"变化才记一行"的实体表，读取时按 tick
  合并回完整状态。它省下的比存储一步的其余几项小一个量级，而 `team_id`、`domain` 会被精神控制、
  钻地改变，读写两端都要处理变化。reopen_when：录像体积成了语料同步或分析的瓶颈。

- **headless 下 Training Ground 偶发的托管 `NullReferenceException`。** 在线时 `main_menu`
  早于登录完成，建主机读的本地玩家 ID 由登录设置，可能与摆阵撞上；未证实。托管异常现在带调用栈。
  reopen_when：再出现一次，或 offline 的 headless 录制跑满 Adapter smoke 与 layout-replay
  等价性而没有出现，那时把这两份脚本也改成 headless。
