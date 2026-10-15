"""Timing and refusal contracts for the offline visual-review pipeline."""
import hashlib
import json
import re
from pathlib import Path
from tempfile import TemporaryDirectory
import unittest
from unittest.mock import patch
from urllib.parse import unquote

from PIL import Image

from preview_gifs import package, reference_inputs
from preview_timing import gif_durations, sampling
import review


class Timing(unittest.TestCase):
    def test_attack_phase_edges_follow_gameplay_ticks_not_blender_fps(self):
        for name, loaded, released, end in [('AttackCleave', .1, .2, 22/60),
                                             ('AttackSlam', .15, 14/60, .5),
                                             ('Bite', .6, .75, 1.4), ('Leap', .7, 1.3, 2.2)]:
            with self.subTest(name=name):
                clip = sampling(name, (1, 31), False, 120, poses=True)
                self.assertEqual(clip['frames'][:3], [1, 13, 19])
                for actual, expected in zip(clip['times'], [0, loaded, released, (released+end)/2, end]):
                    self.assertAlmostEqual(actual, expected)
                self.assertFalse(clip['playback'])

    def test_fractional_frame_has_correct_active_time_without_inventing_playback(self):
        clip = sampling('AttackCleave', (1, 31), False, 30, requested=[14.5])
        self.assertAlmostEqual(clip['times'][0], .125)
        self.assertEqual(clip['pose_indices'], [0])
        self.assertFalse(clip['playback'])

    def test_invalid_frame_requests_refuse(self):
        for frames in ([0], [32], [float('nan')], [float('inf')], [13, 1], [13, 13], []):
            with self.subTest(frames=frames), self.assertRaises(ValueError):
                sampling('AttackCleave', (1, 31), False, 30, requested=frames)

    def test_native_cadence_and_loop_seam(self):
        loop = sampling('Walk_Loop', (10, 34), True, 24)
        self.assertEqual(loop['seconds'], 1)
        self.assertLess(loop['frames'][-1], 34)
        self.assertEqual(sum(gif_durations(loop)), 1000)
        attack = sampling('Sword_Regular_C', (10, 34), True, 24)
        self.assertEqual(attack['frames'][-1], 34)
        self.assertEqual(sum(gif_durations(attack)), 1800)

    def test_gif_rounding_preserves_cadence_at_both_speeds(self):
        clip = sampling('AttackCleave', (1, 31), False, 30)
        for multiplier in (1, 4):
            delays = gif_durations(clip, multiplier)
            self.assertEqual(sum(delays[:-1]), round(22/60 * 100 * multiplier) * 10)
            self.assertEqual(delays[-1], 800)


class Packaging(unittest.TestCase):
    def setUp(self):
        self.directory = TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.source = self.root / 'private-authored-source.blend'
        self.source.write_bytes(b'test-owned source identity')
        (self.root / 'frames').mkdir()
        self.evidence = {'inputs': {'A': {str(self.source): self.sha(self.source)}}, 'tools': {},
                         'resolution': 128, 'clips': {'Idle': sampling('Idle', (1, 4), False, 30)},
                         'series': [{'id': 'Idle-equipped-front', 'clip': 'Idle', 'mode': 'equipped',
                                     'view': 'front', 'images': {'A': []}}]}
        for i, _ in enumerate(self.evidence['clips']['Idle']['frames']):
            path = self.root / f'frames/A-{i}.png'
            Image.new('RGB', (128, 128), (30 + i*40, 90, 110)).save(path)
            self.evidence['series'][0]['images']['A'].append(
                {'path': str(path.relative_to(self.root)), 'sha256': self.sha(path)})
        self.save()

    @staticmethod
    def sha(path):
        return hashlib.sha256(path.read_bytes()).hexdigest()

    def save(self):
        (self.root / 'evidence.json').write_text(json.dumps(self.evidence))

    def test_motion_packet_retains_duration_and_neutral_identity(self):
        package(self.root)
        packet = self.root / 'critic'
        self.assertNotIn(self.source.name, (packet / 'index.html').read_text())
        with Image.open(packet / 'Idle-equipped-front-normal.gif') as gif:
            duration = 0
            for i in range(gif.n_frames):
                gif.seek(i)
                duration += gif.info['duration']
            self.assertEqual(duration, 100)
        evidence = json.loads((self.root / 'evidence.json').read_text())
        for name, expected in evidence['packet'].items():
            self.assertEqual(self.sha(packet / name), expected)

    def test_changed_source_and_changed_render_refuse_before_packet_creation(self):
        for path in (self.source, self.root / 'frames/A-0.png'):
            with self.subTest(path=path):
                previous = path.read_bytes()
                path.write_bytes(b'changed')
                with self.assertRaises(ValueError):
                    package(self.root)
                self.assertFalse((self.root / 'critic').exists())
                path.write_bytes(previous)

    def test_missing_final_frame_refuses_without_partial_packet(self):
        path = self.root / self.evidence['series'][0]['images']['A'][-1]['path']
        path.unlink()
        with self.assertRaises(FileNotFoundError):
            package(self.root)
        self.assertFalse((self.root / 'critic').exists())

    def test_existing_packet_is_preserved(self):
        packet = self.root / 'critic'
        packet.mkdir()
        accepted = packet / 'accepted.png'
        accepted.write_bytes(b'accepted work')
        with self.assertRaises(FileExistsError):
            package(self.root)
        self.assertEqual(accepted.read_bytes(), b'accepted work')

    def test_sparse_stills_do_not_become_a_misleading_animation(self):
        self.evidence['clips']['Idle']['playback'] = False
        self.save()
        reference = self.root / 'private-reference.png'
        Image.new('RGB', (32, 32), 'white').save(reference)
        package(self.root, reference_inputs([reference]))
        self.assertFalse(list((self.root / 'critic').glob('*.gif')))
        self.assertEqual((self.root / 'critic/reference-01.png').read_bytes(), reference.read_bytes())

    def test_gallery_links_resolve_for_action_names_with_spaces_or_slashes(self):
        self.evidence['series'][0]['id'] = 'Sword%20swing%2F01-equipped-front'
        self.save()
        package(self.root)
        packet = self.root / 'critic'
        document = (packet / 'index.html').read_text()
        for url in re.findall(r'(?:href|src)="([^"]+)"', document):
            self.assertTrue((packet / unquote(url)).is_file(), url)

    def test_render_refusal_does_not_write_into_a_concurrently_created_review(self):
        import tempfile
        from types import SimpleNamespace
        output = self.root / 'another-review'
        create_log = tempfile.NamedTemporaryFile

        def refused(*args, **kwargs):
            output.mkdir()
            (output / 'render.log').write_text('accepted review log')
            return SimpleNamespace(returncode=1)

        with patch('sys.argv', ['review.py', str(output)]), \
                patch.object(review.shutil, 'which', return_value='/test/blender'), \
                patch.object(review.subprocess, 'run', side_effect=refused), \
                patch.object(review.tempfile, 'NamedTemporaryFile',
                             side_effect=lambda **kwargs: create_log(dir=self.root, **kwargs)), \
                patch('sys.stderr'):
            self.assertEqual(review.main(), 1)
        self.assertEqual((output / 'render.log').read_text(), 'accepted review log')
        self.assertFalse((output / 'critic').exists())


if __name__ == '__main__':
    unittest.main()
