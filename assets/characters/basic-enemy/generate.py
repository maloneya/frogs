"""Generate the horde character and its coarse shadow caster with headless Blender."""

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


def add_form(name, rings, material, tile):
    """Eight-sided tapered surfaces; ring weights bridge articulating joints."""
    outline = [(-0.7, -1), (0.7, -1), (1, -0.7), (1, 0.7),
               (0.7, 1), (-0.7, 1), (-1, 0.7), (-1, -0.7)]
    vertices, weights = [], []
    for centre, width, depth, joints in rings:
        for x, y in outline:
            vertices.append((centre[0] + x * width, centre[1] + y * depth, centre[2]))
            weights.append(joints)
    faces = [tuple(reversed(range(8)))]
    for ring in range(len(rings) - 1):
        for side in range(8):
            a, b = ring * 8 + side, ring * 8 + (side + 1) % 8
            faces.append((a, b, b + 8, a + 8))
    faces.append(tuple(range(len(vertices) - 8, len(vertices))))
    mesh = bpy.data.meshes.new(name + "Mesh")
    mesh.from_pydata(vertices, [], faces)
    mesh.update()
    part = bpy.data.objects.new(name, mesh)
    bpy.context.collection.objects.link(part)
    mesh.materials.append(material)
    uv = mesh.uv_layers.new(name="UVMap")
    u, v = TILES[tile]
    for polygon in mesh.polygons:
        for corner, loop in enumerate(polygon.loop_indices):
            uv.data[loop].uv = (u + (corner % 2) * 0.34, v + (corner // 2 % 2) * 0.34)
    groups = {}
    for index, joints in enumerate(weights):
        for bone, weight in joints:
            if bone not in groups:
                groups[bone] = part.vertex_groups.new(name=bone)
            groups[bone].add([index], weight, "REPLACE")
    return part


def join_parts(parts, name):
    bpy.ops.object.select_all(action="DESELECT")
    for part in parts:
        part.select_set(True)
    bpy.context.view_layer.objects.active = parts[0]
    assert bpy.ops.object.join() == {"FINISHED"}
    character = bpy.context.object
    character.name = name
    character.data.name = name + "Mesh"
    for polygon in character.data.polygons:
        polygon.material_index = 0
    while len(character.data.materials) > 1:
        character.data.materials.pop(index=1)
    assert bpy.ops.object.transform_apply(location=True, rotation=True, scale=True) == {"FINISHED"}
    return character


def build_mesh(material, omit_details=False):
    """A tall, narrow horde silhouette with articulated arms and crouching legs."""
    parts = []
    form = lambda name, rings, tile: parts.append(add_form(name, rings, material, tile))
    form("Body", [((0, .03, .63), .25, .18, [("Pelvis", 1)]),
                  ((0, .07, .90), .27, .20, [("Spine", .5), ("Chest", .5)]),
                  ((0, .10, 1.18), .33, .23, [("Chest", 1)])], "cloth")
    form("Head", [((0, -.04, 1.19), .17, .18, [("Head", 1)]),
                  ((0, -.06, 1.42), .22, .23, [("Head", 1)]),
                  ((0, 0, 1.55), .18, .17, [("Head", 1)])], "hide")
    specs = [("BackHump", (0, .27, 1.10), (.45, .17, .30), "Chest", "hide"),
             ("Snout", (0, -.29, 1.30), (.25, .17, .17), "Head", "hide")]
    if not omit_details:
        specs += [("Belt", (0, .02, .72), (.54, .39, .08), "Pelvis", "dark"),
                  ("LeftEye", (-.10, -.284, 1.43), (.075, .035, .075), "Head", "bone"),
                  ("RightEye", (.10, -.284, 1.43), (.075, .035, .075), "Head", "bone"),
                  ("Mouth", (0, -.384, 1.25), (.19, .025, .05), "Head", "dark")]
    for side, sign in [("Left", -1), ("Right", 1)]:
        arm, forearm = side+"Arm", side+"Forearm"
        form(side+"ArmMesh", [((sign*.30, .06, 1.12), .11, .13, [(arm, 1)]),
                              ((sign*.38, .02, .87), .085, .105, [(arm, .5), (forearm, .5)]),
                              ((sign*.40, -.05, .62), .075, .095, [(forearm, 1)])], "hide")
        form(side+"LegMesh", [((sign*.15, .03, .66), .115, .14, [(side+"Leg", 1)]),
                              ((sign*.15, -.06, .38), .095, .11, [(side+"Leg", .5), (side+"Shin", .5)]),
                              ((sign*.15, .01, .14), .08, .10, [(side+"Shin", 1)])], "hide")
        form(side+"Horn", [((sign*.16, .02, 1.48), .075, .08, [("Head", 1)]),
                           ((sign*.23, .06, 1.70), .035, .04, [("Head", 1)]),
                           ((sign*.20, .08, 1.79), .005, .005, [("Head", 1)])], "bone")
        specs += [(side+"Claw", (sign*.40, -.09, .56), (.19, .25, .18), side+"Hand", "bone"),
                  (side+"Foot", (sign*.15, -.09, .10), (.22, .40, .20), side+"Foot", "dark")]
    parts += [add_box(name, centre, size, material, bone, tile) for name, centre, size, bone, tile in specs]
    return join_parts(parts, "BasicEnemy")


def build_armature():
    data = bpy.data.armatures.new("BasicEnemyRig")
    rig = bpy.data.objects.new("BasicEnemyRig", data)
    bpy.context.collection.objects.link(rig)
    bpy.context.view_layer.objects.active = rig
    rig.select_set(True)
    bpy.ops.object.mode_set(mode="EDIT")
    def bone(name, head, tail, parent=None):
        joint = data.edit_bones.new(name)
        joint.head, joint.tail, joint.parent = head, tail, parent
        return joint
    root = bone("Root", (0, .02, .04), (0, .03, .62))
    pelvis = bone("Pelvis", root.tail, (0, .04, .74), root)
    spine = bone("Spine", pelvis.tail, (0, .07, .94), pelvis)
    chest = bone("Chest", spine.tail, (0, .10, 1.18), spine)
    bone("Head", chest.tail, (0, -.02, 1.60), chest)
    for side, sign in [("Left", -1), ("Right", 1)]:
        shoulder = bone(side+"Shoulder", (0, .08, 1.12), (sign*.30, .06, 1.12), chest)
        arm = bone(side+"Arm", shoulder.tail, (sign*.38, .02, .87), shoulder)
        forearm = bone(side+"Forearm", arm.tail, (sign*.40, -.05, .62), arm)
        bone(side+"Hand", forearm.tail, (sign*.40, -.10, .48), forearm)
        leg = bone(side+"Leg", (sign*.15, .03, .66), (sign*.15, -.06, .38), pelvis)
        shin = bone(side+"Shin", leg.tail, (sign*.15, .01, .14), leg)
        bone(side+"Foot", shin.tail, (sign*.15, -.20, .08), shin)
    bpy.ops.object.mode_set(mode="OBJECT")
    rig.show_in_front = True
    return rig


ANIMATED_BONES = ("Root", "Pelvis", "Spine", "Chest", "Head", "LeftShoulder", "RightShoulder",
                  "LeftArm", "RightArm", "LeftForearm", "RightForearm", "LeftHand", "RightHand",
                  "LeftLeg", "RightLeg", "LeftShin", "RightShin", "LeftFoot", "RightFoot")


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
    for slot in action.slots:
        for layer in action.layers:
            for strip in layer.strips:
                bag = strip.channelbag(slot)
                if bag:
                    for curve in bag.fcurves:
                        for key in curve.keyframe_points:
                            key.interpolation = "LINEAR"
    return action


def build_actions(armature):
    bpy.context.scene.render.fps = 30
    armature.animation_data_create()
    actions = []
    rest = {"Spine": (7, 0, 0), "Chest": (6, 0, 0), "LeftForearm": (-20, 0, 0), "RightForearm": (-20, 0, 0)}
    actions.append(add_action(armature, "Idle", [(1, rest), (16, {**rest, "Chest": (10, 0, 2), "Head": (-4, 0, -2)}), (31, rest)]))
    keys = []
    for frame, sign in [(1, 1), (5, 0), (8, -1), (12, 0), (16, 1)]:
        keys.append((frame, {**rest, "Spine": (12, 0, -4*sign), "Chest": (8, 0, 0), "Head": (-8, 0, 4*sign),
            "LeftArm": (-25*sign, 0, -8), "RightArm": (25*sign, 0, 8),
            "LeftLeg": (30*sign, 0, 0), "RightLeg": (-30*sign, 0, 0),
            "LeftShin": (-38 if sign < 0 else -8, 0, 0), "RightShin": (-38 if sign > 0 else -8, 0, 0)}))
    actions.append(add_action(armature, "Run", keys))
    recoil = {**rest, "Spine": (-18, 0, 0), "Chest": (-20, 0, 8), "Head": (-16, 0, -6),
              "LeftArm": (-35, 0, -30), "RightArm": (-35, 0, 30),
              "LeftForearm": (-65, 0, 0), "RightForearm": (-65, 0, 0),
              "LeftLeg": (18, 0, -8), "RightLeg": (18, 0, 8), "LeftShin": (-28, 0, 0), "RightShin": (-28, 0, 0)}
    actions.append(add_action(armature, "Hit", [(1, rest), (3, recoil), (5, recoil), (10, rest)]))
    fallen = {"Spine": (18, 0, 10), "Chest": (15, 0, -5), "Head": (22, 0, 0),
              "LeftArm": (15, 0, -42), "RightArm": (-20, 0, 25), "LeftForearm": (-60, 0, 0), "RightForearm": (-45, 0, 0),
              "LeftLeg": (20, 0, -10), "RightLeg": (-15, 0, 10), "LeftShin": (-40, 0, 0), "RightShin": (-20, 0, 0)}
    actions.append(add_action(armature, "Death", [(1, recoil), (13, fallen)]))
    armature.animation_data.action = actions[0]
    bpy.context.scene.frame_set(1)


def bind(character, armature):
    modifier = character.modifiers.new(name="BasicEnemyRig", type="ARMATURE")
    modifier.object = armature
    character.parent = armature


def save_and_export(output_directory, stem="basic-enemy"):
    output_directory.mkdir(parents=True, exist_ok=True)
    blend_path = (output_directory / f"{stem}.blend").resolve()
    glb_path = (output_directory / f"{stem}.glb").resolve()
    bpy.context.scene.render.image_settings.file_format = "PNG"
    assert bpy.ops.wm.save_as_mainfile(filepath=str(blend_path)) == {"FINISHED"}
    bpy.ops.object.select_all(action="SELECT")
    result = bpy.ops.export_scene.gltf(
        filepath=str(glb_path),
        export_format="GLB",
        export_apply=False,
        export_cameras=False,
        export_lights=False,
        export_animations=True,
        export_animation_mode="ACTIONS",
        export_nla_strips=False,
    )
    assert result == {"FINISHED"}
    print(f"wrote {blend_path}")
    print(f"wrote {glb_path}")


def main():
    arguments = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []
    output_directory = Path(arguments[0]) if arguments else OUTPUT_DIRECTORY
    for stem, shadow in [("basic-enemy", False), ("basic-enemy.shadow", True)]:
        clear_scene()
        material = make_material(make_atlas())
        character = build_mesh(material, omit_details=shadow)
        if shadow:
            character.name = "BasicEnemyShadow"
        armature = build_armature()
        bind(character, armature)
        build_actions(armature)
        save_and_export(output_directory, stem)


if __name__ == "__main__":
    main()
