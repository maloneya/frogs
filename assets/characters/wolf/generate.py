"""Compile the retained CC0 wolf anatomy and authored quadruped motions."""
from pathlib import Path
import hashlib
import json
import math
import sys

import bpy
from mathutils import Matrix, Quaternion, Vector
from mathutils.bvhtree import BVHTree

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
MOTION_TOOLS = HERE.parent / 'basic-player/motion.py'
sys.path.insert(0, str(MOTION_TOOLS.parent))
from motion import action_curves

BODY = ROOT / 'experimental/character-references/sources/micket-wolf/wolf.blend'
MOTION = ROOT / 'experimental/character-references/sources/tomek-wolf/wolf.blend'
BODY_SCALE = .30
MOTION_SCALE = 1.8
NAMES = {'Bone': 'Pelvis', 'Bone.005': 'Spine', 'Bone.001': 'Chest',
         'Neck1': 'NeckBase', 'Neck2': 'Neck', 'Neck3': 'UpperNeck',
         'Jaw1': 'JawHinge', 'Jaw2': 'Jaw', 'Bone.012': 'Belly'}
for side in ('L', 'R'):
    prefix = 'Left' if side == 'L' else 'Right'
    for old, new in [('FrontShoulder', 'Shoulder'), ('FrontLeg1', 'Foreleg'),
                     ('FrontLeg2', 'ForeShin'), ('FrontLeg3', 'ForePaw'), ('FrontToe', 'ForeToe'),
                     ('BackLeg1', 'Thigh'), ('BackLeg2', 'Hock'),
                     ('BackLeg3', 'HindPaw'), ('BackToe', 'HindToe')]:
        NAMES[old + '_' + side] = prefix + new
MAP = {'Pelvis': 'Bone', 'Spine': 'Bone', 'Chest': 'Bone', 'Belly': 'Bone',
       'NeckBase': 'neck_01', 'Neck': 'neck_01', 'UpperNeck': 'neck_01',
       'Head': 'head', 'JawHinge': 'head', 'Jaw': 'head',
       'Tail1': 'Bone.001', 'Tail2': 'Bone.002', 'Tail3': 'Bone.003', 'Tail4': 'Bone.004'}
FEET = {'LeftFore': 'Bone.014', 'RightFore': 'Bone.015',
        'LeftHind': 'Bone.020', 'RightHind': 'Bone.021'}
for side, numbers in [('Left', (8, 10, 12, 14, 16, 18, 20, 20)),
                      ('Right', (9, 11, 13, 15, 17, 19, 21, 21))]:
    for target, source in zip(('Shoulder','Foreleg','ForeShin','ForePaw','Thigh','Hock','HindPaw','HindToe'), numbers):
        MAP[side + target] = f'Bone.{source:03}'
PALETTE = [( .27,.30,.31,1), (.085,.105,.12,1), (.48,.49,.46,1),
           (.64,.62,.54,1), (.035,.04,.045,1), (.72,.40,.065,1),
           (.88,.84,.70,1), (.20,.105,.10,1)]


def source_samples():
    assert bpy.ops.wm.open_mainfile(filepath=str(MOTION), use_scripts=False) == {'FINISHED'}
    rig = bpy.data.objects['wolf_armature']
    rig.animation_data.use_nla = False
    bind = {b.name: b.matrix_local.to_quaternion() for b in rig.data.bones}
    origin = rig.data.bones['Bone'].head_local.copy()
    cache = {}
    def sample(name, frame):
        key = (name, round(frame, 5))
        if key not in cache:
            action = bpy.data.actions[name]
            rig.animation_data.action = action
            rig.animation_data.action_slot = action.slots[0]
            bpy.context.scene.frame_set(math.floor(frame), subframe=frame % 1)
            cache[key] = ({b.name: b.matrix.to_quaternion() @ bind[b.name].inverted()
                           for b in rig.pose.bones}, (rig.pose.bones['Bone'].head - origin) * MOTION_SCALE,
                          {name:(rig.pose.bones[bone].head-rig.data.bones[bone].head_local)*MOTION_SCALE
                           for name,bone in FEET.items()})
        return cache[key]
    # Sample before opening the independent body source.
    clips = {}
    for name, last in [('Idle',97), ('Walk',31), ('Run',19), ('Bite',31), ('Leap',31), ('Hit',31), ('Death',31)]:
        clips[name] = []
        for frame in range(1, last + 1):
            p = (frame - 1) / (last - 1)
            if name in ('Idle','Walk','Run'):
                a = sample('wolf_' + name.lower(), p * 24)
                jaw = 0
            elif name == 'Bite':
                f = (p / .4 * 8 if p < .4 else 8 + (p - .4) / .2 * 9 if p < .6 else 17 + (p - .6) / .4 * 7)
                a = sample('wolf_attack01', f)
                jaw = piecewise(p, [(0,0),(.32,30),(.4,34),(.5,0),(.6,3),(1,0)])
            elif name == 'Leap':
                landmarks = [(0,('wolf_idle',0)), (.30,('wolf_jump',0)), (.4,('wolf_jump',6)),
                             (.49,('wolf_fly',0)), (.6,('wolf_land',10)),
                             (.7,('wolf_land',12)), (1,('wolf_land',24))]
                for (t0, (n0,f0)), (t1,(n1,f1)) in zip(landmarks,landmarks[1:]):
                    if p <= t1:
                        a = interpolate(sample(n0,f0), sample(n1,f1), (p-t0)/(t1-t0))
                        break
                # The source landing ends with a different tail gesture. Meet
                # the shared ready pose so landing cannot pop when idle resumes.
                ready=sample('wolf_idle',0)
                settle=max(0,min(1,(p-.7)/.3))
                rotations=dict(a[0])
                for joint in ['Bone.001','Bone.002','Bone.003','Bone.004']:
                    rotations[joint]=rotations[joint].slerp(ready[0][joint],settle)
                a=(rotations,a[1],a[2])
                jaw = piecewise(p, [(0,0),(.3,12),(.4,22),(.54,26),(.6,0),(.75,3),(1,0)])
            elif name == 'Hit':
                a = sample('wolf_idle', 0)
                jaw = 0
                strength = piecewise(p, [(0,0),(.2,1),(.5,.7),(1,0)])
                rotations = dict(a[0])
                for joint in ['Bone','neck_01','head']:
                    rotations[joint] = Quaternion((1,0,0), -.18*strength) @ rotations[joint]
                a = (rotations, Vector((0,.06*strength,-.04*strength)), a[2])
            else:
                # Retain the source recoil, then gather the limbs before a side
                # fall. Directly retargeting its final splay flattens this anatomy.
                settle=max(0,min(1,(p-.45)/.55))
                settle=settle*settle*(3-2*settle)
                a = interpolate(sample('wolf_die',min(p/.6,1)*36),sample('wolf_jump',0),settle)
                jaw = 0
            clips[name].append((frame, a, jaw))
    return clips


def piecewise(p, keys):
    for (x,a),(y,b) in zip(keys,keys[1:]):
        if p <= y: return a + (b-a)*(p-x)/(y-x)
    return keys[-1][1]


def interpolate(a, b, t):
    return ({n: q.slerp(b[0][n], t) for n,q in a[0].items()}, a[1].lerp(b[1], t),
            {n:p.lerp(b[2][n],t) for n,p in a[2].items()})


def support_limbs(rig, feet, rotations):
    """Preserve source paw trajectories on the wolf's different segment lengths."""
    def aim(name, head, tail):
        bone=rig.data.bones[name]
        rotation=(bone.tail_local-bone.head_local).rotation_difference(tail-head) @ bone.matrix_local.to_quaternion()
        rig.pose.bones[name].matrix=Matrix.LocRotScale(head,rotation,Vector((1,1,1)))
        bpy.context.view_layer.update()
    for prefix,delta in feet.items():
        side='Left' if prefix.startswith('Left') else 'Right'
        front=prefix.endswith('Fore')
        upper=side+('Foreleg' if front else 'Thigh')
        lower=side+('ForeShin' if front else 'Hock')
        ankle=prefix+'Paw'
        toe=prefix+'Toe'
        foot_rotation=rotations[FEET[prefix]]
        displacement=delta.copy()
        displacement.x*=.45
        end=rig.data.bones[toe].head_local+displacement
        target=end+foot_rotation @ (rig.data.bones[ankle].head_local-rig.data.bones[toe].head_local)
        start=rig.pose.bones[upper].head.copy()
        l1,l2=rig.data.bones[upper].length,rig.data.bones[lower].length
        direction=target-start
        distance=max(.0001,min(direction.length,(l1+l2)*.9995))
        direction.normalize()
        target=start+direction*distance
        pole=Vector((0,1 if front else -1,0))
        perpendicular=(pole-direction*pole.dot(direction)).normalized()
        along=(l1*l1-l2*l2+distance*distance)/(2*distance)
        height=math.sqrt(max(0,l1*l1-along*along))
        knee=start+direction*along+perpendicular*height
        aim(upper,start,knee)
        aim(lower,knee,target)
        aim(ankle,target,end)
        rig.pose.bones[toe].matrix=Matrix.LocRotScale(end,
            foot_rotation @ rig.data.bones[toe].matrix_local.to_quaternion(),Vector((1,1,1)))
        bpy.context.view_layer.update()


def material():
    image = bpy.data.images.new('WolfCoat', width=128, height=256, alpha=True)
    image.colorspace_settings.name = 'Non-Color'
    # Linear colors are encoded to sRGB for the runtime texture boundary.
    srgb = lambda c: 12.92*c if c <= .0031308 else 1.055*c**(1/2.4)-.055
    pixels = []
    smooth = lambda a,b,x: (lambda t:t*t*(3-2*t))(max(0,min(1,(x-a)/(b-a))))
    mix = lambda a,b,t: tuple(x*(1-t)+y*t for x,y in zip(a,b))
    for y in range(256):
        for x in range(128):
            if y < 32:
                color = PALETTE[x // 16]
            else:
                along = -.9 + (x+.5)/128*1.85
                height = (y-32+.5)/224*1.25
                color = PALETTE[0]
                color = mix(color, PALETTE[3], (1-smooth(.52,.77,height))*.85)
                color = mix(color, PALETTE[2], (1-smooth(-.52,-.35,along))*.7)
                color = mix(color, PALETTE[1], smooth(.76,1.02,height)*smooth(-.5,-.3,along))
                color = mix(color, PALETTE[0], smooth(.73,.87,along))
                color = mix(color, PALETTE[1], 1-smooth(.10,.18,height))
                grain = .98+.035*math.sin(x*17.1+y*37.7)*math.sin(x*5.3-y*11.9)
                color = tuple(c*grain for c in color[:3])+(1,)
            pixels.extend([srgb(v) for v in color[:3]] + [1])
    image.pixels = pixels
    image.file_format = 'PNG'
    image.pack()
    image.colorspace_settings.name = 'sRGB'
    mat = bpy.data.materials.new('WolfCoat')
    mat.use_nodes = True
    mat.use_backface_culling = True
    shader = mat.node_tree.nodes.get('Principled BSDF')
    shader.inputs['Roughness'].default_value = .85
    tex = mat.node_tree.nodes.new('ShaderNodeTexImage')
    tex.image = image
    mat.node_tree.links.new(tex.outputs['Color'], shader.inputs['Base Color'])
    return mat


def paint(obj, mat, tile=None):
    obj.data.materials.clear()
    obj.data.materials.append(mat)
    uv = obj.data.uv_layers.active or obj.data.uv_layers.new(name='UVMap')
    for face in obj.data.polygons:
        for loop in face.loop_indices:
            if tile is not None:
                uv.data[loop].uv = ((tile+.5)/8, .0625)
            else:
                point = obj.data.vertices[obj.data.loops[loop].vertex_index].co
                uv.data[loop].uv = (max(.01,min(.99,(point.y+.9)/1.85)),
                    max(.13,min(.99,.125+point.z/1.25*.875)))
        face.material_index = 0
        face.use_smooth = True


def detail(name, location, scale, bone, tile, rig, mat):
    # Explicit triangles avoid Blender's pointer-dependent sphere face ordering.
    points=[Vector((0,0,1))]
    for latitude in range(1,4):
        angle=math.pi*latitude/4
        points.extend(Vector((math.sin(angle)*math.cos(i*math.tau/8),
            math.sin(angle)*math.sin(i*math.tau/8),math.cos(angle))) for i in range(8))
    points.append(Vector((0,0,-1)))
    faces=[]
    for i in range(8): faces.append((0,1+i,1+(i+1)%8))
    for ring in range(2):
        for i in range(8):
            a,b=1+ring*8+i,1+ring*8+(i+1)%8
            faces.extend([(a,a+8,b+8),(a,b+8,b)])
    for i in range(8): faces.append((25,17+(i+1)%8,17+i))
    mesh=bpy.data.meshes.new(name)
    mesh.from_pydata([Vector(location)+Vector(tuple(p[i]*scale[i] for i in range(3))) for p in points],[],faces)
    mesh.update()
    obj=bpy.data.objects.new(name,mesh)
    bpy.context.collection.objects.link(obj)
    paint(obj, mat, tile)
    group = obj.vertex_groups.new(name=bone)
    group.add(list(range(len(obj.data.vertices))), 1, 'REPLACE')
    modifier = obj.modifiers.new('WolfRig','ARMATURE')
    modifier.object = rig
    obj.parent = rig
    return obj


def normalize_weights(body):
    # Blender and runtime must evaluate the same four normalized influences.
    for vertex in body.data.vertices:
        weights = sorted(((g.group,g.weight) for g in vertex.groups if g.weight > 1e-6), key=lambda item: (-item[1],item[0]))[:4]
        assert weights, f'Unweighted wolf vertex {vertex.index}'
        for group in body.vertex_groups: group.remove([vertex.index])
        total=sum(weight for _,weight in weights)
        for index,weight in weights: body.vertex_groups[index].add([vertex.index],weight/total,'REPLACE')


def build_body():
    assert bpy.ops.wm.open_mainfile(filepath=str(BODY), use_scripts=False) == {'FINISHED'}
    if bpy.context.object and bpy.context.object.mode != 'OBJECT':
        assert bpy.ops.object.mode_set(mode='OBJECT') == {'FINISHED'}
    bpy.context.preferences.filepaths.save_version = 0
    rig, body = bpy.data.objects['Armature'], bpy.data.objects['Wolf']
    rig.name = rig.data.name = 'WolfRig'
    rig.animation_data_clear()
    for pb in rig.pose.bones:
        pb.matrix_basis = Matrix.Identity(4)
    for obj in list(bpy.data.objects):
        if obj not in (rig,body): bpy.data.objects.remove(obj,do_unlink=True)
    # Apply the authored symmetry before normalizing mesh and bind skeleton together.
    bpy.context.view_layer.objects.active = body
    assert bpy.ops.object.modifier_apply(modifier=body.modifiers[0].name) == {'FINISHED'}
    for b in rig.data.bones:
        if b.name in NAMES: b.name = NAMES[b.name]
    # Blender bone renaming updates its skinned vertex groups.
    for vertex in body.data.vertices:
        vertex.co *= BODY_SCALE
        x,y,z=vertex.co
        # Preserve the authored surface, emphasizing paired ears and abdominal tuck.
        if y < -.44 and abs(x) > .045 and z > 1.08:
            ear=max(0,min(1,(z-1.08)/.065))
            vertex.co.z += .075*ear
            vertex.co.x += math.copysign(.016*ear,x)
        if .45 < z < .88 and -.05 < y < .60:
            tuck=math.sin((y+.05)/.65*math.pi)**2 * max(0,1-(z-.45)/.43)
            vertex.co.z += .11*tuck
            vertex.co.x *= 1-.12*tuck
    bpy.context.view_layer.objects.active = rig
    assert bpy.ops.object.mode_set(mode='EDIT') == {'FINISHED'}
    original = {b.name: (b.head.copy(), b.tail.copy()) for b in rig.data.edit_bones}
    for b in rig.data.edit_bones:
        b.use_connect = False
    for b in rig.data.edit_bones:
        b.head = original[b.name][0] * BODY_SCALE
        b.tail = original[b.name][1] * BODY_SCALE
    root = rig.data.edit_bones.new('Root')
    root.head,root.tail = (0,0,0),(0,.15,0)
    rig.data.edit_bones['Pelvis'].parent = root
    rig.data.edit_bones['Tail1'].parent = rig.data.edit_bones['Pelvis']
    assert bpy.ops.object.mode_set(mode='OBJECT') == {'FINISHED'}
    body.name = body.data.name = 'Wolf'
    normalize_weights(body)
    mat = material()
    paint(body, mat)
    bpy.context.view_layer.update()
    surface = BVHTree.FromObject(body,bpy.context.evaluated_depsgraph_get())
    # Authored muzzle, eyelids and teeth stay attached to their anatomical joints.
    for sign,side in [(-1,'Right'),(1,'Left')]:
        point,_,_,_=surface.ray_cast(Vector((sign,-.575,.996)),Vector((-sign,0,0)))
        assert point is not None, 'Eye must meet the authored head surface'
        eye=point+Vector((sign*.003,0,0))
        detail(side+'Eye',eye,(.022,.020,.018),'Head',4,rig,mat)
        detail(side+'Iris',eye+Vector((sign*.010,-.011,.001)),(.010,.010,.010),'Head',5,rig,mat)
    point,_,_,_=surface.ray_cast(Vector((0,-2,.794)),Vector((0,1,0)))
    assert point is not None, 'Nose must meet the authored muzzle surface'
    detail('Nose',point+Vector((0,-.008,0)),(.048,.030,.033),'Head',4,rig,mat)
    return rig, body, mat


def bake(rig, body, clips):
    rest = {b.name: b.matrix_local.copy() for b in rig.data.bones}
    order = list(rig.data.bones)
    rig.animation_data_create()
    scene = bpy.context.scene
    scene.render.fps = 30
    for name, samples in clips.items():
        action = bpy.data.actions.new(name)
        action.use_fake_user = True
        rig.animation_data.action = action
        previous = {}
        baked_rotations = {}
        run_floor = float('inf')
        for frame, (source, travel, feet), jaw in samples:
            for pb in rig.pose.bones:
                pb.matrix_basis = Matrix.Identity(4)
                pb.rotation_mode = 'QUATERNION'
            global_rotations = {}
            head_delta = source['head']
            axis = head_delta @ rest['Head'].to_quaternion() @ Vector((0,1,0))
            tilt = math.atan2(axis.z,-axis.y)
            if name != 'Death':
                head_delta = Quaternion((1,0,0),tilt-max(-.65,min(.35,tilt))) @ head_delta
            for bone in order:
                name_b = bone.name
                if name_b == 'Root':
                    global_rotations[name_b] = Quaternion()
                    continue
                parent_q = global_rotations[bone.parent.name] if bone.parent else Quaternion()
                local_bind = rest[bone.parent.name].inverted() @ rest[name_b] if bone.parent else rest[name_b]
                local_q = local_bind.to_quaternion()
                if name_b in MAP:
                    delta = source[MAP[name_b]]
                    if MAP[name_b] == 'head': delta = head_delta
                    # Tail source is carried horizontally; retain the wolf's hanging bind tail.
                    if name_b.startswith('Tail'): delta = Quaternion().slerp(delta,.3)
                    wanted = delta @ rest[name_b].to_quaternion()
                    q = local_q.inverted() @ parent_q.inverted() @ wanted
                else:
                    q = Quaternion()
                if name_b == 'Jaw': q @= Quaternion((1,0,0),math.radians(jaw))
                if name_b in previous and previous[name_b].dot(q) < 0: q.negate()
                rig.pose.bones[name_b].rotation_quaternion = q
                global_rotations[name_b] = parent_q @ local_q @ q
                previous[name_b] = q.copy()
            # Root travel is visual only; the sim continues to own ground movement.
            root = rig.pose.bones['Root']
            root.location = (0, travel.y if name=='Bite' else 0, travel.z)
            bpy.context.view_layer.update()
            support_limbs(rig,feet,source)
            if name == 'Death':
                progress=(frame-1)/(len(samples)-1)
                fall=max(0,min(1,(progress-.4)/.6))
                fall=fall*fall*(3-2*fall)
                turn=Quaternion((0,1,0),math.pi/2*fall)
                pivot=rest['Pelvis'].translation
                root.rotation_quaternion=turn
                root.location+=pivot-turn@pivot
                bpy.context.view_layer.update()
            deps = bpy.context.evaluated_depsgraph_get()
            evaluated = body.evaluated_get(deps)
            low = min((evaluated.matrix_world @ v.co).z for v in evaluated.data.vertices)
            if name == 'Run': run_floor=min(run_floor,low)
            elif name != 'Leap':
                root.location.z -= low
                bpy.context.view_layer.update()
            elif name == 'Leap':
                root.location.z += max(0,-low)
                bpy.context.view_layer.update()
            for pb in rig.pose.bones:
                # The analytical support solve can alter local offsets at the
                # fitted paw; bake them alongside rotations, never as runtime IK.
                q=pb.rotation_quaternion.normalized()
                if pb.name in baked_rotations and baked_rotations[pb.name].dot(q)<0: q.negate()
                pb.rotation_quaternion=q
                baked_rotations[pb.name]=q.copy()
                pb.keyframe_insert(data_path='rotation_quaternion', frame=frame)
                pb.keyframe_insert(data_path='location',frame=frame)
        for curve in action_curves(action):
            for key in curve.keyframe_points:
                key.interpolation='LINEAR'
                if name=='Run' and curve.data_path=='pose.bones["Root"].location' and curve.array_index==2:
                    key.co.y -= run_floor
    rig.animation_data.action = bpy.data.actions['Idle']
    rig.animation_data.action_slot = bpy.data.actions['Idle'].slots[0]
    scene.frame_set(1)


def export(out, rig, shadow=False):
    stem = 'wolf.shadow' if shadow else 'wolf'
    if shadow:
        for obj in list(bpy.data.objects):
            if obj.type=='MESH' and obj.name != 'Wolf': bpy.data.objects.remove(obj,do_unlink=True)
        body = bpy.data.objects['Wolf']
        bpy.context.view_layer.objects.active = body
        rig.data.pose_position = 'REST'
        decimate=body.modifiers.new('ShadowSilhouette','DECIMATE')
        decimate.ratio=.55
        assert bpy.ops.object.modifier_move_up(modifier=decimate.name) == {'FINISHED'}
        assert bpy.ops.object.modifier_apply(modifier=decimate.name) == {'FINISHED'}
        normalize_weights(body)
        rig.data.pose_position = 'POSE'
    # One material primitive shares all facial details and the coat.
    meshes=[o for o in bpy.context.scene.objects if o.type=='MESH']
    if len(meshes)>1:
        bpy.ops.object.select_all(action='DESELECT')
        for obj in meshes: obj.select_set(True)
        bpy.context.view_layer.objects.active=bpy.data.objects['Wolf']
        assert bpy.ops.object.join() == {'FINISHED'}
    out.mkdir(parents=True,exist_ok=True)
    bpy.context.scene.render.image_settings.file_format='PNG'
    assert bpy.ops.wm.save_as_mainfile(filepath=str(out / (stem+'.blend'))) == {'FINISHED'}
    bpy.ops.object.select_all(action='SELECT')
    assert bpy.ops.export_scene.gltf(filepath=str(out / (stem+'.glb')),export_format='GLB',
        export_cameras=False,export_lights=False,export_apply=False,export_animations=True,
        export_animation_mode='ACTIONS',export_nla_strips=False,export_force_sampling=True,
        export_frame_range=False,export_optimize_animation_size=False,
        export_def_bones=False) == {'FINISHED'}


def main():
    args=sys.argv[sys.argv.index('--')+1:] if '--' in sys.argv else []
    out=(Path(args[0]) if args else HERE).resolve()
    clips=source_samples()
    rig,body,mat=build_body()
    bake(rig,body,clips)
    # Save visible before creating its reduced caster from the same rig/actions.
    export(out,rig)
    export(out,rig,True)
    (out/'build.json').write_text(json.dumps({'blender':bpy.app.version_string,
        'compiler_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        'sources':{str(p.relative_to(ROOT)):hashlib.sha256(p.read_bytes()).hexdigest() for p in [BODY,MOTION,MOTION_TOOLS]},
        'artifacts':{p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(out.glob('*.glb'))}},indent=2)+'\n')


if __name__=='__main__': main()
