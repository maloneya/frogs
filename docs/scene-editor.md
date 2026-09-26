# Visual scene editing

Open **Scenes** in [ARPG Companion](companion.md). Select a file and **Open selected**,
or choose **New scene**. The map is the authored starting layout; enemies moving
or dying in the game never change it. The game continues running during editing.

- **Select:** click an object or choose it in the inspector, then drag to move.
- **Enemy / Source / Formation:** click to place. A formation is a compact
  `BodyGrid`; its rows, columns and spacing remain editable together.
- **Wall:** drag opposite corners of a rectangle, or click for a default wall.
  Set width, thickness and rotation in the inspector. Rotation is shown in degrees.
- **Pan:** drag the map. Zoom buttons and **Fit** change only the editor view.
- **Snap:** off, 0.25, 0.5 or 1 metre; this affects mouse placement and movement.
  Inspector positions remain exact. X increases right, Z up; P marks the fixed
  player start at the origin. The arena border uses the simulation's dimensions.

Inspector fields and source conditions come from Rust. Enter a value and press
Return to apply; Escape discards its text. Toggle and stepper changes apply
immediately to the draft. Invalid edits preserve the previous draft and report
an error. Pending inspector text must be applied or discarded before saving.
Sources expose enablement, seeking, cadence in ticks, spawn radius and the
existing conditions. `FewerThan` counts the world's enemies, not only that source.

**Duplicate** copies physical placement and attributes, offset by at least one
metre. It does not duplicate gameplay relationships. **Delete** refuses objects
needed by a door, chest, enemy attack or encounter. Removing unrelated objects
remaps indices at the gameplay owner so surviving links keep their targets.
**Undo / Redo** retain up to 32 accepted edits, including additions and deletions.
Drafts and history survive a game disconnect/restart; reconnect before submitting
another edit. Closing the companion does not persist an unsaved draft.

**Save** writes the accepted draft. **Save as…** chooses a new file. Existing
files require the revision obtained when opening them: an external edit causes
refusal instead of an overwrite. Saving canonicalizes RON formatting and keeps
leading line comments; inline comments are not retained. Files still use the
same complete `GameScene` definition as hand-authored scenes and scenarios.
Trees, torches, doors, chests, encounters and attack relationships round-trip
intact. Trees and torches appear as reference markers; their authoring is outside
this first editor. Large formations show their outline instead of drawing more
than 2,000 individual markers. The editor admits at most 4,096 explicit placements;
formations can represent the larger populations allowed by scene validation.

**Save & play** prepares a validated replacement with current runtime tuning,
saves the scene, then commits the replacement and performs the ordinary input,
feedback, camera and capture cleanup. Validation, persistence or revision failure
leaves the current run intact. **Restart current** repeats the last loaded scene;
saving alone does not select a different restart layout.

## Protocol

One `scene_editor` command contains a stateless `request`:

```json
{"op":"scene_editor","session":12345,"request":{"action":"open","path":"scenes/walls.ron"}}
{"op":"scene_editor","session":12345,"request":{"action":"new"}}
```

Use the current session from `state`. Replies include `editor` only for successful
authoring requests: complete opaque `scene` data, derived drawing/inspector data,
and (on open/save) the absolute file path and revision. Swift returns that complete
scene in `edit` and `save` requests. The command definitions and edit vocabulary
live in [scene_editor.rs](../crates/app/src/scene_editor.rs); JSON does not introduce
a second scene schema. Edits validate through Game without stepping it. Save
requires `path`, optional `revision`, and `play`; a missing revision is create-only.
Polling does not transfer scene documents or runtime body state.

For a self-capture with a scene open, pass `--scene /absolute/path/scene.ron` to the
companion executable alongside `--socket` and `--tab scenes`. It opens a draft
without starting or changing the running encounter. Capture uses the existing
`ARPG_COMPANION_CAPTURE` facility.

Verification exercises complete scene round-trips, relationship deletion/remapping,
invalid geometry, persistence conflicts, failed saves, tuning retention and run
identity, native undo/reconnect, coordinate conversion, and live save/play/reopen.
