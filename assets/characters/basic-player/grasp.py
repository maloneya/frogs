"""Anatomical grasp calibration, cylindrical closure and baked equipment constraints."""
from dataclasses import dataclass
import math
import bpy
from mathutils import Quaternion, Vector
import anatomy
from motion import world_rotation, set_linear_keys

DIGITS = tuple(anatomy.DIGIT_REGIONS['Right'])
HANDLE_RADIUS = .021
SUPPORT_DISTANCE = .15
CONTACT_TOLERANCE = .006
ARM_POLES = ((-.85,-.1,1.4),(.85,-.1,1.4))


@dataclass
class Hand:
    side: str
    points: dict
    origin: Vector
    axis: Vector
    normal: Vector
    angles: dict


def calibrate(body):
    """Use authored segment boundaries, rather than guessed finger joint locations."""
    sets = body.data.attributes['.sculpt_face_set']
    hands = {}
    for side, sign in anatomy.SIDES:
        regions = {}
        for polygon, region in zip(body.data.polygons, sets.data):
            for index in polygon.vertices:
                if body.data.vertices[index].co.x * sign > 0:
                    regions.setdefault(region.value, set()).add(index)
        palm = regions.get(9, set()) | regions.get(10, set())
        mean = lambda ids: sum((body.data.vertices[i].co for i in ids), Vector()) / len(ids)
        points = {}
        for digit, base in anatomy.DIGIT_REGIONS[side].items():
            boundaries = [palm & regions[base]] + [regions[base+i] & regions[base+i+1] for i in range(3)]
            assert all(boundaries), (side, digit, 'missing anatomical boundary')
            points[digit] = [mean(ids) for ids in boundaries]
        roots = [points[digit][0] for digit in DIGITS if digit != 'Thumb']
        centre = sum(roots, Vector()) / 4
        axis = (roots[-1] - roots[0]).normalized()
        forward = sum((points[d][1] - points[d][0] for d in DIGITS if d != 'Thumb'), Vector()).normalized()
        normal = forward.cross(axis).normalized()
        if normal.x * sign > 0: normal.negate()
        origin = centre + forward * .018
        local_palm = [body.data.vertices[i].co for i in palm
                      if abs((body.data.vertices[i].co - centre).dot(forward)) < .025
                      and abs((body.data.vertices[i].co - centre).dot(axis)) < .04]
        assert local_palm
        surface = max(p.dot(normal) for p in local_palm)
        origin += normal * (surface + HANDLE_RADIUS + .001 - origin.dot(normal))
        hand = Hand(side, points, origin, axis, normal, {})
        hand.angles = {digit: solve_digit(hand, digit) for digit in DIGITS}
        hands[side] = hand
    return hands


def digit_points(hand, digit, angles):
    rest = hand.points[digit]
    # Thumb opposition is a separate basal degree of freedom; flexion alone
    # cannot bring its initially lateral axis around the handle.
    opposition = Quaternion(hand.normal, angles[0]) if digit == 'Thumb' else Quaternion()
    bends = angles[1:] if digit == 'Thumb' else angles
    points = [rest[0]]
    cumulative = 0
    for index, bend in enumerate(bends):
        cumulative += bend
        delta = Quaternion(hand.axis, cumulative) @ (opposition @ (rest[index+1] - rest[index]))
        points.append(points[-1] + delta)
    return points


def solve_digit(hand, digit):
    def loss(angles):
        points = digit_points(hand, digit, angles)
        value = 0
        for index in range(3):
            # Centre-lines stay outside the cylinder by the segment's skin
            # thickness. Include segment interiors to catch a chord through it.
            radius = .008 if digit != 'Thumb' else .010
            for fraction in [.5, 1.0]:
                point = points[index].lerp(points[index+1], fraction)
                offset = point - hand.origin
                radial = (offset - hand.axis * offset.dot(hand.axis)).length
                gap = radial - HANDLE_RADIUS - radius
                if digit != 'Thumb' or index > 0 or gap < 0:
                    value += gap*gap * (10 if gap < 0 else 1)
        return value
    sign = 1 if hand.side == 'Right' else -1
    if digit == 'Thumb': sign = -sign
    angles = [0.0] * (4 if digit == 'Thumb' else 3)
    limit=sign*math.radians(115)
    bends=[(min(0,limit),max(0,limit))]*3
    bounds=[(-math.pi*.7,math.pi*.7),*bends] if digit=='Thumb' else bends
    for step in [math.radians(30),math.radians(10),math.radians(3),math.radians(1)]:
        for _ in range(12):
            changed = False
            for index, (low, high) in enumerate(bounds):
                old = angles[index]
                choices = [max(low,min(high,old+delta)) for delta in [-step,0,step]]
                best = min(choices, key=lambda a: loss(angles[:index]+[a]+angles[index+1:]))
                angles[index] = best
                changed |= abs(best-old) > 1e-8
            if not changed: break
    print('GRASP',hand.side,digit,'angles',[round(math.degrees(a),1)for a in angles],'loss',loss(angles),flush=True)
    return angles


def add_bones(rig, hands, bone):
    for side, hand in hands.items():
        parent = rig.data.edit_bones[side+'Hand']
        for digit, points in hand.points.items():
            previous = parent
            for i in range(3):
                previous = bone(side+digit+str(i+1),points[i],points[i+1],previous,False)
        bone(side+'Grip',hand.origin,hand.origin+hand.axis*.1,parent,False)
    parent = rig.data.edit_bones['RightGrip']
    return bone('Weapon',parent.head,parent.head+hands['Right'].axis,parent,False)


def close_hand(rig, hand, amount=1):
    for digit, angles in hand.angles.items():
        target_points = digit_points(hand,digit,[a*amount for a in angles])
        parent_rotation = rig.pose.bones[hand.side+'Hand'].matrix.to_quaternion()
        hand_rest = rig.data.bones[hand.side+'Hand'].matrix_local.to_quaternion()
        for i in range(3):
            name=hand.side+digit+str(i+1)
            rest=rig.data.bones[name]
            direction=target_points[i+1]-target_points[i]
            rotation=(rest.tail_local-rest.head_local).rotation_difference(direction)@rest.matrix_local.to_quaternion()
            world_rotation(rig,name,parent_rotation@hand_rest.inverted()@rotation)


def constrain(rig,hands,chain,support,poles=ARM_POLES,support_offset=-SUPPORT_DISTANCE):
    weapon=rig.pose.bones['Weapon']
    desired=weapon.matrix.to_quaternion().copy()
    hand=rig.pose.bones['RightHand']
    local=hand.bone.matrix_local.inverted()@rig.data.bones['RightGrip'].matrix_local
    world_rotation(rig,'RightHand',desired@local.to_quaternion().inverted())
    for name in ['RightGrip','LeftGrip','Weapon']:
        rig.pose.bones[name].matrix_basis.identity()
    bpy.context.view_layer.update()
    close_hand(rig,hands['Right'])
    if support>0:
        left=rig.pose.bones['LeftHand'];grip=rig.pose.bones['Weapon']
        local=left.bone.matrix_local.inverted()@rig.data.bones['LeftGrip'].matrix_local
        target_rotation=grip.matrix.to_quaternion()@local.to_quaternion().inverted()
        right_rotation=rig.pose.bones['RightHand'].matrix.to_quaternion().copy()
        original=rig.pose.bones['RightHand'].head.copy()
        offset=grip.head-original+(grip.tail-grip.head).normalized()*support_offset-target_rotation@local.translation
        wrist=original.copy()
        for _ in range(8):
            for side,shift in [('Right',Vector()),('Left',offset)]:
                start=rig.pose.bones[side+'Arm'].head
                reach=rig.data.bones[side+'Arm'].length+rig.data.bones[side+'Forearm'].length-.01
                delta=wrist+shift-start
                if delta.length>reach: wrist-=delta.normalized()*(delta.length-reach)
        chain(rig,'RightArm','RightForearm',original.lerp(wrist,support),poles[0])
        world_rotation(rig,'RightHand',right_rotation)
        bpy.context.view_layer.update()
        target=grip.head+(grip.tail-grip.head).normalized()*support_offset
        wrist=target-target_rotation@local.translation
        start=left.head.copy();old_rotation=left.matrix.to_quaternion().copy()
        chain(rig,'LeftArm','LeftForearm',start.lerp(wrist,support),poles[1])
        world_rotation(rig,'LeftHand',old_rotation.slerp(target_rotation,support))
    close_hand(rig,hands['Left'],support)


def place(rig,hands,chain,origin,rotation,support,poles=ARM_POLES,support_offset=-SUPPORT_DISTANCE):
    """Author a weapon goal through the same anatomical grip relationship."""
    rotation=rotation.normalized()
    hand=rig.data.bones['RightHand']
    local=hand.matrix_local.inverted() @ rig.data.bones['RightGrip'].matrix_local
    hand_rotation=rotation @ local.to_quaternion().inverted()
    wrist=Vector(origin)-hand_rotation @ local.translation
    chain(rig,'RightArm','RightForearm',wrist,poles[0])
    world_rotation(rig,'RightHand',hand_rotation)
    for name in ['RightGrip','Weapon']:
        rig.pose.bones[name].matrix_basis.identity()
    bpy.context.view_layer.update()
    constrain(rig,hands,chain,support,poles=poles,support_offset=support_offset)


def bake(rig,hands,chain):
    """One final pass owns grasping after every retargeter and before export."""
    for action in list(bpy.data.actions):
        rig.animation_data.action=action;rig.animation_data.action_slot=action.slots[0]
        frames=sorted({point.co.x for layer in action.layers for strip in layer.strips
                       for curve in strip.channelbag(action.slots[0]).fcurves for point in curve.keyframe_points})
        previous = {}
        for frame in frames:
            bpy.context.scene.frame_set(int(frame),subframe=frame-int(frame))
            support=max(0,min(1,(frame-4)/6,(31-frame)/5)) if action.name=='AttackSlam' else 0
            constrain(rig,hands,chain,support)
            for joint in rig.pose.bones:
                joint.rotation_quaternion.normalize()
                if joint.name in previous and joint.rotation_quaternion.dot(previous[joint.name]) < 0:
                    joint.rotation_quaternion.negate()
                previous[joint.name] = joint.rotation_quaternion.copy()
                joint.keyframe_insert(data_path='rotation_quaternion',frame=frame)
                joint.keyframe_insert(data_path='location',frame=frame)
        set_linear_keys(action)


def contact_vertices(body, side):
    result={}
    for digit in DIGITS:
        groups={g.index for g in body.vertex_groups if g.name.startswith(side+digit)}
        result[digit]=[v for v in body.data.vertices if sum(g.weight for g in v.groups if g.group in groups)>.5]
    return result


def validate_contact(rig,body,side,vertices,support_offset=-SUPPORT_DISTANCE):
    """Check the skinned surface, not just agreement between named bone origins."""
    origin=rig.pose.bones['Weapon'].head.copy()
    axis=(rig.pose.bones['Weapon'].tail-origin).normalized()
    if side=='Left': origin+=axis*support_offset
    assert (rig.pose.bones[side+'Grip'].head-origin).length<CONTACT_TOLERANCE, 'palm left its equipment grip'
    palette={bone.name:bone.matrix@bone.bone.matrix_local.inverted() for bone in rig.pose.bones}
    for digit,points in vertices.items():
        gaps=[]
        for vertex in points:
            point=sum((palette[body.vertex_groups[g.group].name]@vertex.co*g.weight for g in vertex.groups),Vector())
            offset=point-origin
            gaps.append((offset-axis*offset.dot(axis)).length-HANDLE_RADIUS)
        assert gaps and min(gaps)>=-CONTACT_TOLERANCE and min(abs(gap) for gap in gaps)<=CONTACT_TOLERANCE, (side,digit,'invalid skin contact',min(gaps),min(map(abs,gaps)))


def validate_motion(rig,body,support_window=(10,26),support_offset=-SUPPORT_DISTANCE):
    vertices={side:contact_vertices(body,side) for side in ['Right','Left']}
    surface=forearm_surface(body,'Right')
    for action in bpy.data.actions:
        rig.animation_data.action=action;rig.animation_data.action_slot=action.slots[0]
        first,last=action.frame_range
        count=192 if action.name=='Idle' else 240
        for index in range(count+1):
            frame=first+(last-first)*index/count
            bpy.context.scene.frame_set(int(frame),subframe=frame-int(frame))
            try:
                validate_contact(rig,body,'Right',vertices['Right'])
                validate_clearance(rig,body,surface)
                if action.name=='AttackSlam' and support_window is not None and support_window[0]<=frame<=support_window[1]:
                    validate_contact(rig,body,'Left',vertices['Left'],support_offset)
            except AssertionError as error:
                raise AssertionError((action.name,frame,*error.args)) from error
    print('PASS grasp: skinned finger/handle contact across every clip and fractional samples',flush=True)


def _segment_triangle(start,end,a,b,c):
    from mathutils.geometry import intersect_ray_tri
    direction=end-start
    if direction.length_squared<1e-12: return False
    hit=intersect_ray_tri(a,b,c,direction,start,True)
    return hit is not None and .00001<(hit-start).dot(direction)/direction.length_squared<.99999


def validate_equipment(rig,body,equipment):
    """Check the rendered triangles, excluding hands covered by grip admission.

    Retain the bind triangulation while deforming vertices. Retessellating bent
    quads or treating an articulated human as one closed solid gives misleading
    inside/outside answers near the shoulders.
    """
    from mathutils.bvhtree import BVHTree
    hands={g.index for g in body.vertex_groups if g.name.endswith('Hand') or
           any(digit in g.name for digit in DIGITS)}
    hand_vertices={v.index for v in body.data.vertices if sum(g.weight for g in v.groups if g.group in hands)>.5}
    body.data.calc_loop_triangles();equipment.data.calc_loop_triangles()
    body_triangles=[tuple(t.vertices) for t in body.data.loop_triangles
                    if not all(i in hand_vertices for i in t.vertices)]
    triangles=[tuple(t.vertices) for t in equipment.data.loop_triangles]
    bind_points=[v.co.copy() for v in equipment.data.vertices]
    inverse=rig.data.bones['Weapon'].matrix_local.inverted()
    worst_floor=float('inf')
    for action in bpy.data.actions:
        rig.animation_data.action=action;rig.animation_data.action_slot=action.slots[0]
        first,last=action.frame_range
        count=120 if action.name=='AttackSlam' else 48
        for index in range(count+1):
            frame=first+(last-first)*index/count
            bpy.context.scene.frame_set(int(frame),subframe=frame-int(frame))
            evaluated=body.evaluated_get(bpy.context.evaluated_depsgraph_get())
            points=[v.co.copy() for v in evaluated.data.vertices]
            tree=BVHTree.FromPolygons(points,body_triangles,all_triangles=True)
            transform=rig.pose.bones['Weapon'].matrix @ inverse
            weapon=[transform @ point for point in bind_points]
            low=min(p.z for p in weapon);worst_floor=min(worst_floor,low)
            assert low>=.005, (action.name,frame,'weapon enters floor',low)
            shape=BVHTree.FromPolygons(weapon,triangles,all_triangles=True)
            for left,right in tree.overlap(shape):
                a=[points[i] for i in body_triangles[left]]
                b=[weapon[i] for i in triangles[right]]
                crossing=any(_segment_triangle(p,q,*a) for p,q in zip(b,b[1:]+b[:1])) or any(
                    _segment_triangle(p,q,*b) for p,q in zip(a,a[1:]+a[:1]))
                regions=sorted({body.vertex_groups[g.group].name for i in body_triangles[left]
                                for g in body.data.vertices[i].groups if g.weight>.2}) if crossing else []
                assert not crossing, (action.name,frame,'weapon crosses body surface',regions,left,right)
    print('PASS equipment: actual weapon triangles clear body/floor; minimum floor',worst_floor,flush=True)
    return worst_floor


def forearm_surface(body,side):
    group=body.vertex_groups[side+'Forearm'].index
    selected={v.index for v in body.data.vertices if any(g.group==group and g.weight>.5 for g in v.groups)}
    polygons=[tuple(p.vertices) for p in body.data.polygons if sum(i in selected for i in p.vertices)>=2]
    ids=sorted({i for polygon in polygons for i in polygon});remap={index:i for i,index in enumerate(ids)}
    faces=[tuple(remap[i]for i in polygon)for polygon in polygons]
    directed={}
    for face in faces:
        for a,b in zip(face,face[1:]+face[:1]):
            directed.setdefault(tuple(sorted((a,b))),[]).append((a,b))
    boundary={a:b for edges in directed.values() if len(edges)==1 for a,b in edges}
    caps=[]
    while boundary:
        start=next(iter(boundary));loop=[];current=start
        while True:
            loop.append(current);current=boundary.pop(current)
            if current==start:break
        centre=len(ids)+len(caps);caps.append(loop)
        faces.extend((b,a,centre)for a,b in zip(loop,loop[1:]+loop[:1]))
    return [body.data.vertices[i]for i in ids],faces,caps



def validate_clearance(rig,body,surface):
    from mathutils.bvhtree import BVHTree
    vertices,faces,caps=surface
    palette={bone.name:bone.matrix@bone.bone.matrix_local.inverted() for bone in rig.pose.bones}
    points=[sum((palette[body.vertex_groups[g.group].name]@v.co*g.weight for g in v.groups),Vector()) for v in vertices]
    points.extend(sum((points[i]for i in loop),Vector())/len(loop)for loop in caps)
    tree=BVHTree.FromPolygons(points,faces)
    weapon=rig.pose.bones['Weapon'];axis=(weapon.tail-weapon.head).normalized()
    for i in range(21):
        point=weapon.head+axis*(-.20+i*.235/20)
        _,_,_,distance=tree.find_nearest(point)
        # Ray parity on the closed shell establishes containment independently
        # of the nearest face's orientation, including at the wrist boundary.
        direction=Vector((.713,.443,.54)).normalized()
        probe=point.copy();crossings=0
        for _ in range(32):
            hit,_,_,_=tree.ray_cast(probe,direction)
            if hit is None:break
            crossings+=1;probe=hit+direction*.00001
        else:raise AssertionError('ambiguous forearm containment')
        assert crossings%2==0 and distance>=HANDLE_RADIUS-CONTACT_TOLERANCE, ('handle intersects forearm',distance,crossings)
