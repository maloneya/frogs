"""Retarget CC0 Quaternius jogging and KayKit directional support to the player."""
from pathlib import Path
import bpy
import math
from mathutils import Euler, Vector
from anatomy import SIDES
from motion import QUATERNIUS_MAP as HUMAN_MAP, bind_corrections, imported_source, world_rotation, set_linear_keys

SOURCE = Path(__file__).resolve().parent / 'reference/locomotion-source.glb'
JOG_SOURCE = Path(__file__).resolve().parent / 'reference/jog-source.glb'
# Retain the game's lateral travel roles after removing the old X reflection.
CLIPS = {'RunBackward': 'Walking_Backwards',
         'RunLeft': 'Running_Strafe_Right', 'RunRight': 'Running_Strafe_Left'}
LOWER = ('Pelvis', 'LeftLeg', 'LeftShin', 'LeftFoot', 'RightLeg', 'RightShin', 'RightFoot')
MAP = {'Pelvis': 'hips'}
for side, suffix in [('Left', 'l'), ('Right', 'r')]:
    MAP.update({side + target: source + '.' + suffix for target, source in
                [('Leg', 'upperleg'), ('Shin', 'lowerleg'), ('Foot', 'foot')]})


def source_samples(path, mapping, clips):
    with imported_source(path) as (rig, meshes):
        axes = rig.matrix_world.copy()
        assert (axes @ rig.data.bones[mapping['RightLeg']].head_local).x < 0
        rest = {target: (axes @ rig.data.bones[source].matrix_local).to_quaternion()
                for target, source in mapping.items()}
        pelvis = axes @ rig.data.bones[mapping['Pelvis']].head_local
        meshes = [o for o in meshes
                  if (o.name.startswith('Knight_') and o.name != 'Knight_Cape') or o.name.startswith('Mannequin')]
        points = [o.matrix_world @ v.co for o in meshes for v in o.data.vertices]
        scale = 2 / (max(v.z for v in points) - min(v.z for v in points))
        samples = {}
        for target, name in clips.items():
            action = bpy.data.actions[name]
            rig.animation_data.action = action; rig.animation_data.action_slot = action.slots[0]
            first, last = action.frame_range
            frames = []
            for i in range(61):
                frame = first + (last - first) * (i % 60) / 60
                bpy.context.scene.frame_set(int(frame), subframe=frame - int(frame))
                frames.append({
                    'rotations': {t: (axes @ rig.pose.bones[s].matrix).to_quaternion() for t, s in mapping.items()},
                    'height': ((axes @ rig.pose.bones[mapping['Pelvis']].head).z - pelvis.z) * scale,
                })
            samples[target] = frames
        return rest, samples


def bake(rig, ready_pose):
    rest, samples = source_samples(SOURCE, MAP, CLIPS)
    human_rest, human_samples = source_samples(JOG_SOURCE, HUMAN_MAP, {'Jog': 'Jog_Fwd_Loop'})
    samples = {'Run': [dict(frame) for frame in human_samples['Jog']], **samples}
    corrections = bind_corrections(rig, rest)
    human_corrections = bind_corrections(rig, human_rest)
    for name, frames in samples.items():
        for i, frame in enumerate(frames):
            human = human_samples['Jog'][i]
            original = frame['rotations']
            frame['rotations'] = {target: rotation @ human_corrections[target]
                                  for target, rotation in human['rotations'].items()}
            if name == 'Run':
                frame['height'] = human['height']
            else:
                frame['rotations'].update({target: original[target] @ corrections[target] for target in LOWER})
    for name, frames in samples.items():
        for frame in frames:
            rotations = frame['rotations']
            chest = rotations['Chest']
            chest_bind = rig.data.bones['Chest'].matrix_local.to_quaternion()
            tilt = (chest @ chest_bind.inverted()).to_euler('XYZ')
            carried = Euler((tilt.x * .5, tilt.y * .5, max(-math.radians(12), min(math.radians(12), tilt.z))), 'XYZ').to_quaternion() @ chest_bind
            head_bind = rig.data.bones['Head'].matrix_local.to_quaternion()
            head_tilt = (rotations['Head'] @ head_bind.inverted()).to_euler('XYZ')
            aimed_head = Euler((head_tilt.x * .35, head_tilt.y * .35, 0), 'XYZ').to_quaternion() @ head_bind
            upright = carried @ chest.inverted()
            for target in rotations:
                if target not in LOWER:
                    rotations[target] = upright @ rotations[target]
            rotations['Head'] = aimed_head
            if name == 'Run':
                rotations['Pelvis'] = rig.data.bones['Pelvis'].matrix_local.to_quaternion().slerp(rotations['Pelvis'], .65)
    # Keep the authored lateral/backward support while using the compatible
    # humanoid torso, clavicles, elbows and arm counter-swing.

    ready_pose()
    grip = rig.pose.bones['Weapon'].rotation_quaternion.copy()
    ready_hand = rig.pose.bones['RightHand'].matrix.to_quaternion().copy()
    for name, frames in samples.items():
        action = bpy.data.actions.new(name); action.use_fake_user = True
        rig.animation_data.action = action
        # Carry the long blade in the guard's hand orientation at the source
        # contact pose, then preserve the source's wrist swing through the cycle.
        hand_offset = (frames[0]['rotations']['RightHand']).inverted() @ ready_hand
        previous = {}
        for i, source in enumerate(frames):
            for bone in rig.pose.bones:
                bone.rotation_mode = 'QUATERNION'
                bone.rotation_quaternion = (1, 0, 0, 0); bone.location = (0, 0, 0)
            rig.pose.bones['Root'].location = rig.data.bones['Root'].matrix_local.to_quaternion().inverted() @ Vector((0, 0, source['height'] - .025))
            bpy.context.view_layer.update()
            for target, rotation in source['rotations'].items():
                world_rotation(rig, target, rotation)
            world_rotation(rig, 'RightHand', source['rotations']['RightHand'] @ hand_offset)
            rig.pose.bones['Weapon'].rotation_quaternion = grip
            bpy.context.view_layer.update()
            floor = min((rig.pose.bones[side + 'Foot'].matrix @ rig.data.bones[side + 'Foot'].matrix_local.inverted() @ Vector(point)).z
                        for side, sign in SIDES
                        for point in [(sign * .23, -.25, .015), (sign * .23, .03, .02)])
            rig.pose.bones['Root'].location += rig.data.bones['Root'].matrix_local.to_quaternion().inverted() @ Vector((0, 0, .01 - floor))
            for bone in rig.pose.bones:
                if bone.name in previous and bone.rotation_quaternion.dot(previous[bone.name]) < 0:
                    bone.rotation_quaternion.negate()
                previous[bone.name] = bone.rotation_quaternion.copy()
                frame = 1 + i * .25
                bone.keyframe_insert(data_path='rotation_quaternion', frame=frame)
                bone.keyframe_insert(data_path='location', frame=frame)
        set_linear_keys(action)
