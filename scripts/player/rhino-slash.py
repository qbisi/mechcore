"""Print the Rhino's strike as `crates/player/web/sprites.js` keys it.

    uv run --with UnityPy --with pillow python3 scripts/player/rhino-slash.py

The Rhino strikes with the model's FiringAL and FiringAR, mirror images of
each other: the torso winds back, lifting the chainsaw behind the shoulder,
then turns through to slash it forward and across. Seen from above, most of
that arc is the torso turning, so the sprite turns its torso and hips by the
model's and poses each arm in the torso's frame. This assembles the model as
`model-views.py` does, at keys through FiringAL, and prints for each: how far
through the clip, how far the shoulders and the hips have turned from rest
(degrees, clockwise seen from above), where the spine has moved, and each
arm's shoulder, elbow, hand, the top of its chainsaw's bar and its nose, in
the torso's frame from the spine. It prints the Walk and Idle arms the same
way, the poses the sprite walks and stands in. Lengths are the model's, which
the sprite draws at, front toward -y.
"""

import importlib.util
import math
from pathlib import Path

import UnityPy

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("model_views", HERE / "model-views.py")
mv = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mv)

# FiringAL's frames to key, of the 30 it is sampled at: densest through the
# blow, where the torso turns 130 degrees in four frames.
FRAMES = [0, 3, 6, 8, 9, 10, 11, 12, 13, 14, 15, 17, 19, 21, 23, 25, 27, 30]
ARM = ["Bip001 {} UpperArm", "Bip001 {} Forearm", "Bip001 {} Hand", "Bone {} Forearm2", "Bone {} Saw"]
JOINTS = {"Bip001 Spine", "Bip001 L Clavicle", "Bip001 R Clavicle", "Bip001 L Thigh", "Bip001 R Thigh"}
JOINTS |= {name.format(side) for name in ARM for side in "LR"}


def joints(root, posed):
    """Each joint's position seen from above: x, and y = -z, front toward -y."""
    found = {}

    def walk(transform, parent):
        m = mv.multiply(parent, mv.local_matrix(transform, posed))
        name = transform.m_GameObject.deref().read().m_Name
        if name in JOINTS:
            found[name] = (m[0][3], -m[2][3])
        for child in transform.m_Children:
            walk(child.deref().read(), m)

    walk(root, mv.IDENTITY)
    return found


def turn(found, right, left):
    """How far the line from `right` to `left` has turned from pointing -x."""
    (rx, ry), (lx, ly) = found[right], found[left]
    angle = math.atan2(ly - ry, lx - rx) - math.pi
    return (angle + math.pi) % (2 * math.pi) - math.pi


def arms(found, angle):
    """Both arms in the torso's frame, turned back by `angle` about the spine."""
    px, py = found["Bip001 Spine"]
    c, s = math.cos(angle), math.sin(angle)
    out = []
    for side in "LR":
        for name in ARM:
            x, y = found[name.format(side)]
            x, y = x - px, y - py
            out += [round(x * c + y * s, 1), round(-x * s + y * c, 1)]
    return out


def main():
    env = UnityPy.load(str(mv.DATA / "sharedassets0.assets"))
    root = mv.find_root(env, "Mech_Default_5_1")
    animator, found = mv.states(root)
    clips = {name: clip for name, clip, _ in found}
    rest = None
    for state in ("Walk", "Idle"):
        posed = joints(root, mv.posed_transforms(animator, clips[state], 0.0))
        print(f"{state}: {arms(posed, turn(posed, 'Bip001 R Clavicle', 'Bip001 L Clavicle'))}")
    clip = clips["AttackAL"]
    rate = clip.m_MuscleClip.m_Clip.data.m_DenseClip.m_SampleRate
    for frame in FRAMES:
        # a hair past the frame, so that a sample is not floored onto the one before
        key = round(frame / FRAMES[-1], 3)
        posed = joints(root, mv.posed_transforms(animator, clip, (frame + 1e-3) / rate))
        torso = turn(posed, "Bip001 R Clavicle", "Bip001 L Clavicle")
        hips = turn(posed, "Bip001 R Thigh", "Bip001 L Thigh")
        spine = posed["Bip001 Spine"]
        if rest is None:
            rest = (hips, spine)
        moved = [round(math.degrees(torso)), round(math.degrees(hips - rest[0]))]
        moved += [round(spine[0] - rest[1][0], 1), round(spine[1] - rest[1][1], 1)]
        print(f"    [{key}, {', '.join(map(str, moved + arms(posed, torso)))}],")


if __name__ == "__main__":
    main()
