"""Compile a retained staff onto the accepted player rig."""
import argparse
import hashlib
import json
import math
from pathlib import Path
import sys

import bmesh
import bpy
from mathutils import Matrix, Vector

HERE=Path(__file__).resolve().parent
PLAYER=HERE.parents[1]/'characters/basic-player'
sys.dont_write_bytecode=True
sys.path.insert(0,str(PLAYER))
import grasp
import generate as player
from motion import set_linear_keys


def remove_sword(body):
    lane=body.vertex_groups['Weapon'].index
    ids={v.index for v in body.data.vertices if any(g.group==lane and g.weight>.99 for g in v.groups)}
    assert ids, 'accepted player must contain its rigid sword'
    mesh=bmesh.new()
    mesh.from_mesh(body.data)
    mesh.verts.ensure_lookup_table()
    bmesh.ops.delete(mesh,geom=[mesh.verts[i] for i in sorted(ids)],context='VERTS')
    mesh.to_mesh(body.data)
    mesh.free()
    body.data.update()


def equip(rig,source,landmarks):
    with bpy.data.libraries.load(str(source),link=False) as (_,library):
        library.objects=['ThornboundStaff']
    staff=library.objects[0]
    assert staff is not None
    bpy.context.scene.collection.objects.link(staff)
    frame=rig.data.bones['Weapon'].matrix_local @ Matrix.Rotation(-math.pi/2,4,'X')
    for vertex in staff.data.vertices:
        vertex.co=frame@vertex.co
    group=staff.vertex_groups.new(name='Weapon')
    group.add(list(range(len(staff.data.vertices))),1,'REPLACE')
    staff.parent=rig
    modifier=staff.modifiers.new('RigidStaff','ARMATURE')
    modifier.object=rig
    bpy.context.view_layer.objects.active=rig
    assert bpy.ops.object.mode_set(mode='EDIT') == {'FINISHED'}
    beam=rig.data.edit_bones['BeamOrigin']
    beam.head=frame@Vector(landmarks['blade_tip'])
    beam.tail=beam.head+frame.to_3x3()@Vector((0,0,.1))
    assert bpy.ops.object.mode_set(mode='OBJECT') == {'FINISHED'}
    return staff


def staff_slam(rig,body,support_offset):
    """Retain the accepted bracing/body travel, author a clear two-hand staff plant."""
    hands=grasp.calibrate(body)
    action=bpy.data.actions['AttackSlam']
    rig.animation_data.action=action
    rig.animation_data.action_slot=action.slots[0]
    bpy.context.scene.frame_set(1)
    weapon=rig.pose.bones['Weapon']
    ready_pose={b.name:b.matrix.copy() for b in rig.pose.bones}
    ready=weapon.head.copy()
    axis=(weapon.tail-weapon.head).normalized()
    ready_rotation=weapon.matrix.to_quaternion().copy()
    anchors=[(1,ready,axis,0),
             (4,ready+Vector((0,-.10,.10)),Vector((.15,-.40,.90)),.1),
             (7,Vector((-.12,-.65,1.30)),Vector((0,-.03,1)),.65),
             (13,Vector((-.12,-.65,1.32)),Vector((0,-.03,1)),1),
             (19,Vector((-.12,-.65,1.04)),Vector((0,-.02,1)),1),
             (24,Vector((-.12,-.65,1.10)),Vector((0,-.03,1)),1),
             (28,ready,Vector((.15,-.40,.90)),.25),
             (31,ready,axis,0)]
    # Read all inherited samples before adding replacement arm keys.
    samples=[]
    for i in range(121):
        frame=1+i*.25
        bpy.context.scene.frame_set(int(frame),subframe=frame-int(frame))
        samples.append((frame,{b.name:(b.location.copy(),b.rotation_quaternion.copy()) for b in rig.pose.bones}))
    previous={}
    for frame,pose in samples:
        for name,(location,rotation) in pose.items():
            rig.pose.bones[name].location=location
            rig.pose.bones[name].rotation_quaternion=rotation
        bpy.context.view_layer.update()
        a,b=next((a,b) for a,b in zip(anchors,anchors[1:]) if a[0]<=frame<=b[0])
        t=(frame-a[0])/(b[0]-a[0])
        t=t*t*(3-2*t)
        goal=a[2].lerp(b[2],t).normalized()
        rotation=axis.rotation_difference(goal) @ ready_rotation
        weight=max(0,min(1,(frame-1)/6,(31-frame)/5))
        poles=tuple(rig.pose.bones[side+'Forearm'].head.lerp(Vector((sign*.85,-.85,1.15)),weight)
                    for side,sign in [('Right',-1),('Left',1)])
        grasp.place(rig,hands,player.chain,a[1].lerp(b[1],t),rotation,a[3]+(b[3]-a[3])*t,
                    poles=poles,support_offset=support_offset)
        for joint in rig.pose.bones:
            joint.rotation_quaternion.normalize()
            if joint.name in previous and joint.rotation_quaternion.dot(previous[joint.name])<0:
                joint.rotation_quaternion.negate()
            previous[joint.name]=joint.rotation_quaternion.copy()
            joint.keyframe_insert(data_path='rotation_quaternion',frame=frame)
            joint.keyframe_insert(data_path='location',frame=frame)
    set_linear_keys(action)
    for frame in [1,31]:
        bpy.context.scene.frame_set(frame)
        error=max(abs(a-b) for joint in rig.pose.bones
                  for x,y in zip(ready_pose[joint.name],joint.matrix) for a,b in zip(x,y))
        assert error<.001, ('staff Slam changed its ready seam',frame,error)
    return min(row[0] for row in anchors if row[3]==1), max(row[0] for row in anchors if row[3]==1)


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output',type=Path)
    parser.add_argument('--blade',choices=['large-talon','sword-glaive'],default='sword-glaive')
    args=parser.parse_args(sys.argv[sys.argv.index('--')+1:])
    out=args.output.resolve()
    out.mkdir(parents=True,exist_ok=True)
    source=HERE/'variants'/args.blade/'thornbound-staff.blend'
    accepted=PLAYER/'basic-player.blend'
    assert bpy.ops.wm.open_mainfile(filepath=str(accepted),use_scripts=False) == {'FINISHED'}
    rig=bpy.data.objects['SwordsmanRig']
    body=bpy.data.objects['Swordsman']
    remove_sword(body)
    landmarks=json.loads(source.with_name('build.json').read_text())['landmarks_blender_metres']
    staff=equip(rig,source,landmarks)
    support_offset=landmarks['support_grasp'][2]-landmarks['primary_grasp'][2]
    support_window=staff_slam(rig,body,support_offset)
    # Failed admission remains inspectable without producing a runtime GLB.
    assert bpy.ops.wm.save_as_mainfile(filepath=str(out/'candidate.blend')) == {'FINISHED'}
    grasp.validate_motion(rig,body,support_window=support_window,support_offset=support_offset)
    floor=grasp.validate_equipment(rig,body,staff)
    rig.animation_data.action=bpy.data.actions['Idle']
    rig.animation_data.action_slot=rig.animation_data.action.slots[0]
    bpy.context.scene.frame_set(1)
    assert bpy.ops.wm.save_as_mainfile(filepath=str(out/'player.blend')) == {'FINISHED'}
    bpy.ops.object.select_all(action='DESELECT')
    for obj in [rig,body,bpy.data.objects['HoodedCloak'],staff]: obj.select_set(True)
    assert bpy.ops.export_scene.gltf(filepath=str(out/'player.glb'),export_format='GLB',use_selection=True,
        export_apply=False,export_cameras=False,export_lights=False,export_animations=True,
        export_animation_mode='ACTIONS',export_nla_strips=False,export_force_sampling=False) == {'FINISHED'}
    report={'blade':args.blade,'source_sha256':hashlib.sha256(source.read_bytes()).hexdigest(),
            'player_source_sha256':hashlib.sha256(accepted.read_bytes()).hexdigest(),
            'output_sha256':hashlib.sha256((out/'player.glb').read_bytes()).hexdigest(),
            'minimum_floor_m':floor}
    (out/'build.json').write_text(json.dumps(report,indent=2)+'\n')


if __name__=='__main__': main()
