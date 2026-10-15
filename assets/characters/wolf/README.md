# Wolf enemy

The ordinary enemy is a lean gray wolf, facing Blender `-Y`, authored in metres
and rendered at scale 1.0. It replaces the former horned enemy completely.
The body has a paired-ear silhouette, a tucked abdomen, a charcoal saddle fading
into pale cheeks/underside, dark paws and an attached nose and amber eyes. One
mesh, one coat texture/material and a shared 33-joint rig serve every clip.

The enemy's behavior, collisions, attack admission, damage, targeting, cooldowns,
knockback and ground travel remain owned by Game/Sim. Presentation maps the
existing Swipe action to `Bite` and the existing Leap action to `Leap`. No gameplay
tuning was changed. The leap's existing 1.8 m visual arc remains in presentation;
the asset supplies the takeoff, airborne shape, landing and recovery articulation.

## Sources and compilation

The compiler imports the retained, versioned authored sources directly:

- [Micket's CC0 Eurasian wolf](../../../experimental/character-references/sources/micket-wolf/wolf.blend)
  supplies the continuous mesh, painted weights and anatomical rig.
- [tomek's CC0 wolf](../../../experimental/character-references/sources/tomek-wolf/wolf.blend)
  supplies `wolf_idle`, `wolf_walk`, `wolf_run`, `wolf_attack01`, `wolf_jump`,
  `wolf_fly`, `wolf_land` and `wolf_die`.

[The reference catalog](../../../experimental/character-references/README.md)
retains exact source hashes, license snapshots and attribution. Original source
files remain unmodified. Edit those authored inputs deliberately or edit
`generate.py`; the preview `.blend` and runtime `.glb` files are compiled outputs.

Adaptation fits the source to a metre-scale wolf, emphasizes the ears and waist,
normalizes the strongest four skin weights, and adds a single root. Bind-axis
conversion retains whole-body motion. A baked analytical support solve preserves
source paw trajectories on the wolf's different limb lengths; this is offline
compilation, with no runtime IK. Facial details are fitted against the head
surface. The jaw opens in anticipation and shuts during bite contact. Head tilt
is bounded so the longer muzzle stays readable through landing compression.

```sh
blender --background --factory-startup --disable-autoexec --python-exit-code 1 \
  --python assets/characters/wolf/generate.py -- /tmp/arpg-wolf-build
```

Build twice in fresh directories and compare both GLBs before copying outputs
here. `build.json` records the Blender version, compiler/source/helper hashes and
output GLB hashes. Curve traversal uses the shared Blender motion helper.
`wolf.shadow.glb` is a reduced caster on the identical rig and clips;
the runtime uses only its geometry with the visible asset's palette.

## Motion review

`Idle` breathes over 3.2 seconds, `Walk` uses a four-foot cycle over 1 second,
and `Run` bounds over 0.6 seconds. Faster travel retimes the run. `Bite` and `Leap`
use frames 1/13/19/31 for ready/load/contact-end/recovered poses. App maps those
segments to each action's committed phase lengths, including authored tuning.
Default preview cadence is 36/9/39 ticks for bite and 42/36/54 for leap.
`Hit` supplies the existing three recoil stages. `Death` supplies a frozen,
curled pose that drops onto its measured support over the existing 15 ticks.

Use the maintained character reviewer with the bundled Python/Pillow runtime:

```sh
python3 assets/characters/basic-player/review.py target/wolf-review \
  --after assets/characters/wolf/wolf.blend \
  --clips Idle Walk Run Bite Leap --views side game
```

It produces normal and slow GIFs, semantic pose strips, a neutral critic packet
and hashed evidence. For anatomy, use `--clips Idle --frames 1 --views front side back game`.
Previews are retimed Blender renders; final camera, scale, shading and role
selection must also be checked through `ARPG_HARNESS`. A wolf asset requires
`Idle`, `Walk`, `Run`, `Bite`, `Leap`, `Hit` and `Death`; invalid replacement
preserves the accepted horde.
