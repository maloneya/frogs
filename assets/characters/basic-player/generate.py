"""Compile the accepted authored human body and articulated combat rig.

Blender metres, Z up, -Y forward. Body/sword and fitted clothing have separate
opaque atlases, one articulated skin and eight app-owned clips. No runtime IK.
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
import authored_idle
import grasp

OUTPUT_DIRECTORY = Path('assets/characters/basic-player')
ATLAS_SIZE = 256
COLOURS = {
    'skin': (.40, .235, .15), 'cloth': (.018, .023, .025),
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
    weapon = rig.data.bones['Weapon']
    build_sword(material, weapon.head_local, (weapon.tail_local - weapon.head_local).normalized())
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
    source = Path(__file__).resolve().parent / 'clothing-source.blend'
    with bpy.data.libraries.load(str(source), link=False) as (_, clothing):
        clothing.objects = ['HoodedCloak']
    cloak = clothing.objects[0]
    assert cloak is not None, 'clothing source must contain HoodedCloak'
    bpy.context.collection.objects.link(cloak)
    modifier = cloak.modifiers.new('CloakSkin', 'ARMATURE')
    modifier.object = rig
    cloak.parent = rig
    return body


BLADE_TIP = 1.22


def build_sword(material, grip, axis):
    parts_start = len(PARTS)
    def along(distance): return grip + Vector((0, -distance, 0))
    weight = [('Weapon', 1)]
    radius = grasp.HANDLE_RADIUS
    # Keep the lower grip at the slam's supporting-hand position, 15 cm
    # behind the primary palm grasp. The hilt fits the accepted human hand scale.
    tube('SwordGrip', [(along(-.215), radius * 19/21, radius * 16/21, weight),
        (along(-.19), radius, radius * 17/21, weight),
        (along(.035), radius * 18/21, radius * 15/21, weight),
        (along(.05), radius * 18/21, radius * 15/21, weight)], material, 'wrap', 10)
    for name, start, end in [('PommelCollar', -.225, -.21), ('GuardCollar', .04, .065)]:
        tube(name, [(along(start), .022, .018, weight),
            (along(end), .022, .018, weight)], material, 'gold', 10)
    oval('Pommel', along(-.245), (.037, .016, .037), 'Weapon', material, 'steel', 12, 6)
    tube('Crossguard', [(grip+Vector((-.135, -.09, -.008)), .010, .010, weight),
        (grip+Vector((-.11, -.075, 0)), .012, .012, weight),
        (grip+Vector((0, -.075, 0)), .017, .014, weight),
        (grip+Vector((.11, -.075, 0)), .012, .012, weight),
        (grip+Vector((.135, -.09, -.008)), .010, .010, weight)], material, 'steel', 8)
    vertices = []
    # A recessed fuller on both faces is part of the closed blade, rather
    # than a floating one-sided patch. Distal taper keeps its edge thin.
    sections = [(.085, .028, .0045), (.16, .027, .0045),
                (.78, .022, .0035), (1.04, .017, .0025), (1.16, .007, .0015)]
    for distance, width, depth in sections:
        p = along(distance)
        vertices.extend(p + Vector((x * width, 0, z * depth)) for x, z in [
            (-1, 0), (-.65, 1), (-.18, 1), (0, .55), (.18, 1), (.65, 1),
            (1, 0), (.65, -1), (.18, -1), (0, -.55), (-.18, -1), (-.65, -1)])
    sides = 12
    faces = [tuple(range(sides))]
    for ring in range(len(sections) - 1):
        for side in range(sides):
            a, b = ring * sides + side, ring * sides + (side + 1) % sides
            faces.append((b, a, a + sides, b + sides))
    tip = len(vertices)
    vertices.append(along(BLADE_TIP))
    for side in range(sides):
        faces.append(((len(sections) - 1) * sides + (side + 1) % sides,
                      (len(sections) - 1) * sides + side, tip))
    blade = mesh_part('SwordBlade', vertices, faces, [weight] * len(vertices), material, 'blade', False)
    for face in blade.data.polygons[1:]:
        tile = 'steel' if (face.index - 1) % sides in {2, 3, 8, 9} else 'blade'
        u, v = TILES[tile]
        for i, loop in enumerate(face.loop_indices):
            blade.data.uv_layers.active.data[loop].uv = (u + (i % 2) * .226, v + (i // 2 % 2) * .226)
    rotation = Vector((0,-1,0)).rotation_difference(axis)
    for part in PARTS[parts_start:]:
        for vertex in part.data.vertices: vertex.co = grip + rotation @ (vertex.co - grip)


def build_armature(scale, low, hands):
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
        leg = bone(side + 'Leg', (sign * .095, .015, .855), (sign * .13, -.02, .457), pelvis)
        shin = bone(side + 'Shin', (sign * .13, -.02, .457), (sign * .183, .017, .065), leg)
        bone(side + 'Foot', (sign * .183, .017, .065), (sign * .235, -.115, .017), shin)
    weapon = grasp.add_bones(rig, hands, bone)
    grip = weapon.head.copy()
    axis = (weapon.tail - weapon.head).normalized()
    bone('BeamOrigin', grip + axis * BLADE_TIP, grip + axis * (BLADE_TIP + .1), weapon, False)
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
    chain(rig,'RightArm','RightForearm',wrist,(.52,.06,1.25))
    # Each arm supplies reach and an elbow plane. The final grasp pass derives
    # wrist orientation and finger closure from the requested blade direction.
    rotate_global(rig,'RightHand',(.25,-.12,-1))
    rotate_global(rig,'Weapon',blade)
    chain(rig,'LeftArm','LeftForearm',(-.34,-.08,1.02),(-.48,.10,1.24))
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
    ready={'hand':(.40,-.24,1.06),'blade':(.38,-.90,.14),
           'rotations':{'Spine':(2,0,0),'Chest':(-1,3,0),'Neck':(-1,-3,0),
                        'RightShoulder':(0,-2,-2),'LeftShoulder':(0,1,1)}}
    idle=authored_idle.bake(rig, lambda: pose(rig, ready), chain, rotate_global)
    # Moving carry keeps the blade elevated for clearance during the stride.
    traveling_guard={**ready,'blade':(.40,-.83,.39)}
    authored_locomotion.bake(rig, lambda: pose(rig, traveling_guard))
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
    hands = grasp.calibrate(objects[0])
    rig = build_armature(scale, low, hands)
    body = build_mesh(objects, make_material(), rig)
    build_actions(rig)
    grasp.bake(rig, hands, chain)
    grasp.validate_motion(rig, body)
    rig.animation_data.action = bpy.data.actions['Idle']
    rig.animation_data.action_slot = rig.animation_data.action.slots[0]
    bpy.context.scene.frame_set(1)
    out.mkdir(parents=True,exist_ok=True)
    assert bpy.ops.wm.save_as_mainfile(filepath=str((out/'basic-player.blend').resolve()))=={'FINISHED'}
    bpy.ops.object.select_all(action='SELECT')
    assert bpy.ops.export_scene.gltf(filepath=str((out/'basic-player.glb').resolve()),export_format='GLB',
        export_apply=False,export_cameras=False,export_lights=False,export_animations=True,
        export_animation_mode='ACTIONS',export_nla_strips=False,export_force_sampling=False)=={'FINISHED'}


if __name__=='__main__':main()
