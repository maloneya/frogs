use glam::Vec3;

use crate::gfx::Instance;

/// Ground plane size, in tiles.
const GROUND_TILES: usize = 32;
const TILE: f32 = 1.5;

/// The floor's share of the instance budget.
pub const GROUND_INSTANCES: usize = GROUND_TILES * GROUND_TILES;

/// What exists. Currently a static grid of cubes; this is where the simulation
/// will live as it grows — fixed-timestep stepping, entity storage, spatial
/// partitioning.
pub struct World {
    pub enemy_count: usize,
}

impl Default for World {
    fn default() -> Self {
        Self { enemy_count: 1024 }
    }
}

impl World {
    /// **The seam.** The world describes itself in the renderer's vocabulary and
    /// hands over a flat slice; `gfx` never sees a `World`.
    ///
    /// Writes into a caller-owned buffer that gets reused every frame, so a
    /// steady state costs zero allocations. Next chunk this grows an `alpha`
    /// parameter for interpolating between simulation ticks.
    pub fn extract(&self, out: &mut Vec<Instance>) {
        out.clear();
        self.extract_ground(out);
        self.extract_enemies(out);
    }

    /// The floor is not a special case — it is just more cube instances, flat
    /// and tinted. Same mesh, same pipeline, same draw call as the horde.
    fn extract_ground(&self, out: &mut Vec<Instance>) {
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

    fn extract_enemies(&self, out: &mut Vec<Instance>) {

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
