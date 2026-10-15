"""Render the exported GLB, never the compiler's pre-export mesh.

blender -b --factory-startup --python-exit-code 1 --python .../review.py -- asset.glb output
Matched variant framing is fixed; --views blade-front selects
the orthographic head view used to judge bilateral blade symmetry.
"""
from pathlib import Path
import hashlib
import json
import sys

import bpy
from mathutils import Vector

args=sys.argv[sys.argv.index('--')+1:]
source=Path(args[0]).resolve()
out=Path(args[1]).resolve()
out.mkdir(parents=True,exist_ok=True)
bpy.ops.object.select_all(action='SELECT')
bpy.ops.object.delete(use_global=False)
assert bpy.ops.import_scene.gltf(filepath=str(source)) == {'FINISHED'}
scene=bpy.context.scene
scene.render.engine='CYCLES'
scene.cycles.samples=48
scene.cycles.use_denoising=True
scene.render.image_settings.file_format='PNG'
scene.render.resolution_percentage=100
scene.world.use_nodes=True
scene.world.node_tree.nodes.get('Background').inputs[0].default_value=(.055,.055,.055,1)
scene.world.node_tree.nodes.get('Background').inputs[1].default_value=.7
scene.view_settings.view_transform='AgX'
scene.view_settings.look='AgX - Medium High Contrast'

def light(name,position,energy,size,colour):
    data=bpy.data.lights.new(name,'AREA')
    data.energy=energy
    data.shape='DISK'
    data.size=size
    data.color=colour
    obj=bpy.data.objects.new(name,data)
    scene.collection.objects.link(obj)
    obj.location=position
    obj.rotation_euler=(Vector((0,0,.2))-obj.location).to_track_quat('-Z','Y').to_euler()

light('Key',(-2,-3,3),250,3,(1,.88,.73))
light('Fill',(2,-1,1),100,2,(.7,.82,1))
light('Rim',(.5,2,2),280,2,(1,.85,.65))
data=bpy.data.cameras.new('Review')
cam=bpy.data.objects.new('Review',data)
scene.collection.objects.link(cam)
scene.camera=cam
data.type='ORTHO'
views=[
    ('hero',(1.2,-5,1.05),(0,0,.25),3.05,850,1500),
    ('front',(0,-5,.1),(0,0,.25),3.05,550,1500),
    ('side',(5,0,.1),(0,0,.25),3.05,550,1500),
    ('back',(0,5,.1),(0,0,.25),3.05,550,1500),
    ('blade',(.65,-3,1.49),(.08,0,1.07),1.28,1000,1200),
    ('blade-front',(0,-5,1.07),(0,0,1.07),1.28,1000,1200),
    ('grip',(.6,-3,.2),(0,0,0),.7,1000,1000),
]
if '--views' in args[2:]:
    selected=args[args.index('--views')+1].split(',')
    unknown=set(selected)-{view[0] for view in views}
    if unknown:
        raise ValueError(f'Unknown review views: {sorted(unknown)}')
    views=[view for view in views if view[0] in selected]
for name,position,target,scale,w,h in views:
    cam.location=position
    cam.rotation_euler=(Vector(target)-cam.location).to_track_quat('-Z','Y').to_euler()
    data.ortho_scale=scale
    scene.render.resolution_x=w
    scene.render.resolution_y=h
    scene.render.filepath=str(out/(name+'.png'))
    assert bpy.ops.render.render(write_still=True) == {'FINISHED'}
manifest={'asset':str(source),'sha256':hashlib.sha256(source.read_bytes()).hexdigest(),
          'kind':'Blender studio renders of imported runtime GLB; not game captures',
          'views':views}
(out/'evidence.json').write_text(json.dumps(manifest,indent=2)+'\n')
