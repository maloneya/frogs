# Character assets and animation plan

Status: foundation implemented. Static and animated imports, direct authored
player presentation and pose-bucketed horde presentation are running end to
end; richer transitions and animation tooling remain deliberately deferred.

## Purpose

Open the engine to common character tools and shared assets without supporting
every authoring format or building a general animation package. Characters must
read clearly under the pulled-out camera, and the horde must remain a batched
workload.

## Principles

- **One interchange boundary.** Accept a documented subset of glTF 2.0,
  preferably `.glb`. Convert FBX and tool-native files before they reach the
  engine. glTF types stop at import; callers receive engine-owned data.
- **One authoring convention.** Runtime assets use metres, Y-up and `+Z`
  forward. Import applies authored transforms but does not guess asset-specific
  scale, rotation or ground offsets.
- **Presentation follows simulation.** Simulation owns position, facing,
  attack phases, contact and damage. Animation depicts those facts and never
  feeds bone or weapon motion back into gameplay.
- **Readability earns complexity.** Silhouette, pose, timing, phase variation,
  colour and grounding matter at game distance. Detail the camera cannot show
  does not enter the first implementation.
- **One vocabulary, different budgets.** The player and rare notable enemies
  may use ordinary skeletal animation. The horde shares meshes, materials,
  clips and evaluated poses.
- **Compatibility is demonstrated.** A generated diagnostic fixture proves
  arithmetic and orientation. A representative export from the documented
  authoring workflow proves the boundary is useful. Both are required.
- **Unsupported means rejected.** The importer has an explicit, bounded subset
  and reports unsupported required features instead of drawing damaged data.

## Ownership

- `assets` accepts bytes, validates the interchange subset and owns CPU meshes,
  materials, skeletons and clips. It owns no paths, simulation meaning, window
  state or GPU resources.
- `gfx` uploads validated assets and owns their buffers, textures, pipelines
  and opaque GPU handles.
- `app` owns file selection, loaded-resource lifetime and the mapping from
  presentation roles such as `idle` and `attack` to asset clip names.
- `game`, `sim`, `content` and `scenario` remain independent of asset files and
  graphics resources.

Static and animated assets need not share one catch-all representation. A
static mesh may bake its node chain once at import. A character asset must
retain asset-local geometry, node and joint hierarchy, and inverse bind
transforms. Stage 3 introduces that separate representation rather than
stretching the preview's `StaticMesh` across incompatible responsibilities.

## Initial capability

The first complete character path needs only:

- indexed triangles with positions, normals, one UV set and base colour;
- node translation, rotation and scale;
- one skeleton, inverse bind transforms and up to four joint influences per
  vertex;
- named transform clips with basic interpolation, looping and one-shot
  playback;
- direct role sampling with phase-bound one-shot playback; and
- rigidly weighted weapon geometry in the character mesh.

An existing glTF parsing crate is appropriate plumbing. Writing the interchange
parser is not an engine layer this project is trying to learn.

## Horde strategy

Do not evaluate and upload a unique skeleton for every ordinary enemy. Quantize
shared clips into a small number of pose phases, evaluate one joint palette per
occupied phase, and draw each phase bucket as instances. Derive per-enemy phase
offsets from stable identity so the crowd is varied without stored randomness.

Ordinary enemies should normally share one mesh, material, small skeleton and
clip set. Increase phase count, add visibility culling or adopt a more elaborate
GPU path only after uncapped measurement shows a visible or performance need.

## Staged delivery

Each stage was delivered as a separate running change.

1. **Complete — static textured preview.** The `assets` boundary imports a
   bounded static `.glb`; `gfx` uploads and draws it; `app` owns an atomic,
   harness-driven preview. A reproducible asymmetric fixture proves transforms,
   winding, UV orientation and linear colour handling. Player and horde cubes
   remain unchanged.
2. **Complete — authoring compatibility.** A reproducible
   [Blender workflow](character-assets-authoring.md) exports the checked-in
   training dummy through Blender's normal GLB exporter. The boundary accepts
   its harmless scalar PBR defaults without pretending to render them, requires
   its trilinear sampler, and builds an sRGB-correct complete mip chain. No
   skinning is part of this stage.
3. **Complete — bind-pose character.** A distinct `CharacterAsset` retains one
   named node hierarchy, one rooted skin and validated inverse bind transforms.
   The importer proves that the joint palette reproduces the authored bind pose;
   `gfx` uploads that palette and draws the Blender-authored mannequin through a
   dedicated skinning pipeline. Harness selection is atomic and observable.
4. **Complete — one clip.** The character boundary imports one named, bounded
   transform clip with linear and step interpolation, rejecting cubic splines,
   morph targets and non-joint targets. A reusable CPU pose samples from
   continuous simulation presentation time (`tick + alpha`), rebuilds the joint
   palette without per-frame allocation and reaches the existing skinning draw.
   The Blender fixture's `Idle` action exercises both supported interpolation
   forms through a non-bind-pose GPU pixel test.
5. **Revised foundation — player presentation.** A typed, read-only extraction carries
   stable identity, interpolated ground transform, last-tick displacement and
   authoritative attack state. App-owned role mapping drives idle, run and six
   profile-specific attack clips directly. Each clip's wind-up, contact and
   recovery segments follow the committed simulation phase lengths. The demo
   weapon is ordinary mesh geometry weighted to its animated bone. Character
   selection replaces only the fallback player cubes; no asset or presentation
   state enters gameplay.
6. **Complete — horde presentation.** A read-only, allocation-free enemy view
   supplies stable identity, interpolated ground position and last-tick
   displacement. App maps that view to idle or run and one of four stable phase
   offsets per role. Gfx uploads at most eight shared palettes and issues one
   instanced draw per occupied bucket; no animation state enters simulation.
   A small app-owned table retains each live stable identity's last meaningful
   heading so stopping does not snap it to zero.
   The harness selects this path atomically and reports its instance and draw
   counts.

## Deliberately deferred

- Runtime FBX and authoring-tool project files
- Arbitrary glTF scenes, cameras, lights and extensions
- Full PBR materials and material graphs
- Runtime skeleton retargeting and root motion
- Animation-authored gameplay events
- Blend-tree or animation-graph editors
- IK, facial animation, morph targets, cloth, hair and ragdolls
- Unique per-enemy skeletal evaluation
- Asset references in gameplay scene files
- Hot reload, background loading and a general resource cache

Offline conversion and retargeting are workflow steps, not missing runtime
features. A deferred item moves into scope only for a concrete visual, content
or measured performance requirement.

## Verification

Importer tests cover the accepted path and critical rejection and allocation
boundaries; each newly supported door arrives with its test. Headless GPU tests
assert pixels for vertex layouts, bindings, orientation, texture sampling and
colour arithmetic. The harness proves atomic selection, derived reporting and
readability at the real camera distance.

If a stage changes simulation behaviour, a scenario assertion remains its
completion gate. Presentation alone must not create a second simulation truth.

## Success condition

A character and clips prepared through the documented authoring workflow can be
converted to one `.glb` subset and shown as a readable player and varied horde,
without coupling simulation to assets or turning every enemy into a separate
draw and animation workload.
