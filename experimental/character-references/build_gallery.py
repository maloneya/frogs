"""Package source previews with provenance, native cadence, and pose inspection."""
from pathlib import Path
from functools import cache
import hashlib
import html
import json
import sys

from PIL import Image, ImageDraw

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent


def gif(images, durations, path, speed):
    # GIF delays are centiseconds. Round cumulative time to avoid cadence drift.
    elapsed = 0
    rounded = 0
    delays = []
    for duration in durations:
        elapsed += duration / speed
        end = round(elapsed * 100)
        delays.append((end - rounded) * 10)
        rounded = end
    images[0].save(path, save_all=True, append_images=images[1:],
                   duration=delays, loop=0, disposal=2, optimize=False)


def main():
    out = Path(sys.argv[1]).resolve()
    evidence = json.loads((out / 'evidence.json').read_text())
    catalog = json.loads((HERE / 'catalog.json').read_text())
    sources = {s['id']:s for s in catalog['sources']}
    @cache
    def source_sha256(path):
        return hashlib.sha256(path.read_bytes()).hexdigest()

    cards = []
    outputs = []
    for preview in evidence['previews']:
        path = ROOT / preview['path']
        if source_sha256(path) != preview['source_sha256']:
            raise ValueError(f'Stale preview: {path}')
        for dependency in preview['dependencies']:
            if source_sha256(ROOT / dependency['path']) != dependency['sha256']:
                raise ValueError(f"Stale linked rig: {dependency['path']}")
        source = sources[preview['source']]
        for dependency in source['files']:
            file = ROOT / dependency['path']
            if source_sha256(file) != dependency['sha256']:
                raise ValueError(f'Source changed: {file}')
        stem = preview['id']
        frames = preview['frames']
        images = [Image.open(out / 'frames' / stem / f['file']).convert('RGB') for f in frames]
        if len(images) > 1:
            duration = sum(f['duration_seconds'] for f in frames)
            for suffix, speed in [('normal', 1), ('slow', .25)]:
                filename = f'{stem}-{suffix}.gif'
                gif(images, [f['duration_seconds'] for f in frames], out / filename, speed)
                outputs.append(filename)
            selected = [round(i * (len(images) - 1) / 3) for i in range(4)]
            strip = Image.new('RGB', (480 * 4, 520), (235, 238, 242))
            draw = ImageDraw.Draw(strip)
            for column, index in enumerate(selected):
                strip.paste(images[index], (column * 480, 0))
                frame = frames[index]
                draw.text((column * 480 + 10, 490),
                          f"{frame['action']} | frame {frame['source_frame']:.2f}", fill=(20, 30, 40))
            strip.save(out / f'{stem}-poses.png')
            outputs.append(f'{stem}-poses.png')
            media = f'''<img class="motion" src="{stem}-normal.gif" alt="{html.escape(preview['title'])}">
                <p>{duration:.3f} s source duration · repeats immediately</p>
                <div class="controls"><button data-speed="normal" aria-pressed="true">Native cadence</button>
                <button data-speed="slow" aria-pressed="false">¼ speed</button></div>
                <details><summary>Key poses and exact source frames</summary>
                <a href="{stem}-poses.png"><img src="{stem}-poses.png" alt="Source pose strip"></a></details>'''
        else:
            filename = f'{stem}.png'
            images[0].save(out / filename)
            outputs.append(filename)
            media = f'<img src="{filename}" alt="{html.escape(preview["title"])}">'
        notes = html.escape(preview.get('note', source['assessment']))
        cards.append(f'''<article data-id="{stem}" data-kind="{source['character']}">
          <h2>{html.escape(preview['title'])}</h2>{media}
          <p>{notes}</p><p class="credit"><a href="{html.escape(source['url'])}">{html.escape(source['title'])}</a>
          · {html.escape(source['author'])} · {html.escape(source['license'])}</p>
          <details><summary>Source identity and clips</summary><p>{html.escape(preview['path'])}</p>
          <p>{html.escape(', '.join(preview.get('actions', [])) or 'Body reference; no motion selected')}</p>
          <code>{preview['source_sha256']}</code></details></article>''')
    document = '''<!doctype html><html lang="en"><meta charset="utf-8">
      <meta name="viewport" content="width=device-width, initial-scale=1">
      <title>Character reference catalog</title><style>
      *{box-sizing:border-box}body{margin:0;background:#111923;color:#ecf1f7;font:16px/1.5 system-ui}
      header,main,footer{max-width:1400px;margin:auto;padding:24px}h1{font-size:36px;line-height:1.2}
      .intro{max-width:900px;color:#c6d2df}nav,.controls{display:flex;gap:8px;flex-wrap:wrap}
      button{background:#223247;color:#eef4ff;border:1px solid #536a83;padding:9px 14px;border-radius:8px;cursor:pointer}
      button[aria-pressed="true"]{background:#335b80;border-color:#9ad6ff}
      main{display:grid;grid-template-columns:repeat(auto-fit,minmax(min(100%,340px),1fr));gap:20px}
      article{background:#1b2635;border:1px solid #34465d;border-radius:12px;padding:18px}h2{font-size:20px;margin:0 0 15px}
      img{display:block;width:100%;border-radius:6px}p{margin:12px 0}a{color:#9ad6ff}code{overflow-wrap:anywhere}
      .credit{color:#aebfd1;font-size:14px}details{font-size:14px;margin-top:14px}summary{cursor:pointer}
      [hidden]{display:none}footer{color:#aebfd1}
      </style><header><p>ARPG / source library · researched 12 October 2026</p>
      <h1>Player and wolf references</h1>
      <p class="intro">Authored models and motions to study before creating a new character.
      Plain materials expose the shapes; playback retains source timing. These are Blender source previews.
      Wolf leap references include separate takeoff/airborne/landing poses and a complete fox jump.
      Human judgment of quality and feel remains provisional.</p>
      <nav><button data-filter="all">All references</button><button data-filter="player">Player</button>
      <button data-filter="wolf">Wolf / quadruped</button></nav></header><main>'''
    document += '\n'.join(cards)
    document += '''</main><footer>Sources use CC0 and CC-BY. Attribution requirements are recorded per source.
      Exact source files, licenses, gaps, and additional leads
      are recorded in experimental/character-references/README.md and catalog.json.
      Previews sample authored curves at 24 images/second. GIF delays preserve total duration to 10 ms.
      Gray material, cameras and hidden unused equipment are preview choices; source files stay unmodified.</footer>
      <script>document.querySelectorAll('[data-filter]').forEach(button=>button.onclick=()=>{
        document.querySelectorAll('article').forEach(card=>card.hidden=button.dataset.filter!=='all'&&card.dataset.kind!==button.dataset.filter)});
      document.querySelectorAll('[data-speed]').forEach(button=>button.onclick=()=>{
        const card=button.closest('article');card.querySelector('.motion').src=card.dataset.id+'-'+button.dataset.speed+'.gif';
        card.querySelectorAll('[data-speed]').forEach(b=>b.setAttribute('aria-pressed',String(b===button)))});</script></html>'''
    (out / 'index.html').write_text(document)
    outputs.append('index.html')
    (out / 'gallery-files.json').write_text(json.dumps([
        {'path':p, 'sha256':hashlib.sha256((out / p).read_bytes()).hexdigest()} for p in outputs
    ], indent=2) + '\n')
    print(f'Built {len(cards)} catalog previews: {out / "index.html"}')


if __name__ == '__main__':
    main()
