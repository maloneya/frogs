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
Keep the production rig stable across motions within a character family; add
joints for a demonstrated articulation or deformation need. Runtime rig limits need not restrict the
controls used for authoring: bake supported motion into the export rig.

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
arm participation and a solid grip before smoothing curves. Use a pose strip and
normal-cadence playback, then slow playback to diagnose arcs and deformation.
Retarget explicitly for units, facing, handedness and bind axes; inspect the
loaded and released poses after conversion. Preserve the gameplay hit window.

**Game presentation.** Load the plain candidate through
[ARPG_HARNESS](../playtest/SKILL.md) early. Inspect the actual camera, shading,
scale, standing/moving attacks, transitions and relevant contact feedback.
Add fitted clothing, equipment and effects over the working body and motion.
Keep the neck, waist, arm separation and action silhouette readable.

## Keep review fast and concrete

Compare candidates with the last accepted version using the same camera, scale,
lighting and cadence. Before presenting progress, inspect the comparison yourself
and identify the improvement and remaining defect. Do not rely on the user to
catch basic proportion or articulation failures.

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
where they fit; keep new tooling narrowly tied to the current review need. Work toward
repeatable preview generation instead of rewriting capture/render commands.

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
