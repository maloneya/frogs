"""Bake the editable Cleave poses and authoring IK onto the production rig."""
import math
from pathlib import Path

import bpy
from mathutils import Matrix

from motion import action_curves, set_linear_keys

SOURCE = Path(__file__).resolve().parent / 'cleave-poses.blend'


def ease_release(action):
    # Automatic tangents can continue the wind-up after contact begins. Flatten
    # only the active segment's boundary tangents, preserving all authored poses
    # and the moving wind-up before frame 13.
    for curve in action_curves(action):
        for key in curve.keyframe_points:
            if key.co.x == 13:
                key.handle_right_type = 'FREE'
                key.handle_right = (15, key.co.y)
            elif key.co.x == 19:
                key.handle_left_type = 'FREE'
                key.handle_left = (17, key.co.y)


def source_samples(target):
    """Validate and evaluate the complete input before changing the output rig."""
    collections = [bpy.data.objects, bpy.data.meshes, bpy.data.armatures,
                   bpy.data.actions, bpy.data.materials, bpy.data.images,
                   bpy.data.collections]
    before = [set(collection) for collection in collections]
    scene = bpy.context.scene
    initial_frame, initial_subframe = scene.frame_current, scene.frame_subframe
    try:
        with bpy.data.libraries.load(str(SOURCE), link=False) as (_, data):
            data.objects = ['SwordsmanRig']
        source = data.objects[0]
        assert source and not source.is_missing, 'Missing Cleave source rig'
        bpy.context.scene.collection.objects.link(source)
        source.hide_set(False)
        assert max(abs(a-b) for ra, rb in zip(source.matrix_world, Matrix.Identity(4))
                   for a, b in zip(ra, rb)) < .00001, 'Cleave source object transform changed'
        for bone in target.data.bones:
            authored = source.data.bones.get(bone.name)
            assert authored, ('Missing source bone', bone.name)
            assert max(abs(a-b) for ra, rb in zip(bone.matrix_local, authored.matrix_local)
                       for a, b in zip(ra, rb)) < .00001, ('Cleave bind changed', bone.name)
            if bone.parent:
                assert authored.parent and authored.parent.name == bone.parent.name, ('Cleave parent changed', bone.name)
        for bone in source.data.bones:
            if bone.name not in target.data.bones:
                assert bone.name.startswith('CTRL_') and not bone.use_deform, ('Unexpected source joint', bone.name)
        action = source.animation_data.action
        assert action and tuple(action.frame_range) == (1, 31), 'Cleave must span frames 1 to 31'
        ease_release(action)
        frames = []
        for index in range(61):
            frame = 1 + index*.5
            bpy.context.scene.frame_set(int(frame), subframe=frame-int(frame))
            matrices = {bone.name: source.pose.bones[bone.name].matrix.copy() for bone in target.data.bones}
            assert all(math.isfinite(value) for matrix in matrices.values() for row in matrix for value in row), 'Non-finite Cleave pose'
            frames.append((frame, matrices))
        assert max(abs(a-b) for name, first in frames[0][1].items()
                   for ra, rb in zip(first, frames[-1][1][name])
                   for a, b in zip(ra, rb)) < .00001, 'Cleave ready poses do not meet'
        return frames
    finally:
        for collection, old in zip(collections, before):
            for item in list(collection):
                if item not in old:
                    collection.remove(item, do_unlink=True)
        scene.frame_set(initial_frame, subframe=initial_subframe)


def bake(rig):
    frames = source_samples(rig)
    # Resolve the control rig's evaluated poses against the original hierarchy.
    # This folds the hip offset and foot IK into ordinary local transforms.
    keys = []
    for frame, matrices in frames:
        transforms = {}
        for bone in rig.data.bones:
            arguments = {'parent_matrix': matrices[bone.parent.name],
                         'parent_matrix_local': bone.parent.matrix_local} if bone.parent else {}
            basis = bone.convert_local_to_pose(matrices[bone.name], bone.matrix_local,
                                              invert=True, **arguments)
            location, rotation, scale = basis.decompose()
            assert max(abs(value-1) for value in scale) < .0001, ('Cleave changed bone scale', frame, bone.name)
            transforms[bone.name] = location, rotation.normalized()
        keys.append((frame, transforms))
    action = bpy.data.actions.new('AttackCleave')
    action.use_fake_user = True
    rig.animation_data.action = action
    previous = {}
    for frame, transforms in keys:
        for bone in rig.pose.bones:
            location, rotation = transforms[bone.name]
            if bone.name in previous and rotation.dot(previous[bone.name]) < 0:
                rotation.negate()
            previous[bone.name] = rotation.copy()
            bone.rotation_mode = 'QUATERNION'
            bone.location = location
            bone.rotation_quaternion = rotation
            bone.keyframe_insert(data_path='location', frame=frame)
            bone.keyframe_insert(data_path='rotation_quaternion', frame=frame)
    set_linear_keys(action)
    return action
