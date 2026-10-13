"""Render catalog sources at authored timing; never import or write game assets."""
from pathlib import Path
import hashlib
import json
import math
import sys

import bpy
from mathutils import Vector

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent


def load(path, library_objects=None, linked_libraries=None):
    bpy.ops.wm.read_factory_settings(use_empty=True)
    if library_objects:
        with bpy.data.libraries.load(str(path), link=False) as (_, data):
            data.objects = list(library_objects)
        for obj in data.objects:
            bpy.context.collection.objects.link(obj)
            obj.hide_render = False
    elif path.suffix == '.blend':
        bpy.ops.wm.open_mainfile(filepath=str(path), use_scripts=False)
        for library in bpy.data.libraries:
            if linked_libraries and library.name in linked_libraries:
                library.filepath = str(ROOT / linked_libraries[library.name])
                library.reload()
        # Loaded asset node groups can retain a library path without needing that
        # file again. Missing linked data, rather than the path, refuses a preview.
        missing = [block.name for block in bpy.data.user_map() if block.library and block.is_missing]
        if missing:
            raise ValueError(f'Unresolved linked source data: {missing}')
    else:
        bpy.context.scene.render.fps = 24
        bpy.ops.import_scene.gltf(filepath=str(path))
    for obj in list(bpy.context.scene.objects):
        if obj.type not in {'MESH', 'ARMATURE', 'EMPTY'}:
            bpy.data.objects.remove(obj, do_unlink=True)
        elif obj.type == 'EMPTY' and obj.instance_type == 'COLLECTION':
            obj.hide_render = True
    rigs = [o for o in bpy.context.scene.objects if o.type == 'ARMATURE']
    for rig in rigs:
        if rig.animation_data:
            rig.animation_data.action = None
            # Source files may have solo NLA strips that override an assigned action.
            rig.animation_data.use_nla = False
            for track in rig.animation_data.nla_tracks:
                track.mute = True
    bpy.context.view_layer.update()
    return rigs


def visible_bounds():
    bpy.context.view_layer.update()
    depsgraph = bpy.context.evaluated_depsgraph_get()
    return [o.matrix_world @ Vector(c)
            for obj in bpy.context.scene.objects if obj.type == 'MESH' and not obj.hide_render
            for o in [obj.evaluated_get(depsgraph)] for c in o.bound_box]


def select_pose(rigs, name, frame):
    if name:
        if len(rigs) != 1:
            raise ValueError('Select a unique source rig')
        action = bpy.data.actions[name]
        rig = rigs[0]
        rig.animation_data_create()
        rig.animation_data.action = action
        rig.animation_data.action_slot = action.slots[0]
    bpy.context.scene.frame_set(math.floor(frame), subframe=frame % 1)


def setup(view, bounds):
    scene = bpy.context.scene
    scene.render.engine = 'BLENDER_WORKBENCH'
    scene.render.resolution_x = 480
    scene.render.resolution_y = 480
    scene.render.resolution_percentage = 100
    scene.render.image_settings.file_format = 'PNG'
    scene.render.film_transparent = False
    scene.view_settings.view_transform = 'Standard'
    shading = scene.display.shading
    shading.light = 'STUDIO'
    shading.color_type = 'SINGLE'
    shading.single_color = (.52, .58, .62)
    shading.show_shadows = True
    shading.show_cavity = True
    shading.cavity_type = 'BOTH'
    shading.background_type = 'WORLD'
    if not scene.world:
        scene.world = bpy.data.worlds.new('CatalogWorld')
    scene.world.color = (.055, .066, .085)
    low = Vector(tuple(min(v[i] for v in bounds) for i in range(3)))
    high = Vector(tuple(max(v[i] for v in bounds) for i in range(3)))
    centre = (low + high) / 2
    span = max(high - low)
    data = bpy.data.cameras.new('CatalogCamera')
    camera = bpy.data.objects.new('CatalogCamera', data)
    scene.collection.objects.link(camera)
    scene.camera = camera
    data.type = 'ORTHO'
    data.ortho_scale = span * 1.3
    directions = {'side':(1, -.2, .2), 'front':(0, -1, .03),
                  'back':(0, 1, .03), 'threequarter':(1, -2, 1)}
    direction = Vector(directions[view])
    camera.location = centre + direction.normalized() * span * 4
    camera.rotation_euler = (centre - camera.location).to_track_quat('-Z', 'Y').to_euler()
    return scene


def inventory(path, rigs):
    scene = bpy.context.scene
    return {
        'path': str(path.relative_to(ROOT)),
        'sha256': hashlib.sha256(path.read_bytes()).hexdigest(),
        'scene_fps': scene.render.fps / scene.render.fps_base,
        'rigs': [{'name':r.name, 'bones':len(r.data.bones),
                  'deform_bones':sum(b.use_deform for b in r.data.bones)} for r in rigs],
        'meshes': [{'name':o.name, 'vertices':len(o.data.vertices),
                    'polygons':len(o.data.polygons)} for o in scene.objects if o.type == 'MESH'],
        'actions': [{'name':a.name, 'frames':list(a.frame_range),
                     'seconds':(a.frame_range[1] - a.frame_range[0]) /
                               (scene.render.fps / scene.render.fps_base)}
                    for a in bpy.data.actions],
    }


def main():
    out = Path(sys.argv[sys.argv.index('--') + 1]).resolve()
    out.mkdir(parents=True, exist_ok=True)
    previews = json.loads((HERE / 'previews.json').read_text())
    evidence = {'blender':bpy.app.version_string, 'sample_fps':24, 'sources':[], 'previews':[]}
    seen = set()
    for preview in previews:
        path = ROOT / preview['path']
        rigs = load(path, preview.get('library_objects'), preview.get('linked_libraries'))
        if path not in seen:
            evidence['sources'].append(inventory(path, rigs))
            seen.add(path)
        if preview.get('mesh'):
            for mesh in [o for o in bpy.context.scene.objects if o.type == 'MESH']:
                mesh.hide_render = mesh.name != preview['mesh']
        if preview.get('meshes'):
            for mesh in [o for o in bpy.context.scene.objects if o.type == 'MESH']:
                mesh.hide_render = mesh.name not in preview['meshes']
        scene = bpy.context.scene
        fps = scene.render.fps / scene.render.fps_base
        directory = out / 'frames' / preview['id']
        directory.mkdir(parents=True, exist_ok=True)
        frames = []
        bounds = []
        for name in preview.get('actions', [None]):
            action = bpy.data.actions[name] if name else None
            if action:
                first, last = action.frame_range
                duration = (last - first) / fps
                count = math.ceil(duration * 24 - 1e-6)
            else:
                first, duration, count = 0, 0, 1
            for sample in range(count):
                frame = first + sample / 24 * fps
                select_pose(rigs, name, frame)
                bounds.extend(visible_bounds())
                filename = f'{len(frames):04}.png'
                frames.append({'file':filename, 'action':name, 'source_frame':frame,
                               'duration_seconds':min(1 / 24, duration - sample / 24) if action else 0})
        setup(preview.get('view', 'threequarter'), bounds)
        for frame in frames:
            select_pose(rigs, frame['action'], frame['source_frame'])
            scene.render.filepath = str(directory / frame['file'])
            bpy.ops.render.render(write_still=True)
        evidence['previews'].append({**preview, 'frames':frames,
            'source_sha256':hashlib.sha256(path.read_bytes()).hexdigest(),
            'dependencies':[{'path':p, 'sha256':hashlib.sha256((ROOT / p).read_bytes()).hexdigest()}
                            for p in preview.get('linked_libraries', {}).values()]})
        print('PREVIEW', preview['id'], len(frames), flush=True)
    (out / 'evidence.json').write_text(json.dumps(evidence, indent=2) + '\n')


if __name__ == '__main__':
    main()
