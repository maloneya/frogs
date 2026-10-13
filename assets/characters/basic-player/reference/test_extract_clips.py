"""Check packaging preserves skin, geometry and selected authored curves."""
from pathlib import Path
import json
import struct
import tempfile
import unittest

from extract_clips import extract

HERE = Path(__file__).resolve().parent
USED_CLIPS = {'Sword_Regular_B', 'Sword_Regular_B_Rec'}


def read_glb(path):
    data = path.read_bytes()
    assert data[:4] == b'glTF'
    length = struct.unpack_from('<I', data, 12)[0]
    document = json.loads(data[20:20 + length])
    binary_length, kind = struct.unpack_from('<II', data, 20 + length)
    assert kind == 0x004e4942
    return document, data[28 + length:28 + length + binary_length]


def accessor_payload(document, binary, index):
    accessor = document['accessors'][index]
    view = document['bufferViews'][accessor['bufferView']]
    begin = view.get('byteOffset', 0)
    # Preserve metadata and exact view bytes, including interleaved mesh records.
    return (
        {key: value for key, value in accessor.items() if key != 'bufferView'},
        {key: value for key, value in view.items() if key not in {'buffer', 'byteOffset'}},
        binary[begin:begin + view['byteLength']],
    )


def animation_channels(document, binary, animation):
    channels = []
    for channel in animation['channels']:
        target = dict(channel['target'])
        target['node'] = document['nodes'][target['node']]['name']
        sampler = animation['samplers'][channel['sampler']]
        channels.append((
            target,
            sampler.get('interpolation', 'LINEAR'),
            accessor_payload(document, binary, sampler['input']),
            accessor_payload(document, binary, sampler['output']),
        ))
    return channels


class ClipExtractionTests(unittest.TestCase):
    def check_payloads(self, clips):
        source = HERE / 'cleave-source.glb'
        with tempfile.TemporaryDirectory() as directory:
            destination = Path(directory) / 'sword.glb'
            repeated = Path(directory) / 'repeated.glb'
            extract(source, destination, clips)
            extract(source, repeated, clips)
            self.assertEqual(destination.read_bytes(), repeated.read_bytes())
            before, before_binary = read_glb(source)
            after, after_binary = read_glb(destination)
            self.assertEqual(before['nodes'], after['nodes'])
            self.assertEqual({animation['name'] for animation in after['animations']}, clips)
            self.assertEqual(len(before['skins']), len(after['skins']))
            for left, right in zip(before['skins'], after['skins']):
                self.assertEqual(
                    {key: value for key, value in left.items() if key != 'inverseBindMatrices'},
                    {key: value for key, value in right.items() if key != 'inverseBindMatrices'},
                )
                self.assertEqual(
                    accessor_payload(before, before_binary, left['inverseBindMatrices']),
                    accessor_payload(after, after_binary, right['inverseBindMatrices']),
                )
            self.assertEqual(len(before['meshes']), len(after['meshes']))
            for left, right in zip(before['meshes'], after['meshes']):
                self.assertEqual(len(left['primitives']), len(right['primitives']))
                for original, retained in zip(left['primitives'], right['primitives']):
                    self.assertEqual(
                        {key: value for key, value in original.items() if key not in {'attributes', 'indices'}},
                        {key: value for key, value in retained.items() if key not in {'attributes', 'indices'}},
                    )
                    self.assertEqual(original['attributes'].keys(), retained['attributes'].keys())
                    for attribute, index in original['attributes'].items():
                        self.assertEqual(
                            accessor_payload(before, before_binary, index),
                            accessor_payload(after, after_binary, retained['attributes'][attribute]),
                        )
                    self.assertEqual(
                        accessor_payload(before, before_binary, original['indices']),
                        accessor_payload(after, after_binary, retained['indices']),
                    )
            for name in clips:
                original = next(animation for animation in before['animations'] if animation['name'] == name)
                retained = next(animation for animation in after['animations'] if animation['name'] == name)
                self.assertEqual(
                    animation_channels(before, before_binary, original),
                    animation_channels(after, after_binary, retained),
                )

    def test_used_cleave_pair_preserves_payloads(self):
        self.check_payloads(USED_CLIPS)

    def test_single_recovery_clip_preserves_payloads_after_accessor_remapping(self):
        self.check_payloads({'Sword_Regular_B_Rec'})

    def test_empty_or_missing_clip_refuses_without_replacing_destination(self):
        with tempfile.TemporaryDirectory() as directory:
            destination = Path(directory) / 'accepted.glb'
            destination.write_bytes(b'accepted')
            for clips in [set(), {'NoSuchClip'}]:
                with self.subTest(clips=clips), self.assertRaises(ValueError):
                    extract(HERE / 'cleave-source.glb', destination, clips)
                self.assertEqual(destination.read_bytes(), b'accepted')


if __name__ == '__main__':
    unittest.main()
