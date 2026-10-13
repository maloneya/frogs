"""The accepted CC0 body surface and its semantic-region skin weights."""
from pathlib import Path
import bpy
from mathutils import Matrix, Vector

SOURCE = Path(__file__).resolve().parent / 'reference/realistic-male.blend'
BODY = 'GEO-body_male_realistic'
NAMES = [BODY, BODY + '.eye.L', BODY + '.eye.R']
CENTRAL_REGIONS = {18: 'Pelvis', 19: 'Spine'}
LIMB_REGIONS = {20: 'Arm', 21: 'Arm', 11: 'Forearm', 12: 'Forearm',
                9: 'Hand', 10: 'Hand', 15: 'Shin', 16: 'Shin',
                23: 'Leg', 24: 'Leg', 13: 'Foot', 14: 'Foot'}


def import_body():
    with bpy.data.libraries.load(str(SOURCE), link=False) as (_, data):
        data.objects = list(NAMES)
    body = data.objects[0]
    low = min(v.co.z for v in body.data.vertices)
    height = max(v.co.z for v in body.data.vertices) - low
    scale = 2 / height
    # Only uniform unit normalization. No torso/limb/head reshaping.
    origin = body.location.copy()
    for obj in data.objects:
        bpy.context.collection.objects.link(obj)
        matrix = Matrix.Translation(Vector((0, 0, -low))) @ Matrix.Translation(obj.location - origin)
        obj.data.transform(Matrix.Scale(scale, 4) @ matrix)
        obj.matrix_world = Matrix.Identity(4)
        obj.modifiers.clear()
        obj.hide_viewport = obj.hide_render = False
        for face in obj.data.polygons: face.use_smooth = True
    return data.objects, scale, low


def region_bone(region, centre):
    if region in CENTRAL_REGIONS:
        return CENTRAL_REGIONS[region]
    side = 'Right' if centre.x > 0 else 'Left'
    if region in LIMB_REGIONS:
        return side + LIMB_REGIONS[region]
    if 25 <= region <= 63:
        return side + 'Foot'
    if 80 <= region <= 87:
        return side + 'Thumb'
    if region >= 64:
        return side + 'Fingers'
    if region == 1:
        if centre.z > 1.70: return 'Head'
        if centre.z > 1.58: return 'Neck'
        if centre.z > 1.40: return 'Chest'
        return 'Spine'
    return 'Head'


def skin_body(body):
    sets = body.data.attributes['.sculpt_face_set']
    rows = [{} for _ in body.data.vertices]
    for face, value in zip(body.data.polygons, sets.data):
        centre = sum((body.data.vertices[i].co for i in face.vertices), Vector()) / len(face.vertices)
        name = region_bone(value.value, centre)
        for i in face.vertices: rows[i][name] = rows[i].get(name, 0) + 1
    rows = [{n: w / sum(row.values()) for n, w in row.items()} for row in rows]
    neighbors = [set() for _ in rows]
    for edge in body.data.edges:
        a, b = edge.vertices; neighbors[a].add(b); neighbors[b].add(a)
    # Smooth the author's semantic region boundaries along surface adjacency;
    # never blend a nearby arm into the torso merely because it is close in space.
    for _ in range(8):
        next_rows = []
        for i, row in enumerate(rows):
            blended = {n: w * .5 for n, w in row.items()}
            for j in neighbors[i]:
                for n, w in rows[j].items(): blended[n] = blended.get(n, 0) + w * .5 / len(neighbors[i])
            strongest = sorted(blended.items(), key=lambda pair: (-pair[1], pair[0]))[:4]
            total = sum(w for _, w in strongest)
            next_rows.append({n: w / total for n, w in strongest})
        rows = next_rows
    groups = {n: body.vertex_groups.new(name=n) for n in sorted({n for row in rows for n in row})}
    for i, row in enumerate(rows):
        for n, w in row.items(): groups[n].add([i], w, 'REPLACE')


