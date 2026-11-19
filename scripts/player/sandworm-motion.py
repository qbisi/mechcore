"""Export the Sandworm's 3D attack, burrow and emerge kinematics for `sprites.js`.

    uv run --with UnityPy --with pillow python3 scripts/player/sandworm-motion.py

The Sandworm (`Mech_Default_23_1`, and the Larva `Mech_Default_1001_1` at half
scale) uses the `Mech_Default_23` AnimatorController. Its body is a segmented
spine (`Body_001`..`Body_009`) rising vertically through the ground plane (`y=0`)
to `Head_001`, carrying three 120-degree articulated saw-claw arms under
`Head_002`:
  - ventral/center arm: `Hand_001` -> `Hand_002` -> `Hand_003` (tooth `Hand_028`)
  - right upper arm:    `Hand_004` -> `Hand_005` -> `Hand_006` (tooth `Hand_019`)
  - left upper arm:     `Hand_007` -> `Hand_008` -> `Hand_009` (tooth `Hand_010`)

Each clip (`UpIdle`, `Down`, `Up`, `ground_near`, `ground_far`, `air_near`,
`air_far`) is assembled from the game's `sharedassets0.assets` in sprite-3D
coordinates `(x, y = -z_fwd, h = y_up)` with front toward `-y` and height `h`
above ground. Use `--write` to generate `crates/player/web/sandworm-motion.js`
and `--check` to verify it is up to date (default prints the generated module).
With `--export-model`, it also writes `sandworm-3d-model.json` and multi-view
3D contact sheets under `work/player/sandworm/` (or `--out`).
"""

import argparse
import importlib.util
import json
import math
import struct
from pathlib import Path

import UnityPy
from PIL import Image, ImageDraw

HERE = Path(__file__).resolve().parent
ROOT = Path(__file__).resolve().parents[2]
DEFAULT_OUT = ROOT / "work/player/sandworm"
OUTPUT = ROOT / "crates/player/web/sandworm-motion.js"

spec = importlib.util.spec_from_file_location("model_views", HERE / "model-views.py")
mv = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mv)

SPINE_BONES = [f"Body_{i:03d}" for i in range(4, 10)]
ALL_SPINE_BONES = [f"Body_{i:03d}" for i in range(1, 10)]
ARMS = [
    ("Hand_001", "Hand_002", "Hand_003", "Hand_028"),
    ("Hand_004", "Hand_005", "Hand_006", "Hand_019"),
    ("Hand_007", "Hand_008", "Hand_009", "Hand_010"),
]
TRACKED_BONES = set(ALL_SPINE_BONES) | {"Head_001", "Head_002"} | {b for arm in ARMS for b in arm}

TOOTH_RETRACTED = 1.67
TOOTH_EXTENDED = 2.92

# (js_const_name, animator_state, clip_name, sample_frames)
CLIPS = [
    (
        "SANDWORM_IDLE",
        "UpIdle",
        "Sandworm_LOD_BattleIdle",
        [0, 9, 18, 27, 36, 45, 54, 63, 70],
    ),
    (
        "SANDWORM_DOWN",
        "Down",
        "Down",
        [0, 4, 8, 12, 16, 20, 24, 28, 30, 32, 36, 40, 44, 48, 52, 56, 60],
    ),
    (
        "SANDWORM_UP",
        "Up",
        "Up",
        [0, 6, 12, 18, 24, 28, 32, 36, 38, 40, 42, 44, 46, 48, 52, 56, 60, 64, 68, 75],
    ),
    (
        "SANDWORM_GROUND_NEAR",
        "Attack_Ground_BlendTree",
        "ground_near",
        [0, 5, 10, 15, 20, 24, 26, 28, 29, 30, 32, 36, 42, 46, 50, 55, 60, 65],
    ),
    (
        "SANDWORM_GROUND_FAR",
        "Attack_Ground_BlendTree",
        "ground_far",
        [0, 5, 10, 15, 20, 24, 26, 28, 29, 30, 32, 36, 42, 46, 50, 55, 60, 65],
    ),
    (
        "SANDWORM_AIR_NEAR",
        "Attack_Air_BlendTree",
        "air_near",
        [0, 6, 12, 18, 24, 26, 28, 29, 30, 32, 36, 42, 48, 54, 60, 66, 70],
    ),
    (
        "SANDWORM_AIR_FAR",
        "Attack_Air_BlendTree",
        "air_far",
        [0, 6, 12, 18, 24, 26, 28, 29, 30, 32, 36, 42, 48, 54, 60, 66, 70],
    ),
]


def to_sprite(v):
    """Convert Unity world coordinates (x, y_up, z_fwd) to sprite-3D (x, -z_fwd, y_up)."""
    return (v[0], -v[2], v[1])


def normalize(v):
    length = math.sqrt(sum(c * c for c in v)) or 1.0
    return tuple(c / length for c in v)


def bone_matrices(root, posed):
    """World 4x4 matrices for all tracked Sandworm skeleton bones."""
    found = {}

    def walk(transform, parent):
        matrix = mv.multiply(parent, mv.local_matrix(transform, posed))
        name = transform.m_GameObject.deref().read().m_Name
        if name in TRACKED_BONES:
            found[name] = matrix
        for pointer in transform.m_Children:
            walk(pointer.deref().read(), matrix)

    walk(root, mv.IDENTITY)
    return found


def hierarchy_info(root):
    """Return compact parent links and rest offsets for tracked bones."""
    parents = {}

    def walk(transform, parent_name):
        name = transform.m_GameObject.deref().read().m_Name
        next_parent = parent_name
        if name in TRACKED_BONES:
            p = transform.m_LocalPosition
            parents[name] = {
                "parent": parent_name,
                "localPosition": [round(p.x, 3), round(p.y, 3), round(p.z, 3)],
            }
            next_parent = name
        for pointer in transform.m_Children:
            walk(pointer.deref().read(), next_parent)

    walk(root, None)
    return parents


def sample_frame(root, animator, clip, frame, total_frames):
    """Evaluate a clip at integer `frame` (30 fps) and extract sprite-3D kinematics."""
    stop = clip.m_MuscleClip.m_StopTime
    rate = total_frames / stop if stop > 0 else 30.0
    time = 0.0 if frame <= 0 else min(stop, (frame + 1e-3) / rate)
    posed = mv.posed_transforms(animator, clip, time)
    mats = bone_matrices(root, posed)

    def pos_s(name):
        m = mats[name]
        return to_sprite((m[0][3], m[1][3], m[2][3]))

    def axis_z_s(name):
        m = mats[name]
        return normalize(to_sprite((m[0][2], m[1][2], m[2][2])))

    spine = {name: pos_s(name) for name in ALL_SPINE_BONES}
    head_pos = pos_s("Head_001")
    head_fwd = axis_z_s("Head_001")

    arms = []
    tooth_radii = []
    for shoulder, elbow, hub, tooth in ARMS:
        s_pos = pos_s(shoulder)
        e_pos = pos_s(elbow)
        w_pos = pos_s(hub)
        w_norm = axis_z_s(hub)
        t_pos = pos_s(tooth)
        r = math.sqrt(sum((t_pos[k] - w_pos[k]) ** 2 for k in range(3)))
        tooth_radii.append(r)
        arms.append((s_pos, e_pos, w_pos, w_norm))

    avg_tooth_r = sum(tooth_radii) / len(tooth_radii)
    tooth = max(0.0, min(1.0, (avg_tooth_r - TOOTH_RETRACTED) / (TOOTH_EXTENDED - TOOTH_RETRACTED)))
    return {
        "frame": frame,
        "time": round(time, 4),
        "s": round(frame / total_frames, 3),
        "spine": spine,
        "headPos": head_pos,
        "headFwd": head_fwd,
        "arms": arms,
        "toothRadius": round(avg_tooth_r, 3),
        "tooth": round(tooth, 2),
        "mats": mats,
    }


def pack_keyframe(sample):
    """Pack a sampled frame into the 59-number JS array `[s, ...58 numbers]`."""
    row = [sample["s"]]
    for name in SPINE_BONES:
        x, y, h = sample["spine"][name]
        row.extend([round(x, 1), round(y, 1), round(h, 1)])
    hx, hy, hh = sample["headPos"]
    fx, fy, fh = sample["headFwd"]
    row.extend([round(hx, 1), round(hy, 1), round(hh, 1), round(fx, 2), round(fy, 2), round(fh, 2)])
    for s_pos, e_pos, w_pos, w_norm in sample["arms"]:
        row.extend([round(s_pos[0], 1), round(s_pos[1], 1)])
        row.extend([round(e_pos[0], 1), round(e_pos[1], 1), round(e_pos[2], 1)])
        row.extend([round(w_pos[0], 1), round(w_pos[1], 1), round(w_pos[2], 1)])
        row.extend([round(w_norm[0], 2), round(w_norm[1], 2), round(w_norm[2], 2)])
    row.append(sample["tooth"])
    return row


def read_controller_clips(root):
    """Return ({clip_name: (clip_obj, typetree)}, {state_name: state_speed}, controller_name, attack_max_dist)."""
    animator, _ = mv.states(root)
    go = animator.m_GameObject.deref().read()
    ctrl_ptr = None
    attack_max_dist = 50.0
    for kind, comp in mv.components(go):
        if kind == "Animator":
            ctrl_ptr = comp.read().m_Controller
        elif kind == "MonoBehaviour":
            raw = comp.get_raw_data()
            if len(raw) >= 56:
                val = struct.unpack_from("<f", raw, 52)[0]
                if 10.0 <= val <= 200.0:
                    attack_max_dist = float(val)
    ctrl = ctrl_ptr.deref().read()
    names = dict(ctrl.m_TOS)
    ctrl_tt = ctrl_ptr.deref().read_typetree()
    state_speeds = {}
    for st in ctrl_tt["m_Controller"]["m_StateMachineArray"][0]["data"]["m_StateConstantArray"]:
        sd = st["data"]
        state_speeds[names.get(sd["m_NameID"], str(sd["m_NameID"]))] = sd["m_Speed"]
    clips = {}
    for cp in ctrl.m_AnimationClips:
        obj = cp.deref()
        c = obj.read()
        clips[c.m_Name] = (c, obj.read_typetree())
    return animator, clips, state_speeds, ctrl.m_Name, attack_max_dist


def render_skeleton_overlay(draw, sample, project, bounds, size):
    """Draw 3D skeleton spine, arms, and saw rings over a rendered mesh view."""
    u0, u1, v0, v1 = bounds
    span = max(u1 - u0, v1 - v0) or 1.0
    scale = size * 0.92 / span
    cu, cv = (u0 + u1) / 2.0, (v0 + v1) / 2.0

    def pt(p_sprite):
        # Convert sprite-3D (x, y_sprite, h) back to Unity (x, h, -y_sprite) for shared projection
        u, v = project((p_sprite[0], p_sprite[2], -p_sprite[1]))
        return (size / 2.0 + (u - cu) * scale, size / 2.0 - (v - cv) * scale)

    spine_pts = [pt(sample["spine"][b]) for b in ALL_SPINE_BONES]
    for i in range(len(spine_pts) - 1):
        draw.line([spine_pts[i], spine_pts[i + 1]], fill=(95, 242, 255), width=2)
    h_pt = pt(sample["headPos"])
    draw.line([spine_pts[-1], h_pt], fill=(255, 220, 80), width=2)
    for sp in spine_pts[3:]:
        draw.ellipse([sp[0] - 2, sp[1] - 2, sp[0] + 2, sp[1] + 2], fill=(95, 242, 255))
    draw.ellipse([h_pt[0] - 3, h_pt[1] - 3, h_pt[0] + 3, h_pt[1] + 3], fill=(255, 220, 80))

    arm_colors = [(255, 130, 90), (130, 255, 130), (140, 180, 255)]
    for (s_pos, e_pos, w_pos, _), col in zip(sample["arms"], arm_colors):
        ps, pe, pw = pt(s_pos), pt(e_pos), pt(w_pos)
        draw.line([ps, pe, pw], fill=col, width=2)
        draw.ellipse([pw[0] - 3, pw[1] - 3, pw[0] + 3, pw[1] + 3], fill=col)


def render_fixed(triangles, project, depth, light, bounds, size=200, ground_mode=None):
    u0, u1, v0, v1 = bounds
    span = max(u1 - u0, v1 - v0) or 1.0
    scale = size * 0.92 / span
    cu, cv = (u0 + u1) / 2.0, (v0 + v1) / 2.0
    img = Image.new("RGB", (size, size), (24, 28, 34))
    draw = ImageDraw.Draw(img)
    ox, oy = project((0.0, 0.0, 0.0))
    gx = size / 2.0 + (ox - cu) * scale
    gy = size / 2.0 - (oy - cv) * scale
    draw.line([(gx - 6, gy), (gx + 6, gy)], fill=(90, 90, 90), width=1)
    draw.line([(gx, gy - 6), (gx, gy + 6)], fill=(90, 90, 90), width=1)
    if ground_mode == "y=0":
        draw.line([(0, gy), (size, gy)], fill=(150, 105, 55), width=2)
    for a, b, c, colour in sorted(triangles, key=lambda t: max(depth(p) for p in t[:3])):
        avg_y = (a[1] + b[1] + c[1]) / 3.0
        if ground_mode == "top_clip" and avg_y < 0.0:
            continue
        u = [b[i] - a[i] for i in range(3)]
        v = [c[i] - a[i] for i in range(3)]
        n = (u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0])
        length = math.sqrt(sum(x * x for x in n)) or 1.0
        lit = 0.45 + 0.75 * abs(sum(light[i] * n[i] for i in range(3))) / length
        factor = 0.35 if (ground_mode in ("y=0", "iso") and avg_y < 0.0) else 1.25
        fill = tuple(min(255, int(ch * lit * factor)) for ch in colour)
        draw.polygon(
            [(size / 2.0 + (project(p)[0] - cu) * scale, size / 2.0 - (project(p)[1] - cv) * scale) for p in (a, b, c)],
            fill=fill,
        )
    return img


def iso_project(p):
    """3/4 isometric projection (yaw 38 deg, pitch 26 deg) in Unity coords."""
    cy, sy = math.cos(math.radians(38)), math.sin(math.radians(38))
    cp, sp = math.cos(math.radians(26)), math.sin(math.radians(26))
    x1 = p[0] * cy - p[2] * sy
    z1 = p[0] * sy + p[2] * cy
    y2 = p[1] * cp - z1 * sp
    return (x1, y2)


def iso_depth(p):
    cy, sy = math.cos(math.radians(38)), math.sin(math.radians(38))
    cp, sp = math.cos(math.radians(26)), math.sin(math.radians(26))
    z1 = p[0] * sy + p[2] * cy
    return -(p[1] * sp + z1 * cp)


def export_model(root, animator, clips, state_speeds, controller_name, attack_max_dist, out_dir):
    """Write `sandworm-3d-model.json` and 3-view skeleton/mesh contact sheets."""
    out_dir.mkdir(parents=True, exist_ok=True)
    model_clips = {}

    for js_name, state_name, clip_name, key_frames in CLIPS:
        clip, clip_tt = clips[clip_name]
        stop = clip.m_MuscleClip.m_StopTime
        total_frames = round(stop * 30.0)
        speed = state_speeds.get(state_name, 1.0)
        events = [
            {
                "time": round(ev.get("time", 0.0), 4),
                "normalized": round(ev.get("time", 0.0) / stop, 4) if stop > 0 else 0.0,
                "functionName": ev.get("functionName", ""),
                "intParameter": ev.get("intParameter", 0),
                "floatParameter": round(ev.get("floatParameter", 0.0), 4),
            }
            for ev in clip_tt.get("m_Events", [])
        ]

        dense_frames = []
        crossings = []
        prev_h = None
        for frame in range(total_frames + 1):
            sample = sample_frame(root, animator, clip, frame, total_frames)
            dense_frames.append(
                {
                    "frame": frame,
                    "time": sample["time"],
                    "s": sample["s"],
                    "spine": {k: [round(v, 2) for v in coords] for k, coords in sample["spine"].items()},
                    "head": {
                        "pos": [round(v, 2) for v in sample["headPos"]],
                        "forward": [round(v, 3) for v in sample["headFwd"]],
                    },
                    "arms": [
                        {
                            "shoulder": [round(v, 2) for v in arm[0]],
                            "elbow": [round(v, 2) for v in arm[1]],
                            "sawHub": [round(v, 2) for v in arm[2]],
                            "sawNormal": [round(v, 3) for v in arm[3]],
                        }
                        for arm in sample["arms"]
                    ],
                    "toothRadius": sample["toothRadius"],
                    "tooth": sample["tooth"],
                }
            )
            cur_h = {k: sample["spine"][k][2] for k in SPINE_BONES}
            cur_h["Head_001"] = sample["headPos"][2]
            if prev_h is not None:
                for bname, h_now in cur_h.items():
                    h_prev = prev_h[bname]
                    if (h_prev < 0.0 <= h_now) or (h_prev >= 0.0 > h_now):
                        crossings.append(
                            {
                                "bone": bname,
                                "direction": "emerge" if h_now >= 0.0 else "burrow",
                                "frame": frame,
                                "s": sample["s"],
                                "time": sample["time"],
                            }
                        )
            prev_h = cur_h

        model_clips[clip_name] = {
            "jsConstant": js_name,
            "animatorState": state_name,
            "duration": round(stop, 4),
            "stateSpeed": speed,
            "effectiveDuration": round(stop / speed, 4) if speed else round(stop, 4),
            "loop": bool(clip.m_MuscleClip.m_LoopTime),
            "sampleRate": 30.0,
            "frameCount": total_frames,
            "events": events,
            "groundCrossings": crossings,
            "keyFrames": key_frames,
            "frames": dense_frames,
        }

        # Render 7-step multi-view verification sheet (top, side, 3/4 iso + skeleton overlay)
        step_indices = [round(i * (len(key_frames) - 1) / 6) for i in range(7)]
        vis_frames = [key_frames[idx] for idx in step_indices]
        cell = 200
        sheet = Image.new("RGB", (cell * len(vis_frames), cell * 3 + 22), (10, 12, 16))
        sheet_draw = ImageDraw.Draw(sheet)
        for col_idx, frame in enumerate(vis_frames):
            sample = sample_frame(root, animator, clip, frame, total_frames)
            posed = mv.posed_transforms(animator, clip, sample["time"])
            tris = mv.assemble(root, posed)
            top = render_fixed(
                tris, lambda p: (p[0], p[2]), lambda p: p[1], (0.3, 0.9, 0.3), (-35, 35, -35, 65), cell, "top_clip"
            )
            side = render_fixed(
                tris, lambda p: (p[2], p[1]), lambda p: -p[0], (0.8, 0.5, 0.2), (-40, 65, -45, 65), cell, "y=0"
            )
            iso = render_fixed(tris, iso_project, iso_depth, (0.4, 0.8, 0.3), (-55, 55, -45, 70), cell, "iso")
            render_skeleton_overlay(ImageDraw.Draw(top), sample, lambda p: (p[0], p[2]), (-35, 35, -35, 65), cell)
            render_skeleton_overlay(ImageDraw.Draw(side), sample, lambda p: (p[2], p[1]), (-40, 65, -45, 65), cell)
            render_skeleton_overlay(ImageDraw.Draw(iso), sample, iso_project, (-55, 55, -45, 70), cell)
            sheet.paste(top, (col_idx * cell, 22))
            sheet.paste(side, (col_idx * cell, 22 + cell))
            sheet.paste(iso, (col_idx * cell, 22 + cell * 2))
            sheet_draw.text((col_idx * cell + 4, 4), f"{clip_name} f={frame} ({sample['s']:.2f})", fill=(255, 255, 0))
        sheet_path = out_dir / f"{clip_name}-3d-views.png"
        sheet.save(sheet_path)
        print(f"wrote {sheet_path}", flush=True)

    payload = {
        "prefab": "Mech_Default_23_1",
        "larvaPrefab": "Mech_Default_1001_1",
        "controller": controller_name,
        "meshScale": {"sandworm": 1.2, "larva": 0.6},
        "attackMaxDistance": attack_max_dist,
        "sawToothRadius": {"retracted": TOOTH_RETRACTED, "extended": TOOTH_EXTENDED},
        "coordinateConvention": "sprite-3D (x right, y = -z_fwd with front toward -y, h = y_up above ground y=0)",
        "skeleton": hierarchy_info(root),
        "clips": model_clips,
    }
    json_path = out_dir / "sandworm-3d-model.json"
    json_path.write_text(json.dumps(payload, indent=2) + "\n")
    print(f"wrote {json_path}", flush=True)


def render_js_module(root, animator, clips):
    """Build the JS `SANDWORM_MOTION` keyframe module for `crates/player/web/sandworm-motion.js`."""
    lines = [
        "// Generated by scripts/player/sandworm-motion.py — do not edit by hand.",
        "",
        "'use strict';",
        "",
        "const SANDWORM_MOTION = {",
        "  SPINE: 1,",
        "  SPINE_COUNT: 6,",
        "  SPINE_STRIDE: 3,",
        "  HEAD: 19,",
        "  HEAD_FWD: 22,",
        "  ARMS: 25,",
        "  ARM_COUNT: 3,",
        "  ARM_STRIDE: 11,",
        "  TOOTH: 58,",
        "  FRAME_LEN: 59,",
    ]
    for js_name, _, clip_name, key_frames in CLIPS:
        clip, _ = clips[clip_name]
        stop = clip.m_MuscleClip.m_StopTime
        total_frames = round(stop * 30.0)
        key = js_name.removeprefix("SANDWORM_")
        lines.append(f"  // {clip_name}: {stop:.3f} s @ 30 fps ({total_frames} frames)")
        lines.append(f"  {key}: [")
        for frame in key_frames:
            sample = sample_frame(root, animator, clip, frame, total_frames)
            row = pack_keyframe(sample)
            lines.append(f"    [{', '.join(map(str, row))}],")
        lines.append("  ],")
    lines.append("};")
    return "\n".join(lines) + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--data", type=Path, default=mv.DATA, help="the game's Data directory")
    parser.add_argument("--out", type=Path, default=DEFAULT_OUT, help="output directory for --export-model")
    parser.add_argument(
        "--export-model",
        action="store_true",
        help="write sandworm-3d-model.json and 3-view verification sheets into --out",
    )
    parser.add_argument(
        "--write",
        action="store_true",
        help="write crates/player/web/sandworm-motion.js",
    )
    parser.add_argument(
        "--check",
        action="store_true",
        help="verify crates/player/web/sandworm-motion.js matches the built game",
    )
    args = parser.parse_args()

    env = UnityPy.load(str(args.data / "sharedassets0.assets"))
    root = mv.find_root(env, "Mech_Default_23_1")
    if root is None:
        raise SystemExit("Mech_Default_23_1 not found in sharedassets0.assets")
    animator, clips, state_speeds, controller_name, attack_max_dist = read_controller_clips(root)

    if args.export_model:
        export_model(root, animator, clips, state_speeds, controller_name, attack_max_dist, args.out)
    else:
        text = render_js_module(root, animator, clips)
        if args.check:
            current = OUTPUT.read_text(encoding="utf-8") if OUTPUT.exists() else None
            if current != text:
                raise SystemExit(
                    f"{OUTPUT.relative_to(ROOT)} is out of date; run: "
                    "uv run --with UnityPy --with pillow python3 scripts/player/sandworm-motion.py --write"
                )
            print(f"{OUTPUT.relative_to(ROOT)} matches {args.data}")
        elif args.write:
            OUTPUT.write_text(text, encoding="utf-8")
            print(f"wrote {OUTPUT.relative_to(ROOT)}")
        else:
            print(text, end="")


if __name__ == "__main__":
    main()
