"""Adapt the CC0 overhead Sword_Regular_C strike to the player's two-hand slam."""
from pathlib import Path
import bpy
from mathutils import Matrix, Quaternion, Vector
from authored_cleave import bake_samples
from motion import QUATERNIUS_MAP as MAP, imported_source, world_rotation

SOURCE = Path(__file__).resolve().parent / 'reference/slam-source.glb'


def source_samples():
    with imported_source(SOURCE) as (rig, meshes):
        mesh = next(o for o in meshes if o.name.startswith('Mannequin'))
        scale = 2 / (max(v.co.z for v in mesh.data.vertices) - min(v.co.z for v in mesh.data.vertices))
        mirror = Matrix.Diagonal((-1, 1, 1, 1))
        axes = mirror @ rig.matrix_world
        assert (axes @ rig.data.bones['upperarm_r'].head_local).x > 0
        action = bpy.data.actions['Sword_Regular_C']
        rig.animation_data.action = action
        rig.animation_data.action_slot = action.slots[0]
        fps = bpy.context.scene.render.fps
        hand = rig.data.bones['hand_r']
        blade_axis = hand.matrix_local.to_3x3().inverted() @ (rig.data.bones['index_01_r'].head_local - rig.data.bones['pinky_01_r'].head_local).normalized()
        blade_axis = mirror.to_3x3() @ blade_axis
        rest = {target: (axes @ rig.data.bones[source].matrix_local @ mirror).to_quaternion() for target, source in MAP.items()}
        root_rest = (axes @ rig.data.bones['root'].matrix_local @ mirror).to_quaternion()

        def sample(seconds):
            frame = seconds * fps
            bpy.context.scene.frame_set(int(frame), subframe=frame - int(frame))
            rotations = {target: (axes @ rig.pose.bones[source].matrix @ mirror).to_quaternion() for target, source in MAP.items()}
            return {'rotations': rotations, 'pelvis': (axes @ rig.pose.bones['pelvis'].head) * scale,
                    'root': (axes @ rig.pose.bones['root'].matrix @ mirror).to_quaternion() @ root_rest.inverted(),
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
    ready_pose()
    hand = rig.pose.bones['RightHand']
    weapon = rig.pose.bones['Weapon']
    grip_rotation = hand.matrix.to_quaternion().inverted() @ weapon.matrix.to_quaternion()
    grip_local = weapon.rotation_quaternion.copy()

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
        for side, sign in [('Left', -1), ('Right', 1)]:
            chain(rig, side + 'Leg', side + 'Shin', (sign * .27, -.06 if sign < 0 else .10, .14), (sign * .3, -1, .55))
            rotate_global(rig, side + 'Foot', (sign * .14, -1, -.25))
        wrist = rig.pose.bones['RightHand'].head.copy()
        wrist.x = .02
        wrist.y = min(-.18, max(-.55, wrist.y))
        wrist.z = min(1.95, max(.65, wrist.z))
        blade = Vector((source['blade'].x * .2, source['blade'].y, source['blade'].z * (3.5 if source['blade'].z < 0 else 1))).normalized()
        weapon_rotation = source['blade'].rotation_difference(blade) @ weapon_rotation
        hand_rotation = weapon_rotation @ grip_rotation.inverted()
        # Fit the shared grip to both reach spheres before baking either arm.
        # The shorter supporting reach otherwise separates near late recovery.
        for _ in range(4):
            for side, target_offset in [('Right', Vector()), ('Left', -blade * .15)]:
                start = rig.pose.bones[side + 'Arm'].head
                reach = rig.data.bones[side + 'Arm'].length + rig.data.bones[side + 'Forearm'].length - .01
                delta = wrist + target_offset - start
                if delta.length > reach:
                    wrist -= delta.normalized() * (delta.length - reach)
        chain(rig, 'RightArm', 'RightForearm', wrist, (.85, -.1, 1.4))
        world_rotation(rig, 'RightHand', hand_rotation)
        # Use the guard's rigid attachment through the entire blend, so recovery
        # rotates the wrist rather than turning the sword inside a fixed hand.
        rig.pose.bones['Weapon'].rotation_quaternion = grip_local
        bpy.context.view_layer.update()
        wrist = rig.pose.bones['RightHand'].head.copy()
        chain(rig, 'LeftArm', 'LeftForearm', wrist - blade * .15, (-.85, -.1, 1.4))
        world_rotation(rig, 'LeftHand', hand_rotation)
    return bake_samples(rig, ready_pose, 'AttackSlam', source_samples(), support, continuous_quaternions=True)
