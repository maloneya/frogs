# Aiming while moving

WASD/arrows control travel; the cursor controls aim. Attack buttons use the
[fixed slots](../README.md#controls). A swing starts once
per press. Holding its button does not repeat it, and presses during a swing
are dropped. Beam attacks instead channel while their button is held. A new swing immediately takes the current aim and holds that
direction through recovery. Walking and external impulses continue normally.
Between attacks the character turns toward the current aim at the facing
pass's rate.

`core::Intent` requires a validated `AimDir` on every tick. Zero, vertical and
non-finite directions are rejected at construction. `app::input` projects
physical cursor pixels through the current camera onto the ground. At the
player's feet, outside the window, or after focus loss it retains its last
valid direction. Focus loss releases gameplay input. Restart
restores the initial +Z aim and clears accepted input.

The scenario keyboard driver supplies movement as aim and retains it while
stationary. Explicit `aims` commands select independent directions instead;
see [the retreating attack scenario](../scenarios/retreating_attacks_keep_their_aim.ron).
The live harness supplies cursor coordinates with `cursor <x> <y>`.

Character placement follows the authoritative simulation direction. App blends
forward/backward and sideways movement clips using displacement relative to
that facing. The authored strafes turn the hips and feet toward travel while the
chest stays nearer aim and the head follows it. Both arms counter-swing with the
stride. Moving attacks and beams align their upper-body layer above the turning
hips, preserving the ability's orientation. The asset sampler owns validated
masks, local blending and parent-aligned layers; gfx receives the finished skin
matrices. Animation never changes hitboxes or knockback. See the
[asset contract](character-assets-authoring.md) for clips and required joints.

The preview report includes `locomotion_base`, `locomotion_overlay` and the
latter's weight. Its existing `clip` is the role's timing reference, not the
complete blended pose. All four movement cycles share phase so diagonal blends
keep the same supporting leg. Backward travel uses backpedalling rather than
turning the torso through 180 degrees.
