"""A unit-footprint, 1.5 m tall opaque wall; presentation scales X/Z only."""
from pathlib import Path
import sys
import bpy

output = Path(sys.argv[sys.argv.index('--') + 1]) if '--' in sys.argv else Path(__file__).resolve().parent
output.mkdir(parents=True, exist_ok=True)
assert bpy.ops.object.select_all(action='SELECT') == {'FINISHED'}
assert bpy.ops.object.delete(use_global=False) == {'FINISHED'}
assert bpy.ops.mesh.primitive_cube_add(size=1, location=(0, 0, 0.75)) == {'FINISHED'}
wall = bpy.context.object
wall.name = 'SolidWall'
wall.scale.z = 1.5
assert bpy.ops.object.transform_apply(location=True, rotation=True, scale=True) == {'FINISHED'}
image = bpy.data.images.new('WallStone', width=4, height=4, alpha=True)
image.pixels = [v for _ in range(16) for v in (0.55, 0.52, 0.47, 1.0)]
image.pack()
material = bpy.data.materials.new('WallStone')
material.use_nodes = True
material.use_backface_culling = True
texture = material.node_tree.nodes.new('ShaderNodeTexImage')
texture.image = image
material.node_tree.links.new(texture.outputs['Color'], material.node_tree.nodes.get('Principled BSDF').inputs['Base Color'])
wall.data.materials.append(material)
assert bpy.ops.wm.save_as_mainfile(filepath=str(output / 'wall.blend')) == {'FINISHED'}
assert bpy.ops.export_scene.gltf(filepath=str(output / 'wall.glb'), export_format='GLB', use_selection=True,
    export_cameras=False, export_lights=False, export_animations=False, export_image_format='AUTO') == {'FINISHED'}
