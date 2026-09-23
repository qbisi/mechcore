# 单位数值配置规范

[English](unit-rules.md)

## 范围

`mechcore.unit` 只描述一个 Formation Unit 的基础数据。每个单位使用一个 YAML 文件，
文件名必须为 `<type_name>.yaml`。配置不包含模拟时钟、数值精度、RNG 实现、layout、
科技、Status、装备或研究专用诊断字段。

[`config/units`](../../../config/units) 当前覆盖 P0 目标 build 中 23 个普通、非 Huge Formation
Unit。未知字段必须拒绝；同一配置根目录中的 `type_name` 和 `unit_type_id` 必须分别唯一。
单位文件不携带 schema 版本或游戏 build；`game_build` 只位于顶层 `config.yaml`，且不执行
版本匹配。

## 文件结构

```yaml
schema: mechcore.unit
type_name: marksman
unit_type_id: 2

formation:
  members: 1
  slot_size: 20
  footprint: {width: 20, depth: 20}

domain: ground
max_life: 1622
collision_radius: 8
move_speed: 8
rotate_speed: 70
has_body: true
independent_aim: false

attack:
  base_damage: 2329
  min_range: 0
  range: 140
  attack_half_angle: 20
  targets: {ground: true, air: true}
  lock_target: true
  quick_switch_target: true
  timing:
    interval: 3.1
    interval_offset: 0.6
    initial_cooldown: 0
    prepare: 0.5
    attack_point: 0.1
    backswing: 0
    cooling: 0.2
  splash_radius: 0
  weapons:
    mode: normal
    count: 1
    per_skill: 1
  melee: false
  path:
    type: projectile
    count: 1
    release_interval: 0.2
    speed: 500
    target_offset_radius: 0
    evenly_allocate_targets: false
    extra_search_range: 0
    pre_flight_height: 0
    simulated_motion: false
    interceptible: false
    max_life: 0
```

## 保守的共同字段

| 字段 | 含义 |
| --- | --- |
| `type_name`、`unit_type_id` | layout 名称与 MCFR 稳定单位类型 ID。 |
| `formation.members` | 一个 Formation 原生创建的成员数量。 |
| `formation.slot_size` | 推导成员网格行列数所用的原生成员槽位尺寸。 |
| `formation.footprint` | 生成成员位置所用的原生卡牌基础宽度与深度。 |
| `domain` | 单位属于 `ground` 或 `air`。 |
| `max_life` | 每成员一级、无修正最大生命。 |
| `collision_radius` | 运动、射程与命中判定使用的原生成员半径。 |
| `move_speed`、`rotate_speed` | 成员移动速度和主体旋转速度。 |
| `has_body` | 原生 `MechData` 是否具有独立 mech body。 |
| `independent_aim` | 主武器是否接收独立于 mech body 方向生成的瞄准方向。当前 P0 目录中仅具有 mech body 的单位填写；省略表示不适用，不表示 `false`。 |
| `base_damage` | 路径专用攻击次数倍率生效前的一级、无修正 mech 基础伤害。 |
| `min_range`、`range`、`attack_half_angle` | 原生交战距离边界和主技能有效半角。 |
| `targets`、`lock_target` | 原生对地/对空目标域与目标锁定行为。 |
| `quick_switch_target` | 周期搜索发现不同且仍存活的目标时，是否允许不先退出当前攻击状态而直接替换；死亡或失效目标的替换走另一条原生分支。 |
| `timing.*` | 原生攻击间隔、随机偏移、初始冷却、准备、前摇、后摇与退出冷却阶段。 |
| `splash_radius` | 原生基础作用半径；零表示没有范围效果。 |
| `melee` | 主技能的 `SkillData.isMeleeAttack`。战斗里的近战分支读它，近战和远程两个目标类别也由它回答。 |

Formation 行列数由 `members`、`slot_size` 和 `footprint` 推导；jitter 常量、成员 RNG、
更新顺序和身份分配属于内核机制，不在单位 YAML 中重复保存。Adapter 会按 team、世界 `z`、世界 `x` 排序后分配
初始单位身份；原生成员创建顺序不是 MCFR 身份顺序。

`independent_aim` 对具有 mech body 的单位仍有明确意义。无 body 单位不存在可用于比较的
body 方向，因此必须省略该字段。当前 P0 数据中所有适用值均为 `false`，但这不构成未来
单位或其它武器模式的默认值。

## 武器拓扑与攻击路径

武器调度与效果路径是正交关系：

- `weapons.mode` 为 `normal`、`group` 或 `standalone`；
- `count` 是完成配置后的原生武器数量，`per_skill` 决定多少武器组成一个 `FightSkill`；
- `fusillade` 和 `allow_same_target` 仅在 `group` 时必填，不允许用默认值代替缺失原生数据。
- 仅当原生技能提供独立于主体的武器转速时填写 `rotation_speed`；Wraith 的该值为
  `90`，而主体层 `rotate_speed` 为 `120`。

`path` 是四种变体的标签联合：

- `projectile`：投射物数量、释放间隔、运动、目标偏移、可拦截标记与投射物生命；
- `direct`：直接效果；
- `laser`：按攻击次数使用的伤害倍率；
- `control_beam`：预热攻击次数和预热伤害倍率。

单发/多发由 `path.count` 决定，近战/非近战由每条路径都有的 `melee` 决定，Group/Fusillade
保留在武器拓扑层。配置 schema 不引入 `grouped_projectile` 一类组合类型。

## 单位与数值边界

配置值统一使用 m、s、deg，但条目不带单位后缀。空间量必须量化到 1 mm，时间量必须
量化到 0.0005 s，角度量必须量化到 0.001 deg；激光/控制束无量纲倍率必须为有限正数。
原生 Q32 表示和运算顺序仍由内核维护。

弧光写作 `interval: 0.9`。早期的 `1799` 是原生 Q32 值投影到每秒 2000 内部时间单位后
得到的有损结果，不是设计层攻击间隔。各 timing 阶段必须分开保存，因为原生状态机会
分别量化并消费它们。

## 模拟读取的配置

配置随二进制分发：`config.yaml`、`towers.yaml` 和 `units/` 下每个单位一份
文件都编译在内。模拟不从磁盘读取配置，因此一份二进制只模拟它自带的那个 build，
在哪里运行都一样。schema 可以加载当前
P0 的全部单位路径，但 Simulator 在完成对应 Formation 生成和原生攻击路径前必须拒绝
模拟该单位。当前可执行内核只接受已有原生逐 tick 基线覆盖的 Marksman 与 Arclight
内置行为配置；同一行为配置可以改名，但未闭合单位或任一行为字段变化都必须 fail-stop。
配置能够加载不代表该单位已经取得原生模拟一致性。
