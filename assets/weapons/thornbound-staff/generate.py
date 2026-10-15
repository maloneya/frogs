"""Deterministic, unrigged weapon compiler. Blender 5.2; optional output after --."""
from pathlib import Path
import argparse
import json
import math
import sys

import bpy
import numpy as np
from mathutils import Vector

ROOT = Path(__file__).resolve().parent
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('output',type=Path)
parser.add_argument('--blade',choices=['large-talon','sword-glaive'],default='sword-glaive')
args=parser.parse_args(sys.argv[sys.argv.index('--')+1:] if '--' in sys.argv else [])
OUT = args.output.resolve()
OUT.mkdir(parents=True, exist_ok=True)
bpy.ops.object.select_all(action='SELECT')
assert bpy.ops.object.delete(use_global=False) == {'FINISHED'}

# Four padded longitudinal swatches: bark, polished heartwood, bronze, steel.
# Geometry carries the larger relief; this albedo stays useful in the game's
# base-colour-only material path, without baked directional highlights.
W, H = 2048, 1024
x, y = np.meshgrid(np.linspace(0, 1, W // 4), np.linspace(0, 1, H))
rng = np.random.default_rng(3107)
swatches = []
for kind in range(4):
    noise = rng.random(x.shape) - .5
    warp = x + .014*np.sin(y*21 + x*13) + .009*np.sin(y*47+x*24)
    knot_shade=np.zeros_like(x)
    if kind == 0:
        for cx,cy in [( .21,.22),(.73,.47),(.41,.78)]:
            dx=x-cx
            dy=(y-cy)*.42
            d=np.sqrt(dx*dx+dy*dy)
            influence=np.exp(-d*d/.0017)
            warp += .035*np.sin(np.arctan2(dy,dx))*influence
            knot_shade += influence*(.15+.24*np.cos(d*530)**4)
    grain = np.sin(warp*310 + np.sin(y*10)*2)
    fine = np.sin(warp*880 + y*9)
    if kind == 0:
        crack = np.maximum(0, np.cos(warp*210 + np.sin(y*31)))**12
        weather=np.sin(x*39+y*25)*np.sin(x*21-y*47)
        grain_strength=.3+.7*np.clip(weather+.5,0,1)
        value = .88 + grain_strength*(.21*grain + .10*fine - .55*crack) + .14*noise
        value += .13*weather-knot_shade
        colour = np.array([.25, .18, .12])
    elif kind == 1:
        value = .88 + .065*grain + .025*fine + .04*noise
        colour = np.array([.27, .151, .076])
    elif kind == 2:
        patina = np.maximum(0, np.sin(x*41+y*19)*np.sin(x*17-y*27))**3
        value = .8 + .025*np.sin(y*360) + .14*noise - .43*patina
        colour = np.array([.46, .407, .315])
    else:
        mottling = np.sin(x*29+y*53)*np.sin(x*71-y*91)
        value = .84 + .008*np.sin(y*740+x*33) + .07*noise + .065*mottling
        colour = np.array([.38, .41, .43])
    rgb = np.clip(value[...,None]*colour, 0, 1)
    # Steel swatch's right-hand lane is the honed edge.
    if kind == 3:
        rgb[:, 425:] = np.clip(rgb[:,425:]*1.39,0,1)
    if kind == 2:
        # A small inset of opaque amber; illumination can be added later.
        amber=.8+.16*np.sin(y*17+x*43)*np.sin(y*31-x*27)
        rgb[:,455:]=amber[:,455:,None]*np.array([.59,.30,.075])
    # The generated image buffer is tagged sRGB; save and export preserve these
    # encoded texels. The shader and runtime importer decode at their boundary.
    swatches.append(np.concatenate([rgb, np.ones((*x.shape,1))],axis=2))
pixels = np.concatenate(swatches,axis=1).astype(np.float32)
image = bpy.data.images.new('Thornbound_Albedo', width=W, height=H, alpha=True)
image.colorspace_settings.name = 'sRGB'
image.pixels.foreach_set(pixels.ravel())
image.filepath_raw = str(OUT / 'thornbound-staff-albedo.png')
image.file_format = 'PNG'
image.save()
image.pack()
mat = bpy.data.materials.new('Thornbound_Wood_Bronze_Steel')
mat.use_nodes = True
mat.use_backface_culling = True
shader = mat.node_tree.nodes.get('Principled BSDF')
shader.inputs['Roughness'].default_value = .72
shader.inputs['Metallic'].default_value = 0
tex = mat.node_tree.nodes.new('ShaderNodeTexImage')
tex.image = image
tex.extension = 'REPEAT'
mat.node_tree.links.new(tex.outputs['Color'], shader.inputs['Base Color'])

verts, faces, uvs, smooth = [], [], [], []
landmarks = {}

def face(indices, coords, soft=True):
    faces.append(indices)
    uvs.append(coords)
    smooth.append(soft)

def atlas(kind, u, v):
    return ((kind + .025 + .95*u)/4, .02+.96*v)

def tube(points, radii, kind=0, sides=12, bark=False, phase=0):
    """Closed tubes with real bark fluting and seam-correct longitudinal UVs."""
    offset = len(verts)
    for i, point in enumerate(points):
        p = Vector(point)
        tangent = Vector(points[min(i+1,len(points)-1)]) - Vector(points[max(0,i-1)])
        tangent.normalize()
        guide = Vector((0,1,0)) if abs(tangent.y) < .9 else Vector((1,0,0))
        a = tangent.cross(guide).normalized()
        b = tangent.cross(a).normalized()
        t = i/(len(points)-1)
        for j in range(sides):
            angle = 2*math.pi*j/sides
            relief = 1
            if bark:
                relief += .19*math.sin(angle*5 + .55*math.sin(t*13) + phase)
                relief += .09*math.sin(angle*7 - t*11) + .05*math.sin(t*103+angle*3)
            r = radii[i]*relief
            verts.append(tuple(p + r*(a*math.cos(angle)+b*math.sin(angle))))
    for i in range(len(points)-1):
        for j in range(sides):
            nj = (j+1)%sides
            face((offset+i*sides+j, offset+i*sides+nj, offset+(i+1)*sides+nj, offset+(i+1)*sides+j),
                 [atlas(kind,j/sides,i/(len(points)-1)),atlas(kind,(j+1)/sides,i/(len(points)-1)),
                  atlas(kind,(j+1)/sides,(i+1)/(len(points)-1)),atlas(kind,j/sides,(i+1)/(len(points)-1))])
    face(tuple(offset+j for j in reversed(range(sides))),[atlas(kind,.5,.03)]*sides)
    end=offset+(len(points)-1)*sides
    face(tuple(end+j for j in range(sides)),[atlas(kind,.5,.97)]*sides)

def ring(z, radius, height, kind=2):
    tube([(0,0,z-height/2),(0,0,z-height*.35),(0,0,z+height*.35),(0,0,z+height/2)],
         [radius*.94,radius,radius,radius*.94],kind,sides=32)

# Smooth, uninterrupted grip; its axis and origin are future grasp landmarks.
tube([(0,0,z) for z in [-.225,-.21,-.16,0,.16,.21,.225]],
     [.022,.023,.0225,.0215,.0225,.023,.022],1,32)
landmarks['primary_grasp']=[0,0,0]
landmarks['support_grasp']=[0,0,.15]

for bottom,top,turns in [(-1.0,-.225,1.8),(.225,.76,1.25)]:
    for strand in range(2):
        pts,rs=[],[]
        for i in range(81):
            t=i/80
            z=bottom+(top-bottom)*t
            # Fuse into the polished grip and the terminal sockets.
            envelope=math.sin(math.pi*t)**.65
            radius=.006+.018*envelope
            angle=t*turns*2*math.pi+strand*math.pi+.30*math.sin(t*15)+.08*math.sin(t*37+strand)
            cx=.008*math.sin(z*12)*envelope
            pts.append((cx+radius*math.cos(angle),radius*math.sin(angle),z))
            knot=math.exp(-((t-(.34 if strand==0 else .72))/.045)**2)
            r=.0105+.004*envelope+.0024*math.sin(t*39+strand)+.005*knot
            if bottom > 0:
                r *= 1-.93*t**12
            rs.append(r)
        tube(pts,rs,0,16,True,strand)
        # Raised bark fibres wander along the grain and taper into the wood.
        for ridge in range(2):
            line=[]
            for i in range(41):
                t=i/40
                p=pts[i*2]
                a=t*turns*2*math.pi+strand*math.pi+ridge*math.pi+.25*math.sin(t*19)
                line.append(Vector(p)+Vector((math.cos(a),math.sin(a),0))*rs[i*2]*.84)
            tube(line,[.0003+.0014*math.sin(math.pi*i/40)**.7 for i in range(41)],0,4)
        # Branch scars rise along each main strand rather than float on it.
        for center in ([.23,.54,.78] if bottom<0 else [.32,.67]):
            scar=[]
            for j in range(21):
                t=center+(j/20-.5)*.105
                idx=round(t*80)
                a=t*turns*2*math.pi+strand*math.pi+.30*math.sin(t*15)+.08*math.sin(t*37+strand)
                p=Vector(pts[idx])
                p+=Vector((math.cos(a),math.sin(a),0))*(rs[idx]*.82)
                p+=Vector((0,0,.006*math.sin(j/20*math.pi*2)))
                scar.append(p)
            tube(scar,[.0008+.0025*math.sin(math.pi*j/20) for j in range(21)],0,6)

# Closed curled butt and restrained ferrule retain a walking-staff read.
tube([(0,0,-1.025),(0,0,-1.015),(0,0,-.992)], [.013,.022,.021],0,20,True)
ring(-1.001,.0225,.026)
for z in [-.232,.232]:
    ring(z,.027,.034)
    for dz in [-.013,.013]:
        ring(z+dz,.028,.004)
    # Diagonal collar binding, with an actual helix rather than painted bands.
    points=[]
    for i in range(81):
        t=i/80
        a=math.pi*6*t
        points.append((.0273*math.cos(a),.0273*math.sin(a),z-.011+.022*t))
    tube(points,[.0014]*len(points),2,6)

# Swept blade: thick back, two bevels and a truly thin sharpened edge.
def blade_shape(t):
    if args.blade == 'sword-glaive':
        # Straight, bilateral sword blade: short shoulders, distal taper and
        # a defined point, centered on the existing socket.
        width=float(np.interp(t,[0,.06,.72,.84,1],[.028,.080,.064,.047,.0004]))
        return .006-width/2, .006+width/2, .805+.70*t
    center=.006-.015*t+.19*t**3
    belly,length=.065,.34
    width=.0004+.028*(1-t)+belly*math.sin(math.pi*t)**.85
    center=.006+1.5*(center-.006)
    length *= 1.5
    shoulder=min(t/.10,1)
    width *= 1+.5*shoulder*shoulder*(3-2*shoulder)
    return center-width*.43, center+width*.57, .805+length*t

offset=len(verts)
steps=44
cross=[(0,0),(.16,-1),(.73,-.48),(1,0),(.73,.48),(.16,1)]
if args.blade == 'sword-glaive':
    cross=[(0,0),(.18,-.5),(.5,-1),(.82,-.5),(1,0),(.82,.5),(.5,1),(.18,.5)]
lanes=len(cross)
for i in range(steps+1):
    t=i/steps
    lo,hi,z=blade_shape(t)
    thick=.0065*(1-t)**.65+.00025
    for f,depth in cross:
        verts.append((lo+(hi-lo)*f,depth*thick,z))
for i in range(steps):
    for j in range(lanes):
        nj=(j+1)%lanes
        edge=j in ([0,3,4,7] if args.blade == 'sword-glaive' else [2,3])
        ua,ub=(.88,.98) if edge else (.12,.75)
        face((offset+i*lanes+j,offset+i*lanes+nj,offset+(i+1)*lanes+nj,offset+(i+1)*lanes+j),
             [atlas(3,ua,i/steps),atlas(3,ub,i/steps),atlas(3,ub,(i+1)/steps),atlas(3,ua,(i+1)/steps)],False)
face(tuple(offset+j for j in reversed(range(lanes))),[atlas(3,.3,.03)]*lanes,False)
face(tuple(offset+steps*lanes+j for j in range(lanes)),[atlas(3,.9,.97)]*lanes,False)
tip_left,tip_right,tip_z=blade_shape(1)
landmarks['blade_tip']=[tip_left+(tip_right-tip_left)*(.5 if args.blade == 'sword-glaive' else .43),0,tip_z]

# Thorn-shaped socket embraces the root, leaving a real open casting eye.
# The opening is geometry, with no emissive or transparent material dependency.
for side in [-1,1]:
    pts=[]
    for i in range(51):
        t=i/50
        pts.append((-.012+.052*math.sin(math.pi*t),side*.012,.59+.27*t))
    tube(pts,[.003+.004*math.sin(math.pi*i/50) for i in range(51)],2,8)
    # Bronze spine inlay follows the blade's lower third.
    pts=[]
    for i in range(31):
        t=i/30*.7
        lo,hi,z=blade_shape(t)
        inlay_fraction=.5 if args.blade == 'sword-glaive' else .18
        pts.append((lo+(hi-lo)*inlay_fraction,side*(.0065*(1-t)**.65+.0007),z))
    tube(pts,[.0012]*len(pts),2,6 if args.blade == 'sword-glaive' else 5)
# Forged eye: a hollow, canted frame bridging wood to the blade's spine.
eye=[]
for i in range(65):
    angle=2*math.pi*i/64
    z=.765+.067*math.cos(angle)
    eye.append((.004+.027*math.sin(angle)+.024*math.cos(angle),0,z))
tube(eye,[.0045+.0015*math.sin(math.pi*i/64)**2 for i in range(65)],3,10)
# Bronze vein along the outer edge of the socket frame.
vein=[(p[0],-.0045,p[2]) for p in eye]
tube(vein,[.0011]*len(vein),2,5)
# The stone is seated at the lower eye; the remaining opening stays clear.
tube([(-.015,0,.705),(-.003,0,.725),(.013,0,.744)], [.004,.004,.005],2,8)
stone_start=len(faces)
tube([(.013,0,.736),(.013,0,.755),(.013,0,.783),(.013,0,.807)],
     [.002,.013,.009,.001],2,10)
for k in range(stone_start,len(faces)):
    uvs[k]=[atlas(2,.94,.3+.35*(j%2)) for j in range(len(faces[k]))]
# Small hooked quillon pointing away from the cutting edge.
tube([(0,0,.688),(-.024,0,.696),(-.044,0,.713),(-.049,0,.739)],
     [.018,.014,.007,.0008],2,12)

mesh=bpy.data.meshes.new('ThornboundStaff_Surface')
mesh.from_pydata(verts,[],faces)
mesh.update()
uv=mesh.uv_layers.new(name='Albedo')
for poly,coords,soft in zip(mesh.polygons,uvs,smooth):
    poly.use_smooth=soft
    for loop,coord in zip(poly.loop_indices,coords):
        uv.data[loop].uv=coord
obj=bpy.data.objects.new('ThornboundStaff',mesh)
bpy.context.collection.objects.link(obj)
mesh.materials.append(mat)
bpy.context.view_layer.objects.active=obj
obj.select_set(True)
obj['grip_radius_m']=.022
obj['description']='Traveler bladed staff; static asset checkpoint, no rig or effects.'
for name,point in landmarks.items():
    marker=bpy.data.objects.new(name,None)
    bpy.context.collection.objects.link(marker)
    marker.location=point
    marker.empty_display_size=.025
    marker.hide_render=True
    marker.hide_viewport=True

assert bpy.ops.wm.save_as_mainfile(filepath=str(OUT/'thornbound-staff.blend')) == {'FINISHED'}
assert bpy.ops.export_scene.gltf(filepath=str(OUT/'thornbound-staff.glb'),export_format='GLB',
    use_selection=True,export_cameras=False,export_lights=False,export_animations=False,
    export_skins=False,export_morph=False,export_image_format='AUTO',export_extras=False,
    export_texcoords=True,export_normals=True,export_yup=True) == {'FINISHED'}
mesh.calc_loop_triangles()
report={'vertices':len(mesh.vertices),'triangles':len(mesh.loop_triangles),
        'texture':[W,H],'landmarks_blender_metres':landmarks,
        'bounds_blender':[[min(v.co[k] for v in mesh.vertices) for k in range(3)],
                          [max(v.co[k] for v in mesh.vertices) for k in range(3)]]}
report['blade_profile']=args.blade
report['blade_height_m']=tip_z-blade_shape(0)[2]
report['blade_max_width_m']=max(blade_shape(i/100)[1]-blade_shape(i/100)[0] for i in range(101))
(OUT/'build.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps(report))
