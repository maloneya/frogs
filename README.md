# arpg

A from-scratch action RPG for macOS / Apple Silicon / Metal. Build the game by
composing scenes, playing them, and adjusting their layout and tuning. Extend
the engine when a playtest reveals a rule the existing systems cannot express.

## Build and play

With Rust and the Xcode command-line tools installed, run from this directory:

```sh
python3 scripts/dev.py
```

This builds and launches the game and native **ARPG Companion** independently.
In **Scenes**, open a file or create a scene, place objects, then **Save & play**.
Adjust type-wide values in **Tuning** and use **Restart scene** to repeat the
loaded layout. The game reopens the last played file on the next launch; without
one it starts empty. See [launch options](docs/companion.md).

The [scene editor guide](docs/scene-editor.md) covers placement, duplication,
camera-aligned coordinates, lighting, saving and complete encounter authoring.
Agents use the [same headless API](docs/scene-authoring.md); editing works with the game closed.

## Controls

| Input | Action |
|---|---|
| WASD / arrows | Move |
| Cursor | Aim |
| 1 / left click | Cleave |
| 2 / right click | Slam |
| 3 / Space, held | Disintegrate |
| E | Interact |
| Escape | Quit game |

The HUD shows health, ability state and encounter progress. A melee press during
a swing is dropped; a held beam can begin when the swing finishes. Moving focus
to the companion releases gameplay input while simulation continues.

## Where things live

- `scenes/`: [authoring documents](docs/scene-authoring.md#document-and-runtime-boundary)
  containing playable `GameScene` data and optional object names.
- `scenarios/`: test-owned setups, input timelines and assertions through the same `Game`.
  These never load playable `scenes/` files. Benchmarks own their scenes under `benchmarks/`.
- `tools/companion/`: native authoring, tuning and diagnostic UI.
- `crates/`: simulation, gameplay, content, headless authoring, assets and presentation;
  see [ownership and working rules](CLAUDE.md).
- [Actor behaviors](docs/behaviors.md): composable actions, NPCs and shared physical effects.
- [Verification](docs/verification.md): checks, benchmark cadence and evidence.
- [Experiments](experimental/README.md): independent research, outside the game.
