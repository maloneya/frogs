"""Blender-side checks of diagnostic isolation and matched camera framing."""
from pathlib import Path
import sys
import unittest

import bpy
from mathutils import Vector

sys.path.insert(0, str(Path(__file__).resolve().parent))
from preview import full_camera, grip_cutaway


class Diagnostics(unittest.TestCase):
    def test_cutaway_preserves_authored_mesh_and_restores_intact_context(self):
        bpy.ops.wm.read_factory_settings(use_empty=True)
        parts = []
        for group, x in [('Pelvis', 0), ('RightHand', 2), ('Weapon', 4)]:
            bpy.ops.mesh.primitive_cube_add(location=(x, 0, 0))
            obj = bpy.context.object
            obj.vertex_groups.new(name=group).add(list(range(8)), 1, 'REPLACE')
            parts.append(obj)
        bpy.ops.object.select_all(action='SELECT')
        bpy.context.view_layer.objects.active = parts[0]
        bpy.ops.object.join()
        body = bpy.context.object
        before = [tuple(v.co) for v in body.data.vertices]
        bpy.ops.mesh.primitive_cube_add(location=(0, 0, 2))
        hood = bpy.context.object
        hood.vertex_groups.new(name='Head').add(list(range(8)), 1, 'REPLACE')
        show = grip_cutaway()
        show(True)
        visible = [o for o in bpy.context.scene.objects if o.type == 'MESH' and not o.hide_render]
        self.assertEqual(len(visible), 1)
        self.assertEqual(len(visible[0].data.vertices), 16)
        self.assertTrue(all(v.co.x >= 1 for v in visible[0].data.vertices))
        self.assertEqual([tuple(v.co) for v in body.data.vertices], before)
        self.assertTrue(body.hide_render and hood.hide_render)
        show(False)
        self.assertFalse(body.hide_render or hood.hide_render)
        self.assertTrue(visible[0].hide_render)
        self.assertEqual([tuple(v.co) for v in body.data.vertices], before)

    def test_camera_contains_all_extremes_and_is_independent_of_variant_order(self):
        camera = bpy.data.objects.new('TestCamera', bpy.data.cameras.new('TestCamera'))
        points = [Vector((x, y, z)) for x in (-1, 2) for y in (-2, 1) for z in (0, 3)]
        spec = full_camera(camera, (5, -5, 6), points)
        self.assertEqual(spec, full_camera(camera, (5, -5, 6), list(reversed(points))))
        inverse = camera.rotation_euler.to_quaternion().inverted()
        centre = Vector(spec['target'])
        for point in points:
            projected = inverse @ (point - centre)
            self.assertLess(abs(projected.x), spec['scale'] / 2)
            self.assertLess(abs(projected.y), spec['scale'] / 2)


if __name__ == '__main__':
    result = unittest.TextTestRunner(verbosity=2).run(unittest.defaultTestLoader.loadTestsFromTestCase(Diagnostics))
    if not result.wasSuccessful():
        raise RuntimeError('Preview diagnostics failed')
