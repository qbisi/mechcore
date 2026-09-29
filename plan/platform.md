# 广度与平台

模拟器从单位扩到模块和效果表，然后是对战平台本身。单位基本支持之后，这里的顺序按
fight-coverage 挡得最多的理由重排。

## 栈

1. **模块广度**，按 fight-coverage 里单独补上一项能多放行的回合数排：炮塔的技能（军官和科技
   够不够得着它）、`CommanderSkillSystem`（战场技能）、`InterceptSystem`（contraption）、
   `BuildingSystem`（能量塔技能）、`SuperDeploymentSystem`（空投单位）、`Modifier` 的装备。
   挡住回合最多的理由不一定先做：一个回合常被几项同时挡住，只补其中一项放行不了它。
2. **效果表。** 装备、能量塔技能。
3. **语料层的稀疏验收**，与经验规则。
4. **平台。** `arena`、`shell --json` 和 `game` 后端。
