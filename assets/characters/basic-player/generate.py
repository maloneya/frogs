"""Compile the accepted authored human body and articulated combat rig.

Blender metres, Z up, -Y forward. One opaque material, packed colour atlas,
one articulated skin and the eight app-owned clips. No runtime IK is required.
"""
from pathlib import Path
import math
import sys
import bpy
from mathutils import Euler, Vector

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))
import anatomy
from authored_cleave import bake
from motion import set_linear_keys
import authored_slam
import authored_locomotion

OUTPUT_DIRECTORY = Path('assets/characters/basic-player')
ATLAS_SIZE = 256
COLOURS = {
    'skin': (.40, .235, .15), 'cloth': (.105, .012, .018),
    'leather': (.055, .026, .013), 'steel': (.13, .17, .19),
    'edge': (.36, .40, .42), 'gold': (.44, .25, .065),
    'hair': (.022, .012, .008), 'dark': (.006, .009, .011),
    'fur': (.18, .14, .095), 'blade': (.42, .47, .50),
    'wrap': (.075, .038, .021), 'eye': (.40, .34, .24),
    'scar': (.27, .10, .07), 'boots': (.025, .02, .017),
    'gem': (.19, .012, .012), 'stitch': (.29, .19, .09),
}
TILES = {name: ((i % 4) / 4 + .012, (i // 4) / 4 + .012) for i, name in enumerate(COLOURS)}
PARTS = []


def clear_scene():
    bpy.ops.object.select_all(action='SELECT')
    assert bpy.ops.object.delete(use_global=False) == {'FINISHED'}
    for collection in [bpy.data.meshes, bpy.data.armatures, bpy.data.materials, bpy.data.images, bpy.data.actions]:
        for item in list(collection): collection.remove(item)
    PARTS.clear()


def make_material():
    image = bpy.data.images.new('SwordsmanColour', width=ATLAS_SIZE, height=ATLAS_SIZE, alpha=True)
    image.colorspace_settings.name='sRGB'
    colours = list(COLOURS.values())
    names = list(COLOURS)
    pixels = []
    for y in range(ATLAS_SIZE):
        for x in range(ATLAS_SIZE):
            index = x // 64 + 4 * (y // 64)
            u, v = (x % 64) / 63, (y % 64) / 63
            grain = .025 * math.sin(x * 1.9 + y * 2.7) * math.sin(y * .8 - x * .3)
            if names[index] in {'steel', 'blade', 'edge', 'gold'}:
                shade = .75 + .22 * math.cos((u - .5) * math.pi) + .10 * math.sin(v * 42) + grain
            else:
                shade = .82 + .16 * math.cos((u - .5) * math.pi) + .07 * math.sin(v * 12) + grain
            # Packed generated images retain these byte values. Encode the
            # linear palette here so Blender and the runtime sRGB texture both
            # decode the intended colour rather than darkening it twice.
            pixels.extend([encoded_channel(c * shade) for c in colours[index]] + [1])
    image.pixels = pixels
    image.pack()
    material = bpy.data.materials.new('SwordsmanMaterial')
    material.use_nodes = True
    material.use_backface_culling = True
    texture = material.node_tree.nodes.new('ShaderNodeTexImage')
    texture.image = image
    material.node_tree.links.new(texture.outputs['Color'], material.node_tree.nodes['Principled BSDF'].inputs['Base Color'])
    return material


def encoded_channel(linear):
    linear=max(0,min(1,linear))
    return 12.92*linear if linear <= .0031308 else 1.055*linear**(1/2.4)-.055


def mesh_part(name, vertices, faces, weights, material, tile, smooth=True):
    mesh = bpy.data.meshes.new(name + 'Mesh')
    mesh.from_pydata(vertices, [], faces)
    mesh.update()
    obj = bpy.data.objects.new(name, mesh)
    bpy.context.collection.objects.link(obj)
    mesh.materials.append(material)
    uv = mesh.uv_layers.new(name='UVMap')
    u, v = TILES[tile]
    for face in mesh.polygons:
        face.use_smooth = smooth
        for i, loop in enumerate(face.loop_indices):
            # Each face stays inside its material patch; no atlas boundary sampling.
            uv.data[loop].uv = (u + (i % 2) * .226, v + (i // 2 % 2) * .226)
    groups = {}
    for index, row in enumerate(weights):
        for bone, amount in row:
            if bone not in groups: groups[bone] = obj.vertex_groups.new(name=bone)
            groups[bone].add([index], amount, 'REPLACE')
    PARTS.append(obj)
    return obj


def tube(name, rings, material, tile, sides=12, smooth=True):
    """Elliptical sections follow the limb axis and blend across anatomical joints."""
    vertices, weights = [], []
    for index, (point, width, depth, row) in enumerate(rings):
        point = Vector(point)
        a = Vector(rings[max(0, index - 1)][0])
        b = Vector(rings[min(len(rings) - 1, index + 1)][0])
        axis = (b - a).normalized()
        across = Vector((1, 0, 0))
        if abs(axis.dot(across)) > .85: across = Vector((0, 1, 0))
        across = (across - axis * across.dot(axis)).normalized()
        around = axis.cross(across).normalized()
        for side in range(sides):
            angle = side / sides * math.tau
            vertices.append(point + across * (width * math.cos(angle)) + around * (depth * math.sin(angle)))
            weights.append(row)
    faces = [tuple(reversed(range(sides)))]
    for ring in range(len(rings) - 1):
        for side in range(sides):
            a, b = ring * sides + side, ring * sides + (side + 1) % sides
            faces.append((a, b, b + sides, a + sides))
    faces.append(tuple(range(len(vertices) - sides, len(vertices))))
    return mesh_part(name, vertices, faces, weights, material, tile, smooth)


def oval(name, centre, size, bone, material, tile, segments=12, bands=6):
    centre, size = Vector(centre), Vector(size)
    rings = []
    for i in range(bands + 1):
        angle = -.5 * math.pi + math.pi * i / bands
        radius = max(.02, math.cos(angle))
        rings.append((centre + Vector((0, 0, size.z * math.sin(angle))), size.x * radius, size.y * radius, [(bone, 1)]))
    return tube(name, rings, material, tile, segments)


def patch(name, points, bone, material, tile):
    return mesh_part(name, points, [tuple(range(len(points)))], [[(bone, 1)]] * len(points), material, tile, False)


def build_mesh(objects, material, rig):
    body = objects[0]
    vertices = [tuple(v.co) for v in body.data.vertices]
    faces = [tuple(f.vertices) for f in body.data.polygons]
    anatomy.skin_body(body)
    for obj in objects:
        obj.data.materials.clear()
        obj.data.materials.append(material)
        uv = obj.data.uv_layers.active
        for face in obj.data.polygons:
            centre = sum((obj.data.vertices[i].co for i in face.vertices), Vector()) / len(face.vertices)
            if obj != body:
                tile = 'eye'
            elif centre.z > 1.64 or (abs(centre.x) > .39 and centre.z > .86):
                tile = 'skin'
            elif centre.z > 1.1:
                tile = 'cloth'
            else:
                tile = 'leather'
            u, v = TILES[tile]
            for loop in face.loop_indices: uv.data[loop].uv = (u + .11, v + .11)
        if obj != body:
            obj.vertex_groups.new(name='Head').add(list(range(len(obj.data.vertices))), 1, 'REPLACE')
        PARTS.append(obj)
    build_sword(material, rig.data.bones['Weapon'].head_local)
    bpy.ops.object.select_all(action='DESELECT')
    for part in PARTS: part.select_set(True)
    bpy.context.view_layer.objects.active = body
    assert bpy.ops.object.join() == {'FINISHED'}
    body.name, body.data.name = 'Swordsman', 'SwordsmanMesh'
    while len(body.data.materials) > 1: body.data.materials.pop(index=1)
    for face in body.data.polygons: face.material_index = 0
    assert vertices == [tuple(v.co) for v in body.data.vertices[:len(vertices)]]
    assert faces == [tuple(f.vertices) for f in body.data.polygons[:len(faces)]]
    modifier = body.modifiers.new('SwordsmanSkin', 'ARMATURE')
    modifier.object = rig
    body.parent = rig
    return body


BLADE_TIP = 1.38


def build_sword(material, grip):
    def along(distance): return grip + Vector((0, -distance, 0))
    tube('SwordGrip', [(along(-.22), .032, .032, [('Weapon', 1)]),
        (along(.055), .032, .032, [('Weapon', 1)])], material, 'wrap', 10)
    oval('Pommel', along(-.25), (.055, .045, .055), 'Weapon', material, 'gold', 10, 4)
    tube('Crossguard', [(grip+Vector((-.22, -.08, -.015)), .025, .025, [('Weapon', 1)]),
        (grip+Vector((-.12, -.10, 0)), .035, .035, [('Weapon', 1)]),
        (grip+Vector((0, -.10, .015)), .035, .035, [('Weapon', 1)]),
        (grip+Vector((.12, -.10, 0)), .035, .035, [('Weapon', 1)]),
        (grip+Vector((.22, -.08, -.015)), .025, .025, [('Weapon', 1)])], material, 'gold', 8)
    vertices = []
    for distance, width in [(.12, .105), (.34, .095), (1.16, .078), (BLADE_TIP, .0)]:
        p = along(distance)
        vertices.extend([p+Vector((-width,0,0)), p+Vector((0,0,.026)),
                         p+Vector((width,0,0)), p+Vector((0,0,-.026))])
    faces = [(3,2,1,0)]
    for ring in range(3):
        for side in range(4):
            a,b=ring*4+side,ring*4+(side+1)%4
            faces.append((a,b,b+4,a+4))
    faces.append((12,13,14,15))
    mesh_part('SwordBlade', vertices, faces, [[('Weapon',1)]]*16, material, 'blade', False)
    patch('BladeFuller', [grip+Vector((-.014,-.22,.027)), grip+Vector((-.012,-1.14,.027)),
        grip+Vector((.012,-1.14,.027)), grip+Vector((.014,-.22,.027))], 'Weapon', material, 'steel')


def build_armature(scale, low):
    data = bpy.data.armatures.new('SwordsmanRig')
    rig = bpy.data.objects.new('SwordsmanRig', data)
    bpy.context.collection.objects.link(rig)
    bpy.context.view_layer.objects.active = rig
    rig.select_set(True)
    assert bpy.ops.object.mode_set(mode='EDIT') == {'FINISHED'}
    point = lambda v: Vector((v[0], v[1], v[2] - low)) * scale
    def bone(name, head, tail, parent=None, normalize=True):
        b = data.edit_bones.new(name)
        b.head, b.tail = (point(head), point(tail)) if normalize else (head, tail)
        b.parent = parent
        reference = Vector((0, -1, 0))
        if abs((b.tail - b.head).normalized().dot(reference)) > .9: reference = Vector((0, 0, 1))
        b.align_roll(reference)
        return b
    # Bind landmarks sit inside the accepted source anatomy in source metres.
    root = bone('Root', (0, 0, low), (0, 0, .86))
    pelvis = bone('Pelvis', (0, 0, .86), (0, 0, .99), root)
    spine = bone('Spine', (0, 0, .99), (0, 0, 1.16), pelvis)
    chest = bone('Chest', (0, 0, 1.16), (0, 0, 1.32), spine)
    neck = bone('Neck', (0, 0, 1.32), (0, -.015, 1.43), chest)
    bone('Head', (0, -.015, 1.43), (0, -.01, 1.68), neck)
    for side, sign in [('Left', -1), ('Right', 1)]:
        shoulder = bone(side + 'Shoulder', (0, 0, 1.30), (sign * .19, 0, 1.30), chest)
        arm = bone(side + 'Arm', (sign * .19, 0, 1.30), (sign * .30, -.005, 1.073), shoulder)
        fore = bone(side + 'Forearm', (sign * .30, -.005, 1.073), (sign * .384, -.068, .865), arm)
        hand = bone(side + 'Hand', (sign * .384, -.068, .865), (sign * .413, -.095, .79), fore)
        bone(side + 'Fingers', (sign * .413, -.095, .79), (sign * .431, -.107, .728), hand)
        bone(side + 'Thumb', (sign * .367, -.11, .85), (sign * .37, -.167, .815), hand)
        leg = bone(side + 'Leg', (sign * .095, .015, .855), (sign * .13, -.02, .457), pelvis)
        shin = bone(side + 'Shin', (sign * .13, -.02, .457), (sign * .183, .017, .065), leg)
        bone(side + 'Foot', (sign * .183, .017, .065), (sign * .235, -.115, .017), shin)
        if side == 'Right':
            grip = hand.head.copy()
            weapon = bone('Weapon', grip, grip + Vector((0, -1, 0)), hand, False)
            bone('BeamOrigin', grip + Vector((0, -BLADE_TIP, 0)), grip + Vector((0, -BLADE_TIP - .1, 0)), weapon, False)
    assert bpy.ops.object.mode_set(mode='OBJECT') == {'FINISHED'}
    rig.show_in_front = True
    return rig


def rotate_global(rig, name, direction):
    joint=rig.pose.bones[name]
    rest=joint.bone.matrix_local.to_quaternion()
    desired=(joint.bone.tail_local-joint.bone.head_local).rotation_difference(Vector(direction)) @ rest
    parent_rest=joint.parent.bone.matrix_local.to_quaternion()
    local_rest=parent_rest.inverted() @ rest
    joint.rotation_quaternion=local_rest.inverted() @ joint.parent.matrix.to_quaternion().inverted() @ desired
    bpy.context.view_layer.update()


def chain(rig, upper, lower, goal, pole):
    """Author-time two-link IK, baked to the existing quaternion clip format."""
    start=rig.pose.bones[upper].head.copy()
    delta=Vector(goal)-start
    a,b=rig.data.bones[upper].length,rig.data.bones[lower].length
    distance=max(.02,min(delta.length,a+b-.002))
    axis=delta.normalized()
    lateral=Vector(pole)-start
    lateral=(lateral-axis*lateral.dot(axis)).normalized()
    projection=(a*a-b*b+distance*distance)/(2*distance)
    elbow=start+axis*projection+lateral*math.sqrt(max(0,a*a-projection*projection))
    end=start+axis*distance
    rotate_global(rig,upper,elbow-start)
    rotate_global(rig,lower,end-elbow)


def pose(rig, settings):
    for b in rig.pose.bones:
        b.rotation_mode='QUATERNION'
        b.rotation_quaternion=(1,0,0,0)
        b.location=(0,0,0)
    for name, angles in settings['rotations'].items():
        rig.pose.bones[name].rotation_quaternion=Euler(tuple(math.radians(a) for a in angles),'XYZ').to_quaternion()
    rig.pose.bones['Root'].location=rig.data.bones['Root'].matrix_local.to_quaternion().inverted() @ Vector((0,0,-.025))
    bpy.context.view_layer.update()
    wrist=Vector(settings['hand'])
    blade=Vector(settings['blade']).normalized()
    chain(rig,'RightArm','RightForearm',wrist,(.88,-.04,1.40))
    # Each arm has its own reach and elbow plane. Hand flex and the Weapon
    # child at the wrist remain separate from the shoulder/forearm swing.
    rotate_global(rig,'RightHand',(.25,-.12,-1))
    rotate_global(rig,'Weapon',blade)
    chain(rig,'LeftArm','LeftForearm',(-.49,-.12,1.10),(-.83,.08,1.33))
    rotate_global(rig,'LeftHand',(-.15,-.10,-1))
    for side, sign in [('Left',-1),('Right',1)]:
        goal=(sign*.27, -.06 if sign<0 else .10, .14)
        chain(rig,side+'Leg',side+'Shin',goal,(sign*.25,-1,.55))
        rotate_global(rig,side+'Foot',(sign*.14,-1,-.25))
    bpy.context.view_layer.update()


def action(rig, name, keys):
    a=bpy.data.actions.new(name)
    a.use_fake_user=True
    rig.animation_data.action=a
    # Bake the ready-pose reaches at half-frame spacing.
    frames=[keys[0][0]+i*.5 for i in range(round((keys[-1][0]-keys[0][0])*2)+1)]
    for frame in frames:
        left=next(i for i in range(len(keys)-1) if keys[i][0] <= frame <= keys[i+1][0])
        a_frame,a_settings=keys[left]
        b_frame,b_settings=keys[left+1]
        amount=(frame-a_frame)/(b_frame-a_frame)
        settings=mix(a_settings,b_settings,amount)
        pose(rig,settings)
        for bone in rig.pose.bones:
            bone.rotation_quaternion.normalize()
            bone.keyframe_insert(data_path='rotation_quaternion',frame=frame)
            bone.keyframe_insert(data_path='location',frame=frame)
    set_linear_keys(a)
    return a


def mix(a,b,amount):
    result={}
    for key in ['hand','blade']:
        result[key]=tuple(Vector(a[key]).lerp(Vector(b[key]),amount))
    result['rotations']={name:tuple(Vector(a['rotations'][name]).lerp(
        Vector(b['rotations'][name]),amount)) for name in sorted(a['rotations'])}
    return result


def build_actions(rig):
    bpy.context.scene.render.fps=30
    rig.animation_data_create()
    ready={'hand':(.50,-.32,1.24),'blade':(.40,-.83,.39),
           'rotations':{'Spine':(6,0,0),'Chest':(-3,8,0),'Neck':(-3,-8,0),
                        'RightShoulder':(0,-5,-6),'LeftShoulder':(0,3,4)}}
    breathe=mix(ready,{**ready,'rotations':{**ready['rotations'],'Chest':(-1,10,0)}},.5)
    idle=action(rig,'Idle',[(1,ready),(16,breathe),(31,ready)])
    authored_locomotion.bake(rig, lambda: pose(rig, ready))
    bake(rig,lambda:pose(rig,ready))
    authored_slam.bake(rig, lambda: pose(rig, ready), chain, rotate_global)
    channel={**ready,'hand':(.32,-.64,1.48),'blade':(0,-1,0)}
    action(rig,'Disintegrate',[(1,channel),(31,channel)])
    rig.animation_data.action=idle
    bpy.context.scene.frame_set(1)


def main():
    arguments=sys.argv[sys.argv.index('--')+1:] if '--' in sys.argv else []
    out=Path(arguments[0]) if arguments else OUTPUT_DIRECTORY
    clear_scene()
    objects, scale, low = anatomy.import_body()
    rig = build_armature(scale, low)
    build_mesh(objects, make_material(), rig)
    build_actions(rig)
    out.mkdir(parents=True,exist_ok=True)
    assert bpy.ops.wm.save_as_mainfile(filepath=str((out/'basic-player.blend').resolve()))=={'FINISHED'}
    bpy.ops.object.select_all(action='SELECT')
    assert bpy.ops.export_scene.gltf(filepath=str((out/'basic-player.glb').resolve()),export_format='GLB',
        export_apply=False,export_cameras=False,export_lights=False,export_animations=True,
        export_animation_mode='ACTIONS',export_nla_strips=False,export_force_sampling=False)=={'FINISHED'}


if __name__=='__main__':main()
