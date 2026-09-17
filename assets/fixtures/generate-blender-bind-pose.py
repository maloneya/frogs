"""Build the checked-in one-skin bind-pose fixture with Blender.

Run from the repository root:

    blender --background --python assets/fixtures/generate-blender-bind-pose.py

Pass an alternate output after ``--`` when comparing exporter versions.
"""

from pathlib import Path
import math
import sys

import bpy
from mathutils import Quaternion


def add_box(name, location, dimensions, material):
    """Add one UV-mapped box, applying scale before the final join."""
    bpy.ops.mesh.primitive_cube_add(location=location)
    box = bpy.context.object
    box.name = name
    box.dimensions = dimensions
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
    box.data.materials.append(material)
    return box


bpy.ops.object.select_all(action="SELECT")
bpy.ops.object.delete(use_global=False)

image = bpy.data.images.new("BindPoseBaseColor", width=16, height=16, alpha=True)
pixels = []
for y in range(image.size[1]):
    for x in range(image.size[0]):
        checker = ((x // 4) + (y // 4)) % 2
        if checker:
            pixels.extend((0.12, 0.72, 0.30, 1.0))
        else:
            pixels.extend((0.72, 0.16, 0.85, 1.0))
image.pixels = pixels
image.pack()

material = bpy.data.materials.new("BindPoseMaterial")
material.use_nodes = True
material.use_backface_culling = True
nodes = material.node_tree.nodes
principled = nodes.get("Principled BSDF")
texture = nodes.new("ShaderNodeTexImage")
texture.name = "BindPoseBaseColor"
texture.image = image
material.node_tree.links.new(texture.outputs["Color"], principled.inputs["Base Color"])

# Blender is Z-up and the exporter maps its -Y forward to glTF +Z. The raised
# right arm makes that forward direction readable in the bind pose.
parts = [
    add_box("Torso", (0.0, 0.0, 1.08), (0.64, 0.34, 0.82), material),
    add_box("Head", (0.0, -0.03, 1.68), (0.42, 0.42, 0.42), material),
    add_box("LeftArm", (-0.43, 0.0, 1.08), (0.18, 0.20, 0.70), material),
    add_box("RightArm", (0.43, -0.13, 1.25), (0.18, 0.20, 0.58), material),
    add_box("LeftLeg", (-0.18, 0.0, 0.40), (0.24, 0.28, 0.72), material),
    add_box("RightLeg", (0.18, 0.0, 0.40), (0.24, 0.28, 0.72), material),
]
for part in parts:
    part.select_set(True)
bpy.context.view_layer.objects.active = parts[0]
bpy.ops.object.join()
character = bpy.context.object
character.name = "BindPoseCharacter"
character.data.name = "BindPoseCharacterMesh"
for polygon in character.data.polygons:
    polygon.material_index = 0
while len(character.data.materials) > 1:
    character.data.materials.pop(index=1)
# Put geometry and rig in the same asset-local coordinates. The exporter still
# retains the joint hierarchy; this only removes an irrelevant mesh-node offset.
bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)

armature_data = bpy.data.armatures.new("BindPoseRig")
armature = bpy.data.objects.new("BindPoseRig", armature_data)
bpy.context.collection.objects.link(armature)
bpy.context.view_layer.objects.active = armature
character.select_set(False)
armature.select_set(True)
bpy.ops.object.mode_set(mode="EDIT")

root = armature.data.edit_bones.new("Root")
root.head = (0.0, 0.0, 0.04)
root.tail = (0.0, 0.0, 0.80)
spine = armature.data.edit_bones.new("Spine")
spine.head = root.tail
spine.tail = (0.0, 0.0, 1.45)
spine.parent = root
spine.use_connect = True
head = armature.data.edit_bones.new("Head")
head.head = spine.tail
head.tail = (0.0, 0.0, 1.90)
head.parent = spine
head.use_connect = True

# An unweighted attachment bone. It belongs to the skin's joint tree but does
# not deform the mannequin; presentation resolves it by name and attaches the
# weapon at its animated asset-local transform.
weapon = armature.data.edit_bones.new("Weapon")
weapon.head = (0.43, -0.13, 1.24)
weapon.tail = (0.43, -0.83, 1.24)
weapon.parent = spine
bpy.ops.object.mode_set(mode="OBJECT")

groups = {
    "Root": character.vertex_groups.new(name="Root"),
    "Spine": character.vertex_groups.new(name="Spine"),
    "Head": character.vertex_groups.new(name="Head"),
}
for vertex in character.data.vertices:
    if vertex.co.z < 0.80:
        group = groups["Root"]
    elif vertex.co.z < 1.45:
        group = groups["Spine"]
    else:
        group = groups["Head"]
    group.add([vertex.index], 1.0, "REPLACE")

modifier = character.modifiers.new(name="BindPoseRig", type="ARMATURE")
modifier.object = armature
character.parent = armature

# A small role catalog. Every action is authored over normalized time; runtime
# stretches attack playback over the authoritative swing duration. Bind-pose
# end keys make looping actions seamless and one-shot attacks settle cleanly.
bpy.context.scene.render.fps = 30
armature.animation_data_create()
spine_pose = armature.pose.bones["Spine"]
spine_pose.rotation_mode = "QUATERNION"
weapon_pose = armature.pose.bones["Weapon"]
weapon_pose.rotation_mode = "QUATERNION"


def add_action(name, keys):
    action = bpy.data.actions.new(name)
    action.use_fake_user = True
    armature.animation_data.action = action
    for frame, spine_x, spine_y, weapon_z in keys:
        spine_pose.rotation_quaternion = Quaternion((1.0, 0.0, 0.0), math.radians(spine_x))
        spine_pose.rotation_quaternion @= Quaternion(
            (0.0, 1.0, 0.0), math.radians(spine_y)
        )
        weapon_pose.rotation_quaternion = Quaternion(
            (0.0, 0.0, 1.0), math.radians(weapon_z)
        )
        spine_pose.keyframe_insert(data_path="rotation_quaternion", frame=frame)
        weapon_pose.keyframe_insert(data_path="rotation_quaternion", frame=frame)


add_action("Idle", [(1, 0, 0, 0), (16, 0, 4, 0), (31, 0, 0, 0)])
add_action("Run", [(1, -8, -7, 0), (8, -8, 7, 0), (16, -8, -7, 0)])
add_action("AttackCleave", [(1, 0, 0, -130), (14, 0, 0, 130), (31, 0, 0, 0)])
add_action("AttackSlam", [(1, 18, 0, 0), (16, -22, 0, 0), (31, 0, 0, 0)])
bpy.context.scene.frame_set(1)

bpy.context.scene.render.image_settings.file_format = "PNG"
bpy.ops.object.select_all(action="SELECT")
arguments = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []
output = Path(arguments[0] if arguments else "assets/fixtures/blender-bind-pose.glb")
output.parent.mkdir(parents=True, exist_ok=True)
bpy.ops.export_scene.gltf(
    filepath=str(output.resolve()),
    export_format="GLB",
    export_apply=False,
    export_cameras=False,
    export_lights=False,
    export_animations=True,
    export_animation_mode="ACTIONS",
    export_nla_strips=False,
)
print(f"wrote {output}")
