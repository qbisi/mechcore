"""Read the effects the game flies for its weapons, which the player's shots follow.

    uv run --with UnityPy --with pillow python3 scripts/player/shot-effects.py [effect...]

A weapon's projectile in the game is an effect prefab, mostly particle
systems whose textures are grey and whose colours, sizes and stretch are the
particle systems' own. For each effect named (every one `SHOTS` and `BEAMS`
in `crates/player/web/player.js` cite when none is), this prints each particle
system: its start colour and alpha, start size, lifetime and speed, its
renderer's mode (0 billboard, 1 stretched, 4 mesh, 5 none, the trail its own)
and length scale, and its trail's colour when it has one; and the size of the
effect's mesh, where it flies one (a missile, a shell). Nothing it reads is
kept: the numbers it prints are what the page cites.
"""

import argparse
import importlib.util
from pathlib import Path

import UnityPy

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("model_views", HERE / "model-views.py")
mv = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mv)

EFFECTS = [
    "efx_2016_mech_huguang_projectile", "efx_2016_mech_huguang_behit",
  "efx_2011_mech_changgong_bullet", "efx_2012_mech_jianya_bullet", "Oculus_LaserProjectile",
    "efx_2024_mech_tarantula_bullet", "efx_2015_mech_fenghuang_bullet_New", "efx_2033_Centurion_bullet",
    "FX_Mech_Yema_AutoCannonFire", "efx_2004_mechtiechui_bullet", "efx_2001_mech_baolei_03_bullet",
    "Tiger_MainCannonBullet", "WarFactory_bullet", "FX_LargeCannonFire", "Mech_11_Missile",
    "Mech_12_Projectile", "Hound_FlameMissile", "PhantomRayMissile", "ProphetAntiAirMissile",
    "KrakenMissile", "FireBadger_Attack", "efx_2020_FireBadger_LongAttack", "efx_2008_mech_huoshen_cast",
    "Mech19_Projectile_Normal", "UfoBullet", "FX_TeslaCrab_Attack", "FX_ThunderboltAttack",
    "efx_2013_mech_rongdian_rays_World_Core", "efx_2014_mech_gangqiu_RayEffect", "ControllBeam",
    "Kraken_Laser",
]


def colour(gradient):
    c = gradient.get("maxColor")
    if not c:
        return "?"
    rgb = "".join(f"{int(min(1, c[k]) * 255):02x}" for k in "rgb")
    return f"#{rgb}/{c['a']:.2f}"


def scalar(curve):
    return round(curve["scalar"], 2) if "scalar" in curve else "?"


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("effects", nargs="*", default=EFFECTS)
    parser.add_argument("--data", type=Path, default=mv.DATA, help="the game's Data directory")
    arguments = parser.parse_args()
    env = UnityPy.load(str(arguments.data / "sharedassets0.assets"))
    for name in arguments.effects:
        root = mv.find_root(env, name)
        if root is None:
            print(f"{name}: not found")
            continue
        triangles = mv.assemble(root, {})
        if triangles:
            span = [max(p[i] for t in triangles for p in t[:3]) - min(p[i] for t in triangles for p in t[:3]) for i in range(3)]
            print(f"{name}: mesh {span[0]:.2f} wide, {span[1]:.2f} high, {span[2]:.2f} m long")
        else:
            print(f"{name}:")

        def walk(transform):
            game_object = transform.m_GameObject.deref().read()
            found = dict(mv.components(game_object))
            if "ParticleSystem" in found:
                system = found["ParticleSystem"].read_typetree()
                initial = system.get("InitialModule", {})
                renderer = found["ParticleSystemRenderer"].read_typetree() if "ParticleSystemRenderer" in found else {}
                trail = system.get("TrailModule", {})
                print(
                    f"  {game_object.m_Name[:20]:20} colour {colour(initial.get('startColor', {}))}"
                    f" size {scalar(initial.get('startSize', {}))} life {scalar(initial.get('startLifetime', {}))}"
                    f" speed {scalar(initial.get('startSpeed', {}))} mode {renderer.get('m_RenderMode')}"
                    f" length {renderer.get('m_LengthScale')}"
                    f" trail {colour(trail.get('colorOverLifetime', {})) if trail.get('enabled') else '-'}"
                )
            for child in transform.m_Children:
                walk(child.deref().read())

        walk(root)


if __name__ == "__main__":
    main()
