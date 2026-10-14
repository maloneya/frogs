"""Check intermediate idle, Slam and locomotion poses in a generated Blender asset."""
from pathlib import Path
import sys

import bpy

sys.path.insert(0, str(Path(__file__).resolve().parent))
import grasp


def select_action(rig, name):
    action = bpy.data.actions[name]
    rig.animation_data.action = action
    rig.animation_data.action_slot = action.slots[0]


def set_frame(frame):
    bpy.context.scene.frame_set(int(frame), subframe=frame - int(frame))


def check_grip(matrices):
    first = matrices[0]
    assert all(max(abs(a - b) for row_a, row_b in zip(first, matrix)
                   for a, b in zip(row_a, row_b)) < .001
               for matrix in matrices), 'sword twists within hand'


def check_slam(rig):
    select_action(rig, 'AttackSlam')
    grips, gaps, tips, feet = [], [], [], []
    for index in range(241):
        frame = 1 + index / 8
        set_frame(frame)
        hand = rig.pose.bones['RightHand']
        weapon = rig.pose.bones['Weapon']
        assert (rig.pose.bones['RightGrip'].head - weapon.head).length < .001
        grips.append(hand.matrix.inverted() @ weapon.matrix)
        if 10 <= frame <= 26:
            goal = weapon.head - (weapon.tail - weapon.head).normalized() * .15
            gap = (rig.pose.bones['LeftGrip'].head - goal).length
            gaps.append(gap)
            assert gap < .03, (frame, 'support hand left grip', gap)
            feet.append([rig.pose.bones[side + 'Foot'].head.copy() for side in ['Left', 'Right']])
        tips.append(rig.pose.bones['BeamOrigin'].head.copy())
    check_grip(grips)
    assert tips[96].z > 2.0, ('loaded sword too low', tips[96])
    assert tips[144].z < .3 and tips[144].y < -.8, ('strike misses the ground in front', tips[144])
    assert (tips[144] - tips[168]).length > .1, 'recovery froze'
    assert (tips[0] - tips[-1]).length < .0001, 'ready seam'
    for row in feet:
        for start, current in zip(feet[0], row):
            assert (start - current).length < .001, ('planted foot drift', start, current)
    print('PASS Slam: phase edges, planted feet, supporting grip, rigid sword, '
          'moving recovery, ready seam; max support error', max(gaps))


def check_idle(rig):
    select_action(rig, 'Idle')
    first, last = bpy.data.actions['Idle'].frame_range
    positions = {name: [] for name in ['Pelvis', 'Chest', 'LeftHand', 'RightHand']}
    grips, feet, ends = [], [], []
    for index in range(385):
        set_frame(first + (last - first) * index / 384)
        for name, samples in positions.items():
            samples.append(rig.pose.bones[name].head.copy())
        for side in ['Left', 'Right']:
            assert rig.pose.bones[side + 'Hand'].head.z < 1.18, 'raised idle arm'
        assert rig.pose.bones['BeamOrigin'].head.z > .45, 'idle blade meets floor'
        grips.append(rig.pose.bones['RightHand'].matrix.inverted() @ rig.pose.bones['Weapon'].matrix)
        feet.append([rig.pose.bones[side + 'Foot'].matrix.copy() for side in ['Left', 'Right']])
        if index in [0, 384]:
            ends.append([bone.matrix.copy() for bone in rig.pose.bones])
    check_grip(grips)
    for a, b in zip(ends[0], ends[1]):
        assert max(abs(x - y) for ra, rb in zip(a, b) for x, y in zip(ra, rb)) < .0001, 'idle seam'
    for row in feet:
        for a, b in zip(feet[0], row):
            assert max(abs(x - y) for ra, rb in zip(a, b) for x, y in zip(ra, rb)) < .001, 'idle foot drift'
    spans = {name: max((a - b).length for a in samples for b in samples)
             for name, samples in positions.items()}
    assert all(.015 < span < .10 for span in spans.values()), ('frozen or excessive idle motion', spans)
    print('PASS Idle: relaxed arms, whole-body motion, planted feet, rigid sword, loop seam;', spans)


def check_locomotion(rig):
    for name in ['Run', 'RunBackward', 'RunLeft', 'RunRight']:
        select_action(rig, name)
        hands = {'Left': [], 'Right': []}
        grips = []
        hip_yaw = None
        for index in range(121):
            set_frame(1 + index * .125)
            for side, positions in hands.items():
                positions.append(rig.pose.bones[side + 'Hand'].head.copy())
            if index == 0:
                hip_yaw = rig.pose.bones['Pelvis'].matrix.to_euler().z
            grips.append(rig.pose.bones['RightHand'].matrix.inverted() @ rig.pose.bones['Weapon'].matrix)
        spans = []
        for side, positions in hands.items():
            assert (positions[0] - positions[-1]).length < .0001, (name, side, 'loop seam')
            span = max((a - b).length for a in positions for b in positions)
            assert span > .25, (name, side, 'frozen arms', span)
            spans.append(round(span, 3))
        check_grip(grips)
        print('PASS', name, 'hip yaw', round(hip_yaw * 57.3), 'arm spans', spans, flush=True)


def main():
    path = Path(sys.argv[sys.argv.index('--') + 1]).resolve()
    assert bpy.ops.wm.open_mainfile(filepath=str(path)) == {'FINISHED'}
    rig = bpy.data.objects['SwordsmanRig']
    check_idle(rig)
    check_slam(rig)
    check_locomotion(rig)
    grasp.validate_motion(rig, bpy.data.objects['Swordsman'])


if __name__ == '__main__':
    main()
