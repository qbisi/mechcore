# 指挥官技能冷却

[English](commander_skills.md)

本文索引固定在 build 2259。它说明指挥官技能的冷却怎么计数——按技能、按回合——以及
支撑它的证据。

机器可读的表是
[`config/commander_skills.yaml`](../../config/commander_skills.yaml)。
`scripts/extract_prices.py` 从 `level0` 里每一条 `CommanderSkillData` 读出它：
`initial_cooldown` 取自 `initialCoolDown`，`cooldown` 取自 `releaseInterval`，
两者单位都是回合。

## 规则

一个槽位加入面板时冷却是它所属技能的 `initial_cooldown`，本 build 每个技能都是 0。
每个回合开始时，上一回合用掉的槽位重置为该技能的 `cooldown`，其余槽位各减一，减到
0 为止。释放技能就算用掉这个槽位；强化训练、再部署这类部署阶段技能同样算用掉。
`cooldown` 为 0 的技能，例如战地修复和移动信标，每回合都能用。

录像对一个回合的快照是在该回合倒数之前取的，所以它记录的冷却是上一回合的。

## 证据

在本地观测集里，被使用过的 23 个技能每一次都在下一回合开始时显示表里的 `cooldown`，
共 549 次使用。某回合未使用的槽位共 487 个，每一个都恰好减一、减到 0 为止；观测到
加入面板的 18 个槽位全部从 0 开始。

按这条规则转换，再加上回合开始时的商店额度与装备收入，41 份跟踪对局的全部 668 个
单方回合的开局状态，在 oracle 比较的每个字段上都与原生开局一致。

表里 43 个技能中有 20 个在语料里从未被使用过。它们的冷却是读表得来的，不是观测到的。
