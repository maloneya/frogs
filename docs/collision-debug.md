# Collision and attack debug drawing

F3 toggles collision outlines while playing (close F1/F2 menus first). The
harness equivalent is `debug collision on` or `debug collision off`. It starts
off and remains an app setting across scene starts and restarts.

The player is yellow; other bodies, including fixed props, are cyan. Every
outline uses the simulation's actual centre and radius. These are the last
completed tick's positions, with no render interpolation. Moving art may lead
or trail its outline by part of a tick. This is intentional: the view describes
physical state, not an approximation inferred from the character mesh.

Attack startup shows the committed path as muted orange circles; active ticks
show only the current hitbox in bright orange-red. Idle and recovery show no
attack circles. These come from the same stored samples and placement function
used by hit detection, following the completed tick's player position and
facing. Changing the selected profile mid-swing affects only the next attack.
The outlines describe gameplay hitboxes, not the animated weapon mesh.

Outlines lie on the ground plane, are unlit, and remain visible through assets.
They never write depth. The renderer reuses one 48-segment circle and draws all
outlines in one instanced line draw. Disabling the view submits no debug draw.
This first slice has no contacts, normals, velocities, selection, or history.

`state.collision_debug` reports `enabled`, `sample_tick`, `disc_count`, and
`discs` keyed by stable entity identity. Each disc includes `centre` and `radius`.
`disc_count` totals body and attack outlines; `body_disc_count` counts bodies.
`attack` reports its displayed `disc_count`, sampled `phase` while enabled, and
`discs` keyed by zero-based sample index within the committed active window.
The record describes the same snapshot used to build the outlines, so an agent
can inspect it without interpreting a screenshot. Disabled views report an
empty disc set; switching scenes replaces the snapshot rather than retaining
old entity identities. `state.render.debug_disc_instances` and `debug_draws`
expose the workload separately.

The engine supplies immutable `CollisionDisc` values through `collision_discs()`;
app chooses visibility and colour; gfx receives anonymous geometry. No debug
setting enters `Game`, simulation inputs, hashes, or trace. The population limit plus the longest permitted attack path
is checked against debug GPU capacity at compile time. CPU buffers are reserved
on first enable and reused afterwards, including across off/on toggles.

Validation includes snapshot/no-mutation tests, strict harness parsing, a GPU
pixel test for the XZ outline and through-art visibility, and disabling after a
populated GPU upload. Existing scenarios remain the combat and physics gate.
