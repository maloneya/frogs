"""Shared Blender operations for this player's authored motion inputs."""
from contextlib import contextmanager
import bpy
from mathutils import Vector

QUATERNIUS_MAP = {'Pelvis': 'pelvis', 'Spine': 'spine_02', 'Chest': 'spine_03',
                  'Neck': 'neck_01', 'Head': 'Head'}
for target, source in [('Left', 'l'), ('Right', 'r')]:
    QUATERNIUS_MAP.update({target + name: joint + '_' + source for name, joint in
        [('Shoulder', 'clavicle'), ('Arm', 'upperarm'), ('Forearm', 'lowerarm'),
         ('Hand', 'hand'),
         ('Leg', 'thigh'), ('Shin', 'calf'), ('Foot', 'foot')]})


@contextmanager
def imported_source(path):
    """Remove only datablocks introduced by a temporary reference import."""
    collections = [bpy.data.objects, bpy.data.meshes, bpy.data.armatures,
                   bpy.data.actions, bpy.data.materials, bpy.data.images]
    before = [set(collection) for collection in collections]
    try:
        assert bpy.ops.import_scene.gltf(filepath=str(path)) == {'FINISHED'}
        objects = [obj for obj in bpy.data.objects if obj not in before[0]]
        rig = next(obj for obj in objects if obj.type == 'ARMATURE')
        yield rig, [obj for obj in objects if obj.type == 'MESH']
    finally:
        for collection, old in zip(collections, before):
            for item in list(collection):
                if item not in old:
                    collection.remove(item)


def bind_corrections(rig, reference):
    result = {}
    for target, rotation in reference.items():
        bind = rig.data.bones[target].matrix_local.to_quaternion()
        aligned = (bind @ Vector((0, 1, 0))).rotation_difference(rotation @ Vector((0, 1, 0))) @ bind
        result[target] = rotation.inverted() @ aligned
    return result


def world_rotation(rig, name, desired):
    joint = rig.pose.bones[name]
    rest = joint.bone.matrix_local.to_quaternion()
    if joint.parent:
        local_rest = joint.parent.bone.matrix_local.to_quaternion().inverted() @ rest
        joint.rotation_quaternion = local_rest.inverted() @ joint.parent.matrix.to_quaternion().inverted() @ desired
    else:
        joint.rotation_quaternion = rest.inverted() @ desired
    joint.rotation_quaternion.normalize()
    bpy.context.view_layer.update()


def action_curves(action):
    for slot in action.slots:
        for layer in action.layers:
            for strip in layer.strips:
                bag = strip.channelbag(slot)
                if bag:
                    yield from bag.fcurves


def set_linear_keys(action):
    for curve in action_curves(action):
        for key in curve.keyframe_points:
            key.interpolation = 'LINEAR'
