# 广度与平台

模拟器从单位扩到模块和效果表，然后是对战平台本身。单位基本支持之后，这里的顺序按
fight-coverage 挡得最多的理由重排。

## 栈

1. **模块广度。** `Modifier` 的等级与装备、`CommanderSkillSystem`、`InterceptSystem`、
   `BuildingSystem`。
2. **效果表。** 装备、能量塔技能。
3. **语料层的稀疏验收**，与反应堆伤害、经验两条规则。
4. **平台。** `arena`、`shell --json` 和 `game` 后端。
