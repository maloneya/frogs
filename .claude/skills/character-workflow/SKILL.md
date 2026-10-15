---
name: character-workflow
description: Develop arpg character models and animations from a visual idea to a reviewed in-game checkpoint using concrete references, reusable authored assets, and staged previews. Use for new characters or visible changes to anatomy, skinning, equipment, or motion. Export-only and importer diagnostics belong to author-blender-asset.
---

# Character workflow

Establish convincing visual evidence early, then preserve it through integration.
Treat artistic quality and technical validity as separate judgments. Read
[character motion acceptance](../../../docs/character-motion.md) for the visual
principles and [the asset contract](../../../docs/character-assets-authoring.md)
for runtime constraints. Read the target asset's README before adapting it.

## Establish the target

Turn the request into a short working brief: character identity and proportions,
weapon and action, desired weight/responsiveness, actual game camera, and
mechanical constraints such as the hit schedule. Use the user's existing choices;
ask only for missing information that materially affects the design.

Choose concrete appearance and motion references. Identify the asset or exact
clip/sequence and what it demonstrates. Prefer an authored body with usable
weights and a compatible motion library when they fit the brief. Check the
usable contents and licence before building around a pack. Retain provenance.
A style video provides visual reference; it does not provide measured 3D motion.

Adapt an accepted foundation before inventing a new body, rig, or choreography.
For the current player, start with its retained body, motions and reference catalog.
State the visible properties to preserve and the specific mismatch to change;
“more weight” must identify support, body travel or timing rather than imply a
more extreme bend. Rejected visual directions are evidence for changing the
foundation or method, not just increasing the same pose offsets.
Keep the production rig stable across motions within a character family; add
joints for a demonstrated articulation or deformation need. Runtime rig limits need not restrict the
controls used for authoring: bake supported motion into the export rig.
Choose controls for the edit: hip and foot targets for stance or weight transfer,
with sparse whole-body poses before dense baking. Reuse existing controls.
Preserve the accepted poses when adding them; inspect the evaluated result before
building the rest of the export. Keep any human-facing posing file simple and
ready to key and save, rather than exposing a dense runtime bake by default.

## Develop the visible result

Use the relevant passes below. Reuse already accepted work; a small targeted
change does not need to repeat the entire process. Change one major variable at
a time so comparison can identify what improved or regressed.

**Body and rig.** Compare equal-height front, side, back and game-camera views
with plain materials. Fit joints inside the authored anatomy. Exercise the
poses relevant to the action: overhead/cross-body reach, bent elbows, wrist
rotation and grip, squat/lunge and supporting feet. Inspect proportions,
negative space and shoulder, hip and knee deformation before adding armour.
A rig with more joints is not evidence of better anatomy or skinning.

**Motion.** Start with the source or a few whole-body key poses. Show readiness,
loaded effort, release and follow-through. Review silhouettes, support, opposite
arm participation and a solid grip before smoothing curves. Trace the force:
which foot loads, how pelvis and chest travel through release, and which foot
receives the body. Distinguish torso bending from weight transfer. For the
current Cleave, retain a moving wind-up and rear-foot drive; do not generalize
its exact pose depth or travel to other actions. Use a pose strip and
normal-cadence playback, then slow playback to diagnose arcs and deformation.
Retarget explicitly for units, facing, handedness and bind axes; inspect the
loaded and released poses after conversion. Preserve the gameplay hit window.

**Game presentation.** Load the plain candidate through
[ARPG_HARNESS](../playtest/SKILL.md) early. Inspect the actual camera, shading,
scale, standing/moving attacks, transitions and relevant contact feedback.
Add fitted clothing, equipment and effects over the working body and motion.
Keep the neck, waist, arm separation and action silhouette readable.
For clothing, establish the garment silhouette, attachment/support and continuous
surface before adding folds or texture. Inspect front, side and back joins under
motion. Where hand-shaped folds remain unconvincing, compare an authored garment
or an offline cloth drape baked into source geometry; runtime cloth is a separate
capability decision, and simulation alone does not establish a good garment.

## Keep review fast and concrete

Compare candidates with the last accepted version using the same camera, scale,
lighting and cadence. Before presenting progress, inspect the comparison yourself
and identify the improvement and remaining defect. Do not rely on the user to
catch basic proportion or articulation failures. Choose views for the suspected
defect: side and skeleton views for support/body travel, front/back for garment
volume and joins, contact close-ups for grips. Compare semantic action phases
such as load, contact and recovery; evenly spaced thumbnails can miss them.
State what visibly improved against the target and what still fails before
running promotion checks. A failed visual approach calls for a new hypothesis
and a small diagnostic, rather than another complete build of the same guess.

For substantial visual checkpoints, use a fresh subagent as an independent
visual critic. Give it images and visual references, without the implementation
history, test results or claimed improvements. Follow the
[visual critique protocol](references/visual-critique.md) for evidence selection,
uncertainty and checking the revised result. Reuse relevant review evidence for
small edits; request only the views affected by the change.

Inspect the complete equipped character, not just the requested change. Check
hands enclosing grips, weapon/body clearance, clothing fit, joint deformation,
foot support and motion transitions. Use contact close-ups as well as full-body
views; inspect fractional samples and runtime blends where the interaction can
break. An obvious floating, penetrating or unheld object blocks promotion even
when structural tests pass. Do not label a broken interaction as cosmetic
refinement or defer finding it to the user.

When an interaction fails, trace the shared relationship and its validation.
Prefer a derived constraint with one owner over independent offsets or poses
per clip, item or consumer. Define anatomical and equipment landmarks once,
solve their relationship, and verify the observable contact and clearance.
A constant bone transform alone does not establish a physical grasp. Use the
player's [grasp compiler](../../../assets/characters/basic-player/grasp.py) as
the equipment owner; keep choreography in the authored motion producers.

For motion reviews, provide a directly embedded normal-speed GIF, a slow GIF and
key-pose strip when useful. Prefer these mobile-readable artifacts to requiring a
localhost viewer. Label source playback, retimed asset previews and actual game
captures accurately. Record the asset hash and playback timing with the evidence;
a stale render must not silently stand in for a newer asset.

Use the player's [preview tools](../../../assets/characters/basic-player/README.md#review-motion)
and the [reference catalog](../../../experimental/character-references/README.md)
where they fit. Run `review.py` with the bundled Python/Pillow runtime for one
render-and-package command. Select fixed views, equipped/plain/skeleton mode,
semantic poses or fractional frames, and opposing grip close-ups using its
options. Its `critic/` packet has neutral labels; keep provenance outside the
critic's inputs. Extend this maintained tool for a missing diagnostic instead
of writing another task-specific render script.
Keep the strongest provisional candidate available during exploration; the
latest revision must earn replacement through a matched visual comparison.
Retain a compact record of the visual hypothesis, changed owner, result and
unresolved issue so failed approaches are not rediscovered after context changes.
Use the bundled dependency runtime for Pillow packaging. Run packaging only
after rendering succeeds. Inspect unfamiliar source inventory and evaluated
poses before building adapters around assumed bones, face sets or clips.
When changing handedness or blade paths, include runtime trails and impact
effects in the visual comparison through the active phase.

Seek feedback on concrete visual choices or checkpoints. Use approval already
given in the conversation; do not add repeated confirmations for routine
imports, reversible edits, or integration that the user has authorized.

## Preserve the source and promote

Keep artist-authored geometry, painted weights and poses in a distinct versioned
source asset, such as `source.blend`, which the compiler imports. Keep scripted
adaptation, normalization, baking, validation and export in Python. Generated
preview `.blend` files and runtime `.glb` files are outputs; hand edits to them
must be captured in source inputs before regeneration. A live Blender session
alone is not durable source. Scope this source workflow to the asset being edited.

Follow [author-blender-asset](../author-blender-asset/SKILL.md) for deterministic
compilation, two isolated builds, importer checks and game-renderer evidence.
Use focused checks while exploring; run the
[completion verification](../../../docs/verification.md) when promoting the
accepted checkpoint. Keep visual exploration explicitly provisional until its
required checks are complete. Follow the existing performance policy and honour
user-authorized coverage choices; report missing coverage precisely.

At promotion, compare the candidate with the accepted preview. Verify ordinary
startup/restoration uses it, relevant gameplay phases and attachments still work,
and refusal preserves the accepted asset. Prefer checks of these observable
contracts to brittle checks of incidental implementation details.

Finish with the visible outcome, remaining visual limitations, source/artifact
locations and verification evidence. Preserve a known-good checkpoint. Keep the
work within the requested character/animation scope; an asset task does not
implicitly authorize cleanup of unrelated physics or renderer changes.
