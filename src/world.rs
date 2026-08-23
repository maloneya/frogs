use glam::Vec3;

use crate::gfx::{Instance, InstanceSink, MAX_INSTANCES};

/// Ground plane size, in tiles.
const GROUND_TILES: usize = 32;
const TILE: f32 = 1.5;

/// The floor's share of the instance budget.
const GROUND_INSTANCES: usize = GROUND_TILES * GROUND_TILES;

/// How large the horde may grow. The ground is drawn from the same instance
/// buffer in the same draw call, so the enemy budget is whatever the floor
/// leaves behind.
///
/// This constant lives next to the field it bounds rather than in the caller.
/// That placement is the whole point: previously the subtraction happened in
/// `app.rs`, which meant `World` did not know its own limit and any *second*
/// writer of `enemy_count` would silently overrun the GPU buffer — a failure
/// with no error message, since the upload just truncates.
const MAX_ENEMIES: usize = MAX_INSTANCES - GROUND_INSTANCES;

/// What exists. Currently a static grid of cubes; this is where the simulation
/// will live as it grows — fixed-timestep stepping, entity storage, spatial
/// partitioning.
pub struct World {
    enemy_count: usize,
}

impl Default for World {
    fn default() -> Self {
        Self { enemy_count: 1024 }
    }
}

impl World {
    pub fn enemy_count(&self) -> usize {
        self.enemy_count
    }

    /// The only door in, so the clamp cannot be bypassed or forgotten. A
    /// spawner, a save-load path or a debug console added later inherits it
    /// without having to know `MAX_ENEMIES` exists.
    pub fn set_enemy_count(&mut self, n: usize) {
        self.enemy_count = n.clamp(1, MAX_ENEMIES);
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
