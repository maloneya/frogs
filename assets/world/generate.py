"""Author the default ground and prop assets. Run with blender --background --python.

The ground is a unit square scaled to the arena by presentation. Its texture
repeats a dirt tile 96 times across the 192-metre arena (two metres per tile).
The prop is a half-metre octagonal plinth, with its origin on the ground.
"""
from pathlib import Path
import sys
import bpy

SOURCE = Path(__file__).resolve().parent
OUTPUT = Path(sys.argv[sys.argv.index("--") + 1]) if "--" in sys.argv else SOURCE
OUTPUT.mkdir(parents=True, exist_ok=True)
bpy.ops.object.select_all(action="SELECT")
bpy.ops.object.delete(use_global=False)


def material(name, size=None, pixel=None, source=None):
    if source is not None:
        loaded = bpy.data.images.load(str(source), check_existing=False)
        # Preserve the source's sRGB bytes while copying to explicit RGBA.
        # Runtime owns sRGB decoding; do not apply another transfer function.
        loaded.colorspace_settings.name = "Non-Color"
        loaded.scale(256, 256)
        pixels = list(loaded.pixels)
        image = bpy.data.images.new(name, width=256, height=256, alpha=True)
        image.pixels = [1.0 if i % 4 == 3 else value for i, value in enumerate(pixels)]
        bpy.data.images.remove(loaded)
    else:
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
    assert bpy.ops.object.transform_apply(location=True, rotation=True, scale=True) == {"FINISHED"}
    assert bpy.ops.export_scene.gltf(
        filepath=str(OUTPUT / name), export_format="GLB", use_selection=True,
        export_cameras=False, export_lights=False, export_animations=False,
        export_image_format="AUTO",
    ) == {"FINISHED"}


bpy.ops.mesh.primitive_plane_add(size=1)
ground = bpy.context.object
ground.name = "ArenaGround"
ground.data.materials.append(material("GroundDirt", source=SOURCE / "ground-dirt.png"))
# Repeat every two metres at the current arena scale.
for loop in ground.data.uv_layers.active.data:
    loop.uv *= 96
export(ground, "ground.glb")
assert bpy.ops.wm.save_as_mainfile(filepath=str(OUTPUT / "ground.blend")) == {"FINISHED"}

bpy.ops.mesh.primitive_cylinder_add(vertices=8, radius=0.25, depth=0.5, location=(0, 0, 0.25))
prop = bpy.context.object
prop.name = "InteractionPlinth"
bevel = prop.modifiers.new("StoneEdges", "BEVEL")
bevel.width = 0.04
bevel.segments = 1
bpy.ops.object.modifier_apply(modifier=bevel.name)
prop.data.materials.append(material("PropWhite", 4, lambda x, y: (1, 1, 1, 1)))
export(prop, "prop.glb")
