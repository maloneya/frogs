"""Metre-scale greybox chest: one mesh/material/PNG, +Z facing at runtime.
Run: blender --background --factory-startup --python-exit-code 1 --python
assets/world/chest/generate.py -- /tmp/chest-output
"""
from pathlib import Path
import math
import sys
import bpy
from mathutils import Vector

OUT = Path(sys.argv[sys.argv.index('--') + 1]) if '--' in sys.argv else Path(__file__).resolve().parent
OUT.mkdir(parents=True, exist_ok=True)
assert bpy.ops.object.select_all(action='SELECT') == {'FINISHED'}
assert bpy.ops.object.delete(use_global=False) == {'FINISHED'}
# One tiny palette texture. UVs select wood, metal, or the dark interior.
image = bpy.data.images.new('ChestPalette', width=8, height=8, alpha=True)
colours = [(0.48, 0.24, 0.085, 1), (0.65, 0.48, 0.20, 1), (0.07, 0.035, 0.015, 1), (0.6, 0.33, 0.12, 1)]
image.pixels = [v for y in range(8) for x in range(8) for v in colours[x // 2]]
image.pack()
mat = bpy.data.materials.new('Chest')
mat.use_nodes = True
mat.use_backface_culling = True
tex = mat.node_tree.nodes.new('ShaderNodeTexImage')
tex.image = image
mat.node_tree.links.new(tex.outputs['Color'], mat.node_tree.nodes['Principled BSDF'].inputs['Base Color'])


def box(name, position, size, colour, parts, hinge=False, opened=False):
    assert bpy.ops.mesh.primitive_cube_add(size=1, location=position) == {'FINISHED'}
    obj = bpy.context.object
    obj.name = name
    obj.scale = size
    assert bpy.ops.object.transform_apply(location=False, rotation=False, scale=True) == {'FINISHED'}
    if hinge and opened:
        pivot = Vector((0, 0.3, 0.48))
        from mathutils import Matrix
        rotation = Matrix.Rotation(math.radians(-105), 4, 'X')
        obj.location = pivot + rotation.to_3x3() @ (obj.location - pivot)
        obj.rotation_euler.x = math.radians(-105)
    for uv in obj.data.uv_layers.active.data:
        uv.uv = ((colour * 2 + 1) / 8, 0.5)
    obj.data.materials.append(mat)
    parts.append(obj)


for opened in [False, True]:
    parts = []
    box('Floor', (0, 0, .055), (1.0, .6, .11), 2, parts)
    box('Front', (0, -.265, .27), (1.0, .07, .42), 0, parts)
    box('Back', (0, .265, .27), (1.0, .07, .42), 0, parts)
    for x in [-.465, .465]:
        box('Side', (x, 0, .27), (.07, .46, .42), 0, parts)
    for x in [-.32, .32]:
        for y in [-.306, .306]:
            box('Band', (x, y, .27), (.07, .016, .42), 1, parts)
    box('Lid', (0, 0, .54), (1.04, .64, .12), 3, parts, True, opened)
    for x in [-.32, .32]:
        box('LidBand', (x, 0, .608), (.07, .64, .016), 1, parts, True, opened)
    box('Latch', (0, -.327, .44), (.13, .025, .15), 1, parts, True, opened)
    assert bpy.ops.object.select_all(action='DESELECT') == {'FINISHED'}
    for obj in parts:
        obj.select_set(True)
    bpy.context.view_layer.objects.active = parts[0]
    assert bpy.ops.object.join() == {'FINISHED'}
    obj = bpy.context.object
    obj.name = 'ChestOpen' if opened else 'ChestClosed'
    assert bpy.ops.object.transform_apply(location=True, rotation=True, scale=True) == {'FINISHED'}
    filename = 'open' if opened else 'closed'
    assert bpy.ops.export_scene.gltf(filepath=str(OUT / (filename + '.glb')), export_format='GLB', use_selection=True,
        export_cameras=False, export_lights=False, export_animations=False, export_image_format='AUTO') == {'FINISHED'}
    obj.hide_render = opened
# Both meshes remain editable in the saved source; closed is the diagnostic render subject.
assert bpy.ops.wm.save_as_mainfile(filepath=str(OUT / 'chest.blend')) == {'FINISHED'}
# Fixed diagnostic views, independent of the game camera.
scene = bpy.context.scene
scene.render.engine = 'BLENDER_EEVEE'
scene.render.resolution_x = 480
scene.render.resolution_y = 480
scene.render.resolution_percentage = 100
scene.world.color = (.18, .18, .18)
assert bpy.ops.object.light_add(type='AREA', location=(2, -3, 5)) == {'FINISHED'}
bpy.context.object.data.energy = 500
bpy.context.object.data.shape = 'DISK'
bpy.context.object.data.size = 4
assert bpy.ops.object.camera_add() == {'FINISHED'}
camera = bpy.context.object
scene.camera = camera
camera.data.type = 'ORTHO'
camera.data.ortho_scale = 1.9
for opened in [False, True]:
    bpy.data.objects['ChestClosed'].hide_render = opened
    bpy.data.objects['ChestOpen'].hide_render = not opened
    for name, position in [('front', (0, -3, .8)), ('side', (3, 0, .8)), ('back', (0, 3, .8)), ('three-quarter', (2,-3,2))]:
        camera.location = position
        camera.rotation_euler = (Vector((0,0,.45)) - camera.location).to_track_quat('-Z','Y').to_euler()
        scene.render.filepath = str(OUT / f'{"open" if opened else "closed"}-{name}.png')
        assert bpy.ops.render.render(write_still=True) == {'FINISHED'}
