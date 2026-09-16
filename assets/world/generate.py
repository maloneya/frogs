"""Author the default ground and prop assets. Run with blender --background --python.

The ground is a unit square scaled to the arena by presentation. Its texture
contains 128 checker cells across, preserving the existing floor reference grid.
The prop is a half-metre octagonal plinth, with its origin on the ground.
"""
from pathlib import Path
import bpy

OUTPUT = Path(__file__).resolve().parent
bpy.ops.object.select_all(action="SELECT")
bpy.ops.object.delete(use_global=False)


def material(name, size, pixel):
    image = bpy.data.images.new(name, width=size, height=size, alpha=True)
    image.pixels = [v for y in range(size) for x in range(size) for v in pixel(x, y)]
    image.pack()
    mat = bpy.data.materials.new(name)
    mat.use_nodes = True
    mat.use_backface_culling = True
    tex = mat.node_tree.nodes.new("ShaderNodeTexImage")
    tex.image = image
    shader = mat.node_tree.nodes.get("Principled BSDF")
    mat.node_tree.links.new(tex.outputs["Color"], shader.inputs["Base Color"])
    return mat


def export(obj, name):
    bpy.ops.object.select_all(action="DESELECT")
    obj.select_set(True)
    bpy.context.view_layer.objects.active = obj
    bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)
    bpy.ops.export_scene.gltf(
        filepath=str(OUTPUT / name), export_format="GLB", use_selection=True,
        export_cameras=False, export_lights=False, export_animations=False,
        export_image_format="AUTO",
    )


def checker(x, y):
    shade = 0.022 if (x // 32 + y // 32) % 2 == 0 else 0.038
    # Generated image pixels are stored as sRGB bytes by the exporter. Convert
    # the old linear floor colours once; the runtime texture decodes them.
    def srgb(linear):
        return 12.92 * linear if linear <= 0.0031308 else 1.055 * linear ** (1 / 2.4) - 0.055
    return srgb(shade), srgb(shade * 1.05), srgb(shade * 1.25), 1.0

bpy.ops.mesh.primitive_plane_add(size=1)
ground = bpy.context.object
ground.name = "ArenaGround"
ground.data.materials.append(material("GroundChecker", 64, checker))
# Two cells per texture, repeated 64 times: 128 cells across the arena.
# Generous texels per cell keep the linear-filtered seams narrow.
for loop in ground.data.uv_layers.active.data:
    loop.uv *= 64
export(ground, "ground.glb")

bpy.ops.mesh.primitive_cylinder_add(vertices=8, radius=0.25, depth=0.5, location=(0, 0, 0.25))
prop = bpy.context.object
prop.name = "InteractionPlinth"
bevel = prop.modifiers.new("StoneEdges", "BEVEL")
bevel.width = 0.04
bevel.segments = 1
bpy.ops.object.modifier_apply(modifier=bevel.name)
prop.data.materials.append(material("PropWhite", 4, lambda x, y: (1, 1, 1, 1)))
export(prop, "prop.glb")
