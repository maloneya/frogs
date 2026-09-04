//! What exists, and how it describes itself to a renderer.
//!
//! Depends on `arpg-core` for vocabulary and on nothing else. In particular it
//! does not link wgpu, so simulation tests run without a GPU.

use glam::Vec3;

use arpg_core::{Instance, InstanceSink, MoveDir, MAX_INSTANCES};

/// Ground plane size, in tiles.
///
/// Sized so the world is comfortably larger than the view. A tracking camera
/// is meaningless otherwise: if the whole arena fits on screen there is nothing
/// for the camera to reveal, and following just slides the floor around inside
/// a frame that already showed everything.
///
/// The size is also what keeps the void off screen, and it is why the camera
/// does *not* clamp itself to the world bounds. Under this projection the view
/// covers roughly 57x55 world units of floor, whose axis-aligned footprint is
/// ~40 units either side of the focus. Subtract that from a 48-unit arena and a
/// bounds-clamped camera could travel +/-8 units total — it would be pinned,
/// and following would stop working before the player reached the edge. Making
/// the world bigger is the fix that a camera clamp only pretends to be.
const GROUND_TILES: usize = 128;
const TILE: f32 = 1.5;

/// The floor's share of the instance budget.
const GROUND_INSTANCES: usize = GROUND_TILES * GROUND_TILES;

/// How large the horde may grow. The ground and the player are drawn from the
/// same instance buffer in the same draw call, so the enemy budget is whatever
/// they leave behind.
///
/// This constant lives next to the field it bounds rather than in the caller.
/// That placement is the whole point: previously the subtraction happened in
/// `app.rs`, which meant `World` did not know its own limit and any *second*
/// writer of `enemy_count` would silently overrun the GPU buffer — a failure
/// with no error message, since the upload just truncates.
const MAX_ENEMIES: usize = MAX_INSTANCES - GROUND_INSTANCES - 1;

/// Half the ground plane's width, in world units.
const ARENA_HALF: f32 = GROUND_TILES as f32 * TILE * 0.5;

/// World units per second. Fast enough that the arena crosses in a few seconds,
/// which is the range an ARPG lives in — slow movement makes a horde feel like
/// a traffic jam rather than a threat.
const PLAYER_SPEED: f32 = 9.0;

/// Deliberately taller than an enemy (0.5), so the player stays readable from
/// inside a crowd of them. Silhouette is the cheapest legibility tool there is.
const PLAYER_SCALE: Vec3 = Vec3::new(0.55, 1.2, 0.55);

/// The player-controlled character.
///
/// A single struct held apart from the horde, and it stays that way even once
/// enemy storage becomes SoA arrays: there is exactly one of these, it is the
/// only thing input drives, and it will accumulate state no enemy has — facing,
/// attack phase, i-frames, buffered inputs. Wedging it into the horde's storage
/// to avoid a "special case" would mean paying for those fields N times.
struct Player {
    pos: Vec3,
}

impl Default for Player {
    fn default() -> Self {
        Self { pos: Vec3::new(0.0, PLAYER_SCALE.y * 0.5, 0.0) }
    }
}

/// What exists. This is where the simulation will live as it grows —
/// fixed-timestep stepping, entity storage, spatial partitioning.
pub struct World {
    enemy_count: usize,
    player: Player,
}

impl Default for World {
    fn default() -> Self {
        Self { enemy_count: 1024, player: Player::default() }
    }
}

impl World {
    /// How many enemies the horde currently holds. Always within
    /// `1..=MAX_ENEMIES`, because [`World::set_enemy_count`] is the only writer.
    pub fn enemy_count(&self) -> usize {
        self.enemy_count
    }

    /// The only door in, so the clamp cannot be bypassed or forgotten. A
    /// spawner, a save-load path or a debug console added later inherits it
    /// without having to know `MAX_ENEMIES` exists.
    pub fn set_enemy_count(&mut self, n: usize) {
        self.enemy_count = n.clamp(1, MAX_ENEMIES);
    }

    /// Advances the world by `dt` seconds.
    ///
    /// `dt` is the raw frame time and movement is integrated against it, so
    /// speed is frame-rate independent today. It is a plain `f32` rather than
    /// the `Dt` newtype the roadmap wants, and deliberately so: the whole point
    /// of `Dt` is that it is *fixed*, and minting one from a variable frame time
    /// would be a type asserting something untrue. It arrives with the
    /// fixed-timestep accumulator, not before.
    ///
    /// `move_dir` is world-space and already unit-or-zero — the type says so,
    /// so this does not have to check.
    pub fn step(&mut self, dt: f32, move_dir: MoveDir) {
        self.player.pos += move_dir.as_vec3() * PLAYER_SPEED * dt;

        // The ground plane *is* the world; there is nothing beyond it to walk
        // onto. This was previously a stand-in for the camera being pinned to
        // the origin — that reason is gone now the camera tracks, but the
        // boundary itself is real and stays until the world has an outside.
        let limit = ARENA_HALF - PLAYER_SCALE.x * 0.5;
        self.player.pos.x = self.player.pos.x.clamp(-limit, limit);
        self.player.pos.z = self.player.pos.z.clamp(-limit, limit);

    }

    /// Where the character is standing. The camera will want this shortly.
    pub fn player_pos(&self) -> Vec3 {
        self.player.pos
    }

    /// **The seam.** The world describes itself in the renderer's vocabulary;
    /// `gfx` never sees a `World`.
    ///
    /// Takes the sink by value, so it is single-use and cannot outlive the
    /// frame. Everything about the buffer — that it was reset, that it is
    /// capacity-bounded, that pushing is the only thing anyone may do to it —
    /// is settled by the type rather than by remembering. Next chunk this
    /// grows an `alpha` parameter for interpolating between simulation ticks.
    pub fn extract(&self, mut out: InstanceSink<'_>) {
        self.extract_ground(&mut out);
        self.extract_enemies(&mut out);
        self.extract_player(&mut out);
    }

    /// The player is not a special case to the renderer either — one more cube
    /// in the same draw call. Only the colour and the silhouette distinguish it.
    fn extract_player(&self, out: &mut InstanceSink<'_>) {
        // Linear, and it looks wrong here on purpose: the surface is sRGB, so
        // the hardware encodes on write. This is roughly sRGB (0.35, 0.72, 0.95)
        // — a bright cyan-blue, chosen to sit opposite the horde's muted red on
        // the colour wheel so the eye separates them without effort.
        out.push(Instance::new(
            self.player.pos,
            PLAYER_SCALE,
            Vec3::new(0.10, 0.47, 0.88),
        ));
    }

    /// The floor is not a special case — it is just more cube instances, flat
    /// and tinted. Same mesh, same pipeline, same draw call as the horde.
    fn extract_ground(&self, out: &mut InstanceSink<'_>) {
        let offset = (GROUND_TILES as f32 - 1.0) * TILE * 0.5;

        for z in 0..GROUND_TILES {
            for x in 0..GROUND_TILES {
                let checker = (x + z) % 2 == 0;
                let shade = if checker { 0.022 } else { 0.038 };
                out.push(Instance::new(
                    Vec3::new(x as f32 * TILE - offset, -0.05, z as f32 * TILE - offset),
                    Vec3::new(TILE, 0.1, TILE),
                    Vec3::new(shade, shade * 1.05, shade * 1.25),
                ));
            }
        }
    }

    fn extract_enemies(&self, out: &mut InstanceSink<'_>) {
        let side = (self.enemy_count as f32).sqrt().ceil().max(1.0) as usize;
        // Deliberately *not* TILE. Matching the floor's spacing made the horde
        // tile it exactly edge-to-edge, hiding the ground and — worse — making
        // a change in N invisible, because the horde only grew off-screen.
        let spacing = 0.7;
        let offset = (side as f32 - 1.0) * spacing * 0.5;

        for i in 0..self.enemy_count {
            let x = (i % side) as f32 * spacing - offset;
            let z = (i / side) as f32 * spacing - offset;

            // Linear-space colour, since the surface is sRGB and the hardware
            // encodes on write. These look darker here than they will on screen.
            let t = (i % 7) as f32 / 7.0;
            let color = Vec3::new(0.30 + t * 0.12, 0.06 + t * 0.05, 0.05);

            // Half-height 0.4, so the cube sits on the ground rather than in it.
            out.push(Instance::new(Vec3::new(x, 0.25, z), Vec3::splat(0.5), color));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arpg_core::InstanceBuffer;

    /// Speed must come from `dt`, not from how often `step` happens. Two half
    /// steps and one whole step have to land in the same place, or the game
    /// runs faster on a faster machine — the oldest bug in the medium.
    #[test]
    fn movement_is_frame_rate_independent() {
        let east = MoveDir::new(Vec3::X);

        let mut coarse = World::default();
        coarse.step(0.2, east);

        let mut fine = World::default();
        for _ in 0..20 {
            fine.step(0.01, east);
        }

        assert!((coarse.player_pos() - fine.player_pos()).length() < 1e-4);
    }

    #[test]
    fn no_input_does_not_move_the_player() {
        let mut world = World::default();
        let start = world.player_pos();
        world.step(1.0, MoveDir::NONE);
        assert_eq!(world.player_pos(), start);
    }

    /// Walking into the wall must stop, not leave the ground plane — and must
    /// stay finite, since a NaN position would silently vanish the character.
    #[test]
    fn the_player_cannot_walk_off_the_arena() {
        let mut world = World::default();
        for dir in [Vec3::X, Vec3::Z, Vec3::NEG_X, Vec3::NEG_Z] {
            for _ in 0..100 {
                world.step(1.0, MoveDir::new(dir));
            }
            let pos = world.player_pos();
            assert!(pos.is_finite());
            assert!(pos.x.abs() <= ARENA_HALF && pos.z.abs() <= ARENA_HALF, "escaped: {pos}");
        }
    }

    /// Movement is horizontal: the character stays planted on the floor no
    /// matter what direction is asked for.
    #[test]
    fn movement_stays_on_the_ground_plane() {
        let mut world = World::default();
        let height = world.player_pos().y;
        world.step(0.5, MoveDir::new(Vec3::new(1.0, 5.0, 1.0)));
        assert_eq!(world.player_pos().y, height);
    }

    /// The whole world — floor, horde and player — has to fit the one buffer
    /// they share, at the largest horde the clamp permits.
    #[test]
    fn a_full_horde_still_fits_alongside_the_ground_and_the_player() {
        let mut world = World::default();
        world.set_enemy_count(usize::MAX);

        let mut buf = InstanceBuffer::default();
        world.extract(buf.sink());

        assert_eq!(buf.as_slice().len(), MAX_INSTANCES);
    }
}
