"""Check authored and intermediate poses in a generated Blender asset."""
from pathlib import Path
import sys
from tempfile import TemporaryDirectory
from unittest.mock import patch

import bpy

sys.path.insert(0, str(Path(__file__).resolve().parent))
import grasp
import authored_cleave


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


def check_cleave(rig):
    select_action(rig, 'AttackCleave')
    samples = authored_cleave.source_samples(rig)
    error = 0
    for frame, expected in samples:
        set_frame(frame)
        for name, matrix in expected.items():
            error = max(error, max(abs(a-b) for ra, rb in zip(matrix, rig.pose.bones[name].matrix)
                                   for a, b in zip(ra, rb)))
    assert error < .001, ('compiled Cleave differs from its authored poses', error)
    assert not any(bone.name.startswith('CTRL_') or bone.constraints for bone in rig.pose.bones), 'authoring controls leaked into runtime rig'
    set_frame(9)
    head = rig.pose.bones['Head'].head.copy()
    set_frame(13)
    assert (head-rig.pose.bones['Head'].head).length > .001, 'wind-up freezes'
    # Inspect the actual boot soles, not a bone tip above the ground.
    boots = bpy.data.objects['HoodedCloak']
    soles = {}
    for side in ('Left', 'Right'):
        group = boots.vertex_groups[side+'Foot'].index
        foot = [v for v in boots.data.vertices
                if any(w.group == group and w.weight > .99 for w in v.groups)]
        floor = min(v.co.z for v in foot)
        soles[side] = [v.index for v in foot if v.co.z < floor+.0001]
    rear_toe = min(soles['Left'], key=lambda i: boots.data.vertices[i].co.y)
    set_frame(13)
    front = rig.pose.bones['RightFoot'].matrix.copy()
    hips = rig.pose.bones['Pelvis'].head.copy()
    evaluated = boots.evaluated_get(bpy.context.evaluated_depsgraph_get())
    anchor = evaluated.data.vertices[rear_toe].co.copy()
    drift = 0
    for index in range(89):
        frame = 13 + index*.125
        set_frame(frame)
        assert max(abs(a-b) for ra, rb in zip(front, rig.pose.bones['RightFoot'].matrix)
                   for a, b in zip(ra, rb)) < .001, ('front foot slides during transfer', frame)
        evaluated = boots.evaluated_get(bpy.context.evaluated_depsgraph_get())
        drift = max(drift, (evaluated.data.vertices[rear_toe].co-anchor).length)
        for side, indices in soles.items():
            height = min(evaluated.data.vertices[i].co.z for i in indices)
            assert .005 < height < .025, ('supporting boot floats or penetrates', frame, side, height)
    assert drift < .01, ('rear toe slides during push-off', drift)
    set_frame(19)
    assert .18 < hips.y-rig.pose.bones['Pelvis'].head.y < .25, 'missing forward weight transfer'
    set_frame(19.25)
    accepted = rig.animation_data.action
    actions = set(bpy.data.actions)
    pose = [bone.matrix.copy() for bone in rig.pose.bones]
    scene = bpy.context.scene
    frame = scene.frame_current, scene.frame_subframe
    with TemporaryDirectory() as directory, patch.object(authored_cleave, 'SOURCE', Path(directory)/'missing.blend'):
        try:
            authored_cleave.bake(rig)
        except OSError:
            pass
        else:
            raise AssertionError('missing authored source was accepted')
    ease_release = authored_cleave.ease_release
    def invalid_ready_seam(action):
        ease_release(action)
        curve = next(curve for curve in authored_cleave.action_curves(action)
                     if curve.data_path == 'pose.bones["Root"].location' and curve.array_index == 0)
        curve.keyframe_points[-1].co.y += .1
        curve.update()
    with patch.object(authored_cleave, 'ease_release', invalid_ready_seam):
        try:
            authored_cleave.bake(rig)
        except AssertionError as refusal:
            assert str(refusal) == 'Cleave ready poses do not meet', refusal
        else:
            raise AssertionError('invalid authored ready seam was accepted')
    assert (scene.frame_current, scene.frame_subframe) == frame, 'refusal changed the accepted frame'
    assert rig.animation_data.action == accepted and set(bpy.data.actions) == actions, 'refusal replaced an accepted action'
    assert max(abs(a-b) for before, bone in zip(pose, rig.pose.bones)
               for ra, rb in zip(before, bone.matrix) for a, b in zip(ra, rb)) < .000001, 'refusal changed the accepted pose'
    print('PASS Cleave: authored poses/IK preserved, moving wind-up, grounded push-off, planted front foot, original deform rig, atomic source refusal; max matrix error', error, 'rear toe drift', drift)


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
    check_cleave(rig)
    check_slam(rig)
    check_locomotion(rig)
    grasp.validate_motion(rig, bpy.data.objects['Swordsman'])


if __name__ == '__main__':
    main()
