"""Fit Quaternius' CC0 Sword_Idle weight transfer to a relaxed low guard."""
from pathlib import Path

import bpy
from mathutils import Matrix, Quaternion, Vector
from motion import QUATERNIUS_MAP, imported_source, world_rotation, set_linear_keys

SOURCE = Path(__file__).resolve().parent / 'reference/idle-source.glb'
SECONDS = 3.2
BODY = ('Pelvis', 'Spine', 'Chest', 'Neck', 'Head', 'LeftShoulder', 'RightShoulder')


def source_samples():
    with imported_source(SOURCE) as (rig, meshes):
        mirror = Matrix.Diagonal((-1, 1, 1, 1))
        axes = mirror @ rig.matrix_world
        points = [obj.matrix_world @ vertex.co for obj in meshes for vertex in obj.data.vertices]
        scale = 2 / (max(point.z for point in points) - min(point.z for point in points))
        action = bpy.data.actions['Sword_Idle']
        rig.animation_data.action = action
        rig.animation_data.action_slot = action.slots[0]
        first, last = action.frame_range
        frames = []
        for index in range(97):
            frame = first + (last - first) * (index % 96) / 96
            bpy.context.scene.frame_set(int(frame), subframe=frame - int(frame))
            frames.append({
                'rotations': {target: (axes @ rig.pose.bones[QUATERNIUS_MAP[target]].matrix @ mirror).to_quaternion()
                              for target in (*BODY, 'LeftHand', 'RightHand')},
                'positions': {target: (axes @ rig.pose.bones[QUATERNIUS_MAP[target]].head) * scale
                              for target in ('Pelvis', 'LeftHand', 'RightHand')},
            })
        return frames


def bake(rig, ready_pose, chain, rotate_global):
    frames = source_samples()
    action = bpy.data.actions.new('Idle')
    action.use_fake_user = True
    rig.animation_data.action = action
    ready_pose()
    ready_rotations = {name: rig.pose.bones[name].matrix.to_quaternion().copy()
                       for name in (*BODY, 'LeftHand', 'RightHand')}
    hands = {side: rig.pose.bones[side + 'Hand'].head.copy() for side in ('Left', 'Right')}
    feet = {side: rig.pose.bones[side + 'Foot'].head.copy() for side in ('Left', 'Right')}
    grip = rig.pose.bones['Weapon'].rotation_quaternion.copy()
    neutral = frames[0]
    previous = {}
    for index, source in enumerate(frames):
        ready_pose()
        sway = (source['positions']['Pelvis'] - neutral['positions']['Pelvis']) * .55
        rig.pose.bones['Root'].location += rig.data.bones['Root'].matrix_local.to_quaternion().inverted() @ sway
        bpy.context.view_layer.update()
        for name in BODY:
            delta = source['rotations'][name] @ neutral['rotations'][name].inverted()
            world_rotation(rig, name, Quaternion().slerp(delta, .65) @ ready_rotations[name])
        # Keep support planted while pelvis and torso transfer the reference's
        # weight. The arms follow with independent small reaches and wrist arcs.
        for side, sign in [('Left', -1), ('Right', 1)]:
            chain(rig, side + 'Leg', side + 'Shin', feet[side], (sign * .25, -1, .55))
            rotate_global(rig, side + 'Foot', (sign * .14, -1, -.25))
            name = side + 'Hand'
            drift = (source['positions'][name] - neutral['positions'][name]) * .65
            chain(rig, side + 'Arm', side + 'Forearm', hands[side] + drift,
                  (sign * .52, .08, 1.24))
            delta = source['rotations'][name] @ neutral['rotations'][name].inverted()
            world_rotation(rig, name, Quaternion().slerp(delta, .45) @ ready_rotations[name])
        rig.pose.bones['Weapon'].rotation_quaternion = grip
        for bone in rig.pose.bones:
            bone.rotation_quaternion.normalize()
            if bone.name in previous and bone.rotation_quaternion.dot(previous[bone.name]) < 0:
                bone.rotation_quaternion.negate()
            previous[bone.name] = bone.rotation_quaternion.copy()
            frame = 1 + index * SECONDS * 30 / 96
            bone.keyframe_insert(data_path='rotation_quaternion', frame=frame)
            bone.keyframe_insert(data_path='location', frame=frame)
    set_linear_keys(action)
    return action
