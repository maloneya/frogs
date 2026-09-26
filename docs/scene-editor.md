# Build the game with scenes

A scene is authored game content: a starting layout and the gameplay relationships
that make it work. The native **Scenes** tab is the main tool for composing and
trying layouts. A *scenario* is a separate automated test with inputs and
assertions; the editor does not author those timelines.

## Build, play, adjust

1. Run `python3 scripts/dev.py` from the repository root to open the game and
   [ARPG Companion](companion.md).
2. In **Scenes**, choose a file and **Open selected**, or **New scene**. Use
   **Refresh files** after adding files outside the app. The catalog lists regular
   `.ron` files directly inside `scenes/`.
3. Place and duplicate objects, adjust their inspector fields and scene lighting,
   then **Save & play**. Save a new scene under `scenes/` to keep it discoverable.
4. Play the result. Adjust type-wide movement/combat values in **Tuning**;
   **Restart scene** repeats the loaded layout with those values.
5. Change the layout and **Save & play** again. Keep a variant with **Save as…**
   when comparing ideas. Describe what to try in a leading scene-file comment.

The map edits the starting layout, not moving or defeated runtime entities. The
running game continues while you edit. These states have separate lifetimes:

| State | What changes it | What preserves it |
|---|---|---|
| Editor draft | Accepted inspector/map edits, undo/redo | Save writes it; it survives reconnect, but not closing the companion |
| Scene file | Save, Save as, Save & play, or external authoring | Stores layout, props, lighting and gameplay relationships |
| Running game and restart snapshot | Save & play or harness `scene start`; restart rebuilds the snapshot | Saving alone leaves both unchanged; restart does not reread disk |
| Runtime tuning | Tuning controls | Survives scene replacement/restart; **Save tuning** writes a separate `tuning.ron` |

A new game launch rereads the last played file, unlike a restart. See
[scene lifetime](scene-playtests.md) for startup preferences and automation.

## Placement and map

Choose a **Tool**, then return to **Select** to move existing objects. Selection
also works through the inspector's object list. Every placement supports
move, duplicate, delete and undo/redo.

| Tool | Authoring controls |
|---|---|
| Enemy | Position and seeking |
| Source | Position, enablement, seeking, cadence in ticks, spawn radius and condition |
| Formation | Position, rows, columns, spacing and seeking; one compact `BodyGrid` |
| Wall | Drag opposite corners, or click for a default rectangle; edit width, thickness and rotation |
| Tree | Pine, broadleaf or bare; position, scale and rotation; solid trunk footprint |
| Campfire | Position, light colour, brightness, range and height; ring shows illumination range |

Source `FewerThan` conditions count enemies across the world. A formation expands
into bodies on load; its preview draws at most 2,000 individual markers across
all formations. The editor supports 4,096 explicit placements, subject to the
game's tighter per-kind and total-capacity limits. Campfires share the
[eight-light budget](lighting.md); tree trunks share [wall capacity](walls.md).

The map uses the game camera's rotation and vertical compression: up on the map
is up in the game view. The compass shows world X/Z; **P** is the fixed player
start. **Snap** affects mouse placement/movement in world metres; inspector
positions remain exact. Inspector rotation is degrees; RON yaw is radians.
**Pan**, zoom and **Fit** change only the editor view. Ground footprints cannot
show tree height, canopy occlusion or the final lighting; judge those in the game.

## Inspector and history

Press Return to apply numeric text; Escape discards it. Toggles and steppers
apply immediately to the draft. Colour pickers need **Apply colour** or
**Discard**. Apply or discard pending inspector edits before saving. Rejected
edits preserve the accepted draft and display an error.

**Scene lighting** controls ambient fill and sun/moon colour, brightness,
direction and elevation. Colours are stored as linear RGB. Adding a campfire
does not change the environment; **Save & play** applies the authored lighting.

**Duplicate** copies placement and attributes with at least a one-metre offset,
without copying gameplay relationships. **Delete** refuses endpoints needed by
encounters, doors, chests or enemy attacks; unrelated deletions remap surviving
links. Deletion clears selection so inspector drafts cannot target a shifted
index. **Undo / Redo** retain 32 accepted edits. Accepted drafts and history
survive disconnects; submitting further edits requires a game connection.

## Files and current limits

Files use the complete game-owned `GameScene` definition. Doors, chests,
encounters, static interaction props and attack relationships round-trip through
the editor, but their creation/binding still requires [RON authoring](encounters.md).
The editor can move existing body placements, including fixed props; it cannot
create their gameplay bindings. Campfires are stored in `torches`. There is no
player-start tool, arbitrary asset browser, terrain sculpting, audio placement
or live-world capture. Assets follow the separate [Blender workflow](../.claude/skills/author-blender-asset/SKILL.md).

Saving canonicalizes RON, preserves leading line comments and drops inline
comments. Existing files require the revision read on open: an external edit
causes refusal. Preserve a draft with **Save as…** to a new path before reopening
a changed file. Save & play validates/prepares the replacement before writing;
validation, revision or write failure leaves the running game intact.

Scenes referenced by `File(...)` in `scenarios/` are also regression inputs.
Use **Save as…** for exploration, or deliberately update their predictions when
the intended content changes. Never bless a failing trace simply because a
layout changed. Once a useful gameplay composition should remain reliable,
[test the same file](../.claude/skills/create-scene/SKILL.md#verify-and-hand-off).

## Protocol

The companion holds the draft and undo history. Rust derives drawing/inspector
data from `GameScene` and validates edits through `Game`, without stepping it.
Swift returns the complete opaque scene for edits/saves; it owns no parallel schema.

```json
{"op":"scene_editor","session":12345,"request":{"action":"open","path":"scenes/walls.ron"}}
{"op":"scene_editor","session":12345,"request":{"action":"new"}}
```

Use the current session from `state`. Successful replies contain `editor`: the
scene, derived UI data, selection and file path/revision. Save requires `path`
and `play`; a missing `revision` permits only creating a new file. Definitions
live in [scene_editor.rs](../crates/app/src/scene_editor.rs). Polling does not
transfer documents or runtime bodies. See [companion capture and checks](companion.md#checks-and-capture)
for inspecting the native editor.
