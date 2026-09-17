"""Build the first playable character asset with headless Blender.

Run from the repository root:

    blender --background --python assets/characters/basic-player/generate.py

The script deliberately mirrors the engine's small character interchange
contract: one joined mesh (including the sword), one material with a packed
PNG, one armature, rigid vertex groups, and eight presentation actions.
"""

from pathlib import Path
import math
import sys

import bpy
from mathutils import Euler


OUTPUT_DIRECTORY = Path("assets/characters/basic-player")
ATLAS_SIZE = 64


def clear_scene():
    """Start from an empty file so regeneration never depends on UI state."""
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
    """Create a tiny four-colour atlas and pack it into the Blend/GLB."""
    image = bpy.data.images.new(
        "BasicPlayerBaseColor", width=ATLAS_SIZE, height=ATLAS_SIZE, alpha=True
    )
    # Blender image pixels are scene-linear. These deliberately dark values are
    # encoded to sRGB on export and decoded by the renderer on sampling.
    colours = {
        "cloth": (0.025, 0.20, 0.28, 1.0),
        "skin": (0.62, 0.29, 0.14, 1.0),
        "leather": (0.055, 0.018, 0.008, 1.0),
        "metal": (0.48, 0.55, 0.62, 1.0),
    }
    pixels = []
    for y in range(ATLAS_SIZE):
        for x in range(ATLAS_SIZE):
            if x < ATLAS_SIZE // 2 and y >= ATLAS_SIZE // 2:
                colour = colours["cloth"]
            elif x >= ATLAS_SIZE // 2 and y >= ATLAS_SIZE // 2:
                colour = colours["skin"]
            elif x < ATLAS_SIZE // 2:
                colour = colours["leather"]
            else:
                colour = colours["metal"]
            # A faint checker keeps UV orientation visible without adding detail
            # that disappears at the game's pulled-out camera distance.
            shade = 1.08 if ((x // 8) + (y // 8)) % 2 else 0.92
            pixels.extend((*[min(channel * shade, 1.0) for channel in colour[:3]], 1.0))
    image.pixels = pixels
    image.pack()
    return image


def make_material(image):
    material = bpy.data.materials.new("BasicPlayerMaterial")
    material.use_nodes = True
    material.use_backface_culling = True
    nodes = material.node_tree.nodes
    principled = nodes.get("Principled BSDF")
    texture = nodes.new("ShaderNodeTexImage")
    texture.name = "BasicPlayerBaseColor"
    texture.image = image
    texture.interpolation = "Linear"
    material.node_tree.links.new(texture.outputs["Color"], principled.inputs["Base Color"])
    return material


TILES = {
    # Interior UV bounds avoid colour bleeding between atlas quadrants in mips.
    "cloth": (0.08, 0.58),
    "skin": (0.58, 0.58),
    "leather": (0.08, 0.08),
    "metal": (0.58, 0.08),
}


def remap_uvs(mesh, tile):
    origin_u, origin_v = TILES[tile]
    uv_layer = mesh.uv_layers.active
    if uv_layer is None:
        raise RuntimeError(f"{mesh.name} has no UV map")
    for loop in uv_layer.data:
        loop.uv.x = origin_u + loop.uv.x * 0.34
        loop.uv.y = origin_v + loop.uv.y * 0.34


def add_box(name, location, dimensions, material, bone, tile):
    """Add one applied, UV-atlased box rigidly assigned to one bone."""
    bpy.ops.mesh.primitive_cube_add(location=location)
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
    """Assemble a two-metre, forward-readable low-poly adventurer."""
    specs = [
        # name, centre, dimensions, deforming bone, atlas tile
        ("LeftBoot", (-0.17, -0.08, 0.15), (0.28, 0.46, 0.26), "LeftLeg", "leather"),
        ("RightBoot", (0.17, -0.08, 0.15), (0.28, 0.46, 0.26), "RightLeg", "leather"),
        ("LeftLeg", (-0.17, 0.02, 0.49), (0.23, 0.25, 0.54), "LeftLeg", "cloth"),
        ("RightLeg", (0.17, 0.02, 0.49), (0.23, 0.25, 0.54), "RightLeg", "cloth"),
        ("Hips", (0.0, 0.0, 0.80), (0.58, 0.34, 0.26), "Root", "cloth"),
        ("Belt", (0.0, -0.01, 0.92), (0.64, 0.38, 0.10), "Root", "leather"),
        ("Torso", (0.0, 0.0, 1.20), (0.70, 0.36, 0.58), "Spine", "cloth"),
        ("Cape", (0.0, 0.215, 1.16), (0.60, 0.07, 0.72), "Spine", "leather"),
        ("LeftArm", (-0.46, 0.0, 1.12), (0.19, 0.23, 0.62), "LeftArm", "cloth"),
        ("RightArm", (0.46, 0.0, 1.12), (0.19, 0.23, 0.62), "RightArm", "cloth"),
        ("LeftHand", (-0.46, -0.01, 0.78), (0.18, 0.20, 0.18), "LeftArm", "skin"),
        ("RightHand", (0.46, -0.01, 0.78), (0.18, 0.20, 0.18), "RightArm", "skin"),
        ("RightPauldron", (0.46, 0.0, 1.43), (0.28, 0.30, 0.14), "RightArm", "metal"),
        ("Head", (0.0, 0.0, 1.70), (0.41, 0.38, 0.40), "Head", "skin"),
        ("Hair", (0.0, 0.015, 1.91), (0.44, 0.40, 0.13), "Head", "leather"),
        # Blender -Y is forward. These small pieces make facing obvious.
        ("Nose", (0.0, -0.215, 1.69), (0.10, 0.08, 0.12), "Head", "skin"),
        ("LeftEye", (-0.10, -0.222, 1.76), (0.055, 0.035, 0.055), "Head", "leather"),
        ("RightEye", (0.10, -0.222, 1.76), (0.055, 0.035, 0.055), "Head", "leather"),
        ("ChestMark", (0.0, -0.205, 1.25), (0.18, 0.055, 0.18), "Spine", "metal"),
        # The sword is ordinary skinned geometry. Its Weapon weighting preserves
        # the full authored joint transform without a separate rigid renderer.
        ("SwordGrip", (0.46, -0.10, 0.76), (0.11, 0.22, 0.11), "Weapon", "leather"),
        ("SwordGuard", (0.46, -0.22, 0.76), (0.34, 0.07, 0.10), "Weapon", "metal"),
        ("SwordBlade", (0.46, -0.61, 0.76), (0.10, 0.74, 0.065), "Weapon", "metal"),
    ]
    parts = [add_box(name, centre, size, material, bone, tile) for name, centre, size, bone, tile in specs]
    bpy.ops.object.select_all(action="DESELECT")
    for part in parts:
        part.select_set(True)
    bpy.context.view_layer.objects.active = parts[0]
    bpy.ops.object.join()
    character = bpy.context.object
    character.name = "BasicPlayer"
    character.data.name = "BasicPlayerMesh"
    for polygon in character.data.polygons:
        polygon.material_index = 0
    while len(character.data.materials) > 1:
        character.data.materials.pop(index=1)
    # The mesh node must be identity; the armature remains authored hierarchy.
    bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)
    return character


def build_armature():
    armature_data = bpy.data.armatures.new("BasicPlayerRig")
    armature = bpy.data.objects.new("BasicPlayerRig", armature_data)
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

    root = bone("Root", (0.0, 0.0, 0.05), (0.0, 0.0, 0.86))
    spine = bone("Spine", root.tail, (0.0, 0.0, 1.48), root, True)
    bone("Head", spine.tail, (0.0, 0.0, 2.00), spine, True)
    left_arm = bone("LeftArm", (-0.35, 0.0, 1.42), (-0.46, 0.0, 0.76), spine)
    right_arm = bone("RightArm", (0.35, 0.0, 1.42), (0.46, 0.0, 0.76), spine)
    bone("LeftLeg", (-0.17, 0.0, 0.86), (-0.17, 0.0, 0.07), root)
    bone("RightLeg", (0.17, 0.0, 0.86), (0.17, 0.0, 0.07), root)
    # Bone local +Y runs from head to tail. Pointing its tail toward Blender -Y
    # therefore makes the runtime weapon extend along the character's +Z front.
    bone("Weapon", (0.46, -0.02, 0.76), (0.46, -0.77, 0.76), right_arm)

    bpy.ops.object.mode_set(mode="OBJECT")
    armature.show_in_front = True
    return armature


ANIMATED_BONES = (
    "Root",
    "Spine",
    "Head",
    "LeftArm",
    "RightArm",
    "LeftLeg",
    "RightLeg",
    "Weapon",
)


def rotation(degrees):
    return Euler(tuple(math.radians(value) for value in degrees), "XYZ").to_quaternion()


def add_action(armature, name, keys):
    """Author one normalized role clip; every channel shares both endpoints."""
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
    """Create role clips with shared ready/wind-up/contact/recovery edges."""
    bpy.context.scene.render.fps = 30
    armature.animation_data_create()
    ready, windup, contact, recovered = 1, 13, 19, 31
    actions = []
    actions.append(add_action(armature, "Idle", [
        (1, {}),
        (16, {"Spine": (0, 0, 2), "Head": (0, 0, -1), "LeftArm": (0, 0, -2), "RightArm": (0, 0, 2)}),
        (31, {}),
    ]))
    actions.append(add_action(armature, "Run", [
        (1, {"Spine": (7, 0, 0), "LeftArm": (-24, 0, 0), "RightArm": (24, 0, 0), "LeftLeg": (28, 0, 0), "RightLeg": (-28, 0, 0)}),
        (8, {"Spine": (7, 0, 0), "LeftArm": (24, 0, 0), "RightArm": (-24, 0, 0), "LeftLeg": (-28, 0, 0), "RightLeg": (28, 0, 0)}),
        (16, {"Spine": (7, 0, 0), "LeftArm": (-24, 0, 0), "RightArm": (24, 0, 0), "LeftLeg": (28, 0, 0), "RightLeg": (-28, 0, 0)}),
    ]))
    # Spine local Y twists about the upright axis. Keep the lateral strike
    # inside the shared active segment; simulation owns its timing and reach.
    actions.append(add_action(armature, "AttackCleave", [
        (ready, {}),
        (windup, {"Spine": (0, -35, 0), "RightArm": (0, 0, 80)}),
        (contact, {"Spine": (0, 35, 0)}),
        (recovered, {}),
    ]))
    actions.append(add_action(armature, "AttackSlam", [
        (ready, {}),
        (windup, {"Spine": (-18, 0, 0), "LeftArm": (0, 0, -32), "RightArm": (0, 0, 32), "Weapon": (-30, 0, 0)}),
        (contact, {"Spine": (30, 0, 0), "LeftArm": (0, 0, 48), "RightArm": (0, 0, -48), "Weapon": (50, 0, 0)}),
        (recovered, {}),
    ]))
    armature.animation_data.action = actions[0]
    bpy.context.scene.frame_set(1)


def bind(character, armature):
    modifier = character.modifiers.new(name="BasicPlayerRig", type="ARMATURE")
    modifier.object = armature
    character.parent = armature


def save_and_export(output_directory):
    output_directory.mkdir(parents=True, exist_ok=True)
    blend_path = (output_directory / "basic-player.blend").resolve()
    glb_path = (output_directory / "basic-player.glb").resolve()
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
