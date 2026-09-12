"""Build the checked-in static preview through Blender's normal GLB exporter.

Run from the repository root:

    blender --background --python assets/fixtures/generate-blender-static-preview.py

Pass an alternate output after ``--`` when comparing exporter versions.
"""

from pathlib import Path
import sys

import bpy


def add_box(name, location, dimensions, material):
    """Add one UV-mapped box, applying its scale before the final join."""
    bpy.ops.mesh.primitive_cube_add(location=location)
    box = bpy.context.object
    box.name = name
    box.dimensions = dimensions
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
    box.data.materials.append(material)
    return box


bpy.ops.object.select_all(action="SELECT")
bpy.ops.object.delete(use_global=False)

image = bpy.data.images.new("TrainingDummyBaseColor", width=16, height=16, alpha=True)
pixels = []
for y in range(image.size[1]):
    for x in range(image.size[0]):
        checker = ((x // 4) + (y // 4)) % 2
        if checker:
            pixels.extend((0.16, 0.48, 0.95, 1.0))
        else:
            pixels.extend((0.95, 0.36, 0.08, 1.0))
image.pixels = pixels
image.pack()

material = bpy.data.materials.new("TrainingDummyMaterial")
material.use_nodes = True
# The renderer is intentionally single-sided, so the authoring material says so
# too. Blender's exporter then omits glTF's doubleSided flag.
material.use_backface_culling = True
nodes = material.node_tree.nodes
principled = nodes.get("Principled BSDF")
texture = nodes.new("ShaderNodeTexImage")
texture.name = "TrainingDummyBaseColor"
texture.image = image
material.node_tree.links.new(texture.outputs["Color"], principled.inputs["Base Color"])

# A tiny, deliberately asymmetric training dummy. It stays one Blender mesh and
# one material after joining, matching the engine's current static boundary.
parts = [
    # Blender is Z-up and the exporter maps its -Y forward to glTF +Z.
    add_box("Torso", (0.0, 0.0, 1.08), (0.64, 0.34, 0.82), material),
    add_box("Head", (0.0, -0.03, 1.68), (0.42, 0.42, 0.42), material),
    add_box("LeftArm", (-0.43, 0.0, 1.08), (0.18, 0.20, 0.70), material),
    add_box("RightArm", (0.43, -0.13, 1.16), (0.18, 0.20, 0.58), material),
    add_box("LeftLeg", (-0.18, 0.0, 0.40), (0.24, 0.28, 0.72), material),
    add_box("RightLeg", (0.18, 0.0, 0.40), (0.24, 0.28, 0.72), material),
]
for part in parts:
    part.select_set(True)
bpy.context.view_layer.objects.active = parts[0]
bpy.ops.object.join()
dummy = bpy.context.object
dummy.name = "TrainingDummy"
dummy.data.name = "TrainingDummyMesh"
for polygon in dummy.data.polygons:
    polygon.material_index = 0
while len(dummy.data.materials) > 1:
    dummy.data.materials.pop(index=1)

bpy.context.scene.render.image_settings.file_format = "PNG"
arguments = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []
output = Path(arguments[0] if arguments else "assets/fixtures/blender-static-preview.glb")
output.parent.mkdir(parents=True, exist_ok=True)
bpy.ops.export_scene.gltf(
    filepath=str(output.resolve()),
    export_format="GLB",
    export_apply=False,
    export_cameras=False,
    export_lights=False,
)
print(f"wrote {output}")
