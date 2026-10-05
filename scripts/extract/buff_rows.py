"""The buff a buff source adds, as `config/` writes it for every kind of source.

A `BuffEquipment` and a `BuffTech` are both an `IEffectBuffDataSource`, which
`BuffEffectProvider` reads the same way whatever owns it: what triggers the
buff (`buffTechTrigger`), whom it reaches (`effectTargetTypes`), how likely,
and the `buffDatas` row it adds. `extract-equipment-effects.py` and
`extract-technology-effects.py` both write them through `buff_lines`.
"""

# The rest of a source's fields, any of which set is named in `buff_special`.
# The effect's name and whether it follows are what the client shows.
SOURCE_OTHER = (
    "energyShieldDamageMultiplier", "min", "max", "intervalTime", "delayTime", "targetFlyType",
    "targetDamageDistanceType", "buffTargetUpdateModel", "triggerRangeItemBuffId",
    "triggerRangeItemType", "triggerLifeTime", "triggerRangeItemRange", "triggerRoundDuration",
    "isDistanceCalculateTargetRadius", "isDistanceCalculateSelfRadius",
)
# The fields of the buffDatas row a source names that the simulator reads;
# any other set is named in the buff's `special`.
BUFF_READ = {
    "buffDivide": "divide", "isAdditiveMode": "additive", "debuff": "debuff",
    "invincible": "invincible", "disableTechnology": "disable_technology",
    "amplifyDamageRate": "amplify_damage_rate", "damageChangeRate": "damage_rate",
    "maxLifeChangeRate": "max_life_rate", "stepTime": "step_time",
    "isAdditiveEffect": "additive_effect",
    "buffEffectAdditiveCondition": "additive_condition",
    "maxAdditiveStack": "max_additive_stack",
    "isClearSelfBuffWhenDisableTech": "clear_when_technologies_disabled",
}
# The ones that are Q32.32 raw values, read beside them as a decimal.
BUFF_RATES = {"amplifyDamageRate", "damageChangeRate", "maxLifeChangeRate", "stepTime"}
BUFF_DESCRIPTIVE = {"id", "name", "isTestData", "duration", "effectType"}


def raw(value):
    return value["m_rawValue"] if isinstance(value, dict) else value


def reading(value):
    text = f"{value / (1 << 32):+.4f}".rstrip("0").rstrip(".")
    return f"  # {text}"


def set_fields(row, fields):
    return [field for field in fields if raw(row.get(field)) not in (0, False, None, "", [], {})]


def buff_lines(row, buffs, indent="    "):
    """A buff source's trigger and the buffDatas row it adds."""
    lines = [
        f"{indent}buff_trigger: {row['buffTechTrigger']}",
        f"{indent}buff_targets: [{', '.join(map(str, row['effectTargetTypes']))}]",
        f"{indent}probability: {raw(row['probability'])}{reading(raw(row['probability']))}",
    ]
    special = set_fields(row, SOURCE_OTHER)
    if special:
        lines.append(f"{indent}buff_special: [{', '.join(special)}]")
    buff = buffs[row["buffID"]]
    lines += [
        f"{indent}buff:",
        f"{indent}  id: {buff['id']}",
        f"{indent}  name: {buff['name']}",
        f"{indent}  duration: {raw(buff['duration'])}{reading(raw(buff['duration']))}",
    ]
    for field, name in BUFF_READ.items():
        value = raw(buff[field])
        if isinstance(value, bool):
            lines.append(f"{indent}  {name}: {str(value).lower()}")
        elif field in BUFF_RATES:
            lines.append(f"{indent}  {name}: {value}{reading(value) if value else ''}")
        else:
            lines.append(f"{indent}  {name}: {value}")
    special = set_fields(buff, [field for field in buff if field not in BUFF_READ and field not in BUFF_DESCRIPTIVE])
    if special:
        lines.append(f"{indent}  special: [{', '.join(special)}]")
    return lines
