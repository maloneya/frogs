"""Author the fitted cloak source in bind-space metres (Z up, -Y forward).

The player compiler appends clothing-source.blend; it does not recreate it.
This script records the initial sewn panels, folds and body-fitted sleeves.
"""
import math
from pathlib import Path
import sys

import bpy
from mathutils import Vector

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import anatomy

PARTS = []
PALETTE = [(0.033, 0.039, 0.042), (0.012, 0.016, 0.018),
           (0.028, 0.023, 0.016), (0.15, 0.13, 0.085)]


def material():
    image = bpy.data.images.new('WovenCharcoal', width=256, height=256, alpha=True)
    image.colorspace_settings.name = 'sRGB'
    pixels = []
    for y in range(256):
        for x in range(256):
            colour = PALETTE[x // 128 + 2 * (y // 128)]
            weave = 1 + .045 * math.sin(x * math.pi / 2) * math.sin(y * math.pi / 2)
            grain = .025 * math.sin(x * 2.13 + y * 3.71)
            for channel in colour:
                value = channel * (weave + grain)
                pixels.append(12.92 * value if value <= .0031308 else 1.055 * value ** (1 / 2.4) - .055)
            pixels.append(1)
    image.pixels = pixels
    image.pack()
    mat = bpy.data.materials.new('CharcoalCloth')
    mat.use_nodes = True
    mat.use_backface_culling = True
    bsdf = mat.node_tree.nodes['Principled BSDF']
    bsdf.inputs['Roughness'].default_value = 1
    texture = mat.node_tree.nodes.new('ShaderNodeTexImage')
    texture.image = image
    mat.node_tree.links.new(texture.outputs['Color'], bsdf.inputs['Base Color'])
    return mat


def mesh(name, vertices, faces, weights, mat, tile=0, thickness=.008):
    data = bpy.data.meshes.new(name + 'Mesh')
    data.from_pydata(vertices, [], faces)
    data.update()
    obj = bpy.data.objects.new(name, data)
    bpy.context.collection.objects.link(obj)
    data.materials.append(mat)
    uv = data.uv_layers.new(name='UVMap')
    for face in data.polygons:
        face.use_smooth = True
        for loop in face.loop_indices:
            point = data.vertices[data.loops[loop].vertex_index].co
            uv.data[loop].uv = ((tile % 2) * .5 + .035 + (point.x + 1) % 1 * .43,
                               (tile // 2) * .5 + .035 + point.z % 1 * .43)
    groups = {}
    for i, row in enumerate(weights):
        for name, weight in row:
            if name not in groups:
                groups[name] = obj.vertex_groups.new(name=name)
            groups[name].add([i], weight, 'REPLACE')
    bpy.context.view_layer.objects.active = obj
    obj.select_set(True)
    if thickness:
        solid = obj.modifiers.new('SewnThickness', 'SOLIDIFY')
        solid.thickness = thickness
        solid.offset = 0
        assert bpy.ops.object.modifier_apply(modifier=solid.name) == {'FINISHED'}
    obj.select_set(False)
    PARTS.append(obj)
    return obj


def panels(name, rows, mat, tile=0, reverse=False):
    vertices, weights = [], []
    for points, row_weights in rows:
        vertices.extend(points)
        weights.extend(row_weights)
    count = len(rows[0][0])
    faces = []
    for row in range(len(rows) - 1):
        for col in range(count - 1):
            a = row * count + col
            face = (a, a + 1, a + count + 1, a + count)
            faces.append(tuple(reversed(face)) if reverse else face)
    return mesh(name, vertices, faces, weights, mat, tile)


def cloak(mat):
    rows = []
    # The front slit stays open; the long skirt hangs behind the legs instead
    # of borrowing knee weights, which would split the cloth on every stride.
    sections = [(.24, .44, .34, .88), (.45, .43, .32, .88),
                (.72, .38, .29, .84), (1.00, .34, .26, .80),
                (1.20, .31, .23, .79), (1.40, .29, .22, .85),
                (1.51, .33, .20, .99), (1.61, .30, .19, .85),
                (1.69, .17, .15, .60)]
    for z, width, depth, opening in sections:
        points, weights = [], []
        chest = max(0, min(1, (z - 1.1) / .43))
        for col in range(65):
            angle = opening + (math.tau - 2 * opening) * col / 64
            fold = (.012 + .016 * (1 - z / 1.8)) * math.cos(angle * 14 + z * .6)
            points.append(((width + fold) * math.sin(angle),
                           .055 - (depth + fold) * math.cos(angle),
                           z + .012 * math.sin(angle * 7) * max(0, 1 - z)))
            weights.append([('Chest', .85 * chest), ('Spine', .15 * chest),
                            ('Pelvis', 1 - chest)])
        rows.append((points, weights))
    panels('OpenCloak', rows, mat)
    # Layered diagonal folds give the shoulder wrap its own silhouette.
    for layer in range(3):
        rows = []
        for row in range(5):
            points = []
            for col in range(41):
                angle = math.tau * col / 40
                width = .20 + .105 * row / 4
                depth = .165 + .055 * row / 4
                z = 1.70 - .075 * layer - .13 * row / 4
                z += .065 * math.sin(angle + .6 + layer * .3) + .012 * math.cos(row / 4 * math.pi * 2)
                points.append((width * math.sin(angle), -.016 - depth * math.cos(angle), z))
            rows.append((points, [[('Chest', .85), ('Neck', .15)]] * len(points)))
        panels('ShoulderFold' + str(layer), rows, mat, reverse=True)


def hood(mat):
    rows = []
    for z, width, depth, opening in [
        (1.65, .14, .16, .38), (1.72, .19, .21, .72),
        (1.82, .215, .235, .83), (1.92, .205, .24, .62),
        (2.00, .145, .18, .25), (2.035, .065, .085, .04),
    ]:
        points = []
        for col in range(49):
            angle = opening + (math.tau - 2 * opening) * col / 48
            fold = .006 * math.cos(angle * 8 + z * 6)
            points.append(((width + fold) * math.sin(angle),
                           -.03 - (depth + fold) * math.cos(angle), z))
        rows.append((points, [[('Head', 1)]] * len(points)))
    obj = panels('RaisedHood', rows, mat)
    # Close the crown while preserving the open face and a thick visible rim.
    points = rows[-1][0] + [(0, -.03, 2.055)]
    mesh('HoodCrown', points, [(i, i + 1, len(points) - 1) for i in range(48)],
         [[('Head', 1)]] * len(points), mat)
    for edge in [0, -1]:
        points = []
        for row, _ in rows:
            p = Vector(row[edge])
            points.extend([p + Vector((-.007, -.009, 0)), p + Vector((.007, -.009, 0))])
        mesh('HoodBinding', points,
             [(i, i + 1, i + 3, i + 2) for i in range(0, len(points) - 2, 2)],
             [[('Head', 1)]] * len(points), mat, tile=2)
    return obj


def fitted_underlayer(body, mat):
    # Retain the accepted body's surface/weights; the actual overlying shell
    # has thickness and ends at sewn cuffs, a neckline and boot tops.
    groups = {g.index: g.name for g in body.vertex_groups}
    vertices, weights, faces = [], [], []
    remap = {}
    regions = body.data.attributes['.sculpt_face_set']
    for face in body.data.polygons:
        centre = face.center
        region = regions.data[face.index].value
        # End sleeves at the authored wrist boundary. A coordinate cutoff
        # accidentally included the thumb web in the sleeve and the grasp.
        cover = .30 < centre.z < 1.60 and region not in {9, 10} and region < 64
        if not cover:
            continue
        for i in face.vertices:
            if i not in remap:
                vertex = body.data.vertices[i]
                remap[i] = len(vertices)
                vertices.append(vertex.co + vertex.normal * .014)
                weights.append([(groups[g.group], g.weight) for g in vertex.groups])
        faces.append(tuple(remap[i] for i in face.vertices))
    mesh('TravelTunicAndLeggings', vertices, faces, weights, mat, tile=1, thickness=.005)
    # A real raised leather belt reads across the open cloak front.
    rows = []
    for z in [1.12, 1.165]:
        points = [(.205 * math.sin(i * math.tau / 40),
                   -.025 - .17 * math.cos(i * math.tau / 40), z) for i in range(41)]
        rows.append((points, [[('Pelvis', .7), ('Spine', .3)]] * len(points)))
    panels('LeatherBelt', rows, mat, tile=2, reverse=True)


def boots(mat):
    for side, sign in [('Left', -1), ('Right', 1)]:
        rows = []
        for z, x, y, width, depth, foot in [
            (.018, .245, -.02, .10, .165, 1),
            (.08, .245, -.02, .10, .165, 1),
            (.15, .235, -.01, .095, .15, .9),
            (.22, .215, .02, .093, .105, .3),
            (.38, .20, .012, .098, .10, 0),
        ]:
            points = [(sign * x + width * math.sin(i * math.tau / 32),
                       y - depth * math.cos(i * math.tau / 32), z) for i in range(33)]
            rows.append((points, [[(side + 'Foot', foot), (side + 'Shin', 1 - foot)]] * len(points)))
        panels(side + 'Boot', rows, mat, tile=2)
        mesh(side + 'BootSole', rows[0][0], [tuple(reversed(range(32)))],
             rows[0][1], mat, tile=1)


def main():
    args = sys.argv[sys.argv.index('--') + 1:] if '--' in sys.argv else []
    output = Path(args[0]) if args else HERE / 'clothing-source.blend'
    bpy.ops.wm.read_factory_settings(use_empty=True)
    body_objects, _, _ = anatomy.import_body()
    body = body_objects[0]
    anatomy.skin_body(body)
    mat = material()
    fitted_underlayer(body, mat)
    boots(mat)
    cloak(mat)
    hood(mat)
    for obj in body_objects:
        bpy.data.objects.remove(obj, do_unlink=True)
    bpy.ops.object.select_all(action='DESELECT')
    for obj in PARTS:
        obj.select_set(True)
    bpy.context.view_layer.objects.active = PARTS[0]
    assert bpy.ops.object.join() == {'FINISHED'}
    obj = bpy.context.object
    obj.name, obj.data.name = 'HoodedCloak', 'HoodedCloakMesh'
    while len(obj.data.materials) > 1:
        obj.data.materials.pop(index=1)
    for face in obj.data.polygons:
        face.material_index = 0
    output.parent.mkdir(parents=True, exist_ok=True)
    assert bpy.ops.wm.save_as_mainfile(filepath=str(output.resolve())) == {'FINISHED'}


if __name__ == '__main__':
    main()
