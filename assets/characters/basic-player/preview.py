"""Render current/previous player motion or a native source clip with fixed lighting."""
import argparse
import hashlib
import json
from pathlib import Path
import sys

import bpy
from mathutils import Vector

HERE = Path(__file__).resolve().parent
MELEE = {'AttackCleave': (6, 6, 10), 'AttackSlam': (9, 5, 16)}


def setup():
    scene = bpy.context.scene
    scene.render.engine = 'CYCLES'
    scene.cycles.samples = 4
    scene.render.resolution_x = scene.render.resolution_y = 420
    scene.render.resolution_percentage = 100
    scene.render.image_settings.file_format = 'PNG'
    scene.view_settings.view_transform = 'Standard'
    if not scene.world:
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
    camera.data.type, camera.data.ortho_scale = 'ORTHO', 4.8
    camera.location = (5, -5, 6)
    camera.rotation_euler = (Vector((0, 0, 1)) - camera.location).to_track_quat('-Z', 'Y').to_euler()
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
        rig = next(obj for obj in bpy.data.objects if obj.type == 'ARMATURE')
        points = [obj.matrix_world @ vertex.co for obj in bpy.data.objects if obj.type == 'MESH'
                  for vertex in obj.data.vertices]
        scale = 2 / (max(point.z for point in points) - min(point.z for point in points))
        rig.rotation_mode, rig.rotation_euler.z = 'XYZ', 0
        rig.scale = (-scale, scale, scale)
        source_sword(rig)
    else:
        assert bpy.ops.wm.open_mainfile(filepath=str(path)) == {'FINISHED'}
        rig = bpy.data.objects['SwordsmanRig']
    return rig


def sampling(action, native, source_fps):
    first, last = action.frame_range
    if not native and action.name in MELEE:
        startup, active, recovery = MELEE[action.name]
        total = startup + active + recovery
        def frame(tick):
            phase = (tick / startup * .4 if tick < startup else
                     .4 + (tick - startup) / active * .2 if tick < startup + active else
                     .6 + (tick - startup - active) / recovery * .4)
            return first + phase * (last - first)
        return [frame(tick) for tick in range(total + 1)], total / 60, False
    duration = (last - first) / source_fps
    count = max(2, round(duration * (24 if native else 30)))
    looping = not native or 'Loop' in action.name or action.name.startswith(('Running', 'Walking'))
    denominator = count if looping else count - 1
    return [first + (last - first) * i / denominator for i in range(count)], duration, looping


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output', type=Path)
    inputs = parser.add_mutually_exclusive_group()
    inputs.add_argument('--after', type=Path, default=HERE / 'basic-player.blend')
    inputs.add_argument('--source', type=Path, help='GLB reference; play its original cadence')
    parser.add_argument('--before', type=Path, help='Previous player checkpoint for equal-camera comparison')
    parser.add_argument('--clips', nargs='+', default=['AttackCleave', 'AttackSlam', 'Run', 'RunBackward', 'RunLeft', 'RunRight'])
    args = parser.parse_args(sys.argv[sys.argv.index('--') + 1:])
    if args.source and args.before:
        parser.error('native source playback uses one source, without a player comparison')
    output = args.output.resolve()
    frames = output / 'frames'
    frames.mkdir(parents=True, exist_ok=True)
    variants = [('source', args.source)] if args.source else (
        ([('before', args.before)] if args.before else []) + [('after', args.after)])
    evidence = {'kind': 'native source preview' if args.source else 'retimed Blender player preview',
                'camera': 'isometric game angle', 'inputs': {}, 'clips': {}}
    for variant, path in variants:
        path = path.resolve()
        files = [path] if args.source else [path, path.with_suffix('.glb')]
        evidence['inputs'][variant] = {str(file): hashlib.sha256(file.read_bytes()).hexdigest() for file in files}
        rig = load(path, bool(args.source))
        source_fps = bpy.context.scene.render.fps
        scene = setup()
        for name in args.clips:
            action = bpy.data.actions[name]
            rig.animation_data.action, rig.animation_data.action_slot = action, action.slots[0]
            samples, duration, looping = sampling(action, bool(args.source), source_fps)
            specification = {'frames': samples, 'seconds': duration, 'loop': looping}
            if name in evidence['clips']:
                assert evidence['clips'][name] == specification, 'comparison cadence must match'
            evidence['clips'][name] = specification
            for index, frame in enumerate(samples):
                scene.frame_set(int(frame), subframe=frame - int(frame))
                scene.render.filepath = str(frames / f'{variant}-{name}-{index:03}.png')
                assert bpy.ops.render.render(write_still=True) == {'FINISHED'}
            print('RENDERED', variant, name, flush=True)
    (output / 'evidence.json').write_text(json.dumps(evidence, indent=2) + '\n')


if __name__ == '__main__':
    main()
