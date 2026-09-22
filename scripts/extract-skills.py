#!/usr/bin/env python3
"""Read skill rows out of the build's `MechSkillGroupData` without a type tree.

    work/tools/asset-venv/bin/python scripts/extract-skills.py 3003001 3002001
    work/tools/asset-venv/bin/python scripts/extract-skills.py --all

The skills a unit or a construction fires are `SkillData` and
`ProjectileSkillData` rows of the `MechSkillGroupData` object at path 173 of
`level0`. An IL2CPP build ships no type tree for them, and the dummy
assemblies a generator would need are not kept, so this reads the object's
raw bytes in the order `GRCore`'s classes declare their serialized fields:
`GRObject` (id, name), `ConfigData` (isTestData), `SkillData`, then
`ProjectileSkillData`. The order is the one in
`mechcore-decomp/<build>/cpp2il/DiffableCs/GRCore/GameRiver/`, and a field
added to either class in a later build moves everything after it, so a row
that does not reproduce a known unit's numbers means the layout moved.

The check is built in: the Marksman's row (2001) has to say range 140,
interval 3.1 with a random 0.6, prepare 0.5, attack point 0.1, cooling 0.2
and bullet speed 500, which `config/units/marksman.yaml` carries from the
earlier type-tree extraction; the script refuses to print anything when it
does not.

Prints one JSON object per requested row. Every FPoint is given in units,
already divided by 2^32; `damage` is empty for every row, because a unit's
and a construction's damage sit on their own rows in `ConfigDataContainer`.
"""

import json
import struct
import sys
from pathlib import Path

import UnityPy

LEVEL0 = Path(
    "/Users/qbisi/Library/Application Support/Steam/steamapps/common/Mechabellum/"
    "Mechabellum.app/Contents/Resources/Data/level0"
)
PATH_ID = 173
ONE = 1 << 32
MARKSMAN = 2001


class Reader:
    def __init__(self, data):
        self.data = data
        self.at = 0

    def align(self):
        self.at = (self.at + 3) & ~3

    def i32(self):
        value = struct.unpack_from("<i", self.data, self.at)[0]
        self.at += 4
        return value

    def i64(self):
        value = struct.unpack_from("<q", self.data, self.at)[0]
        self.at += 8
        return value

    def fpoint(self):
        return self.i64() / ONE

    def flag(self):
        value = self.data[self.at] != 0
        self.at += 1
        self.align()
        return value

    def string(self):
        length = self.i32()
        value = self.data[self.at : self.at + length].decode("utf-8", "replace")
        self.at += length
        self.align()
        return value

    def ints(self):
        return [self.i32() for _ in range(self.i32())]


def weapon(r):
    return {
        "index": r.i32(),
        "defaultAngle": r.i32(),
        "rotateAngleLeft": r.i32(),
        "rotateAngleRight": r.i32(),
        "skillID": r.i32(),
    }


def skill(r):
    d = {"id": r.i32(), "name": r.string(), "isTestData": r.flag()}
    d["runtimeSkillID"] = r.i32()
    d["weapons"] = [weapon(r) for _ in range(r.i32())]
    d["weaponMode"] = r.i32()
    d["weaponMountNode"] = r.i32()
    d["weaponCountPerSkill"] = r.i32()
    d["useDefaultRotationSearchTarget"] = r.flag()
    d["damage"] = r.ints()
    for name in ("damageRate", "minAttackRange", "attackRange", "canAttackAngle", "attackDuration", "attackDurationRandomValue"):
        d[name] = r.fpoint()
    d["useSelfSplash"] = r.flag()
    d["splashRange"] = r.fpoint()
    d["isDiffusion"] = r.flag()
    d["diffusionInteval"] = r.fpoint()
    d["diffusionSpeed"] = r.fpoint()
    d["canAttackGround"] = r.flag()
    d["canAttackAir"] = r.flag()
    d["isLockTarget"] = r.flag()
    for name in ("initialCoolDownTime", "prepareTime", "coolingTime", "attackPoint", "attackBackswing"):
        d[name] = r.fpoint()
    d["canCrossAdvancedShield"] = r.flag()
    d["extraShieldAttackRange"] = r.fpoint()
    d["enableQuickSwitchTarget"] = r.flag()
    d["extraWeaponRotateSpeed"] = r.fpoint()
    d["isPreemptive"] = r.flag()
    d["isPreemptivePermanent"] = r.flag()
    d["permanentPreemptiveActiveConditionType"] = r.i32()
    d["permanentPreemptiveActiveConditionParamFloat"] = r.fpoint()
    d["permanentPreemptiveActiveBuffID"] = r.i32()
    d["isMeleeAttack"] = r.flag()
    d["isFusillade"] = r.flag()
    d["canAttackSameTarget"] = r.flag()
    d["isLoadingType"] = r.flag()
    d["loadingCapacity"] = r.i32()
    d["reloadingTime"] = r.fpoint()
    d["ignoreEquipmentEffect"] = r.flag()
    return d


def projectile(r):
    d = skill(r)
    d["kind"] = "projectile"
    d["projectileCount"] = r.i32()
    d["projectileDuration"] = r.fpoint()
    d["randomTargetRange"] = r.fpoint()
    d["isEvenlyAllocated"] = r.flag()
    d["extraSearchRange"] = r.fpoint()
    d["bulletSpeed"] = r.fpoint()
    d["preFlyHeight"] = r.fpoint()
    d["isSimulateMode"] = r.flag()
    d["damageType"] = r.i32()
    d["attackStrength"] = r.i32()
    d["canBeIntercept"] = r.flag()
    d["maxLife"] = r.ints()
    return d


def rows():
    environment = UnityPy.load(str(LEVEL0))
    raw = next(o for o in environment.objects if o.path_id == PATH_ID).get_raw_data()
    r = Reader(raw)
    r.i32(), r.i64(), r.flag(), r.i32(), r.i64(), r.string()  # m_GameObject, m_Enabled, m_Script, m_Name
    folder = r.string()
    if folder != "mechSkillDatas":
        sys.exit(f"path {PATH_ID} is not MechSkillGroupData: folder {folder!r}")
    plain = [dict(skill(r), kind="skill") for _ in range(r.i32())]
    shots = [projectile(r) for _ in range(r.i32())]
    return plain + shots


def close(a, b):
    return abs(a - b) < 1e-6


def main(argv):
    if len(argv) < 2:
        print(__doc__.strip(), file=sys.stderr)
        return 2
    every = rows()
    marksman = next((row for row in every if row["id"] == MARKSMAN), None)
    if marksman is None or not (
        close(marksman["attackRange"], 140)
        and close(marksman["attackDuration"], 3.1)
        and close(marksman["attackDurationRandomValue"], 0.6)
        and close(marksman["prepareTime"], 0.5)
        and close(marksman["attackPoint"], 0.1)
        and close(marksman["coolingTime"], 0.2)
        and close(marksman["bulletSpeed"], 500)
    ):
        sys.exit("the Marksman's row does not reproduce config/units/marksman.yaml; the field layout moved")
    wanted = None if argv[1] == "--all" else {int(a) for a in argv[1:]}
    for row in every:
        if wanted is None or row["id"] in wanted:
            print(json.dumps(row, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
