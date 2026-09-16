"""Generate the dedicated horde character with headless Blender."""

from pathlib import Path
import math
import sys

import bpy
from mathutils import Euler


OUTPUT_DIRECTORY = Path("assets/characters/basic-enemy")
ATLAS_SIZE = 64
TILES = {
    "hide": (0.08, 0.58),
    "cloth": (0.58, 0.58),
    "bone": (0.08, 0.08),
    "dark": (0.58, 0.08),
}
ANIMATED_BONES = ("Root", "Spine", "Head", "LeftArm", "RightArm", "LeftLeg", "RightLeg")


def clear_scene():
    """Start clean so regeneration is independent of the open Blender file."""
    bpy.ops.object.select_all(action="SELECT")
    bpy.ops.object.delete(use_global=False)
    for datablocks in (
        bpy.data.meshes,
        bpy.data.armatures,
        bpy.data.materials,
        bpy.data.images,
        bpy.data.actions,
    ):
        for datablock in list(datablocks):
            datablocks.remove(datablock)


def make_atlas():
    """Create four bold colour fields that survive the distant horde view."""
    image = bpy.data.images.new(
        "BasicEnemyBaseColor", width=ATLAS_SIZE, height=ATLAS_SIZE, alpha=True
    )
    colours = {
        "hide": (0.12, 0.31, 0.055, 1.0),
        "cloth": (0.30, 0.035, 0.025, 1.0),
        "bone": (0.72, 0.58, 0.30, 1.0),
        "dark": (0.018, 0.012, 0.008, 1.0),
    }
    pixels = []
    for y in range(ATLAS_SIZE):
        for x in range(ATLAS_SIZE):
            if x < ATLAS_SIZE // 2 and y >= ATLAS_SIZE // 2:
                colour = colours["hide"]
            elif x >= ATLAS_SIZE // 2 and y >= ATLAS_SIZE // 2:
                colour = colours["cloth"]
            elif x < ATLAS_SIZE // 2:
                colour = colours["bone"]
            else:
                colour = colours["dark"]
            shade = 1.07 if ((x // 8) + (y // 8)) % 2 else 0.93
            pixels.extend((*[min(channel * shade, 1.0) for channel in colour[:3]], 1.0))
    image.pixels = pixels
    image.pack()
    return image


def make_material(image):
    material = bpy.data.materials.new("BasicEnemyMaterial")
    material.use_nodes = True
    material.use_backface_culling = True
    nodes = material.node_tree.nodes
    principled = nodes.get("Principled BSDF")
    texture = nodes.new("ShaderNodeTexImage")
    texture.name = "BasicEnemyBaseColor"
    texture.image = image
    texture.interpolation = "Linear"
    material.node_tree.links.new(texture.outputs["Color"], principled.inputs["Base Color"])
    return material


def remap_uvs(mesh, tile):
    origin_u, origin_v = TILES[tile]
    uv_layer = mesh.uv_layers.active
    if uv_layer is None:
        raise RuntimeError(f"{mesh.name} has no UV map")
    for loop in uv_layer.data:
        loop.uv.x = origin_u + loop.uv.x * 0.34
        loop.uv.y = origin_v + loop.uv.y * 0.34


def add_box(name, location, dimensions, material, bone, tile, rotation=(0, 0, 0)):
    """Add one applied, UV-atlased box rigidly assigned to a joint."""
    bpy.ops.mesh.primitive_cube_add(location=location, rotation=rotation)
    part = bpy.context.object
    part.name = name
    part.dimensions = dimensions
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
    part.data.materials.append(material)
    remap_uvs(part.data, tile)
    group = part.vertex_groups.new(name=bone)
    group.add([vertex.index for vertex in part.data.vertices], 1.0, "REPLACE")
    return part


def build_mesh(material):
    """Assemble a forward-readable, metre-scale hunched creature."""
    specs = [
        ("LeftFoot", (-0.18, -0.09, 0.10), (0.30, 0.48, 0.20), "LeftLeg", "dark", (0, 0, 0)),
        ("RightFoot", (0.18, -0.09, 0.10), (0.30, 0.48, 0.20), "RightLeg", "dark", (0, 0, 0)),
        ("LeftShin", (-0.18, 0.01, 0.36), (0.25, 0.25, 0.42), "LeftLeg", "hide", (0, 0, 0)),
        ("RightShin", (0.18, 0.01, 0.36), (0.25, 0.25, 0.42), "RightLeg", "hide", (0, 0, 0)),
        ("Hips", (0.0, 0.03, 0.63), (0.62, 0.38, 0.27), "Root", "cloth", (0, 0, 0)),
        ("Belt", (0.0, 0.02, 0.72), (0.68, 0.41, 0.10), "Root", "dark", (0, 0, 0)),
        ("Torso", (0.0, 0.08, 0.95), (0.82, 0.45, 0.50), "Spine", "cloth", (0, 0, 0)),
        ("BackHump", (0.0, 0.25, 1.08), (0.58, 0.22, 0.36), "Spine", "hide", (0, 0, 0)),
        ("LeftUpperArm", (-0.52, 0.04, 0.90), (0.24, 0.27, 0.57), "LeftArm", "hide", (0, 0, math.radians(-10))),
        ("RightUpperArm", (0.52, 0.04, 0.90), (0.24, 0.27, 0.57), "RightArm", "hide", (0, 0, math.radians(10))),
        ("LeftClaw", (-0.59, -0.04, 0.57), (0.28, 0.29, 0.20), "LeftArm", "bone", (0, 0, 0)),
        ("RightClaw", (0.59, -0.04, 0.57), (0.28, 0.29, 0.20), "RightArm", "bone", (0, 0, 0)),
        ("Head", (0.0, -0.03, 1.32), (0.52, 0.46, 0.40), "Head", "hide", (0, 0, 0)),
        ("Snout", (0.0, -0.29, 1.27), (0.30, 0.20, 0.18), "Head", "hide", (0, 0, 0)),
        ("LeftEye", (-0.12, -0.275, 1.39), (0.085, 0.045, 0.085), "Head", "bone", (0, 0, 0)),
        ("RightEye", (0.12, -0.275, 1.39), (0.085, 0.045, 0.085), "Head", "bone", (0, 0, 0)),
        ("Mouth", (0.0, -0.397, 1.23), (0.20, 0.025, 0.055), "Head", "dark", (0, 0, 0)),
        ("LeftHorn", (-0.21, 0.00, 1.56), (0.14, 0.15, 0.35), "Head", "bone", (0, math.radians(-18), math.radians(-18))),
        ("RightHorn", (0.21, 0.00, 1.56), (0.14, 0.15, 0.35), "Head", "bone", (0, math.radians(18), math.radians(18))),
    ]
    parts = [
        add_box(name, centre, size, material, bone, tile, rotation)
        for name, centre, size, bone, tile, rotation in specs
    ]
    bpy.ops.object.select_all(action="DESELECT")
    for part in parts:
        part.select_set(True)
    bpy.context.view_layer.objects.active = parts[0]
    bpy.ops.object.join()
    character = bpy.context.object
    character.name = "BasicEnemy"
    character.data.name = "BasicEnemyMesh"
    for polygon in character.data.polygons:
        polygon.material_index = 0
    while len(character.data.materials) > 1:
        character.data.materials.pop(index=1)
    bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)
    return character


def build_armature():
    armature_data = bpy.data.armatures.new("BasicEnemyRig")
    armature = bpy.data.objects.new("BasicEnemyRig", armature_data)
    bpy.context.collection.objects.link(armature)
    bpy.context.view_layer.objects.active = armature
    armature.select_set(True)
    bpy.ops.object.mode_set(mode="EDIT")

    def bone(name, head, tail, parent=None, connected=False):
        edit_bone = armature.data.edit_bones.new(name)
        edit_bone.head = head
        edit_bone.tail = tail
        edit_bone.parent = parent
        edit_bone.use_connect = connected
        return edit_bone

    root = bone("Root", (0.0, 0.02, 0.04), (0.0, 0.04, 0.68))
    spine = bone("Spine", root.tail, (0.0, 0.08, 1.16), root, True)
    bone("Head", spine.tail, (0.0, -0.02, 1.60), spine, True)
    bone("LeftArm", (-0.38, 0.05, 1.08), (-0.58, -0.03, 0.53), spine)
    bone("RightArm", (0.38, 0.05, 1.08), (0.58, -0.03, 0.53), spine)
    bone("LeftLeg", (-0.18, 0.03, 0.66), (-0.18, -0.02, 0.07), root)
    bone("RightLeg", (0.18, 0.03, 0.66), (0.18, -0.02, 0.07), root)
    bpy.ops.object.mode_set(mode="OBJECT")
    armature.show_in_front = True
    return armature


def rotation(degrees):
    return Euler(tuple(math.radians(value) for value in degrees), "XYZ").to_quaternion()


def add_action(armature, name, keys):
    action = bpy.data.actions.new(name)
    action.use_fake_user = True
    armature.animation_data.action = action
    for frame, rotations in keys:
        for bone_name in ANIMATED_BONES:
            pose_bone = armature.pose.bones[bone_name]
            pose_bone.rotation_mode = "QUATERNION"
            pose_bone.rotation_quaternion = rotation(rotations.get(bone_name, (0, 0, 0)))
            pose_bone.keyframe_insert(data_path="rotation_quaternion", frame=frame)
    return action


def build_actions(armature):
    bpy.context.scene.render.fps = 30
    armature.animation_data_create()
    actions = []
    actions.append(add_action(armature, "Idle", [
        (1, {"Spine": (7, 0, 0), "LeftArm": (3, 0, -4), "RightArm": (3, 0, 4)}),
        (16, {"Spine": (11, 0, 0), "Head": (-4, 0, 0), "LeftArm": (-2, 0, 2), "RightArm": (-2, 0, -2)}),
        (31, {"Spine": (7, 0, 0), "LeftArm": (3, 0, -4), "RightArm": (3, 0, 4)}),
    ]))
    actions.append(add_action(armature, "Run", [
        (1, {"Spine": (18, 0, -5), "Head": (-8, 0, 5), "LeftArm": (-32, 0, -8), "RightArm": (30, 0, 8), "LeftLeg": (32, 0, 0), "RightLeg": (-32, 0, 0)}),
        (8, {"Spine": (18, 0, 5), "Head": (-8, 0, -5), "LeftArm": (30, 0, -8), "RightArm": (-32, 0, 8), "LeftLeg": (-32, 0, 0), "RightLeg": (32, 0, 0)}),
        (16, {"Spine": (18, 0, -5), "Head": (-8, 0, 5), "LeftArm": (-32, 0, -8), "RightArm": (30, 0, 8), "LeftLeg": (32, 0, 0), "RightLeg": (-32, 0, 0)}),
    ]))
    armature.animation_data.action = actions[0]
    bpy.context.scene.frame_set(1)


def bind(character, armature):
    modifier = character.modifiers.new(name="BasicEnemyRig", type="ARMATURE")
    modifier.object = armature
    character.parent = armature


def save_and_export(output_directory):
    output_directory.mkdir(parents=True, exist_ok=True)
    blend_path = (output_directory / "basic-enemy.blend").resolve()
    glb_path = (output_directory / "basic-enemy.glb").resolve()
    bpy.context.scene.render.image_settings.file_format = "PNG"
    bpy.ops.wm.save_as_mainfile(filepath=str(blend_path))
    bpy.ops.object.select_all(action="SELECT")
    bpy.ops.export_scene.gltf(
        filepath=str(glb_path),
        export_format="GLB",
        export_apply=False,
        export_cameras=False,
        export_lights=False,
        export_animations=True,
        export_animation_mode="ACTIONS",
        export_nla_strips=False,
    )
    print(f"wrote {blend_path}")
    print(f"wrote {glb_path}")


def main():
    arguments = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []
    output_directory = Path(arguments[0]) if arguments else OUTPUT_DIRECTORY
    clear_scene()
    material = make_material(make_atlas())
    character = build_mesh(material)
    armature = build_armature()
    bind(character, armature)
    build_actions(armature)
    save_and_export(output_directory)


main()
