"""Check intermediate Slam and locomotion poses in a generated Blender asset."""
from pathlib import Path
import sys

import bpy


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
        assert (hand.head - weapon.head).length < .001
        grips.append(hand.matrix.inverted() @ weapon.matrix)
        if 10 <= frame <= 26:
            goal = hand.head - (weapon.tail - weapon.head).normalized() * .15
            gap = (rig.pose.bones['LeftHand'].head - goal).length
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
    check_slam(rig)
    check_locomotion(rig)


if __name__ == '__main__':
    main()
