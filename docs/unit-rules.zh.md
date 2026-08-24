# 单位数值配置规范

[English](unit-rules.md)

## 范围

`mechcore.unit` 只描述一个单位自身的基础数值。每个单位使用一个 YAML 文件，文件名必须
是 `<type_name>.yaml`。配置中不包含模拟时钟、坐标精度、RNG、layout、科技、Status、
装备或某个具体研究项目的诊断字段。

当前包含：

- [`marksman.yaml`](../config/units/marksman.yaml)
- [`arclight.yaml`](../config/units/arclight.yaml)

## 文件结构

```yaml
schema: mechcore.unit
type_name: marksman
unit_type_id: 2
domain: ground
max_life: 1622
collision_radius: 8
move_speed: 8
rotate_speed: 70
independent_aim: false

attack:
  attack_type: direct_projectile
  target_domain: both
  damage: 2329
  range: 140
  interval: 3.1
  interval_offset: 0.6
  release_delay: 0.6
  projectile_speed: 500
  effect_radius: 0
```

未知字段必须拒绝。一个目录内的 `type_name` 和 `unit_type_id` 必须分别唯一。单位配置
不携带 schema 版本或游戏构建版本，也不执行版本匹配检查。

## 保守的共同字段

| 字段 | 含义 |
| --- | --- |
| `type_name`、`unit_type_id` | layout 名称和 MCFR 稳定类型 ID。 |
| `domain` | 单位属于 `ground` 或 `air`。 |
| `max_life` | 基础最大生命。 |
| `collision_radius` | 运动、射程和投射物命中边界，单位 m。 |
| `move_speed`、`rotate_speed` | 移动和主体转向速度，单位分别为 m/s、deg/s。 |
| `independent_aim` | 武器瞄准是否独立于主体朝向。 |
| `attack_type` | 当前为 `direct_projectile` 或 `area_projectile`。 |
| `target_domain` | 可攻击 `ground`、`air` 或 `both`。 |
| `damage`、`range` | 单次基础伤害和攻击范围；单位分别为无量纲和 m。 |
| `interval`、`interval_offset` | 基础攻击间隔和确定性随机偏移，单位 s。 |
| `release_delay` | 动作开始至投射物释放总延迟，单位 s。 |
| `projectile_speed`、`effect_radius` | 投射物速度和作用半径，单位分别为 m/s、m；直射投射物半径必须为零。 |

这里不预先抽象编队卡牌尺寸、slot、固定减伤、后摇/冷却阶段等当前场景没有消费的字段。
需要新的游戏机制时，先证明它参与状态转换，再扩展 schema。

配置边界使用 m、s、deg。加载时，空间量必须能精确量化到 1 mm，时间量必须能精确量化
到 0.0005 s，角度速度必须能精确量化到 0.001 deg/s；量化失败直接拒绝配置。

弧光的基础攻击间隔写作 `interval: 0.9`。旧内核中的 `1799` 是 Q32 固定点的
`0.8999999999... s` 投影到每秒 2000 单位后得到的有损整数，不是单位设计数值。旧内核
随后按 0.05 s 逻辑步最近舍入为 18 步；新配置直接从 0.9 s 得到相同的 18 步。

## 加载与确定性

默认读取内置配置。也可以指定一个外部配置根目录：

```text
mechcore sim layout.yaml --config config
```

配置根目录的 `config.yaml` 只包含 `game_build`，单位文件位于 `units/`。
`game_build` 仅复制到 MCFR 持久上下文；它不是 schema 版本，也不触发版本匹配。
模拟时钟、坐标单位、RNG 算法和更新顺序仍由内核维护。

## 当前内核边界

当前 `mechcore sim` 每侧只接受一个一级、单成员、投射物单位；科技、装备、塔强化、
战场技能和旋转编队会显式拒绝。配置 schema 表达的是共同单位数值，不代表任意单位或
字段组合已经取得原生一致性证据。
