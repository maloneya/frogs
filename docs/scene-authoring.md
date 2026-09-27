# Headless scene authoring

Agents and the native editor use the same `arpg-authoring` library through
`arpg-author`. No running game, socket, window or GPU is needed. All placement,
lighting, naming and encounter edits available in the editor are available here.
The library's dependency guard excludes the application and graphics crates.

```sh
cargo build --release -p arpg-authoring --bin arpg-author
printf '%s\n' '{"action":"new"}' | target/release/arpg-author
printf '%s\n' '{"action":"open","path":"scenes/soul_trial.ron"}' | target/release/arpg-author
```

Each invocation reads one JSON request through stdin EOF and returns one JSON
reply. Close stdin after sending it; a newline alone does not finish the request.
Success is `{"ok":true,"editor":{...},"error":null}`; refusal sets `ok:false`,
provides an error and exits nonzero. Requests are bounded to 8 MiB; the derived
editor representation is bounded to 6 MiB. Both clients share the
[placement limits](scene-editor.md#placement-and-map). Paths resolve from the
CLI's working directory. There is no session, retained state or automatic retry.

| Action | Fields |
|---|---|
| `new` | None |
| `open` | `path` |
| `edit` | `document`, `edit` |
| `batch` | `document`, ordered `edits` |
| `validate` | `document` |
| `save` | `document`, `path`, optional `revision` |

Carry `editor.document` intact between requests. Derived `objects`, `lighting`,
`encounters` and `rewards` expose names, target identities and inspector fields;
use them instead of reconstructing a partial document. Object targets contain
`kind` and `index`; deletions can shift indices, so use the returned document to
resolve names again. [Edit](../crates/authoring/src/editor.rs) and
[EncounterDraft](../crates/authoring/src/editor/encounters.rs) define the complete
operation vocabulary. Rust callers can use the typed `Request`, `Edit`, `execute`
and `validate` API directly. Inspector fields marked `integer: true` preserve
whole-number precision; send JSON integers without converting through floating point.

A batch applies edits in order and returns only a fully accepted result; it
never writes files. A rejected batch returns no partial draft. Each intermediate
edit must be valid; even an empty batch validates its input. Complete encounters
and Actor definitions each have a typed replacement operation for changes that
need several fields to become valid together.
A new file omits `revision`; replacing an existing file requires the revision
returned by `open` or the previous `save`. Keep `editor.path` and `editor.revision`
separately while editing: only `open` and `save` return disk identity. Revisions
cover file bytes, including comments. On a conflict, save the draft to a new path
or reopen and reapply the intended edits. Refused saves preserve existing files.
Saves through this API take an OS directory lock through revision checking and
replacement, so simultaneous agent/editor saves cannot both replace the same
revision. The lock releases on process exit and creates no lock files. External
editors do not participate; their changes are detected by the existing byte
checks, which cannot exclude an external write racing the final replacement.

## Example: create, name, save

```python
import json, subprocess

def author(**request):
    result = subprocess.run(["target/release/arpg-author"],
        input=json.dumps(request), text=True, capture_output=True)
    reply = json.loads(result.stdout)
    if not reply["ok"]:
        raise RuntimeError(reply["error"])
    return reply["editor"]

view = author(action="new")
view = author(action="batch", document=view["document"], edits=[
    {"action": "place", "kind": "switch", "pos": [1, 0]},
    {"action": "name", "target": {"kind": "body", "index": 0}, "name": "Shrine"},
    {"action": "add", "kind": "source", "pos": [0, 8]},
    {"action": "encounter", "index": None, "value": {
        "name": "Shrine trial", "trigger": 0, "sources": [0],
        "count": 10, "within_ticks": 1800,
        "on_start": [], "on_success": [], "on_failure": []}},
])
author(action="save", document=view["document"], path="scenes/my_trial.ron")
```

For reward actions, select entries from `editor.rewards` and copy their `action`
values into the desired encounter transition. The native form uses those same
owner-derived values. Open this file in the companion, edit it visually, and
reopen it with `arpg-author`; both clients retain the same document and metadata.

## Document and runtime boundary

Scene files contain a [SceneDocument](../crates/content/src/document.rs):

```ron
(
    scene: (engine: (name: "Empty scene")),
    labels: [],
)
```

`scene` is the game-owned `GameScene`. Labels belong to the authoring wrapper;
Game never stores, validates or hashes them. `SceneDocument::validate` checks
labels and admits the playable scene through Game. Content passes only that
playable data to runtime consumers. Direct RON authors use the same
file format; `open` validates it without a game process.

Scenarios own `GameScene` setups directly, with `engine` at the top level and
no authoring labels or file references. They do not load playable documents.

The editor holds drafts and undo history. Its bundled CLI performs edits,
validation and saving outside the game process. **Save & play** first saves,
then asks the game to load that path. A play failure reports that the file was
saved and preserves the running scene. Saving alone never contacts the game.
The game still validates admission itself and preserves tuning on replacement.