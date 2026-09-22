"""Rebuild the multipart skin/material diagnostic from the base mannequin source.

blender --background --factory-startup --python-exit-code 1 --python \
    assets/fixtures/generate-blender-multipart.py -- /tmp/multipart-fixture

Produces equivalent single-mesh and multi-mesh files for GPU comparisons. Both
have three materials/images and one shared four-bone skeleton.
"""
from pathlib import Path
import runpy
import sys
from tempfile import TemporaryDirectory
import bpy

arguments = sys.argv[sys.argv.index('--') + 1:] if '--' in sys.argv else []
output = Path(arguments[0]).resolve() if arguments else Path(__file__).resolve().parent
output.mkdir(parents=True, exist_ok=True)
# Reuse the established geometry, weights and actions, not a second mannequin.
with TemporaryDirectory(prefix='arpg-multipart-') as temporary:
    sys.argv = [sys.argv[0], '--', str(Path(temporary) / 'base.glb')]
    runpy.run_path(str(Path(__file__).with_name('generate-blender-bind-pose.py')))
character = bpy.data.objects['BindPoseCharacter']
rig = bpy.data.objects['BindPoseRig']
# Solid RGB images expose incorrect material bindings in GPU readback. The
# body parts share the red material while blue/green use distinct images.
original = bpy.data.materials['BindPoseMaterial']
materials = []
for name, colour in (("Red", (1,0,0,1)), ("Green", (0,1,0,1)), ("Blue", (0,0,1,1))):
    image = bpy.data.images.new(name + "Image", width=8, height=8, alpha=True)
    image.pixels = list(colour) * 64
    image.pack()
    material = original.copy()
    material.name = name
    material.node_tree.nodes['BindPoseBaseColor'].image = image
    materials.append(material)
character.data.materials.clear()
for material in materials:
    character.data.materials.append(material)
for polygon in character.data.polygons:
    centre = polygon.center
    polygon.material_index = 2 if centre.z > 1.45 else (1 if centre.z < .7 else 0)

rig.animation_data.action = bpy.data.actions['Idle']
bpy.context.scene.frame_set(1)
def export(name):
    for obj in bpy.data.objects:
        obj.select_set(True)
    assert bpy.ops.export_scene.gltf(filepath=str(output / name), export_format='GLB',
        use_selection=True, export_apply=False, export_cameras=False, export_lights=False,
        export_animations=True, export_animation_mode='ACTIONS', export_nla_strips=False) == {'FINISHED'}
export('blender-multipart-reference.glb')
for obj in bpy.context.selected_objects:
    obj.select_set(False)
character.select_set(True)
bpy.context.view_layer.objects.active = character
assert bpy.ops.object.mode_set(mode='EDIT') == {'FINISHED'}
assert bpy.ops.mesh.select_all(action='SELECT') == {'FINISHED'}
assert bpy.ops.mesh.separate(type='LOOSE') == {'FINISHED'}
assert bpy.ops.object.mode_set(mode='OBJECT') == {'FINISHED'}
meshes = sorted((obj for obj in bpy.data.objects if obj.type == 'MESH'), key=lambda o: o.name)
# Combine head and torso only: this mesh retains several material primitives.
upper = [obj for obj in meshes if abs(sum(v.co.x for v in obj.data.vertices)/len(obj.data.vertices)) < .1]
for obj in bpy.context.selected_objects:
    obj.select_set(False)
for obj in upper:
    obj.select_set(True)
bpy.context.view_layer.objects.active = upper[0]
assert bpy.ops.object.join() == {'FINISHED'}
bpy.context.object.name = 'UpperBody'
export('blender-multipart.glb')
