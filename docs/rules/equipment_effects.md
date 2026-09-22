# Equipment corrections

This rule is pinned to build **1.11.1.3.2259**. Ordinary `EquipmentData`
corrects the unit wearing it through the same common modifier mechanism as
an officer. It does not replace the unit's base description. The scope is
first deployment at level one; lifetime, locking and other equipment
subclasses are separate mechanisms.

## Where an equipment writes

`Equipment.AddData(ISkillOwner)` calls `MechDataModifer.TryAddCommonData`.
The overload taking a `FightSkill` calls `SkillDataModifier.AddData`.
`EquipmentData` implements `ICommonMechDataChangeDataSource`,
`ISkillDataChangeDataSource` and `IUnitDataChangeDataSource`, so the field
kinds and their aggregation are those of [officer corrections](officer_effects.md).
The item is worn by a formation; another formation of the same unit type
on the same side does not inherit its corrections.

Life rate belongs to the unit's modifier set. Damage rate and attack-range
value belong to the skill's modifier set. Positive equipment and officer
rates on one field sum before multiplying; an equipment is not a second
multiplicative bracket. Q32.32 rates are kept as raw integers, with the
final integer result truncated toward zero.

For a level-one Marksman, Heavy Armor's raw `3221225472` life rate gives
2838 life from 1622. With Advanced Defensive Tactics (`1288490188`), their
sum `4509715660` gives 3325. Improved Firepower Control System's raw
`2791728742` damage rate gives 3842 from 2329; with Advanced Offensive
Tactics their sum `4080218930` gives 4541. Laser Sights adds
`85899345920` raw range units, twenty metres, to the main skill's 140.
The native unit and skill fields confirm the channel as well as the
resulting number. An increased range changes attack eligibility when a
target lies outside the original range; it need not change a fight whose
targets are already in reach.

## The extracted table

[`config/equipment_effects.yaml`](../../config/equipment_effects.yaml)
contains all eleven ordinary rows in `EquipmentGroupData.equipmentDatas`,
path ID 188 in `level0`. The extractor
[`extract-equipment-effects.py`](../../scripts/extract-equipment-effects.py)
reads the serialized fields in `GRObject`, `ConfigData`, `ItemData`,
`ReinforceItemData` and `EquipmentData` order. It preserves raw Q32.32
integers, both skill-selection flags, targeting lists, lifetime fields,
and ledger fields. Speed changes are integer metres per second. Ledger
values such as supply and upgrade price do not change a combat number.

| ID | Item | Nonzero combat fields |
| --- | --- | --- |
| 13030001 | Laser Sights | range +20 m; main skill only; mechType 4 |
| 13030002 | Heavy Armor | life +0.75 |
| 13030003 | Improved Firepower Control System | damage +0.65 |
| 13030004 | Enhancement Module | life +0.25, damage +0.25 |
| 13030005 | Haste Module | damage +0.35, speed +5 m/s |
| 13030006 | Super Heavy Armor | life +1.5 |
| 13030007 | Amplifying Core | life +0.5, damage +0.5 |
| 13030008 | 高级火控系统 | damage +2 |
| 13030009 | Small Amplifying Core | life +0.22, damage +0.22 |
| 13030010 | Dominion Core | life +1, damage +0.5; importantUnit |
| 13030011 | 快速装填机 | interval -0.5; roundDuration 1 |

These are resource values, not fitted coefficients. The extractor checks
IDs and count and cross-checks the armor, firepower and range fields
against the native readings before writing. Their printed decimals are
readings of raw values, not inputs to fixed-point arithmetic.

## Boundaries

The recorded Laser Sights target is the Marksman. The table calls its
category `mechType` 4 and carries no unit list; this rule does not enumerate
other ranged units or establish extra-skill eligibility. Other ordinary
rows use category 0, all units, with main and extra skill effects enabled.

`roundDuration`, `durability`, `IsLocked(CardLevel)`, permanent effects and
Dominion Core's `importantUnit` behavior are not established here. The
other named equipment classes own shields, buffs, recovery, deployment
and production rather than ordinary corrections. They are outside this
rule, as are constructions, towers and reactor-supply accounting.

Reopen this scope when the build changes, a first-deployment recording
contradicts these shared channels, or a question supplies native evidence
for a currently unverified lifetime, targeting or extra-skill branch.
