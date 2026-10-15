"""Add authoring-only hip/foot IK controls to a copy of the five-pose Cleave.

Run in Blender with -- INPUT.blend OUTPUT.blend. Existing authored inputs are
never overwritten; the deform rig and gameplay export path remain unchanged.
"""
import math
from pathlib import Path
import sys

import bpy
from mathutils import Vector
from anatomy import SIDES as ANATOMICAL_SIDES
from motion import action_curves

FRAMES = (1, 13, 19, 24, 31)
SIDES = ('Left', 'Right')


def key_control(bone, frame, previous):
    if bone.name in previous and bone.rotation_quaternion.dot(previous[bone.name]) < 0:
        bone.rotation_quaternion.negate()
    previous[bone.name] = bone.rotation_quaternion.copy()
    for channel in ('location', 'rotation_quaternion'):
        bone.keyframe_insert(data_path=channel, frame=frame, group=bone.name)


def pole_position(hip, knee, ankle):
    axis = (ankle - hip).normalized()
    bend = knee - hip - axis * (knee - hip).dot(axis)
    assert bend.length > .001, 'A straight leg needs an explicit knee direction'
    return knee + bend.normalized() * .65


def match_knee(rig, side, reference, constraint):
    # Match the solver's pole convention to the existing authored knee plane.
    # The two directions account for the mirrored bind rolls of the legs.
    hip = reference[side+'Leg'].translation
    knee = reference[side+'Shin'].translation
    axis = (reference[side+'Foot'].translation - hip).normalized()
    constraint.pole_angle = 0
    bpy.context.view_layer.update()
    current = rig.pose.bones[side+'Shin'].head - hip
    current = (current - axis*current.dot(axis)).normalized()
    desired = knee - hip
    desired = (desired - axis*desired.dot(axis)).normalized()
    angle = math.atan2(current.cross(desired).dot(axis), current.dot(desired))
    candidates = []
    for candidate in (angle, -angle):
        constraint.pole_angle = candidate
        bpy.context.view_layer.update()
        candidates.append(((rig.pose.bones[side+'Shin'].head-knee).length, candidate))
    error, constraint.pole_angle = min(candidates)
    for _ in range(12):
        if error < .00001:
            break
        bpy.context.view_layer.update()
        current = rig.pose.bones[side+'Shin'].head - hip
        current = (current - axis*current.dot(axis)).normalized()
        residual = math.atan2(current.cross(desired).dot(axis), current.dot(desired))
        center = constraint.pole_angle
        candidates = [(error, center)]
        for candidate in (center+residual*.5, center-residual*.5):
            constraint.pole_angle = candidate
            bpy.context.view_layer.update()
            candidates.append(((rig.pose.bones[side+'Shin'].head-knee).length, candidate))
        error, constraint.pole_angle = min(candidates)
    assert error < .001, ('Could not match the authored knee plane', side, error)


def widget(collection, name, vertices, edges):
    mesh = bpy.data.meshes.new(name)
    mesh.from_pydata(vertices, edges, [])
    obj = bpy.data.objects.new(name, mesh)
    collection.objects.link(obj)
    obj.hide_render = True
    obj.hide_set(True)
    return obj


def main():
    source, output = [Path(arg).resolve() for arg in sys.argv[sys.argv.index('--') + 1:]]
    assert source != output and not output.exists(), 'Choose a new output; preserve authored work'
    assert bpy.ops.wm.open_mainfile(filepath=str(source)) == {'FINISHED'}
    rig = bpy.data.objects['SwordsmanRig']
    assert not any(b.name.startswith('CTRL_') for b in rig.data.bones), 'Controls already exist'
    bpy.context.view_layer.objects.active = rig
    rig.select_set(True)
    if rig.mode != 'OBJECT':
        bpy.ops.object.mode_set(mode='OBJECT')
    scene = bpy.context.scene
    samples = {}
    for frame in FRAMES:
        scene.frame_set(frame)
        samples[frame] = {bone.name: bone.matrix.copy() for bone in rig.pose.bones}
        for side in SIDES:
            assert (rig.pose.bones[side+'Leg'].tail - rig.pose.bones[side+'Shin'].head).length < .0001
            assert (rig.pose.bones[side+'Shin'].tail - rig.pose.bones[side+'Foot'].head).length < .0001

    bpy.ops.object.mode_set(mode='EDIT')
    bones = rig.data.edit_bones
    hips = bones.new('CTRL_Hips')
    hips.head = bones['Pelvis'].head
    hips.tail = hips.head + Vector((0, 0, .2))
    hips.use_deform = False
    bones['Root'].parent = hips
    for side in SIDES:
        foot = bones.new('CTRL_'+side+'Foot')
        foot.matrix = bones[side+'Foot'].matrix.copy()
        foot.length = .2
        foot.use_deform = False
        knee = bones.new('CTRL_'+side+'Knee')
        knee.head = Vector((dict(ANATOMICAL_SIDES)[side]*.25, -.7, .6))
        knee.tail = knee.head + Vector((0, 0, .15))
        knee.use_deform = False
    bpy.ops.object.mode_set(mode='POSE')

    controls = rig.data.collections.new('Posing controls')
    guides = rig.data.collections.new('Knee guides')
    shapes = bpy.data.collections.new('Pose control shapes')
    scene.collection.children.link(shapes)
    circle = widget(shapes, 'Hips ring',
                    [(math.cos(i*math.tau/32)*.34, 0, math.sin(i*math.tau/32)*.34) for i in range(32)],
                    [(i, (i+1)%32) for i in range(32)])
    box = widget(shapes, 'Foot handle', [(-.14, -.12, 0), (.14, -.12, 0),
                 (.14, .30, 0), (-.14, .30, 0)], [(0,1), (1,2), (2,3), (3,0)])
    diamond = widget(shapes, 'Knee handle', [(-.08,0,0), (0,0,.08), (.08,0,0), (0,0,-.08)],
                     [(0,1), (1,2), (2,3), (3,0)])
    for bone in rig.pose.bones:
        if not bone.name.startswith('CTRL_'):
            continue
        bone.rotation_mode = 'QUATERNION'
        bone.lock_scale = (True, True, True)
        bone.use_custom_shape_bone_size = False
        bone.custom_shape_wire_width = 3
        bone.custom_shape = circle if bone.name == 'CTRL_Hips' else diamond if bone.name.endswith('Knee') else box
        for collection in list(bone.bone.collections):
            collection.unassign(bone.bone)
        (guides if bone.name.endswith('Knee') else controls).assign(bone.bone)
        bone.bone.color.palette = 'THEME04' if 'Right' in bone.name else 'THEME02' if 'Left' in bone.name else 'THEME09'
        bone.select = False
    rig.pose.bones['CTRL_Hips'].lock_rotation = (True, True, True)
    # Draw the offset control at the animated hips, rather than its bind height.
    hips_control = rig.pose.bones['CTRL_Hips']
    hips_control.custom_shape_transform = rig.pose.bones['Root']
    hips_control.custom_shape_translation = (rig.data.bones['Root'].matrix_local.inverted()
                                             @ rig.data.bones['Pelvis'].head_local)
    hips_control.use_transform_at_custom_shape = True
    for name in ['Root'] + [side+part for side in SIDES for part in ('Leg', 'Shin', 'Foot')]:
        rig.data.bones[name].hide_select = True
    rig.data.bones['Root'].hide = True
    guides.is_visible = False

    constraints = {}
    for side in SIDES:
        shin = rig.pose.bones[side+'Shin']
        constraint = shin.constraints.new('IK')
        constraint.name = 'Foot placement (two-bone IK)'
        constraint.target = rig
        constraint.subtarget = 'CTRL_'+side+'Foot'
        constraint.pole_target = rig
        constraint.pole_subtarget = 'CTRL_'+side+'Knee'
        constraint.chain_count = 2
        constraint.use_stretch = False
        constraint.iterations = 100
        constraints[side] = constraint
        foot = rig.pose.bones[side+'Foot']
        rotation = foot.constraints.new('COPY_ROTATION')
        rotation.name = 'Foot orientation'
        rotation.target = rig
        rotation.subtarget = 'CTRL_'+side+'Foot'
        rotation.target_space = rotation.owner_space = 'POSE'

    previous = {}
    for frame in FRAMES:
        scene.frame_set(frame)
        key_control(rig.pose.bones['CTRL_Hips'], frame, previous)
        for side in SIDES:
            original = samples[frame]
            hip = original[side+'Leg'].translation
            knee = original[side+'Shin'].translation
            ankle = original[side+'Foot'].translation
            pole = pole_position(hip, knee, ankle)
            target = rig.pose.bones['CTRL_'+side+'Foot']
            target.matrix = original[side+'Foot']
            key_control(target, frame, previous)
            guide = rig.pose.bones['CTRL_'+side+'Knee']
            matrix = guide.bone.matrix_local.copy()
            matrix.translation = pole
            guide.matrix = matrix
            key_control(guide, frame, previous)
            match_knee(rig, side, original, constraints[side])
            constraints[side].keyframe_insert(data_path='pole_angle', frame=frame)
        bpy.context.view_layer.update()

    for curve in action_curves(rig.animation_data.action):
        if 'CTRL_' in curve.data_path or 'constraints[' in curve.data_path:
            for key in curve.keyframe_points:
                key.interpolation = 'BEZIER'
                key.handle_left_type = key.handle_right_type = 'AUTO_CLAMPED'

    errors = []
    for frame in FRAMES:
        scene.frame_set(frame)
        for side in SIDES:
            for part in ('Leg', 'Shin', 'Foot'):
                name = side+part
                error = (rig.pose.bones[name].head - samples[frame][name].translation).length
                errors.append(error)
    assert max(errors) < .001, ('IK changed an authored key pose', max(errors))
    print('Maximum authored joint position error:', max(errors), 'metres', flush=True)
    scene.frame_set(13)
    hips = rig.pose.bones['CTRL_Hips']
    feet = [rig.pose.bones[side+'Foot'].matrix.copy() for side in SIDES]
    knee_start = [rig.pose.bones[side+'Shin'].head.copy() for side in SIDES]
    original_location = hips.location.copy()
    hips.location += hips.bone.matrix_local.to_quaternion().inverted() @ Vector((0,0,.08))
    bpy.context.view_layer.update()
    for side, start, knee in zip(SIDES, feet, knee_start):
        foot = rig.pose.bones[side+'Foot']
        assert (foot.head - start.translation).length < .001, ('Foot drift while raising hips', side)
        assert (rig.pose.bones[side+'Shin'].head - knee).length > .01, ('Knee did not respond', side)
    hips.location = original_location
    scene.frame_set(13)
    for bone in rig.pose.bones:
        bone.select = False
    hips.select = True
    rig.data.bones.active = hips.bone
    rig.show_in_front = True
    guide = bpy.data.texts.new('HOW TO - hips and foot controls')
    guide.write('''HIP AND FOOT CONTROLS (authoring only)

Select the ring at the hips: G Z raises/lowers the body while feet hold their
current targets. G also moves it freely. Esc cancels. R on Spine still adjusts
the torso independently.

Select either foot rectangle: G moves that foot; R changes its orientation.
Move the rear foot toward the body to shorten the stance. Feet cannot reach
beyond the actual leg length: stretching is disabled.

Edit frames 1, 13, 19, 24, 31. Press I to record selected controls, Cmd+S to save.
The original deform bones keep their five poses; controls add your changes.
Knee guides are in a hidden bone collection. Reveal them only to steer a knee.

HOW THIS WAS BUILT
In Edit Mode add unconnected, non-deforming bones for the hips, each foot, and
each knee guide. Leave foot/guide controls outside the leg/hip hierarchy. Parent
Root to the hip control, preserving the rest transforms. On each shin add an
Inverse Kinematics constraint targeting that foot control. Set Chain Length=2,
disable Stretch, and set the knee guide as Pole Target. Adjust Pole Angle to
retain the intended knee plane. Copy Rotation on each foot keeps it aligned to
its control. Match and key the controls to every existing pose before enabling
the constraints. Custom wire shapes make the handles easy to select.

These controls will be baked to the original deform rig before game export.
''')
    output.parent.mkdir(parents=True, exist_ok=True)
    assert bpy.ops.wm.save_as_mainfile(filepath=str(output)) == {'FINISHED'}
    print('PASS: five authored poses preserved; raising hips bends knees with planted feet.', flush=True)


if __name__ == '__main__':
    main()
