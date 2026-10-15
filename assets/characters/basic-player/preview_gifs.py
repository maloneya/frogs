"""Package completed renders into neutral, image-only visual critique packets."""
import hashlib
import html
import json
from pathlib import Path
import shutil
import sys
from urllib.parse import quote

from PIL import Image, ImageDraw, ImageFont

from preview_timing import gif_durations


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def media_url(name):
    return html.escape(quote(name), quote=True)


def reference_inputs(paths):
    references = []
    formats = {'PNG': '.png', 'JPEG': '.jpg', 'GIF': '.gif', 'WEBP': '.webp'}
    for path in paths:
        path = path.resolve()
        expected = digest(path)
        with Image.open(path) as image:
            suffix = formats[image.format]
            image.verify()
        references.append({'path': str(path), 'sha256': expected, 'suffix': suffix})
    return references


def package(output, references=()):
    evidence = json.loads((output / 'evidence.json').read_text())
    packet = output / 'critic'
    if packet.exists():
        raise FileExistsError(f'Choose a new review directory; packet already exists: {packet}')
    for files in [*evidence['inputs'].values(), evidence['tools']]:
        for path, expected in files.items():
            if digest(Path(path)) != expected:
                raise ValueError(f'Stale preview input: {path}')
    for reference in references:
        if digest(Path(reference['path'])) != reference['sha256']:
            raise ValueError(f'Reference changed: {reference["path"]}')
    # Validate the complete render set before publishing any review images.
    for series in evidence['series']:
        clip = evidence['clips'][series['clip']]
        for images in series['images'].values():
            if len(images) != len(clip['frames']):
                raise ValueError(f'Incomplete render sequence: {series["id"]}')
            for record in images:
                relative = Path(record['path'])
                if relative.is_absolute() or relative.parts[0] != 'frames' or '..' in relative.parts:
                    raise ValueError(f'Frame is outside the render directory: {relative}')
                path = output / relative
                if digest(path) != record['sha256']:
                    raise ValueError(f'Stale or changed frame: {path}')
                with Image.open(path) as image:
                    if image.size != (evidence['resolution'], evidence['resolution']):
                        raise ValueError(f'Unexpected frame dimensions: {path}')
                    image.verify()
        if clip['playback']:
            gif_durations(clip)
            gif_durations(clip, 4)
    font = ImageFont.truetype('/System/Library/Fonts/Helvetica.ttc', 17)
    packet.mkdir()
    media, cards = [], []
    for i, reference in enumerate(references):
        name = f'reference-{i + 1:02}{reference["suffix"]}'
        shutil.copyfile(reference['path'], packet / name)
        cards.append(f'<article><h2>Reference {i + 1}</h2><img src="{name}" alt="Reference {i + 1}"></article>')
        media.append(name)
    for series in evidence['series']:
        clip = evidence['clips'][series['clip']]
        variants = list(series['images'])
        resolution = max(256, evidence['resolution'])

        def frame(index):
            canvas = Image.new('RGB', (resolution * len(variants), resolution + 50), '#20272e')
            draw = ImageDraw.Draw(canvas)
            for column, variant in enumerate(variants):
                with Image.open(output / series['images'][variant][index]['path']) as image:
                    canvas.paste(image.convert('RGB').resize((resolution, resolution)), (column * resolution, 50))
                draw.text((column * resolution + 8, 5),
                          f'{variant} · {series["mode"]} · {series["view"]}', fill='white', font=font)
                draw.text((column * resolution + 8, 26),
                          f'{clip["times"][index] * 1000:.0f} ms · frame {clip["frames"][index]:g}',
                          fill='white', font=font)
            return canvas

        stem = series['id']
        links = []
        if clip['playback']:
            frames = [frame(index) for index in range(len(clip['frames']))]
            for speed, multiplier in [('normal', 1), ('slow', 4)]:
                name = f'{stem}-{speed}.gif'
                frames[0].save(packet / name, save_all=True, append_images=frames[1:],
                               duration=gif_durations(clip, multiplier), loop=0)
                media.append(name)
                links.append(f'<a href="{media_url(name)}">{speed}</a>')
            del frames
        indices = list(range(len(clip['frames']))) if not clip['playback'] else clip['pose_indices']
        # Explicit frame requests can be long; wrap them without shrinking the subject.
        columns = min(5, len(indices))
        rows = (len(indices) + columns - 1) // columns
        tile = min(360, resolution)
        strip = Image.new('RGB', (tile * columns, (tile + 50) * rows * len(variants)), '#20272e')
        draw = ImageDraw.Draw(strip)
        for row, variant in enumerate(variants):
            for cell, index in enumerate(indices):
                x, y = (cell % columns) * tile, (row * rows + cell // columns) * (tile + 50)
                with Image.open(output / series['images'][variant][index]['path']) as image:
                    strip.paste(image.convert('RGB').resize((tile, tile)), (x, y + 50))
                draw.text((x + 8, y + 5), f'{variant} · {series["view"]} · {series["mode"]}', fill='white', font=font)
                label = clip['labels'].get(str(index), f'Frame {clip["frames"][index]:g}')
                draw.text((x + 8, y + 26), f'{label} · {clip["times"][index] * 1000:.0f} ms', fill='white', font=font)
        name = f'{stem}-poses.png'
        strip.save(packet / name)
        media.append(name)
        title = html.escape(f'{series["clip"]} · {series["mode"]} · {series["view"]}')
        note = ('Normal speed; slow playback is ¼ speed. One-shot clips pause for 800 ms between repeats.'
                if clip['playback'] else 'Selected stills only; these do not establish playback timing or feel.')
        cards.append(f'<article><h2>{title}</h2><p>{note}</p><p>{" · ".join(links)}</p>'
                     f'<img src="{media_url(name)}" alt="{title}"></article>')
    document = ('<!doctype html><meta charset="utf-8"><title>Character visual review</title>'
                '<style>body{background:#20272e;color:#eee;font:16px system-ui;margin:24px}'
                'article{margin:24px 0}img{max-width:100%;height:auto}a{color:#9cd}</style>'
                '<h1>Character visual review</h1><p>Blender diagnostics. Labels A/B are neutral identifiers. '
                'Full-body cameras stay fixed; grip cameras follow the selected hand. '
                'The floor grid is spaced two metres apart. Skeleton colours follow bone names; '
                'equipped views establish surface contact. Cutaway views isolate hands, forearms and '
                'equipment; use intact equipped views for body/clothing clearance.</p>' + ''.join(cards))
    (packet / 'index.html').write_text(document)
    evidence['references'] = list(references)
    evidence['packet'] = {name: digest(packet / name) for name in [*media, 'index.html']}
    (output / 'evidence.json').write_text(json.dumps(evidence, indent=2) + '\n')
    print(f'Visual packet: {packet}\nEvidence: {output / "evidence.json"}')


def main():
    if len(sys.argv) != 2:
        raise SystemExit('Usage: preview_gifs.py REVIEW_DIRECTORY (requires Python with Pillow)')
    package(Path(sys.argv[1]).resolve())


if __name__ == '__main__':
    main()
