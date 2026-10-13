"""Package explicit authored GLB clips without changing retained payloads."""
from pathlib import Path
import argparse
import hashlib
import json
import struct
def extract(source, destination, clips):
    clips = set(clips)
    if not clips:
        raise ValueError('Select at least one clip')
    data = source.read_bytes()
    assert data[:4] == b'glTF'
    length = struct.unpack_from('<I', data, 12)[0]
    document = json.loads(data[20:20 + length])
    binary_offset = 20 + length
    binary_length, kind = struct.unpack_from('<II', data, binary_offset)
    assert kind == 0x004e4942
    binary = data[binary_offset + 8:binary_offset + 8 + binary_length]
    document['animations'] = [a for a in document['animations'] if a['name'] in clips]
    missing = clips - {a['name'] for a in document['animations']}
    if missing:
        raise ValueError(f'Clips not present in source: {sorted(missing)}')
    # Retain only the used accessors/views; each retained byte is copied unchanged.
    used = set()
    for mesh in document['meshes']:
        for primitive in mesh['primitives']:
            used.update(primitive['attributes'].values())
            if 'indices' in primitive: used.add(primitive['indices'])
    for skin in document['skins']: used.add(skin['inverseBindMatrices'])
    for animation in document['animations']:
        for sampler in animation['samplers']: used.update((sampler['input'], sampler['output']))
    accessor_map = {old: new for new, old in enumerate(sorted(used))}
    accessors = [document['accessors'][old] for old in sorted(used)]
    used_views = {a['bufferView'] for a in accessors}
    used_views.update(image['bufferView'] for image in document.get('images', []))
    view_map = {old: new for new, old in enumerate(sorted(used_views))}
    packed = bytearray()
    views = []
    for old in sorted(used_views):
        view = dict(document['bufferViews'][old])
        packed.extend(b'\0' * (-len(packed) % 4))
        start = view.get('byteOffset', 0)
        view['byteOffset'] = len(packed)
        packed.extend(binary[start:start + view['byteLength']])
        views.append(view)
    for accessor in accessors: accessor['bufferView'] = view_map[accessor['bufferView']]
    for mesh in document['meshes']:
        for primitive in mesh['primitives']:
            primitive['attributes'] = {k: accessor_map[v] for k, v in primitive['attributes'].items()}
            if 'indices' in primitive: primitive['indices'] = accessor_map[primitive['indices']]
    for skin in document['skins']: skin['inverseBindMatrices'] = accessor_map[skin['inverseBindMatrices']]
    for animation in document['animations']:
        for sampler in animation['samplers']:
            for key in ['input', 'output']: sampler[key] = accessor_map[sampler[key]]
    for image in document.get('images', []): image['bufferView'] = view_map[image['bufferView']]
    document['accessors'], document['bufferViews'] = accessors, views
    document['buffers'] = [{'byteLength': len(packed)}]
    encoded = json.dumps(document, separators=(',', ':'), ensure_ascii=False).encode()
    encoded += b' ' * (-len(encoded) % 4)
    packed.extend(b'\0' * (-len(packed) % 4))
    result = struct.pack('<III', 0x46546c67, 2, 12 + 8 + len(encoded) + 8 + len(packed))
    result += struct.pack('<II', len(encoded), 0x4e4f534a) + encoded
    result += struct.pack('<II', len(packed), 0x004e4942) + packed
    destination.write_bytes(result)
    print(f'input sha256={hashlib.sha256(data).hexdigest()}')
    print(f'output sha256={hashlib.sha256(result).hexdigest()} bytes={len(result)}')

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('destination', type=Path)
    parser.add_argument('clips', nargs='+')
    arguments = parser.parse_args()
    extract(arguments.source, arguments.destination, arguments.clips)


if __name__ == '__main__':
    main()
