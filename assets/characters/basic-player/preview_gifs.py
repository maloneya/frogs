"""Package preview.py frames as normal/quarter-speed GIFs and pose strips."""
import hashlib
import json
from pathlib import Path
import sys

from PIL import Image, ImageDraw, ImageFont


def main():
    output = Path(sys.argv[1])
    evidence = json.loads((output / 'evidence.json').read_text())
    for files in evidence['inputs'].values():
        for path, expected in files.items():
            assert hashlib.sha256(Path(path).read_bytes()).hexdigest() == expected, f'Stale preview: {path}'
    variants = list(evidence['inputs'])
    font = ImageFont.truetype('/System/Library/Fonts/Helvetica.ttc', 20)
    for name, clip in evidence['clips'].items():
        images = []
        for index in range(len(clip['frames'])):
            canvas = Image.new('RGB', (420 * len(variants), 460), '#20272e')
            for column, variant in enumerate(variants):
                with Image.open(output / 'frames' / f'{variant}-{name}-{index:03}.png') as image:
                    canvas.paste(image.convert('RGB'), (column * 420, 40))
                ImageDraw.Draw(canvas).text((column * 420 + 10, 10), f'{variant} • {name}', fill='white', font=font)
            images.append(canvas)
        intervals = len(images) if clip['loop'] else len(images) - 1
        centiseconds = clip['seconds'] * 100
        durations = [(round((i + 1) * centiseconds / intervals) - round(i * centiseconds / intervals)) * 10
                     for i in range(intervals)]
        for speed, multiplier in [('normal', 1), ('slow', 4)]:
            delays = [duration * multiplier for duration in durations]
            if not clip['loop']:
                delays.append(800)
            images[0].save(output / f'{name}-{speed}.gif', save_all=True,
                          append_images=images[1:], duration=delays, loop=0)
        indices = [round(i * (len(images) - 1) / 4) for i in range(5)]
        strip = Image.new('RGB', (300 * 5, 325 * len(variants)), '#20272e')
        for row, variant in enumerate(variants):
            for column, index in enumerate(indices):
                with Image.open(output / 'frames' / f'{variant}-{name}-{index:03}.png') as image:
                    strip.paste(image.convert('RGB').resize((300, 300)), (column * 300, row * 325 + 25))
                ImageDraw.Draw(strip).text((column * 300 + 8, row * 325 + 2),
                    f'{variant} • sample {index}', fill='white', font=font)
        strip.save(output / f'{name}-poses.png')


if __name__ == '__main__':
    main()
