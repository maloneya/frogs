# Build the game with scenes

A scene is authored game content: a starting layout and the gameplay relationships
that make it work. The native **Scenes** tab authors every current `GameScene`
concept, from layouts to complete encounters. A *scenario* is a separate automated
test with inputs and assertions; the editor does not author those timelines.

## Build, play, adjust

1. Run `python3 scripts/dev.py` for the game and [ARPG Companion](companion.md),
   or `python3 scripts/companion.py open` to author without a running game.
2. In **Scenes**, open the **Scene files** menu and choose a file or **New scene**.
   Use **Refresh files** in that menu after adding files outside the app. The catalog
   lists regular `.ron` files directly inside `scenes/`.
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

Choose a tool from the left sidebar. **Select / Pan** uses either mouse button:
click an object to select it, drag an object to move it, or drag empty space to
pan. Right-click and right-drag also select, move and pan while a placement tool
is active; the left button places or draws. Selection also works through the
inspector's object list. Press **Delete** (or forward Delete) with the map focused
to delete the selected object; Delete in a text field still edits text. Every
placement supports move, duplicate, delete and undo/redo.

| Tool | Authoring controls |
|---|---|
| Enemy / Prop / Switch | Movable body, fixed prop or interactable prop; editable body template |
| Door / Chest | Locked gameplay component on a fixed prop |
| Source | Position, enablement, emitted template, cadence in ticks, spawn radius and condition |
| Formation | Position, rows, columns, spacing and template; one compact `BodyGrid` |
| Wall | Drag opposite corners, or click for a default rectangle; edit width, thickness and rotation |
| Tree | Pine, broadleaf or bare; position, scale and rotation; solid trunk footprint |
| Campfire | Position, light colour, brightness, range and height; ring shows illumination range |

Body templates expose **Fixed prop**, **Seeks player** and **Can interact**;
sources and formations apply these to their emitted bodies. Fixed props cannot
seek. A body's **Role** adds/removes its door or chest component. **Contact attacker
(one per scene)** assigns the current single-attacker mechanic to a seeking enemy;
assigning another moves that binding.

Source `FewerThan` conditions count enemies across the world. A formation expands
into bodies on load; its preview draws at most 2,000 individual markers across
all formations. The shared authoring API supports 4,096 placement entries,
counting a formation as one entry, subject to the game's tighter per-kind and
total-capacity limits. Campfires share the
[eight-light budget](lighting.md); tree trunks share [wall capacity](walls.md).

The map uses the game camera's rotation and vertical compression: up on the map
is up in the game view. The compass shows world X/Z; **P** is the fixed player
start. **Snap** affects mouse placement/movement in world metres; inspector
positions are not snapped. Inspector rotation is degrees; RON yaw is radians.
Panning, zoom and **Fit** change only the editor view. Ground footprints cannot
show tree height, canopy occlusion or the final lighting; judge those in the game.

## Inspector and history

Press Return to apply numeric text; Escape discards it. Toggles and steppers
apply immediately to the draft. Colour pickers need **Apply colour** or
**Discard**. Apply or discard pending inspector edits before saving. Rejected
edits preserve the accepted draft and display an error.

Name objects in the inspector so encounter choices read “North gate” or “Shrine”
instead of numbered entries. Names are unique within the scene, persist in the
file, and do not affect gameplay; clear a name to restore its generated label.
Names label choices; bindings still use authored indices, which the shared edits
remap on deletion. Manual file reordering must preserve those references.

**Scene lighting** controls ambient fill and sun/moon colour, brightness,
direction and elevation. Colours are stored as linear RGB. Adding a campfire
does not change the environment; **Save & play** applies the authored lighting.

**Duplicate** copies placement, attributes and any door/chest component with at
least a one-metre offset. Names, encounter links and the single-attacker binding
stay with the original. **Delete** refuses encounter endpoints; remove their
links first. Otherwise it removes attached components and remaps surviving links
and names. Deletion clears selection so inspector drafts cannot target a shifted
index. **Undo / Redo** retain 32 accepted edits. Accepted drafts and history
survive disconnects. Browsing, editing, validation and saving work offline;
playing, tuning and diagnostics require the game.

## Compose an encounter

1. Place a **Switch**, enemy **Sources**, and any reward **Doors** or **Chests**.
   Give them recognizable names.
2. Choose **New encounter**. Name it, select its activation body and source group,
   then enter a defeat target and deadline in simulation ticks (1,800 = 30 seconds).
3. Choose door/chest unlock actions for **On start**, **On success** and **On timeout**.
4. **Apply encounter** validates the complete form as one edit and disables its
   selected sources initially. Conflicting ownership is refused; invalid forms
   leave the accepted scene unchanged. Removing a source from a group or deleting
   an encounter leaves that source's initial enablement unchanged.
5. Select **Show links** to highlight the trigger, sources and action targets on
   the map. **Frame** zooms to those objects. **Edit** revises the entire encounter;
   **Save & play** tries it.

Activation enables the group; success or timeout stops it while spawned enemies
remain. Unlocking makes a door/chest available for a fresh interaction, rather
than opening it automatically. See [encounters](encounters.md) for ownership,
progress and tick rules. Forms edit authored state: opening a chest or completing
an encounter in play never changes the draft or restart snapshot.

## Files and current limits

Files use the [shared authoring format](scene-authoring.md#document-and-runtime-boundary),
including optional object names. Campfires are stored in `torches`. There is no
player-start tool, arbitrary asset browser, terrain sculpting, audio placement
or live-world capture. Assets follow the separate [Blender workflow](../.claude/skills/author-blender-asset/SKILL.md).

Saving canonicalizes RON, preserves leading line comments and drops inline
comments. Existing files require the revision read on open: a detected external
edit causes refusal. The [shared save API](scene-authoring.md) serializes agent
and companion saves. Preserve a draft with **Save as…** to a new path before reopening
a changed file. **Save & play** saves the validated file, then sends a load request.
Validation, revision or write failure prevents that request. A failed load
preserves the running game and reports that the file was saved.

Playable scenes are freely editable. Use **Save as…** to keep variants;
see [scene handoff](../.claude/skills/create-scene/SKILL.md#verify-and-hand-off).

## Shared headless interface

See the [authoring API](scene-authoring.md) for offline agent edits and
[capture and checks](companion.md#checks-and-capture) for native view inspection.