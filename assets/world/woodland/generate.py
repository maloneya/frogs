"""Compile simple faceted woodland props; metres, Z up, one palette material."""
from pathlib import Path
import math
import sys
import bpy
from mathutils import Vector

OUTPUT = Path(sys.argv[sys.argv.index('--') + 1]) if '--' in sys.argv else Path(__file__).resolve().parent
OUTPUT.mkdir(parents=True, exist_ok=True)
PALETTE = [(0.25, 0.13, 0.065), (0.48, 0.29, 0.13), (0.34, 0.36, 0.33),
           (0.43, 0.44, 0.39), (0.12, 0.28, 0.17), (0.20, 0.37, 0.20),
           (0.32, 0.44, 0.23), (0.10, 0.08, 0.06)]


def begin():
    for obj in list(bpy.data.objects):
        bpy.data.objects.remove(obj, do_unlink=True)
    # Names and datablocks are independent of assets exported earlier in this run.
    for blocks in (bpy.data.meshes, bpy.data.materials, bpy.data.images):
        for block in list(blocks):
            if block.users == 0:
                blocks.remove(block)
    image = bpy.data.images.new('WoodlandPalette', width=64, height=8, alpha=True)
    image.pixels = [v for y in range(8) for x in range(64) for v in (*PALETTE[x // 8], 1)]
    image.pack()
    mat = bpy.data.materials.new('WoodlandPalette')
    mat.use_nodes = True
    mat.use_backface_culling = True
    tex = mat.node_tree.nodes.new('ShaderNodeTexImage')
    tex.image = image
    mat.node_tree.links.new(tex.outputs['Color'], mat.node_tree.nodes.get('Principled BSDF').inputs['Base Color'])
    return mat


def finish_piece(obj, colour):
    assert bpy.ops.object.transform_apply(location=True, rotation=True, scale=True) == {'FINISHED'}
    uv = obj.data.uv_layers.active or obj.data.uv_layers.new(name='UVMap')
    for loop in uv.data:
        loop.uv = ((colour + 0.5) / 8, 0.5)
    return obj


def branch(a, b, radius, tip, colour=0, vertices=7):
    a, b = Vector(a), Vector(b)
    assert bpy.ops.mesh.primitive_cone_add(vertices=vertices, radius1=radius, radius2=tip,
        depth=(b-a).length, location=(a+b)/2) == {'FINISHED'}
    obj = bpy.context.object
    obj.rotation_euler = (b-a).to_track_quat('Z', 'Y').to_euler()
    return finish_piece(obj, colour)


def rock(pos, scale, colour):
    assert bpy.ops.mesh.primitive_ico_sphere_add(subdivisions=1, radius=1, location=pos) == {'FINISHED'}
    obj = bpy.context.object
    obj.scale = scale
    return finish_piece(obj, colour)


def export(name, mat):
    assert bpy.ops.object.select_all(action='SELECT') == {'FINISHED'}
    bpy.context.view_layer.objects.active = bpy.context.selected_objects[0]
    assert bpy.ops.object.join() == {'FINISHED'}
    obj = bpy.context.object
    obj.name = name
    obj.data.materials.clear()
    obj.data.materials.append(mat)
    for face in obj.data.polygons:
        face.material_index = 0
    assert bpy.ops.wm.save_as_mainfile(filepath=str(OUTPUT / f'{name}.blend')) == {'FINISHED'}
    assert bpy.ops.export_scene.gltf(filepath=str(OUTPUT / f'{name}.glb'), export_format='GLB',
        use_selection=True, export_cameras=False, export_lights=False,
        export_animations=False, export_image_format='AUTO') == {'FINISHED'}


mat = begin()
# Broad, shallow ash bed and a deliberately irregular ring of twelve stones.
branch((0, 0, 0), (0, 0, 0.07), 0.63, 0.63, 7, 12)
for i in range(12):
    angle = i * math.tau / 12
    rock((0.72*math.cos(angle), 0.72*math.sin(angle), 0.17 + 0.015*(i % 3)),
         (0.23, 0.20, 0.17 + 0.015*(i % 3)), 2 + i % 2)
for a, b in [((-0.46, -0.28, 0.20), (0.46, 0.28, 0.25)),
             ((-0.46, 0.28, 0.28), (0.46, -0.28, 0.34)),
             ((0, -0.49, 0.35), (0, 0.49, 0.39))]:
    branch(a, b, 0.115, 0.10)
    direction = (Vector(b)-Vector(a)).normalized()
    branch(Vector(b), Vector(b)+direction*0.006, 0.085, 0.085, 1)
    branch(Vector(a)-direction*0.006, Vector(a), 0.095, 0.095, 1)
export('campfire', mat)

mat = begin()
branch((0, 0, 0), (0, 0, 3.8), 0.23, 0.07)
for bottom, top, radius, colour in [(0.85, 2.75, 1.25, 4), (1.65, 3.5, 1.0, 5), (2.55, 4.35, 0.7, 4)]:
    branch((0, 0, bottom), (0, 0, top), radius, 0, colour, 9)
export('pine', mat)

mat = begin()
branch((0, 0, 0), (0.10, 0, 2.9), 0.28, 0.12)
for end in [(-0.85, 0, 2.8), (0.85, 0.3, 3), (0.2, -0.8, 2.7)]:
    branch((0, 0, 1.55), end, 0.14, 0.055)
for pos, scale, colour in [((0, 0, 3.5), (1.1, 1.0, 1.05), 5),
    ((-0.85, 0, 3), (1.0, 0.9, 0.9), 4), ((0.85, 0.3, 3.15), (0.95, 0.9, 1), 6),
    ((0.2, -0.75, 2.95), (0.9, 0.9, 0.85), 5)]:
    rock(pos, scale, colour)
export('broadleaf', mat)

mat = begin()
branch((0, 0, 0), (0.13, 0, 2), 0.25, 0.16)
branch((0.13, 0, 2), (-0.12, 0.1, 3.65), 0.16, 0.035)
for a, b, c in [((0.08, 0, 1.25), (-0.8, 0, 2.15), (-1.1, 0.1, 2.8)),
    ((0.1, 0, 1.9), (0.85, 0.15, 2.5), (1.05, 0.18, 3.2)),
    ((0, 0, 2.5), (0.15, -0.7, 3), (0.4, -0.95, 3.45))]:
    branch(a, b, 0.11, 0.065)
    branch(b, c, 0.065, 0.012)
    branch(b, Vector(b)+Vector((0.35, 0.25, 0.35)), 0.045, 0.008)
export('bare', mat)
