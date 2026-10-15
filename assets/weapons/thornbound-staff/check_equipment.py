"""Test-owned geometry for the shared equipment/body/floor admission check."""
from pathlib import Path
import sys

import bpy

sys.path.insert(0,str(Path(__file__).resolve().parents[2]/'characters/basic-player'))
import grasp

from mathutils import Vector
# Rays beyond either segment endpoint must not count as surface crossings.
a,b,c=map(Vector,[(0,-1,-1),(0,1,-1),(0,0,1)])
assert grasp._segment_triangle(Vector((-1,0,0)),Vector((1,0,0)),a,b,c)
assert not grasp._segment_triangle(Vector((-2,0,0)),Vector((-1,0,0)),a,b,c)
assert not grasp._segment_triangle(Vector((1,0,0)),Vector((2,0,0)),a,b,c)
assert not grasp._segment_triangle(Vector((0,0,0)),Vector((1,0,0)),a,b,c)

bpy.ops.wm.read_factory_settings(use_empty=True)
data=bpy.data.armatures.new('TestRig')
rig=bpy.data.objects.new('TestRig',data)
bpy.context.collection.objects.link(rig)
bpy.context.view_layer.objects.active=rig
rig.select_set(True)
bpy.ops.object.mode_set(mode='EDIT')
bone=data.edit_bones.new('Weapon');bone.head=(0,0,0);bone.tail=(0,1,0)
bpy.ops.object.mode_set(mode='OBJECT')
rig.pose.bones['Weapon'].keyframe_insert(data_path='location',frame=1)
rig.pose.bones['Weapon'].keyframe_insert(data_path='location',frame=2)

bpy.ops.mesh.primitive_cube_add(size=.5,location=(0,0,1))
body=bpy.context.object
bpy.ops.object.transform_apply(location=True,rotation=True,scale=True)
bpy.ops.mesh.primitive_cube_add(size=1,location=(0,0,1))
staff=bpy.context.object
staff.scale=(2,.04,.08)
bpy.ops.object.transform_apply(location=True,rotation=True,scale=True)

try:
    grasp.validate_equipment(rig,body,staff)
except AssertionError as error:
    assert 'weapon crosses body surface' in str(error), error
else: raise AssertionError('a shaft through the body was admitted')
for vertex in staff.data.vertices: vertex.co.x+=3
grasp.validate_equipment(rig,body,staff)
for vertex in staff.data.vertices: vertex.co.z-=1
try:
    grasp.validate_equipment(rig,body,staff)
except AssertionError as error:
    assert 'weapon enters floor' in str(error), error
else: raise AssertionError('a weapon below the floor was admitted')
print('PASS equipment admission: refuse body crossing and floor penetration; admit separated geometry')
