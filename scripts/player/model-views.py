"""Render the game's models from above, the views the player's sprites follow.

    uv run --with UnityPy --with pillow python3 scripts/player/model-views.py [sprite...]

Each sprite in `crates/player/web/sprites.js` is drawn after one model of the
installed game, seen from straight above. A model is not one mesh: a unit's
prefab is a hierarchy of transforms whose renderers hold its parts, and a
skinned body leaves its weapons and limbs wherever its bind pose puts them
until an animation poses the bones. So this assembles each prefab the way the
game does: it composes every transform from the root, poses the bones with one
sample of the unit's attack clip from its own animator controller, skins each
vertex to its bones, keeps only the renderers a LOD group shows first, and
colours each triangle from its material's albedo texture.

It writes `<sprite>-top.png` and `<sprite>-side.png` for each, and
`models.png` with all of them, into `work/player/models/`. Nothing it writes
is tracked: the views are the game's art, and the repository keeps the script
that makes them again. `--data` names the game's `Data` directory when it is
not the default Steam install.
"""

import argparse
import math
import struct
import sys
import zlib
from pathlib import Path

import UnityPy
from PIL import Image, ImageDraw
from UnityPy.helpers.MeshHelper import MeshHandler

DATA = Path.home() / (
    "Library/Application Support/Steam/steamapps/common/Mechabellum/"
    "Mechabellum.app/Contents/Resources/Data"
)
OUT = Path(__file__).resolve().parents[2] / "work/player/models"

# sprite: (asset file, prefab, attack clip or None, seconds into it). A tower
# is a node of the battle scene rather than a prefab of its own.
MODELS = {
    "marksman": ("sharedassets0.assets", "Mech_Default_2_1", "Longbow_FiringA", 0.3),
    "arclight": ("sharedassets0.assets", "Mech_Default_15_1", "attack", 0.2),
    "rhino": ("sharedassets0.assets", "Mech_Default_5_1", "Rhinoceros_FiringAL", 0.2),
    "crawler": ("sharedassets0.assets", "Mech_Default_10_1", "attack", 0.2),
    "sledgehammer": ("sharedassets0.assets", "Mech_Default_13_1", None, 0.0),
    "wasp": ("sharedassets0.assets", "Mech_Default_6_1", "Attack", 0.2),
    "defensive_wall": ("sharedassets0.assets", "Construction_Default_1", None, 0.0),
    "anti_armor_turret": ("sharedassets0.assets", "Construction_Default_2", None, 0.0),
    "rapid_fire_turret": ("sharedassets0.assets", "Construction_Default_3", None, 0.0),
    "energy_tower": ("level1", "Energy_Tower_Blue_Left", None, 0.0),
    "research_center": ("level1", "Research_Center_Blue_Right", None, 0.0),
}

IDENTITY = [[1, 0, 0, 0], [0, 1, 0, 0], [0, 0, 1, 0], [0, 0, 0, 1]]


def multiply(a, b):
    return [[sum(a[i][k] * b[k][j] for k in range(4)) for j in range(4)] for i in range(4)]


def compose(position, rotation, scale):
    x, y, z, w = rotation
    r = [
        [1 - 2 * (y * y + z * z), 2 * (x * y - z * w), 2 * (x * z + y * w)],
        [2 * (x * y + z * w), 1 - 2 * (x * x + z * z), 2 * (y * z - x * w)],
        [2 * (x * z - y * w), 2 * (y * z + x * w), 1 - 2 * (x * x + y * y)],
    ]
    return [
        [r[i][0] * scale[0], r[i][1] * scale[1], r[i][2] * scale[2], position[i]]
        for i in range(3)
    ] + [[0, 0, 0, 1]]


def apply(m, v):
    return tuple(m[i][0] * v[0] + m[i][1] * v[1] + m[i][2] * v[2] + m[i][3] for i in range(3))


def components(game_object):
    found = []
    for entry in game_object.m_Component:
        pointer = entry.component if hasattr(entry, "component") else entry
        try:
            target = pointer.deref()
        except Exception:
            continue
        found.append((target.type.name, target))
    return found


# ------------------------------------------------------------- animation
def sample_clip(clip, time):
    """Every curve of a Mecanim clip at `time`, by curve index: the streamed
    curves first, then the dense, then the constant, as the bindings count
    them."""
    data = clip.m_MuscleClip.m_Clip.data
    values = {}
    streamed = data.m_StreamedClip
    raw = struct.pack(f"<{len(streamed.data)}I", *streamed.data)
    keys = {}
    offset = 0
    while offset + 8 <= len(raw):
        key_time, count = struct.unpack_from("<fI", raw, offset)
        offset += 8
        for _ in range(count):
            index, *coefficients = struct.unpack_from("<I4f", raw, offset)
            offset += 20
            keys.setdefault(index, []).append((key_time, coefficients))
    for index, frames in keys.items():
        at, (a, b, c, d) = frames[0]
        for key_time, coefficients in frames:
            if key_time <= time:
                at, (a, b, c, d) = key_time, coefficients
        dt = time - at if math.isfinite(at) and time >= at else 0.0
        values[index] = ((a * dt + b) * dt + c) * dt + d
    dense = data.m_DenseClip
    if dense.m_CurveCount:
        frame = 0
        if dense.m_SampleRate and time > dense.m_BeginTime:
            frame = min(dense.m_FrameCount - 1, int((time - dense.m_BeginTime) * dense.m_SampleRate))
        for i in range(dense.m_CurveCount):
            values[streamed.curveCount + i] = dense.m_SampleArray[frame * dense.m_CurveCount + i]
    if data.m_ConstantClip is not None:
        base = streamed.curveCount + dense.m_CurveCount
        for i, value in enumerate(data.m_ConstantClip.data):
            values[base + i] = value
    return values


def transform_pose(clip, time):
    """{path hash: {attribute: values}} for the clip's transform curves:
    1 position, 2 rotation, 3 scale."""
    values = sample_clip(clip, time)
    pose = {}
    index = 0
    for binding in clip.m_ClipBindingConstant.genericBindings:
        transform = (getattr(binding, "typeID", None) or getattr(binding, "classID", None)) == 4
        width = {1: 3, 2: 4, 3: 3, 4: 3}.get(binding.attribute, 1) if transform else 1
        if transform and binding.attribute in (1, 2, 3):
            sample = tuple(values.get(index + k) for k in range(width))
            if None not in sample:
                pose.setdefault(binding.path, {})[binding.attribute] = sample
        index += width
    return pose


# -------------------------------------------------------------- assembly
def find_root(env, name):
    best = None
    for obj in env.objects:
        if obj.type.name != "Transform":
            continue
        transform = obj.read()
        try:
            if transform.m_GameObject.deref().read().m_Name != name:
                continue
        except Exception:
            continue
        # a prefab's root, or the fullest node of that name in a scene
        if best is None or len(transform.m_Children) > len(best.m_Children):
            best = transform
    return best


def find_clip(root, name):
    stack = [root]
    while stack:
        transform = stack.pop()
        for kind, component in components(transform.m_GameObject.deref().read()):
            if kind != "Animator":
                continue
            controller = component.read().m_Controller
            if not controller.path_id:
                continue
            for pointer in controller.deref().read().m_AnimationClips:
                if pointer.path_id and pointer.deref().peek_name() == name:
                    return transform, pointer.deref().read()
        stack.extend(child.deref().read() for child in transform.m_Children)
    return None, None


def posed_transforms(animator, clip, time):
    """Transform path id -> its animated (position, rotation, scale)."""
    curves = transform_pose(clip, time)
    posed = {}

    def walk(transform, path):
        if path is not None:
            hit = curves.get(zlib.crc32(path.encode()) & 0xFFFFFFFF)
            if hit:
                posed[transform.object_reader.path_id] = hit
        for pointer in transform.m_Children:
            child = pointer.deref().read()
            name = child.m_GameObject.deref().read().m_Name
            walk(child, name if path is None else f"{path}/{name}")

    walk(animator, None)
    return posed


def local_matrix(transform, posed):
    p, q, s = transform.m_LocalPosition, transform.m_LocalRotation, transform.m_LocalScale
    position, rotation, scale = (p.x, p.y, p.z), (q.x, q.y, q.z, q.w), (s.x, s.y, s.z)
    override = posed.get(transform.object_reader.path_id, {})
    if 1 in override:
        position = override[1]
    if 2 in override:
        length = math.sqrt(sum(c * c for c in override[2])) or 1
        rotation = tuple(c / length for c in override[2])
    if 3 in override:
        scale = override[3]
    return compose(position, rotation, scale)


def albedo(material_pointer, cache):
    if material_pointer is None or not material_pointer.path_id:
        return None, (150, 150, 150)
    if material_pointer.path_id in cache:
        return cache[material_pointer.path_id]
    image, colour = None, (150, 150, 150)
    try:
        material = material_pointer.deref().read()
        for name, texture in material.m_SavedProperties.m_TexEnvs:
            if name in ("_MainTex", "_BaseMap", "_BaseColorMap", "_Albedo") and texture.m_Texture.path_id:
                image = texture.m_Texture.deref().read().image.convert("RGB").resize((256, 256))
                break
        for name, c in material.m_SavedProperties.m_Colors:
            if name in ("_Color", "_BaseColor"):
                colour = (int(c.r * 255), int(c.g * 255), int(c.b * 255))
    except Exception:
        pass
    cache[material_pointer.path_id] = (image, colour)
    return image, colour


def assemble(root, posed):
    """Every triangle the prefab shows at its first LOD, posed, with a colour."""
    world, nodes, hidden_lods = {}, [], set()

    def walk(transform, parent, active):
        game_object = transform.m_GameObject.deref().read()
        matrix = multiply(parent, local_matrix(transform, posed))
        active = active and bool(game_object.m_IsActive)
        world[transform.object_reader.path_id] = matrix
        found = components(game_object)
        for kind, component in found:
            if kind == "LODGroup":
                for level, lod in enumerate(component.read().m_LODs):
                    if level:
                        hidden_lods.update(r.renderer.path_id for r in lod.renderers if r.renderer.path_id)
        nodes.append((transform.object_reader.path_id, found, active))
        for child in transform.m_Children:
            walk(child.deref().read(), matrix, active)

    walk(root, IDENTITY, True)
    triangles, cache = [], {}
    for node, found, active in nodes:
        if not active:
            continue
        kinds = dict(found)
        if "SkinnedMeshRenderer" in kinds:
            renderer_object = kinds["SkinnedMeshRenderer"]
            renderer = renderer_object.read()
            mesh_pointer, bones = renderer.m_Mesh, renderer.m_Bones
        elif "MeshFilter" in kinds and "MeshRenderer" in kinds:
            renderer_object = kinds["MeshRenderer"]
            renderer = renderer_object.read()
            mesh_pointer, bones = kinds["MeshFilter"].read().m_Mesh, None
        else:
            continue
        if not renderer.m_Enabled or renderer_object.path_id in hidden_lods or not mesh_pointer.path_id:
            continue
        try:
            mesh = mesh_pointer.deref().read()
        except Exception:
            continue  # a built-in mesh of the engine's own resources
        handler = MeshHandler(mesh)
        handler.process()
        vertices, uv = handler.m_Vertices, handler.m_UV0
        if not vertices:
            continue
        if bones and handler.m_BoneIndices and mesh.m_BindPose:
            skin = []
            for i, bone in enumerate(bones):
                b = mesh.m_BindPose[i]
                bind = [[b.e00, b.e01, b.e02, b.e03], [b.e10, b.e11, b.e12, b.e13],
                        [b.e20, b.e21, b.e22, b.e23], [b.e30, b.e31, b.e32, b.e33]]
                skin.append(multiply(world.get(bone.path_id, world[node]), bind))
            points = []
            for i, vertex in enumerate(vertices):
                indices = handler.m_BoneIndices[i]
                # a mesh skinned rigidly to one bone stores no weights
                weights = handler.m_BoneWeights[i] if handler.m_BoneWeights else (1.0,) + (0.0,) * (len(indices) - 1)
                total, acc = 0.0, [0.0, 0.0, 0.0]
                for weight, bone in zip(weights, indices):
                    if weight > 0:
                        p = apply(skin[bone], vertex)
                        acc = [acc[k] + weight * p[k] for k in range(3)]
                        total += weight
                points.append(tuple(c / total for c in acc) if total else apply(world[node], vertex))
        else:
            points = [apply(world[node], vertex) for vertex in vertices]
        materials = renderer.m_Materials
        for submesh, faces in enumerate(handler.get_triangles()):
            material = materials[min(submesh, len(materials) - 1)] if materials else None
            image, colour = albedo(material, cache)
            for face in faces:
                if len(face) != 3:
                    continue
                shade = colour
                if image is not None and uv:
                    u = (sum(uv[i][0] for i in face) / 3) % 1.0
                    v = (sum(uv[i][1] for i in face) / 3) % 1.0
                    shade = image.getpixel((min(255, int(u * 256)), min(255, int((1 - v) * 256))))
                triangles.append((points[face[0]], points[face[1]], points[face[2]], shade))
    return triangles


# --------------------------------------------------------------- drawing
def render(triangles, project, depth, light, size=640):
    """Painter's algorithm: the triangles nearest the eye drawn last."""
    us = [project(p)[0] for t in triangles for p in t[:3]]
    vs = [project(p)[1] for t in triangles for p in t[:3]]
    span = max(max(us) - min(us), max(vs) - min(vs)) or 1
    scale = size * 0.92 / span
    cu, cv = (max(us) + min(us)) / 2, (max(vs) + min(vs)) / 2
    image = Image.new("RGB", (size, size), (24, 28, 34))
    draw = ImageDraw.Draw(image)
    for a, b, c, colour in sorted(triangles, key=lambda t: max(depth(p) for p in t[:3])):
        u = [b[i] - a[i] for i in range(3)]
        v = [c[i] - a[i] for i in range(3)]
        n = (u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0])
        length = math.sqrt(sum(x * x for x in n)) or 1
        lit = 0.45 + 0.75 * abs(sum(light[i] * n[i] for i in range(3))) / length
        fill = tuple(min(255, int(ch * lit * 1.25)) for ch in colour)
        draw.polygon(
            [(size / 2 + (project(p)[0] - cu) * scale, size / 2 - (project(p)[1] - cv) * scale) for p in (a, b, c)],
            fill=fill,
        )
    # one metre, for scale
    draw.line([(10, size - 10), (10 + scale, size - 10)], fill=(255, 255, 0), width=3)
    return image


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("sprites", nargs="*", help="sprites to render; all when none is named")
    parser.add_argument("--data", type=Path, default=DATA, help="the game's Data directory")
    parser.add_argument("--out", type=Path, default=OUT)
    arguments = parser.parse_args()
    wanted = arguments.sprites or list(MODELS)
    unknown = [name for name in wanted if name not in MODELS]
    if unknown:
        sys.exit(f"no model for {', '.join(unknown)}; known: {', '.join(MODELS)}")
    arguments.out.mkdir(parents=True, exist_ok=True)
    environments, views = {}, []
    for sprite in wanted:
        asset, prefab, clip_name, time = MODELS[sprite]
        if asset not in environments:
            environments[asset] = UnityPy.load(str(arguments.data / asset))
        root = find_root(environments[asset], prefab)
        if root is None:
            print(f"{sprite}: {prefab} is not in {asset}", file=sys.stderr)
            continue
        posed = {}
        if clip_name:
            animator, clip = find_clip(root, clip_name)
            if clip is None:
                print(f"{sprite}: {prefab} has no clip {clip_name}; drawing its rest pose", file=sys.stderr)
            else:
                posed = posed_transforms(animator, clip, time)
        triangles = assemble(root, posed)
        if not triangles:
            print(f"{sprite}: {prefab} shows no mesh", file=sys.stderr)
            continue
        # Unity is y-up with +z forward: from above, x right and z up the page
        top = render(triangles, lambda p: (p[0], p[2]), lambda p: p[1], (0.3, 0.9, 0.3))
        side = render(triangles, lambda p: (p[2], p[1]), lambda p: -p[0], (0.8, 0.5, 0.2))
        top.save(arguments.out / f"{sprite}-top.png")
        side.save(arguments.out / f"{sprite}-side.png")
        xs = [p[0] for t in triangles for p in t[:3]]
        zs = [p[2] for t in triangles for p in t[:3]]
        print(f"{sprite}: {len(triangles)} triangles, {max(xs) - min(xs):.1f} m wide, {max(zs) - min(zs):.1f} m long")
        views.append((sprite, top, side))
    if views:
        cell = 256
        sheet = Image.new("RGB", (cell * len(views), cell * 2 + 18), (0, 0, 0))
        draw = ImageDraw.Draw(sheet)
        for i, (sprite, top, side) in enumerate(views):
            sheet.paste(top.resize((cell, cell)), (i * cell, 18))
            sheet.paste(side.resize((cell, cell)), (i * cell, cell + 18))
            draw.text((i * cell + 4, 3), sprite, fill=(255, 255, 0))
        sheet.save(arguments.out / "models.png")
        print(arguments.out / "models.png")


if __name__ == "__main__":
    main()
