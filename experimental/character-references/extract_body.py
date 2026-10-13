"""Retain one Blender Human Base Mesh body and eyes without changing base topology."""
from pathlib import Path
import sys

import bpy


def main():
    source, destination, body = sys.argv[sys.argv.index('--') + 1:]
    names = [body, body + '.eye.L', body + '.eye.R']
    bpy.ops.wm.read_factory_settings(use_empty=True)
    with bpy.data.libraries.load(str(Path(source).resolve()), link=False) as (available, retained):
        if not all(name in available.objects for name in names):
            raise ValueError(f'Body not present in bundle: {body}')
        retained.objects = names
        retained.texts = ['README']
    # Retain the authored base surface, matching the existing anatomy reference.
    # The multires sculpt cache is an authoring detail, not a second base mesh.
    for obj in retained.objects:
        obj.modifiers.clear()
    bpy.data.libraries.write(str(Path(destination).resolve()),
                             set(retained.objects + retained.texts),
                             fake_user=True, compress=True)
    print('EXTRACTED', body, destination)


if __name__ == '__main__':
    main()
