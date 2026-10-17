# Player and wolf reference catalog

Openly licensed character bodies and authored motions to reuse as references.
Scope: player **model, walk, Cleave**; wolf **model, walk, run, bite, leap**.
The source survey dates to **2026-10-12**. Native previews retain original
motions; production compilers now adapt selected sources into the
[player foundation](../../assets/characters/basic-player/README.md) and
[wolf](../../assets/characters/wolf/README.md). Those asset READMEs own the
shipped adaptations and their review status.

Start with the realistic human body and Quaternius motions for the player.
Compare KayKit's equipped Knight when studying a more stylized
silhouette. For the wolf, compare Micket's body with tomek's animated wolf, then
use Mesh2Motion's fox for additional quadruped motion and editable controls.
The catalog retains ten source entries. Most use CC0; thehumbug meshes use
CC-BY 3.0 and Mannequiny uses CC-BY 4.0. Exact files, hashes, authors, required
credits, acquisition details and assessment are in [catalog.json](catalog.json).

## Coverage and concrete clips

Durations below are measured from retained action ranges and source scene timing,
or the GLB's animation times. They describe the original motion, before any game
retiming. A named clip establishes availability; quality remains a visual judgment.

| Need | First reference | Useful comparison | What to study / remaining gap |
|---|---|---|---|
| Player model | Blender Human Base Meshes: realistic male/female and stylized male | thehumbug rigged male/female; Mannequiny; KayKit Knight | Compare adult anatomy and editable bodies with a motion mannequin and compact armor silhouette. Base Blender bodies have no animation rig. |
| Player walk | Quaternius UAL1 `Walk_Loop` — 1.333 s | `Walk_Formal_Loop` — 1.333 s; Mannequiny `walk` — 1.000 s; Gres `Walk_F` — 0.533 s | Supporting foot, stride, pelvis and opposite arm. Evaluate suitability on the chosen body. |
| Player Cleave | UAL2 `Sword_Regular_B` + `Sword_Regular_B_Rec` — 0.533 + 1.033 s | UAL2 A + recovery — 1.400 s; UAL1 `Sword_Attack` — 1.533 s; Gres one/two-handed attacks | Whole-body load, weight transfer, release and follow-through. The existing descending slash remains the strongest established reference here. |
| Wolf model | Micket's Eurasian wolf base | tomek `wolf_female`; Mesh2Motion fox | Micket provides a leaner wolf silhouette; tomek is strongly stylized; the fox is a motion proxy with different proportions. |
| Wolf walk | tomek `wolf_walk` — 1.000 s | Mesh2Motion `Walk` — 1.000 s | Four-foot support, foot order and shoulder/hip movement. Review foot contact and hock articulation before adaptation. |
| Wolf run | tomek `wolf_run` — 1.000 s | Mesh2Motion `Run` — 0.583 s | Gather/extension, suspension, spinal movement and leg recovery. These are useful comparison cycles, not approval of a natural wolf gait. |
| Wolf bite | tomek `wolf_attack01` — 1.000 s | Mesh2Motion `Bite` — 0.833 s | Forward body/head movement and jaw opening/closing. `wolf_attack01` is an attack name; identifying it as a bite/lunge is our interpretation of the inspected poses and jaw curves. |
| Wolf leap | tomek `wolf_jump`, `wolf_fly`, `wolf_land` — 1.000 s each | Mesh2Motion `Jump` — 2.167 s | Takeoff, airborne shape and landing compression. The fox provides a complete high jump. Production assembly and remaining visual questions belong to the wolf README. |

The native gallery displays the wolf's three leap ingredients separately. The
production wolf compiler assembles and retimes them; the creator did not supply
a finished three-second leap attack. A jump is not automatically a pounce.

## Retained libraries and models

### Player anatomy — Blender Studio/community

[Human Base Meshes](https://www.blender.org/download/demo-files/#assets),
v1.4.1, CC0. The [existing extracted realistic male](../../assets/characters/basic-player/reference/README.md#accepted-body-source)
preserves the authored connected body, eyes and provenance. Reuse that source
rather than downloading or duplicating it. The
[official bundle mirror](https://mirror.blender.org/demo/asset-bundles/human-base-meshes/)
also supplies other bodies for later comparisons.

The catalog also retains `realistic-female.blend` and `stylized-male.blend`
from the same official bundle. These preserve base vertices, faces, anatomical
face sets and eyes, removing only the multires authoring cache. Their previews
show source proportions with plain materials; no rig or skin weights were added.
[extract_body.py](extract_body.py) packages another body by its exact object name:

```sh
blender --background --disable-autoexec --python-exit-code 1 \
  --python experimental/character-references/extract_body.py -- \
  /absolute/path/to/human_base_meshes_bundle.blend \
  /absolute/path/to/realistic-female.blend GEO-body_female_realistic
```

### Adult rigged bodies — thehumbug

[Human basemeshes](https://opengameart.org/content/human-basaemeshes),
CC-BY 3.0. The original male/female `.blend` files retain adult low-poly surfaces,
eyes, mouths and 84-bone rigs (64 bones marked for deformation). They contain no
motion actions. The author retopologized MakeHuman exports; that upstream origin
does not make these adaptations CC0. Attribute **thehumbug**, the asset title,
[CC-BY 3.0](https://creativecommons.org/licenses/by/3.0/), and the source page;
identify any subsequent adaptations. The listing is retained as license evidence.

Previews show the saved hands-on-hips poses. The pointed shoulder forms and
joint deformation require review before choosing these as a body foundation.

### Independent walk and paired body — Mannequiny

[Mannequiny v0.4.0](https://github.com/gdquest-demos/godot-3d-mannequin/releases/tag/v0.4.0),
CC-BY 4.0, by **GDQuest, Luciano Muñoz and contributors**. The official Blender
release supplies a segmented adult mannequin, 45 bones and 12 baked actions,
including `walk` (1.000 s) and `run` (0.600 s), at 30 source frames/second.
The gallery adds its body and walk as an independent comparison to Quaternius.
The release's “rig-free” wording means no authoring control rig; the deformation
skeleton and baked curves are present.

Required credit: **Mannequiny CC-By 4.0 GDQuest, Luciano Muñoz, and contributors.**
Retain the release and [license link](https://creativecommons.org/licenses/by/4.0/)
and identify adaptations. The source download is unmodified; gallery materials,
framing and displayed animation selection are presentation choices.

### Directional movement and sword choreography — Gres

[Humanoid SoulsLike Animations](https://opengameart.org/content/humanoid-soulslike-animations),
CC0. The original straight-sword `.blend` supplies the body, 50-bone rig, both
swords and authored curves. The source scene runs at **60 fps**. The added previews
select `Walk_F`, `Walk_L`, `SS_Main_Light_Attack_01`,
`SS_Main_Charge02_Heavy` and `SS_TH_Light_Attack_01`.

| Exact source action | Native duration | Reference purpose |
|---|---|---|
| `Walk_F` | 0.533 s | Forward combat gait |
| `Walk_L` | 0.517 s | Sideways support and recovery |
| `SS_Main_Light_Attack_01` | 1.067 s | One-handed preparation, blade arc and recovery |
| `SS_Main_Charge02_Heavy` | 1.333 s | Charged heavy release; start/charge phases are separate |
| `SS_TH_Light_Attack_01` | 1.100 s | Two-handed grip and torso/shoulder participation |

The source retains additional main/off-hand, dual-wield, charged, running and
rolling variations. This catalog treats their availability as verified and their
artistic suitability as provisional. The original right-hand sword and its bone
constraint are shown; the unused left-hand sword is hidden only for these previews.

Separate author-exported FBXs were inspected, but their control-bone bind matrices
differ between body and motion exports. They were not combined or retained.
The original Blender file avoids that pairing issue. Its `Smooth by Angle` asset
node group is present in loaded data despite an old external library path;
preview admission checks missing linked data rather than requiring every recorded
path to exist. No repair or source script execution is needed.

### Player motion — Quaternius / Gonzalo Furnier

[Universal Animation Library](https://quaternius.itch.io/universal-animation-library)
and [Library 2](https://quaternius.itch.io/universal-animation-library-2),
free **Standard** editions, CC0. The retained mannequin has 65 joints.
[player-walk.glb](sources/quaternius/player-walk.glb) adds ordinary/formal walk,
sword idle and a single attack. It copies original mesh, skin and curve bytes
with the [clip extractor](../../assets/characters/basic-player/reference/extract_clips.py).
The [existing Cleave source](../../assets/characters/basic-player/reference/README.md)
already retains regular A/B attacks and their recoveries.

The advertised whole-library animation counts include paid tiers. We inspected
the actual free files; do not assume every previewed clip is in Standard.
Editable Blender source controls are a paid tier, while the retained exported
rigs and curves are CC0. The [player compiler](../../assets/characters/basic-player/README.md)
owns the accepted Cleave adaptation and its preview tools.

### Player model and matching motions — Kay Lousberg

[KayKit Adventurers 1.0](https://github.com/KayKit-Game-Assets/KayKit-Character-Pack-Adventures-1.0),
CC0. The unmodified
[Knight](sources/kaykit/Knight.glb) GLB is retained from the author's repository
at the commit pinned in the catalog. It contains 76 named clips (including T-pose), embedded textures
and a shared 41-joint rig. Walking A/B/C, backward walking, one/two-handed chops,
horizontal/diagonal slices and stabs give useful choices beyond the first preview.

The gallery hides unused weapons and shields so they do not overlap the body;
the original files retain all equipment. Stylized proportions limit its use as an anatomy reference.
[KayKit Character Animations](https://kaylousberg.itch.io/kaykit-character-animations)
is the author's larger CC0 motion library for a later expansion; its additional
files have not been downloaded for this catalog.

### Wolf-specific body and motion — tomek

[3D wolf animation for game](https://opengameart.org/content/3d-wolf-animation-for-game).
The original [wolf.blend](sources/tomek-wolf/wolf.blend) contains two body meshes,
a 26-bone rig and nine actions: idle, walk, run, attack, jump, airborne, land,
howl/cry and death. The animated `wolf_female` is used in previews; `wolf_male2`
is an additional mesh preserved in the file.

The listing offers CC0, GPL 2.0 and GPL 3.0 options. This catalog selects CC0,
following [OGA's multiple-license guidance](https://opengameart.org/content/faq).
There is no embedded license text; the original
[listing snapshot](sources/tomek-wolf/source-page.html) preserves the offered
licenses and author alongside the file hash. The cartoon ears/head and simple
surface make this a motion starting point, not a realistic wolf model.

### Wolf anatomy alternative — Micket

[Wolf](https://opengameart.org/content/wolf-1), CC0. Retained original
[wolf.blend](sources/micket-wolf/wolf.blend) is an untextured low-poly Eurasian
wolf with a rig and no actions. The [listing snapshot](sources/micket-wolf/source-page.html)
preserves attribution and license evidence. Use it to compare muzzle, neck,
torso, tail and digitigrade leg silhouette. The author's triangle count describes
the displayed model; raw editable mesh counts are recorded separately in preview
evidence and should not be conflated with an evaluated/exported count.

### Quadruped motion and editable controls — Mesh2Motion

[Mesh2Motion source art](https://github.com/Mesh2Motion/mesh2motion-assets),
by Scott Petrovic and contributors, CC0. The
[project](https://mesh2motion.org/) describes its models and motions as manually
authored. Retained files include the fox model, control rig and individual
`Walk`, `Run`, `Bite`, `Jump` Blender sources. The rig has 111 total bones,
51 marked for deformation: authoring controls are part of that total.

The original animation files refer to the author's absolute Windows rig path.
The renderer reconnects the pinned local `rig-fox.blend` **in memory** and records
its hash; original files remain unchanged. Opening an animation manually in
Blender requires relocating that linked library. Keep the control rig with the
clips. The fox body and high jump are useful references, but require explicit
anatomy and trajectory decisions for a wolf attack leap.

## Additional leads

| Source | License and coverage | Status |
|---|---|---|
| [Quaternius Ultimate Animated Animal Pack](https://quaternius.com/packs/ultimateanimatedanimals.html) | CC0; advertised animal walk/gallop/attack/jump motions and Blender source | Official page checked; no download or wolf-specific clip inventory inspection yet. |
| [LPC Wolf Animation](https://opengameart.org/content/lpc-wolf-animation) | CC-BY 4.0 option; 2D walk, run, bite, howl and death | Supplementary silhouette/timing reference; no measured 3D motion or leap. Credit Stephen “Redshrike” Challener and William.Thompsonj. |

The newer [Fantasy Creatures pack](https://algumacoisaqq.itch.io/fantasy-creatures-wolf-giant-rat-spider-wasp-centipede)
offers CC0 wolf clips but explicitly says its models and animation keys are
generated by code written by an AI assistant. It was not selected as an authored
motion foundation for this request. No files were downloaded.

## Browse and regenerate

The generated offline gallery is `target/character-references/index.html`.
It has Player/Wolf filters, native/quarter-speed GIFs, key-pose strips, source
links and exact clip names. The source files and this catalog live in the repo;
previews are reproducible outputs under ignored `target/`.

From the repository root with Blender 5.2.1 LTS and Python with Pillow:

```sh
blender --background --disable-autoexec --python-exit-code 1 \
  --python experimental/character-references/render.py -- target/character-references
python3 experimental/character-references/build_gallery.py target/character-references
python3 -m http.server 8764 --bind 127.0.0.1 --directory target/character-references
```

Open `http://127.0.0.1:8764/`, or open the generated HTML locally. No network/CDN
dependency is needed to browse the generated gallery. Disable Blender autoexec
when opening downloaded sources. Missing linked data refuses a preview. Source NLA tracks are disabled only in the
preview process so a solo strip cannot override the selected action. Workbench
renders use plain materials, fixed cameras fitted to each clip's complete bounds
and 24 image samples/second. Native source duration is retained; GIF delays round
cumulative timing to 10 ms. Loops repeat immediately, including one-shot attacks.

[previews.json](previews.json) selects exact source clips and displayed meshes.
`evidence.json` records action ranges, source scene timing, rig/mesh inventory,
source hashes, linked-rig hashes and sampled frames. `gallery-files.json` hashes
the generated outputs. Gallery building refuses changed source files or linked
rigs. These are source previews, not captures through the game renderer.

Pose sequences were inspected to distinguish the motions and catch framing or
equipment problems. Native previews establish source availability, not approval
of every candidate or adaptation. Use the production asset READMEs for integrated
checkpoints, and review further adaptations through the
[character workflow](../../.claude/skills/character-workflow/SKILL.md) and
[motion acceptance principles](../../docs/character-motion.md).
