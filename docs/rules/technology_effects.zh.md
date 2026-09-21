# 科技对战斗做什么

[English](technology_effects.md)

本索引锚定 build 2259。它说明一项科技会往研发了它的那个单位身上写下什么修正、这些修正
怎么编码、又怎么随单位等级增长。一个单位能研发哪些科技、每项多少钱，是
[`unit_techs.md`](unit_techs.zh.md) 和
[`config/unit_techs.yaml`](../../config/unit_techs.yaml) 的事。

机器可读的表是
[`config/technology_effects.yaml`](../../config/technology_effects.yaml)，由
`scripts/extract-technology-effects.py` 从 build `level0` 里 path id 184 的
`TechnologyGroupData` 写出。32 张卡提供的 233 项科技里，**137 项往单位的数值上写东西**。

这些字段就是 `GameRiver.TechnologyData` 用来实现
`ICommonMechDataChangeDataSource` 的那一组——`OfficerData` 实现的也是同一个接口。所以
这张表和 [`officer_effects.md`](officer_effects.zh.md) 的形状一样，一条修正在两边含义
相同，**合成规则也是同一条**：

```text
(base + Σ value) × (1 + Σ enhance) × Π (1 − impair)
```

那条规则和量出它的那几次捕获，写在
[`officer_effects.md`](officer_effects.zh.md#一条修正怎么合成) 里。

## 一条效果是一个列表，按等级索引

`lifeChangeRate` 和它的邻居都是 `List<FPoint>` 而不是单个值，因为一项科技的效果可以随
单位等级增长。精英射手是最清楚的例子：它的 `damage_rate` 读作 `+0.25, +0.5, +0.75 …`，
`attack_range_value` 读作 `+5, +10, +15 …`，各九条——正是游戏文案里的"每提升一级，射程
+5、攻击 +25%"。

137 行里有 5 行会增长，132 行只有一条。**增长的列表就是首项乘以等级**，每一级各自舍入
——提取脚本在这条不成立时**拒绝写表**，这也正是误解析会很吵的原因：从错误偏移读到的
字节不会恰好构成等差。

## 一个数怎么读

| 种类 | 编码 | 例 |
| --- | --- | --- |
| `*_rate` | `FPoint`，Q32.32 原始整数 | `1073741824` 是 `+0.25` |
| `*_value` | `FPoint`，单位是那个数自己的单位 | `21474836480` 是射程 `+5` |
| `speed_value`、`min_attack_range_value` | 普通整数 | `5` |

文件里存的是原始整数，因为那才是 build 存的；每行后面的注释是同一个数读成小数的样子，
之所以只是注释，是因为小数才是有损的那个。

## 这 137 行都带什么

| 字段 | 行数 |
| --- | ---: |
| `attack_range_value` | 57 |
| `life_rate` | 36 |
| `damage_rate` | 36 |
| `attack_interval_rate` | 17 |
| `speed_value` | 14 |
| `attack_interval_value` | 12 |
| `splash_range_value` | 5 |
| `projectile_speed_value` | 3 |
| `min_attack_range_value` | 1 |
| `projectile_life_rate` | 1 |

一行常常同时带两三个，因为科技常常是拿一个数换另一个：发射器过载是
`attack_interval_rate` 的 `−0.5` **加上** `attack_range_value` 的 `−20`。

## 这张表不带什么

另外 96 项科技做的事不是"修正自己单位的数值"，而 96 这个数不是解析的缺口。一项科技也
可以：

- **召唤或发射某种东西**——尖牙制造每 36 秒造八只尖牙，防空弹幕一次打十六发导弹；
- **改的是技能而不是单位**——防空专精那句"对空中单位攻击 +90%"是针对某个域的伤害比率，
  属于 `ISkillDataChangeDataSource`，住在这项科技用 `targetSkillID` 指向的那个技能里；
- **把东西施加到敌人身上**——电磁爆炸命中时让目标的科技暂时失效。

这张表里的一行也可能只是那项科技的一半：光子涂层"受到伤害 −30%"不在这里，尽管它同时
也带着在这里的数。

**这些数字拿游戏自己的话核对过。** [`unit_techs.md`](unit_techs.zh.md) 用本地化文案列出
每项科技的效果，那是从 build 2227 用完全另一条路读出来的。**这张表说出的每一个数字——
137 行——都能在那份文案里找到。** 校验之所以朝这个方向做、而不是像军官那张表那样反过来，
是因为科技的文案会说很多这张表不带的效果，反着查会把上面那些行全部误判。

## 这个 build 应用哪些

`crates/simulation/src/modifier/technologies.rs` 把一行变成落在它所属单位上的修正，和军官一样打上
`Modifier` 标签。137 行里应用了 **125** 行（其中 43 行落在内核打得动的 11 个单位上），
其余 12 行会指名拒绝持有它的那一方：

| 为什么 | 行数 |
| --- | ---: |
| 修的是这个模拟器不推导的数：最小射程、溅射半径、弹体速度或寿命 | 7 |
| 效果随等级增长，而哪一级读哪一条还没确立 | 5 |

会增长的科技是**被拒绝**而不是读第 0 条——即使 layout 里的单位都是一级：哪一级读哪一条
正是下面那个未决项，猜它就等于立一条没人量过的规则。

`technology.mcscript` 是说明这条通道真的接到游戏上的那一份。一只弧光研发射程强化、同时
它那一方带着增程弧光，答案是描述里的 95 变成 **155 米**，而录像把科技的 `+40` 和军官的
`+20` 存成**一条** `attack_range_value = +60`——build 合并两个来源，和它合并两名军官
完全一样。

攻击间隔那几项科技还顺带做成了一件事：机械狂暴和穿甲弹是整个 build 里唯一一对**把 value
和 rate 放在同一个数上**的组合，合成规则里那个顺序最终就是靠它们量出来的。读数记在
[`officer_effects.md`](officer_effects.zh.md#value-先于-rate)。

## 本索引未确立的

- **另外 96 项科技做什么**，用模拟器需要的那种说法。每一项都欠它所属的那个机制：召唤、
  技能自己的数值、施加在目标身上的减益。
- **战斗读的是哪一级的下标。** 列表按等级索引，而本索引没有说"施加科技的那一刻单位是几
  级"，也没有说战斗中升级会不会重读。今天 layout 里的单位都是一级，那也是所有捕获覆盖
  过的唯一情形。
- **`min_attack_range_value`、`splash_range_value`、`projectile_speed_value` 和
  `projectile_life_rate`**：7 行带着它们，而 `crates/simulation` 里没有任何机制读这些
  数。剩下 130 行修正的都是它推导得出的数，其中 47 行落在它内核打得动的那 11 个单位上。
