# Collision and attack debug drawing

Enable **Collision outlines** in the [companion](companion.md) Debug tab. The
harness equivalent is `debug collision on` or `debug collision off`. It starts
off and remains an app setting across scene starts and restarts.

The player is yellow; other bodies, including fixed props, are cyan. Every
disc uses the simulation's actual centre and radius. Hinged panels use their
actual rectangular dimensions and rotation, with no circular approximation. These are the last
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
outlines in one instanced line draw. For rectangles, the shader projects the
circle points onto a square, then applies the panel dimensions and rotation. Disabling the view submits no debug draw.
It does not draw contacts, normals, velocities or history; editor selection is
separate from this runtime view.

`state.collision_debug` reports `enabled`, `sample_tick`, `outline_count`, and
`discs` keyed by stable entity identity. Each disc includes `centre` and `radius`.
`outline_count` totals body, panel and attack outlines; `body_disc_count` counts
circular bodies. `panel_count` and `panels` describe each rectangle by stable
identity, with its `centre`, `width`, `thickness` and `yaw`.
`attack` reports its displayed `disc_count`, sampled `phase` while enabled, and
`discs` keyed by zero-based sample index within the committed active window.
The record describes the same snapshot used to build the outlines, so an agent
can inspect it without interpreting a screenshot. Disabled views report an
empty disc set; switching scenes replaces the snapshot rather than retaining
old entity identities. `state.render.debug_outline_instances` and `debug_draws`
expose the workload separately.
