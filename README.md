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

For the storm-night visual study, open **Storm watch — the last fire**
(`scenes/storm_watch.ron`) in Scenes. Fight near the fire and watch lightning
reveal the approaching crowd. **Scene lighting → Rain and lightning** toggles
the weather.

The scene catalog contains five starting points:

- `scenes/single_enemy.ron`: one stationary, attackable enemy with no pursuit or attacks.
- `scenes/enemy_group.ron`: twelve enemies with the same passive behavior.
- `scenes/three_enemy_spawners.ron`: three sources emitting pursuing enemies,
  once per second each, up to sixty enemies in total.
- `scenes/horde_1000.ron`: exactly 1,000 pursuing enemies, with no reinforcements.
- `scenes/storm_watch.ron`: the storm-night clearing described above.

Restart restores the selected scene's starting layout.

## Controls

| Input | Action |
|---|---|
| WASD / arrows | Move |
| Cursor | Aim |
| 1 / left click | Cleave |
| 2 / right click | Slam |
| 3 / Space, held | Disintegrate |
| E | Interact |
| + / - (also keypad) | Zoom in / out one step per press; = also zooms in |
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
- [Physical doors](docs/physical-doors.md): hinged panels, transmitted crowd pressure and breakable latches.
- [Felled trees](docs/felled-trees.md): breakable roots, falls that strike what they come down on, and logs.
- [Verification](docs/verification.md): checks, benchmark cadence and evidence.
- [Experiments](experimental/README.md): independent research, outside the game.
