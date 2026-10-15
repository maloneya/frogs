"""Render matched character diagnostics without modifying the authored assets."""
import argparse
import hashlib
import json
import math
from pathlib import Path
import sys
from urllib.parse import quote

import bmesh
import bpy
from mathutils import Quaternion, Vector

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from preview_timing import sampling

VIEWS = {'game': (5, -5, 6), 'front': (0, -6, 1),
         'side': (6, 0, 1), 'opposite-side': (-6, 0, 1), 'back': (0, 6, 1)}
GRIP_VIEWS = ('grip-palm', 'grip-back')
MODES = ('equipped', 'plain', 'skeleton')


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def material(name, color):
    mat = bpy.data.materials.new(name)
    mat.diffuse_color = (*color, 1)
    mat.use_nodes = True
    shader = mat.node_tree.nodes.get('Principled BSDF')
    shader.inputs['Base Color'].default_value = (*color, 1)
    shader.inputs['Roughness'].default_value = .75
    return mat


def setup(resolution):
    scene = bpy.context.scene
    for obj in list(bpy.data.objects):
        if obj.type in ('CAMERA', 'LIGHT'):
            bpy.data.objects.remove(obj, do_unlink=True)
    scene.render.engine = 'CYCLES'
    scene.cycles.samples = 4
    scene.cycles.seed = 0
    scene.cycles.use_animated_seed = False
    scene.render.resolution_x = scene.render.resolution_y = resolution
    scene.render.resolution_percentage = 100
    scene.render.image_settings.file_format = 'PNG'
    scene.render.image_settings.color_mode = 'RGB'
    scene.render.use_compositing = scene.render.use_sequencer = False
    scene.render.film_transparent = False
    scene.view_settings.view_transform = 'Standard'
    scene.view_settings.exposure, scene.view_settings.gamma = 0, 1
    scene.world = bpy.data.worlds.new('PreviewWorld')
    scene.world.color = (.10, .10, .10)
    for location, power, size in [((3, -4, 5), 650, 5), ((-3, -1, 3), 350, 4), ((0, 3, 4), 450, 3)]:
        light = bpy.data.lights.new('PreviewLight', 'AREA')
        light.energy, light.size = power, size
        obj = bpy.data.objects.new('PreviewLight', light)
        scene.collection.objects.link(obj)
        obj.location = location
        obj.rotation_euler = (Vector((0, 0, 1)) - obj.location).to_track_quat('-Z', 'Y').to_euler()
    camera = bpy.data.objects.new('PreviewCamera', bpy.data.cameras.new('PreviewCamera'))
    scene.collection.objects.link(camera)
    scene.camera = camera
    camera.data.type = 'ORTHO'
    # A shared two-metre ground grid makes support and body travel readable.
    bpy.ops.mesh.primitive_plane_add(size=200)
    floor = bpy.context.object
    floor.name = 'PreviewFloor'
    mat = material('PreviewFloor', (.08, .09, .10))
    nodes = mat.node_tree.nodes
    geometry = nodes.new('ShaderNodeNewGeometry')
    separate = nodes.new('ShaderNodeSeparateXYZ')
    mat.node_tree.links.new(geometry.outputs['Position'], separate.inputs[0])
    lines = []
    for axis in ('X', 'Y'):
        ping = nodes.new('ShaderNodeMath')
        ping.operation = 'PINGPONG'
        ping.inputs[1].default_value = 1
        mat.node_tree.links.new(separate.outputs[axis], ping.inputs[0])
        line = nodes.new('ShaderNodeMath')
        line.operation = 'LESS_THAN'
        line.inputs[1].default_value = .008
        mat.node_tree.links.new(ping.outputs[0], line.inputs[0])
        lines.append(line)
    maximum = nodes.new('ShaderNodeMath')
    maximum.operation = 'MAXIMUM'
    for i, line in enumerate(lines):
        mat.node_tree.links.new(line.outputs[0], maximum.inputs[i])
    mix = nodes.new('ShaderNodeMixRGB')
    mix.inputs[1].default_value = (.08, .09, .10, 1)
    mix.inputs[2].default_value = (.18, .20, .22, 1)
    mat.node_tree.links.new(maximum.outputs[0], mix.inputs[0])
    mat.node_tree.links.new(mix.outputs[0], nodes['Principled BSDF'].inputs['Base Color'])
    floor.data.materials.append(mat)
    return scene


def source_sword(rig):
    hand = rig.data.bones.get('hand_r')
    if hand is None:
        return
    axis = hand.matrix_local.to_3x3().inverted() @ (
        rig.data.bones['index_01_r'].head_local - rig.data.bones['pinky_01_r'].head_local).normalized()
    for name, start, end, radius in [('Handle', -.16, .07, .022), ('Blade', .10, 1.15, .035)]:
        bpy.ops.mesh.primitive_cone_add(vertices=8, radius1=radius,
            radius2=radius if name == 'Handle' else 0, depth=end - start)
        obj = bpy.context.object
        obj.name, obj.parent, obj.parent_type, obj.parent_bone = 'Reference' + name, rig, 'BONE', 'hand_r'
        obj.location = Vector((0, .04, 0)) + axis * (start + end) * .5 - Vector((0, hand.length, 0))
        obj.rotation_euler = axis.to_track_quat('Z', 'Y').to_euler()


def load(path, native):
    if native:
        bpy.ops.wm.read_factory_settings(use_empty=True)
        assert bpy.ops.import_scene.gltf(filepath=str(path)) == {'FINISHED'}
        rigs = [obj for obj in bpy.data.objects if obj.type == 'ARMATURE']
        if len(rigs) != 1:
            raise ValueError('Native preview requires exactly one armature')
        rig = rigs[0]
        points = [obj.matrix_world @ vertex.co for obj in bpy.data.objects if obj.type == 'MESH'
                  for vertex in obj.data.vertices]
        scale = 2 / (max(point.z for point in points) - min(point.z for point in points))
        rig.rotation_mode, rig.rotation_euler.z = 'XYZ', 0
        rig.scale = (scale, scale, scale)
        source_sword(rig)
    else:
        assert bpy.ops.wm.open_mainfile(filepath=str(path), use_scripts=False) == {'FINISHED'}
        rigs = [obj for obj in bpy.context.scene.objects if obj.type == 'ARMATURE']
        if len(rigs) != 1:
            raise ValueError('Character preview requires exactly one armature')
        rig = rigs[0]
    missing = [block.name for block in bpy.data.user_map() if block.library and block.is_missing]
    if missing:
        raise ValueError(f'Missing linked source data: {missing}')
    if rig.animation_data:
        rig.animation_data.use_nla = False
        for track in rig.animation_data.nla_tracks:
            track.mute = True
    return rig


def select(rig, name, frame):
    action = bpy.data.actions[name]
    rig.animation_data.action, rig.animation_data.action_slot = action, action.slots[0]
    bpy.context.scene.frame_set(int(frame), subframe=frame - int(frame))


def grip_landmarks(rig, side, native):
    names = ([f'hand_{side[0]}', f'index_01_{side[0]}', f'pinky_01_{side[0]}'] if native else
             [side.title() + part for part in ('Hand', 'Index1', 'Little1')])
    if any(name not in rig.data.bones for name in names):
        raise ValueError(f'Grip views require hand/index/little landmarks: {names}')
    hand, index, little = [rig.data.bones[name] for name in names]
    across = index.head_local - little.head_local
    forward = index.tail_local - index.head_local + little.tail_local - little.head_local
    normal = forward.cross(across).normalized()
    # Choose the palm-facing side using the bind-space position of the hand.
    if normal.x * hand.head_local.x > 0:
        normal.negate()
    return hand.name, hand.matrix_local.to_3x3().inverted() @ normal


def remove_vertices(obj, indices):
    mesh = bmesh.new()
    mesh.from_mesh(obj.data)
    bmesh.ops.delete(mesh, geom=[v for v in mesh.verts if v.index in indices], context='VERTS')
    mesh.to_mesh(obj.data)
    mesh.free()


def grip_cutaway():
    """Keep skinned hands/forearms and equipment in disposable preview copies."""
    originals, copies = [], []
    for obj in list(bpy.context.scene.objects):
        if obj.type != 'MESH' or obj.hide_render or obj.name.startswith('Reference'):
            continue
        originals.append(obj)
        groups = {group.index for group in obj.vertex_groups if any(
            part in group.name.lower() for part in
            ('hand', 'wrist', 'forearm', 'lowerarm', 'thumb', 'index', 'middle', 'ring', 'little', 'pinky', 'weapon'))}
        keep = {v.index for v in obj.data.vertices
                if sum(w.weight for w in v.groups if w.group in groups) > .5}
        if not keep:
            continue
        copy = obj.copy()
        copy.data = obj.data.copy()
        bpy.context.scene.collection.objects.link(copy)
        remove_vertices(copy, {v.index for v in copy.data.vertices} - keep)
        copy.hide_render = True
        copies.append(copy)
    if not copies:
        raise ValueError('Grip cutaway requires skinned hand/forearm vertex groups')

    def show(enabled):
        for obj in originals:
            obj.hide_render = enabled
        for obj in copies:
            obj.hide_render = not enabled
    return show


def prepare_mode(rig, mode, native):
    meshes = [obj for obj in bpy.context.scene.objects if obj.type == 'MESH']
    if mode == 'equipped':
        return None
    if mode == 'plain':
        clay = material('PreviewClay', (.22, .26, .30))
        for obj in meshes:
            if ((not native and 'Swordsman' in bpy.data.objects and obj.name != 'Swordsman')
                    or (native and obj.name.startswith('Reference'))):
                obj.hide_render = True
                continue
            obj.data = obj.data.copy()
            weapon = obj.vertex_groups.get('Weapon')
            if weapon:
                indices = {v.index for v in obj.data.vertices
                           if any(w.group == weapon.index and w.weight > .5 for w in v.groups)}
                remove_vertices(obj, indices)
            obj.data.materials.clear()
            obj.data.materials.append(clay)
            for polygon in obj.data.polygons:
                polygon.material_index = 0
        return None
    for obj in meshes:
        obj.hide_render = not (native and obj.name.startswith('Reference'))
    colors = {'left': material('PreviewLeft', (.95, .28, .10)),
              'right': material('PreviewRight', (.12, .52, .95)),
              'centre': material('PreviewSpine', (.90, .64, .12))}
    rods = []
    for bone in rig.data.bones:
        if bone.name.startswith('CTRL_') or bone.name in ('Root', 'Weapon', 'BeamOrigin'):
            continue
        if any(digit in bone.name.lower() for digit in ('thumb', 'index', 'middle', 'ring', 'little', 'pinky', 'grip')):
            continue
        name = bone.name.lower()
        side = 'left' if name.startswith('left') or name.endswith('_l') else (
            'right' if name.startswith('right') or name.endswith('_r') else 'centre')
        bpy.ops.mesh.primitive_cylinder_add(vertices=8, radius=1, depth=1)
        rod = bpy.context.object
        rod.data.materials.append(colors[side])
        rods.append((rod, bone.name))
    if not native and 'Weapon' in rig.data.bones:
        bpy.ops.mesh.primitive_cylinder_add(vertices=8, radius=1, depth=1)
        rod = bpy.context.object
        rod.data.materials.append(colors['centre'])
        rods.append((rod, 'Weapon'))

    def update():
        for obj, name in rods:
            bone = rig.pose.bones[name]
            end = rig.pose.bones['BeamOrigin'].head if name == 'Weapon' else bone.tail
            a, b = rig.matrix_world @ bone.head, rig.matrix_world @ end
            obj.location = (a + b) / 2
            obj.rotation_euler = (b - a).to_track_quat('Z', 'Y').to_euler()
            radius = .06 if name.lower() == 'head' else .022
            obj.scale = (radius, radius, (b - a).length)
    return update


def bounds():
    depsgraph = bpy.context.evaluated_depsgraph_get()
    points = [obj.matrix_world @ Vector(corner) for obj in bpy.context.scene.objects
              if obj.type == 'MESH' and not obj.hide_render
              for corner in obj.evaluated_get(depsgraph).bound_box]
    if not points:
        raise ValueError('No visible geometry to frame')
    return points


def full_camera(camera, direction, points):
    low, high = [Vector([function(p[i] for p in points) for i in range(3)]) for function in (min, max)]
    target = (low + high) / 2
    rotation = (-Vector(direction)).to_track_quat('-Z', 'Y')
    projected = [rotation.inverted() @ (p - target) for p in points]
    camera.location = target + Vector(direction)
    camera.rotation_euler = rotation.to_euler()
    camera.data.ortho_scale = max(max(p[i] for p in projected) - min(p[i] for p in projected)
                                  for i in (0, 1)) * 1.15
    return {'target': list(target), 'direction': list(direction), 'scale': camera.data.ortho_scale}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output', type=Path, help='new directory; existing work is never overwritten')
    inputs = parser.add_mutually_exclusive_group()
    inputs.add_argument('--after', type=Path, default=HERE / 'basic-player.blend')
    inputs.add_argument('--source', type=Path, help='native GLB reference; retain its original cadence')
    parser.add_argument('--before', type=Path, help='another player .blend; neutral label A, candidate B')
    parser.add_argument('--clips', nargs='+', default=['AttackCleave', 'AttackSlam', 'Run', 'RunBackward', 'RunLeft', 'RunRight'])
    parser.add_argument('--views', nargs='+', choices=[*VIEWS, *GRIP_VIEWS], default=['game'])
    parser.add_argument('--modes', nargs='+', choices=MODES, default=['equipped'])
    samples = parser.add_mutually_exclusive_group()
    samples.add_argument('--samples', choices=('motion', 'poses'), default='motion')
    samples.add_argument('--frames', nargs='+', type=float, help='specific Blender frames; stills only')
    parser.add_argument('--hand', choices=('right', 'left'), default='right', help='hand for grip views')
    parser.add_argument('--grip-orbit', type=float, default=0,
                        help='degrees around the hand axis when another body part obscures a grip')
    parser.add_argument('--grip-cutaway', action='store_true',
                        help='labelled hand/forearm/weapon isolation for otherwise occluded grip views')
    parser.add_argument('--resolution', type=int, default=420)
    args = parser.parse_args(sys.argv[sys.argv.index('--') + 1:])
    if args.source and args.before:
        parser.error('native source playback uses one source')
    if not 128 <= args.resolution <= 2048:
        parser.error('--resolution must be within 128..2048')
    if not math.isfinite(args.grip_orbit):
        parser.error('--grip-orbit must be finite')
    if any(len(items) != len(set(items)) for items in (args.clips, args.views, args.modes)):
        parser.error('duplicate clips, views or modes')
    if any(view in GRIP_VIEWS for view in args.views) and set(args.modes) != {'equipped'}:
        parser.error('grip views require --modes equipped; use a separate packet for plain/skeleton')
    if args.grip_cutaway and not any(view in GRIP_VIEWS for view in args.views):
        parser.error('--grip-cutaway requires a grip view')
    output = args.output.resolve()
    if output.exists():
        raise FileExistsError(f'Choose a new review directory: {output}')
    paths = ([args.source] if args.source else ([args.before] if args.before else []) + [args.after])
    variants = [(chr(65 + i), path.resolve()) for i, path in enumerate(paths)]
    evidence = {'kind': 'native source preview' if args.source else 'retimed Blender character preview',
                'inputs': {}, 'tools': {str(HERE / name): digest(HERE / name) for name in
                                       ('preview.py', 'preview_timing.py', 'preview_gifs.py')},
                'clips': {}, 'series': [], 'resolution': args.resolution,
                'hand': args.hand, 'grip_orbit_degrees': args.grip_orbit,
                'grip_cutaway': args.grip_cutaway,
                'floor_grid_metres': 2, 'blender': bpy.app.version_string}
    all_points = []
    # Admit every requested input, clip and diagnostic before creating output.
    for variant, path in variants:
        expected = digest(path)
        rig = load(path, bool(args.source))
        dependencies = [Path(bpy.path.abspath(library.filepath)).resolve() for library in bpy.data.libraries]
        evidence['inputs'][variant] = {str(p): digest(p) for p in [path, *dependencies]}
        if evidence['inputs'][variant][str(path)] != expected:
            raise ValueError(f'Input changed during load: {path}')
        if any(view in GRIP_VIEWS for view in args.views):
            grip_landmarks(rig, args.hand, bool(args.source))
        for name in args.clips:
            action = bpy.data.actions[name]
            if not action.slots:
                raise ValueError(f'Action has no animation slot: {name}')
            clip = sampling(name, action.frame_range, bool(args.source),
                            bpy.context.scene.render.fps / bpy.context.scene.render.fps_base,
                            args.samples == 'poses', args.frames)
            if name in evidence['clips'] and evidence['clips'][name] != clip:
                raise ValueError(f'Comparison cadence/range differs for {name}')
            evidence['clips'][name] = clip
            for frame in clip['frames']:
                select(rig, name, frame)
                all_points.extend(bounds())
    camera = bpy.data.objects.new('FramingCamera', bpy.data.cameras.new('FramingCamera'))
    cameras = {view: full_camera(camera, VIEWS[view], all_points) for view in args.views if view in VIEWS}
    evidence['cameras'] = cameras
    output.mkdir(parents=True)
    (output / 'frames').mkdir()
    for variant, path in variants:
        for mode in args.modes:
            rig = load(path, bool(args.source))
            update = prepare_mode(rig, mode, bool(args.source))
            show_cutaway = grip_cutaway() if args.grip_cutaway else None
            scene = setup(args.resolution)
            for name, clip in evidence['clips'].items():
                for view in args.views:
                    cutaway = bool(args.grip_cutaway and view in GRIP_VIEWS)
                    if show_cutaway:
                        show_cutaway(cutaway)
                    bpy.data.objects['PreviewFloor'].hide_render = view in GRIP_VIEWS
                    series_id = f'{quote(name, safe="._-")}-{mode}-{view}' + ('-cutaway' if cutaway else '')
                    series = next((s for s in evidence['series'] if s['id'] == series_id), None)
                    if series is None:
                        series = {'id': series_id, 'clip': name,
                                  'mode': 'cutaway' if cutaway else mode, 'view': view, 'images': {}}
                        evidence['series'].append(series)
                    if view in cameras:
                        spec = cameras[view]
                        scene.camera.location = Vector(spec['target']) + Vector(spec['direction'])
                        scene.camera.rotation_euler = (-Vector(spec['direction'])).to_track_quat('-Z', 'Y').to_euler()
                        scene.camera.data.ortho_scale = spec['scale']
                    series['images'][variant] = []
                    for index, frame in enumerate(clip['frames']):
                        select(rig, name, frame)
                        if update:
                            update()
                        if view in GRIP_VIEWS:
                            hand_name, normal = grip_landmarks(rig, args.hand, bool(args.source))
                            hand = rig.pose.bones[hand_name]
                            target = rig.matrix_world @ ((hand.head + hand.tail) / 2)
                            direction = (rig.matrix_world.to_3x3() @ hand.matrix.to_3x3() @ normal).normalized()
                            axis = (rig.matrix_world.to_3x3() @ (hand.tail - hand.head)).normalized()
                            direction = Quaternion(axis, math.radians(args.grip_orbit)) @ direction
                            if view == 'grip-back':
                                direction.negate()
                            scene.camera.location = target + direction * 2
                            scene.camera.rotation_euler = (-direction).to_track_quat('-Z', 'Y').to_euler()
                            scene.camera.data.ortho_scale = .65
                        relative = f'frames/{variant}-{series_id}-{index:03}.png'
                        scene.render.filepath = str(output / relative)
                        assert bpy.ops.render.render(write_still=True) == {'FINISHED'}
                        series['images'][variant].append({'path': relative, 'sha256': digest(output / relative)})
                    print('RENDERED', variant, series_id, flush=True)
    for inputs in [*evidence['inputs'].values(), evidence['tools']]:
        for path, expected in inputs.items():
            if digest(Path(path)) != expected:
                raise ValueError(f'Input changed during rendering: {path}; no complete evidence written')
    (output / 'evidence.json').write_text(json.dumps(evidence, indent=2) + '\n')


if __name__ == '__main__':
    main()
