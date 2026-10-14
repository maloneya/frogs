"""Regression probes for anatomical contact and forearm clearance (Blender CLI)."""
from pathlib import Path
import sys
import bpy
sys.path.insert(0,str(Path(__file__).resolve().parent))
import grasp

path=Path(sys.argv[sys.argv.index('--')+1]).resolve()
bpy.ops.wm.open_mainfile(filepath=str(path))
rig=bpy.data.objects['SwordsmanRig'];body=bpy.data.objects['Swordsman']
action=bpy.data.actions['Idle'];rig.animation_data.action=action;rig.animation_data.action_slot=action.slots[0]
bpy.context.scene.frame_set(1)
vertices=grasp.contact_vertices(body,'Right')
grasp.validate_contact(rig,body,'Right',vertices)
surface=grasp.forearm_surface(body,'Right');grasp.validate_clearance(rig,body,surface)
for digit in grasp.DIGITS:
 for segment in range(1,4):rig.pose.bones['Right'+digit+str(segment)].rotation_quaternion=(1,0,0,0)
bpy.context.view_layer.update()
try:grasp.validate_contact(rig,body,'Right',vertices)
except AssertionError:pass
else:raise AssertionError('open hand passed grasp admission')
bpy.context.scene.frame_set(1)
# Restore keyed pose, then put the handle at the wrist like the original bug.
bpy.context.scene.frame_set(2);bpy.context.scene.frame_set(1)
weapon=rig.pose.bones['Weapon'];matrix=weapon.matrix.copy()
matrix.translation=rig.pose.bones['RightHand'].head;weapon.matrix=matrix
bpy.context.view_layer.update()
try:grasp.validate_contact(rig,body,'Right',vertices)
except AssertionError:pass
else:raise AssertionError('wrist-mounted weapon passed anatomical contact')
print('PASS rejected open hand and wrist-mounted weapon; accepted anatomical contact and clearance')
