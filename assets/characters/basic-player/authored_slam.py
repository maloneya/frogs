"""Adapt the CC0 overhead Sword_Regular_C strike to the player's two-hand slam."""
from pathlib import Path
import bpy
from anatomy import SIDES
from mathutils import Quaternion, Vector
from motion import QUATERNIUS_MAP as MAP, bind_corrections, imported_source, set_linear_keys, world_rotation

SOURCE = Path(__file__).resolve().parent / 'reference/slam-source.glb'


def source_samples():
    with imported_source(SOURCE) as (rig, meshes):
        mesh = next(o for o in meshes if o.name.startswith('Mannequin'))
        scale = 2 / (max(v.co.z for v in mesh.data.vertices) - min(v.co.z for v in mesh.data.vertices))
        axes = rig.matrix_world.copy()
        assert (axes @ rig.data.bones['upperarm_r'].head_local).x < 0
        action = bpy.data.actions['Sword_Regular_C']
        rig.animation_data.action = action
        rig.animation_data.action_slot = action.slots[0]
        fps = bpy.context.scene.render.fps
        hand = rig.data.bones['hand_r']
        blade_axis = hand.matrix_local.to_3x3().inverted() @ (rig.data.bones['index_01_r'].head_local - rig.data.bones['pinky_01_r'].head_local).normalized()
        rest = {target: (axes @ rig.data.bones[source].matrix_local).to_quaternion() for target, source in MAP.items()}
        root_rest = (axes @ rig.data.bones['root'].matrix_local).to_quaternion()

        def sample(seconds):
            frame = seconds * fps
            bpy.context.scene.frame_set(int(frame), subframe=frame - int(frame))
            rotations = {target: (axes @ rig.pose.bones[source].matrix).to_quaternion() for target, source in MAP.items()}
            return {'rotations': rotations, 'pelvis': (axes @ rig.pose.bones['pelvis'].head) * scale,
                    'root': (axes @ rig.pose.bones['root'].matrix).to_quaternion() @ root_rest.inverted(),
                    'blade': rotations['RightHand'] @ blade_axis}

        neutral = sample(2.0)
        # Exclude the preceding combo's recovery. Hold the high load at frame 13;
        # the descending stroke occupies contact, and the source keeps settling.
        anchors = [(4, .45), (10, .625), (13, .625), (15, .65), (19, .70),
                   (22, .85), (26, 1.5), (31, 2.0)]
        frames = []
        for i in range(121):
            frame = 1 + i * .25
            if frame < 4:
                seconds, weight = .45, (frame - 1) / 3
            else:
                left = next(j for j in range(len(anchors) - 1) if anchors[j][0] <= frame <= anchors[j + 1][0])
                fa, sa = anchors[left]; fb, sb = anchors[left + 1]
                seconds = sa + (sb - sa) * (frame - fa) / (fb - fa)
                weight = 1 if frame <= 26 else (31 - frame) / 5
            frames.append((frame, weight, sample(seconds)))
        return rest, neutral, frames


def bake(rig, ready_pose, chain, rotate_global):

    def support(rig, source):
        # The reference is an airborne, one-handed combo finisher. Preserve its
        # compression and wrist arc while fitting a planted two-handed action.
        weapon_rotation = rig.pose.bones['Weapon'].matrix.to_quaternion().copy()
        root = rig.pose.bones['Root']
        offset = rig.data.bones['Root'].matrix_local.to_quaternion() @ root.location
        offset.x *= .15; offset.y *= .25; offset.z = min(-.025, offset.z)
        root.location = rig.data.bones['Root'].matrix_local.to_quaternion().inverted() @ offset
        for name in ['Pelvis', 'Spine', 'Chest']:
            bone = rig.pose.bones[name]
            bone.rotation_quaternion = bone.rotation_quaternion.slerp(Quaternion((1, 0, 0, 0)), .5)
        bpy.context.view_layer.update()
        for side, sign in SIDES:
            chain(rig, side + 'Leg', side + 'Shin', (sign * .27, -.06 if side == 'Left' else .10, .14), (sign * .3, -1, .55))
            rotate_global(rig, side + 'Foot', (sign * .14, -1, -.25))
        wrist = rig.pose.bones['RightHand'].head.copy()
        wrist.x = -.02
        wrist.y = min(-.18, max(-.55, wrist.y))
        wrist.z = min(1.95, max(.65, wrist.z))
        blade = Vector((source['blade'].x * .2, source['blade'].y, source['blade'].z * (3.5 if source['blade'].z < 0 else 1))).normalized()
        weapon_rotation = source['blade'].rotation_difference(blade) @ weapon_rotation
        chain(rig, 'RightArm', 'RightForearm', wrist, (-.85, -.1, 1.4))
        # This producer supplies the blade goal. The shared grasp pass derives
        # wrist orientation, both contact frames and supporting-arm reach.
        world_rotation(rig, 'Weapon', weapon_rotation)

    rest,neutral,frames=source_samples()
    corrections = bind_corrections(rig, rest)
    action=bpy.data.actions.new('AttackSlam');action.use_fake_user=True;rig.animation_data.action=action
    previous = {}
    for frame,weight,source in frames:
        ready_pose()
        ready={bone.name:(bone.rotation_quaternion.copy(),bone.location.copy()) for bone in rig.pose.bones}
        for bone in rig.pose.bones:
            bone.rotation_quaternion=(1,0,0,0);bone.location=(0,0,0)
        world_rotation(rig,'Root',source['root']@rig.data.bones['Root'].matrix_local.to_quaternion())
        rig.pose.bones['Root'].location=rig.data.bones['Root'].matrix_local.to_quaternion().inverted()@(source['pelvis']-neutral['pelvis']+Vector((0,0,-.025)))
        bpy.context.view_layer.update()
        for target in MAP:world_rotation(rig,target,source['rotations'][target]@corrections[target])
        hand=rig.pose.bones['RightHand'].matrix.to_quaternion()
        weapon_bind=rig.data.bones['RightHand'].matrix_local.to_quaternion().inverted()@rig.data.bones['Weapon'].matrix_local.to_quaternion()
        base=hand@weapon_bind
        world_rotation(rig,'Weapon',(base@Vector((0,1,0))).rotation_difference(source['blade'])@base)
        support(rig, source)
        bpy.context.view_layer.update()
        floor=1
        for side,sign in SIDES:
            bone=rig.pose.bones[side+'Foot'];palette=bone.matrix@bone.bone.matrix_local.inverted()
            for point in [(sign*.23,-.25,.015),(sign*.23,.03,.02)]:floor=min(floor,(palette@Vector(point)).z)
        if floor<.01:
            rig.pose.bones['Root'].location+=rig.data.bones['Root'].matrix_local.to_quaternion().inverted()@Vector((0,0,.01-floor))
        for bone in rig.pose.bones:
            rotation,location=ready[bone.name]
            bone.rotation_quaternion=rotation.slerp(bone.rotation_quaternion,weight)
            bone.location=location.lerp(bone.location,weight)
            # Blender interpolates quaternion components. Matching signs
            # prevents a half-key from passing through a zero quaternion.
            if bone.name in previous and bone.rotation_quaternion.dot(previous[bone.name]) < 0:
                bone.rotation_quaternion.negate()
            previous[bone.name] = bone.rotation_quaternion.copy()
            bone.keyframe_insert(data_path='rotation_quaternion',frame=frame)
            bone.keyframe_insert(data_path='location',frame=frame)
    set_linear_keys(action)
    return action
