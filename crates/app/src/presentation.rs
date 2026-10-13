//! Asset selection and character presentation; no write access to simulation.

use arpg_core::{Instance, InstanceBuffer, LinearRgb, Report};
use arpg_gfx::{
    CharacterBucket, CharacterHorde, CharacterMesh, CharacterPreview, MAX_HORDE_POSE_BUCKETS,
    MeshAsset, MeshBatch,
};
use arpg_sim::{Alpha, AttackPhase, AttackProfile, CharacterPresentation, Fnv, PlayerPresentation, TICK_HZ, };
use glam::Vec3;

// Static placements include an all-prop world, the ground and one campfire per light and bounded trees.
// This also covers an all-enemy world plus the player in the character buffer.
const _: () = assert!(arpg_sim::MAX_BODIES + arpg_game::MAX_CORPSES + arpg_sim::MAX_WALLS + 1 + arpg_core::MAX_POINT_LIGHTS + arpg_game::MAX_TREES <= arpg_core::MAX_INSTANCES);

struct LoadedMesh<T> {
    path: std::path::PathBuf,
    vertex_count: usize,
    index_count: usize,
    texture_width: u32,
    texture_height: u32,
    gpu: T,
}

impl<T> LoadedMesh<T> {
    fn report(&self, out: &mut Report) {
        out.bool("active", true);
        out.text("path", &self.path.to_string_lossy());
        out.int("vertices", self.vertex_count as u64);
        out.int("indices", self.index_count as u64);
        out.int("texture_width", u64::from(self.texture_width));
        out.int("texture_height", u64::from(self.texture_height));
    }
}

impl LoadedMesh<MeshAsset> {
    fn draw<'a>(&'a self, instances: &'a [Instance]) -> MeshBatch<'a> {
        MeshBatch::new(&self.gpu, instances)
    }
}

/// Required static world assets and their reusable placement buffers.
/// Every required asset must import and upload before this value can exist.
pub(super) struct WorldAssets<T> {
    ground: LoadedMesh<T>,
    prop: LoadedMesh<T>,
    wall: LoadedMesh<T>,
    campfire: LoadedMesh<T>,
    trees: [(arpg_game::TreeKind, LoadedMesh<T>, Vec<Instance>); 3],
    walls: Vec<Instance>,
    panels: Vec<Instance>,
    chest_closed: LoadedMesh<T>,
    chest_open: LoadedMesh<T>,
    closed_chests: Vec<Instance>,
    open_chests: Vec<Instance>,
    ground_instance: [Instance; 1],
    props: InstanceBuffer,
    pub(super) torches: crate::torch::Torches,
}

impl<T> WorldAssets<T> {
    pub(super) fn load(mut upload: impl FnMut(&arpg_assets::StaticMesh) -> Result<T, String>) -> Result<Self, String> {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/world");
        let ground = load_static(root.join("ground.glb"), &mut upload)?;
        let prop = load_static(root.join("prop.glb"), &mut upload)?;
        let chest_closed = load_static(root.join("chest/closed.glb"), &mut upload)?;
        let wall = load_static(root.join("wall/wall.glb"), &mut upload)?;
        let campfire = load_static(root.join("woodland/campfire.glb"), &mut upload)?;
        let trees = [
            (arpg_game::TreeKind::Pine, load_static(root.join("woodland/pine.glb"), &mut upload)?, Vec::with_capacity(arpg_game::MAX_TREES)),
            (arpg_game::TreeKind::Broadleaf, load_static(root.join("woodland/broadleaf.glb"), &mut upload)?, Vec::with_capacity(arpg_game::MAX_TREES)),
            (arpg_game::TreeKind::Bare, load_static(root.join("woodland/bare.glb"), &mut upload)?, Vec::with_capacity(arpg_game::MAX_TREES)),
        ];
        let chest_open = load_static(root.join("chest/open.glb"), upload)?;
        Ok(Self {
            ground,
            prop,
            wall,
            campfire,
            trees,
            walls: Vec::with_capacity(arpg_sim::MAX_WALLS),
            panels: Vec::with_capacity(arpg_sim::MAX_PANELS),
            chest_closed,
            chest_open,
            closed_chests: Vec::with_capacity(arpg_game::MAX_CHESTS),
            open_chests: Vec::with_capacity(arpg_game::MAX_CHESTS),
            ground_instance: [Instance::new(
                Vec3::ZERO,
                Vec3::new(arpg_sim::ARENA_HALF * 2.0, 1.0, arpg_sim::ARENA_HALF * 2.0),
                LinearRgb::WHITE,
            )],
            props: InstanceBuffer::default(),
            torches: crate::torch::Torches::default(),
        })
    }

    pub(super) fn rebuild(&mut self, props: impl Iterator<Item = arpg_sim::PropPresentation>, chest_state: impl Fn(arpg_sim::EntityId) -> Option<arpg_game::ChestState>) {
        self.closed_chests.clear();
        self.open_chests.clear();
        let mut sink = self.props.sink();
        for prop in props {
            if let Some(state) = chest_state(prop.id()) {
                let instances = if state == arpg_game::ChestState::Open { &mut self.open_chests } else { &mut self.closed_chests };
                assert!(instances.len() < arpg_game::MAX_CHESTS, "game chest admission bound");
                instances.push(Instance::new(prop.ground_position(), Vec3::ONE, LinearRgb::WHITE));
                continue;
            }
            let tint = if prop.interaction() == Some(arpg_sim::InteractionState::Activated) {
                LinearRgb::new(0.06, 0.55, 0.12)
            } else {
                LinearRgb::new(0.04, 0.18, 0.32)
            };
            sink.push(Instance::new(prop.ground_position(), Vec3::ONE, tint));
        }
    }

    pub(super) fn rebuild_walls(&mut self, walls: impl Iterator<Item = (arpg_sim::SceneId, usize, arpg_sim::Wall)>) {
        self.walls.clear();
        for (_, _, wall) in walls {
            assert!(self.walls.len() < arpg_sim::MAX_WALLS, "scene wall admission bound");
            let centre = wall.centre();
            let size = wall.size();
            self.walls.push(Instance::new(Vec3::new(centre.x, 0.0, centre.y),
                Vec3::new(size.x, 1.0, size.y), LinearRgb::WHITE).with_yaw(wall.yaw()));
        }
    }

    pub(super) fn rebuild_panels(&mut self, panels: impl Iterator<Item = arpg_sim::PanelPresentation>) {
        self.panels.clear();
        for panel in panels {
            assert!(self.panels.len() < arpg_sim::MAX_PANELS, "physical panel admission bound");
            let centre = panel.centre();
            let size = panel.size();
            let colour = match panel.latch() {
                arpg_sim::LatchState::Latched => LinearRgb::new(0.35, 0.12, 0.035),
                arpg_sim::LatchState::Released => LinearRgb::new(0.25, 0.22, 0.09),
                arpg_sim::LatchState::Broken => LinearRgb::new(0.55, 0.22, 0.055),
            };
            self.panels.push(Instance::new(Vec3::new(centre.x, 0.0, centre.y),
                Vec3::new(size.x, 1.0, size.y), colour).with_yaw(panel.yaw()));
        }
    }

    pub(super) fn rebuild_trees(&mut self, trees: impl Iterator<Item = arpg_game::TreeSpec>) {
        for (_, _, instances) in &mut self.trees { instances.clear(); }
        for (count, tree) in trees.enumerate() {
            assert!(count < arpg_game::MAX_TREES, "game tree admission bound");
            let (_, _, instances) = self.trees.iter_mut().find(|(kind, _, _)| *kind == tree.kind()).expect("complete tree catalog");
            let pos = tree.position();
            instances.push(Instance::new(Vec3::new(pos.x, 0.0, pos.y), Vec3::splat(tree.scale()), LinearRgb::WHITE).with_yaw(tree.yaw()));
        }
    }

    pub(super) fn instance_count(&self) -> usize { 1 + self.trees.iter().map(|(_, _, instances)| instances.len()).sum::<usize>() + self.walls.len() + self.panels.len() + self.torches.stand().len() + self.props.as_slice().len() + self.closed_chests.len() + self.open_chests.len() }

    pub(super) fn draw_count(&self) -> usize { 1 + self.trees.iter().filter(|(_, _, instances)| !instances.is_empty()).count() + usize::from(!self.walls.is_empty()) + usize::from(!self.panels.is_empty()) + usize::from(!self.torches.stand().is_empty()) + usize::from(!self.props.as_slice().is_empty()) + usize::from(!self.closed_chests.is_empty()) + usize::from(!self.open_chests.is_empty()) }

    pub(super) fn report(&self, out: &mut Report) {
        out.object("campfire", |out| self.campfire.report(out));
        out.int("tree_instances", self.trees.iter().map(|(_, _, instances)| instances.len()).sum::<usize>() as u64);
        out.object("trees", |out| {
            for (kind, mesh, instances) in &self.trees {
                out.object(&format!("{kind:?}"), |out| { mesh.report(out); out.int("instances", instances.len() as u64); });
            }
        });
        out.object("wall", |out| self.wall.report(out));
        out.int("wall_instances", self.walls.len() as u64);
        out.int("panel_instances", self.panels.len() as u64);
        out.object("ground", |out| self.ground.report(out));
        out.object("prop", |out| self.prop.report(out));
        out.object("chest_closed", |out| self.chest_closed.report(out));
        out.object("chest_open", |out| self.chest_open.report(out));
        out.int("closed_chest_instances", self.closed_chests.len() as u64);
        out.int("open_chest_instances", self.open_chests.len() as u64);
        out.int("torch_instances", self.torches.stand().len() as u64);
        out.int("ground_instances", 1);
        out.int("prop_instances", self.props.as_slice().len() as u64);
        out.int("draws", self.draw_count() as u64);
    }
}

impl WorldAssets<MeshAsset> {
    pub(super) fn draws(&self) -> [MeshBatch<'_>; 10] {
        [
            self.ground.draw(&self.ground_instance),
            self.wall.draw(&self.walls),
            self.wall.draw(&self.panels),
            self.prop.draw(self.props.as_slice()),
            self.campfire.draw(self.torches.stand()),
            self.chest_closed.draw(&self.closed_chests),
            self.chest_open.draw(&self.open_chests),
            self.trees[0].1.draw(&self.trees[0].2),
            self.trees[1].1.draw(&self.trees[1].2),
            self.trees[2].1.draw(&self.trees[2].2),
        ]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PresentationRole {
    Idle,
    Run,
    Attack(AttackProfile),
}

impl PresentationRole {
    fn label(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Run => "run",
            Self::Attack(AttackProfile::Cleave) => "attack_cleave",
            Self::Attack(AttackProfile::Slam) => "attack_slam",
            Self::Attack(AttackProfile::Disintegrate) => "channel",
        }
    }
}

struct CharacterClips {
    idle: arpg_assets::ClipId,
    run: arpg_assets::ClipId,
    attacks: Vec<arpg_assets::ClipId>,
    backward: arpg_assets::ClipId,
    left: arpg_assets::ClipId,
    right: arpg_assets::ClipId,
}

impl CharacterClips {
    fn resolve(asset: &arpg_assets::CharacterAsset) -> Result<Self, String> {
        let clip = |name| {
            asset
                .clip_named(name)
                .ok_or_else(|| format!("missing player presentation clip {name}"))
        };
        let idle = clip("Idle")?;
        let attack = |profile| clip(Self::attack_name(profile));
        let attacks = AttackProfile::ALL
            .into_iter()
            .map(attack)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            idle,
            run: clip("Run")?,
            backward: clip("RunBackward")?,
            left: clip("RunLeft")?,
            right: clip("RunRight")?,
            attacks,
        })
    }

    fn attack_name(profile: AttackProfile) -> &'static str {
        match profile {
            AttackProfile::Cleave => "AttackCleave",
            AttackProfile::Slam => "AttackSlam",
            AttackProfile::Disintegrate => "Disintegrate",
        }
    }

    fn for_role(&self, role: PresentationRole) -> arpg_assets::ClipId {
        match role {
            PresentationRole::Idle => self.idle,
            PresentationRole::Run => self.run,
            PresentationRole::Attack(profile) => self.attack(profile),
        }
    }

    fn attack(&self, profile: AttackProfile) -> arpg_assets::ClipId {
        let index = AttackProfile::ALL
            .iter()
            .position(|candidate| *candidate == profile)
            .expect("every attack profile is present in ALL");
        self.attacks[index]
    }
}

/// The exact locomotion blend used to build the last displayed pose.
struct LocomotionSample {
    base: arpg_assets::ClipId,
    overlay: arpg_assets::ClipId,
    weight: f32,
}

impl LocomotionSample {
    fn idle(clips: &CharacterClips) -> Self {
        Self { base: clips.idle, overlay: clips.idle, weight: 0.0 }
    }
}

/// CPU hierarchy, role mapping and GPU resources committed after a complete load.
pub(super) struct LoadedCharacter<T> {
    path: std::path::PathBuf,
    asset: arpg_assets::CharacterAsset,
    pose: arpg_assets::CharacterPose,
    clips: CharacterClips,
    scratch: arpg_assets::CharacterPose,
    full_mask: arpg_assets::PoseMask,
    upper_mask: arpg_assets::PoseMask,
    beam_origin: arpg_assets::JointId,
    sampled_role: PresentationRole,
    sampled_seconds: f32,
    channel_weight: f32,
    sampled_at: Option<f64>,
    locomotion: LocomotionSample,
    gpu: T,
}

impl<T> LoadedCharacter<T> {
    pub(super) fn summary(&self) -> String {
        let clip = self
            .asset
            .clip(self.clips.idle)
            .expect("resolved clip remains live");
        format!(
            "player character vertices={} indices={} nodes={} joints={} clips={} parts={} materials={} idle={} path={}",
            self.asset.vertex_count(),
            self.asset.index_count(),
            self.asset.node_count(),
            self.asset.joint_count(),
            self.asset.clip_count(),
            self.asset.parts().len(),
            self.asset.materials().len(),
            clip.name(),
            self.path.display()
        )
    }

    pub(super) fn beam_emitter(&self, player: PlayerPresentation) -> crate::attack_effects::BeamEmitter {
        let socket = self.asset.joint_transform(&self.pose, self.beam_origin)
            .expect("validated attachment belongs to this character");
        let facing = glam::Quat::from_rotation_y(player.facing());
        crate::attack_effects::BeamEmitter {
            tip: player.ground_position() + facing * socket.transform_point3(Vec3::ZERO),
            direction: (facing * socket.transform_vector3(Vec3::Y)).normalize(),
        }
    }

    pub(super) fn draw_count(&self) -> usize { self.asset.parts().len() }

    pub(super) fn reset(&mut self) {
        self.sampled_role = PresentationRole::Idle;
        self.sampled_seconds = 0.0;
        self.channel_weight = 0.0;
        self.sampled_at = None;
        self.locomotion = LocomotionSample::idle(&self.clips);
    }

    fn role(player: PlayerPresentation) -> PresentationRole {
        let attack = player.attack();
        if attack.channeling {
            PresentationRole::Attack(AttackProfile::Disintegrate)
        } else if attack.phase != AttackPhase::Idle {
            PresentationRole::Attack(
                attack
                    .swing_profile
                    .expect("a non-idle attack retains its committed profile"),
            )
        } else if player.displacement().length_squared() > 1.0e-8 {
            PresentationRole::Run
        } else {
            PresentationRole::Idle
        }
    }

    pub(super) fn sample(
        &mut self,
        player: PlayerPresentation,
        alpha: Alpha,
        presentation_seconds: f64,
    ) {
        let role = Self::role(player);
        let elapsed = self.sampled_at.map_or(0.0, |last| (presentation_seconds - last).max(0.0) as f32);
        self.sampled_at = Some(presentation_seconds);
        let channel = role == PresentationRole::Attack(AttackProfile::Disintegrate);
        // A time-based half-life keeps the raise/lower consistent at any redraw rate.
        self.channel_weight = arpg_core::damp(self.channel_weight, if channel { 1.0 } else { 0.0 },
            if channel { 0.045 } else { 0.06 }, elapsed);
        if !channel && self.channel_weight < 0.001 { self.channel_weight = 0.0; }
        let clip = self.clips.for_role(role);
        let metadata = self.asset.clip(clip).expect("resolved clip remains live");
        let clip_seconds = match role {
            PresentationRole::Idle | PresentationRole::Run => {
                metadata.loop_time(presentation_seconds)
            }
            // The dedicated channel clip holds an extended arm and horizontal sword.
            PresentationRole::Attack(AttackProfile::Disintegrate) => metadata.duration_seconds() * 0.5,
            PresentationRole::Attack(_) => {
                let attack = player.attack();
                let resolved = attack
                    .swing_resolved
                    .expect("a non-idle attack retains its committed resolution");
                attack_clip_time(
                    attack.phase,
                    attack.elapsed,
                    alpha,
                    resolved,
                    metadata.duration_seconds(),
                )
            }
        };
        let movement = player.displacement();
        if movement.length_squared() > 1.0e-8 {
            let (sin, cos) = player.facing().sin_cos();
            let forward = movement.x * sin + movement.y * cos;
            let right = movement.x * cos - movement.y * sin;
            let longitudinal = if forward >= 0.0 { self.clips.run } else { self.clips.backward };
            let lateral = if right >= 0.0 { self.clips.right } else { self.clips.left };
            let weight = right.abs() / (right.abs() + forward.abs());
            self.locomotion = LocomotionSample { base: longitudinal, overlay: lateral, weight };
            // All movement clips share one normalized phase, including diagonals.
            let gait = self.asset.clip(self.clips.run).expect("resolved clip");
            let phase = gait.loop_time(presentation_seconds) / gait.duration_seconds();
            let time = |id| phase * self.asset.clip(id).expect("resolved clip").duration_seconds();
            // Cardinal movement uses one pose; do not sample and rebuild an
            // unused second hierarchy at the blend endpoints.
            if weight == 0.0 {
                self.asset.sample(longitudinal, time(longitudinal), &mut self.pose);
            } else if weight == 1.0 {
                self.asset.sample(lateral, time(lateral), &mut self.pose);
            } else {
                self.asset.sample(longitudinal, time(longitudinal), &mut self.pose);
                self.asset.sample(lateral, time(lateral), &mut self.scratch);
                self.asset.blend(&mut self.pose, &self.scratch, &self.full_mask, weight);
            }
        } else {
            self.locomotion = LocomotionSample::idle(&self.clips);
            let idle = self.asset.clip(self.clips.idle).expect("resolved clip");
            self.asset.sample(self.clips.idle, idle.loop_time(presentation_seconds), &mut self.pose);
        }
        if matches!(role, PresentationRole::Attack(_)) && !channel {
            // Melee keeps its committed contact timing when replacing a channel.
            self.channel_weight = 0.0;
            self.asset.sample(clip, clip_seconds, &mut self.scratch);
            // Stationary strikes can brace through hips and knees; walking keeps
            // its actual displacement and gait underneath the articulated arms.
            let mask = if movement.length_squared() > 1.0e-8 { &self.upper_mask } else { &self.full_mask };
            self.asset.blend_parent_aligned(&mut self.pose, &self.scratch, mask, 1.0);
        } else if self.channel_weight > 0.0 {
            let channel_clip = self.clips.attack(AttackProfile::Disintegrate);
            let duration = self.asset.clip(channel_clip).expect("resolved channel").duration_seconds();
            self.asset.sample(channel_clip, duration * 0.5, &mut self.scratch);
            self.asset.blend_parent_aligned(&mut self.pose, &self.scratch, &self.upper_mask, self.channel_weight);
        }
        self.sampled_role = role;
        self.sampled_seconds = clip_seconds;
    }
}

impl LoadedCharacter<CharacterMesh> {
    pub(super) fn draw(&self, player: PlayerPresentation) -> CharacterPreview<'_> {
        let instance =
            Instance::new(player.ground_position(), Vec3::ONE, LinearRgb::WHITE).with_yaw(player.facing());
        CharacterPreview::new(&self.gpu, &self.pose, instance)
    }
}

fn attack_clip_time(
    phase: AttackPhase,
    elapsed: u32,
    alpha: Alpha,
    resolved: arpg_sim::ResolvedAttack,
    duration: f32,
) -> f32 {
    // Player attacks share frame 1/13/19/31 segment edges at 30 FPS.
    const WINDUP_END: f32 = 12.0 / 30.0;
    const CONTACT_END: f32 = 18.0 / 30.0;
    let progress = |phase_start: u32, phase_ticks: u32| {
        ((elapsed.saturating_sub(phase_start) as f32 + alpha.get()) / phase_ticks as f32)
            .clamp(0.0, 1.0)
    };
    let normalized = match phase {
        AttackPhase::Startup => progress(0, resolved.startup()) * WINDUP_END,
        AttackPhase::Active => {
            WINDUP_END
                + progress(resolved.startup(), resolved.active()) * (CONTACT_END - WINDUP_END)
        }
        AttackPhase::Recovery => {
            CONTACT_END
                + progress(
                    resolved.startup() + resolved.active(),
                    resolved.recovery().get(),
                ) * (1.0 - CONTACT_END)
        }
        AttackPhase::Idle => 0.0,
    };
    normalized * duration
}

const HORDE_ROLES: [HordeRole; 4] = [HordeRole::Idle, HordeRole::Walk, HordeRole::Run, HordeRole::Sprint];
const CORPSE_BUCKET: usize = MAX_HORDE_POSE_BUCKETS - 1;
const HORDE_PHASES_PER_ROLE: usize = 2;
const RECOIL_START: usize = HORDE_ROLES.len() * HORDE_PHASES_PER_ROLE;
const RECOIL_BUCKETS: usize = crate::enemy_feedback::Recoil::POSE_PHASES.len();
const HORDE_SCALE: f32 = 0.95;
const _: () = assert!(RECOIL_START + RECOIL_BUCKETS == CORPSE_BUCKET);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HordeRole {
    Idle,
    Walk,
    Run,
    Sprint,
}

const REFERENCE_GROUND_SPEEDS: [f32; 3] = [1.5, 2.5, 4.0];

impl HordeRole {
    // Animation buckets use built-in reference speeds. Runtime tuning changes
    // actual displacement, which selects a bucket; it must not redefine what
    // the authored Run clip represents or divide by a user-selected zero speed.
    fn from_displacement(displacement: glam::Vec2) -> Self {
        let speed = displacement.length() / arpg_sim::Dt::SECS;
        if displacement.length_squared() <= 1.0e-8 { Self::Idle }
        else if speed < (REFERENCE_GROUND_SPEEDS[0] + REFERENCE_GROUND_SPEEDS[1]) * 0.5 { Self::Walk }
        else if speed < (REFERENCE_GROUND_SPEEDS[1] + REFERENCE_GROUND_SPEEDS[2]) * 0.5 { Self::Run }
        else { Self::Sprint }
    }

    // Reuse the authored Run clip with bounded cadences. Selection follows real
    // displacement, so a blocked sprinter does not run in place at full cadence.
    fn playback_rate(self) -> f64 {
        match self {
            Self::Idle | Self::Run => 1.0,
            Self::Walk => f64::from(REFERENCE_GROUND_SPEEDS[0] / REFERENCE_GROUND_SPEEDS[1]),
            Self::Sprint => f64::from(REFERENCE_GROUND_SPEEDS[2] / REFERENCE_GROUND_SPEEDS[1]),
        }
    }
}

#[derive(Clone, Copy)]
struct HordeClips {
    hit: arpg_assets::ClipId,
    death: arpg_assets::ClipId,
    idle: arpg_assets::ClipId,
    run: arpg_assets::ClipId,
}

impl HordeClips {
    fn resolve(asset: &arpg_assets::CharacterAsset) -> Result<Self, String> {
        let clip = |name| {
            asset
                .clip_named(name)
                .ok_or_else(|| format!("missing horde presentation clip {name}"))
        };
        Ok(Self {
            idle: clip("Idle")?,
            run: clip("Run")?,
            hit: clip("Hit")?,
            death: clip("Death")?,
        })
    }

    fn for_role(self, role: HordeRole) -> arpg_assets::ClipId {
        match role {
            HordeRole::Idle => self.idle,
            HordeRole::Walk | HordeRole::Run | HordeRole::Sprint => self.run,
        }
    }
}

struct HordeBucket {
    pose: arpg_assets::CharacterPose,
    instances: Vec<Instance>,
}

#[derive(Clone, Copy)]
struct RememberedHeading {
    yaw: f32,
    seen_in_rebuild: u64,
}

/// One mesh and a bounded set of shared poses for every ordinary enemy.
pub(super) struct LoadedHorde<T> {
    path: std::path::PathBuf,
    asset: arpg_assets::CharacterAsset,
    clips: HordeClips,
    buckets: [HordeBucket; MAX_HORDE_POSE_BUCKETS],
    corpse_min_height: f32,
    corpse_front: f32,
    headings: std::collections::HashMap<arpg_sim::EntityId, RememberedHeading>,
    rebuild_id: u64,
    gpu: T,
}

impl<T> LoadedHorde<T> {
    pub(super) fn summary(&self) -> String {
        format!(
            "horde character vertices={} indices={} joints={} pose_buckets={} idle=Idle run=Run path={}",
            self.asset.vertex_count(),
            self.asset.index_count(),
            self.asset.joint_count(),
            MAX_HORDE_POSE_BUCKETS,
            self.path.display()
        )
    }

    pub(super) fn instance_count(&self) -> usize {
        self.buckets
            .iter()
            .map(|bucket| bucket.instances.len())
            .sum()
    }

    pub(super) fn occupied_buckets(&self) -> usize {
        self.buckets
            .iter()
            .filter(|bucket| !bucket.instances.is_empty())
            .count()
    }

    fn new(
        path: std::path::PathBuf,
        asset: arpg_assets::CharacterAsset,
        clips: HordeClips,
        gpu: T,
    ) -> Self {
        let mut buckets: [HordeBucket; MAX_HORDE_POSE_BUCKETS] = std::array::from_fn(|_| HordeBucket {
            pose: asset.bind_pose(),
            instances: Vec::new(),
        });
        // This pose stays frozen for the lifetime of the loaded asset.
        let death_duration = asset.clip(clips.death).expect("resolved death").duration_seconds();
        let sampled = asset.sample(clips.death, death_duration, &mut buckets[CORPSE_BUCKET].pose);
        // The larger corpse needs its frozen skin's support extent. Compute it
        // once, rather than clipping its head into the floor or skinning each
        // corpse again on the CPU every frame.
        let palette = buckets[CORPSE_BUCKET].pose.joint_matrices();
        let mut corpse_min_height = f32::INFINITY;
        let mut corpse_front = f32::NEG_INFINITY;
        for vertex in asset.vertices() {
            let point = vertex.joints().into_iter().zip(vertex.weights())
                .fold(Vec3::ZERO, |sum, (joint, weight)| sum
                    + palette[usize::from(joint)].matrix().transform_point3(vertex.position()) * weight);
            corpse_min_height = corpse_min_height.min(point.y * HORDE_SCALE);
            corpse_front = corpse_front.max(point.z * HORDE_SCALE);
        }
        // Three fixed stages articulate every struck enemy without sampling
        // a separate skeleton per body or increasing cost with hit age variety.
        let hit_duration = asset.clip(clips.hit).expect("resolved hit").duration_seconds();
        for (index, phase) in crate::enemy_feedback::Recoil::POSE_PHASES.into_iter().enumerate() {
            asset.sample(clips.hit, phase * hit_duration, &mut buckets[RECOIL_START + index].pose);
        }
        debug_assert!(sampled, "resolved death clip remains live");
        Self {
            path,
            asset,
            clips,
            buckets,
            corpse_min_height,
            corpse_front,
            headings: std::collections::HashMap::new(),
            rebuild_id: 0,
            gpu,
        }
    }

    pub(super) fn draw_count(&self) -> usize {
        self.occupied_buckets() * self.asset.parts().len()
    }

    fn phase_key(id: arpg_sim::EntityId) -> u64 {
        let mut hash = Fnv::default();
        id.hash_into(&mut hash);
        hash.finish()
    }

    fn bucket(enemy: CharacterPresentation) -> (usize, u64) {
        let role = HordeRole::from_displacement(enemy.displacement());
        let key = Self::phase_key(enemy.id());
        let phase = (key % HORDE_PHASES_PER_ROLE as u64) as usize;
        let role_offset = HORDE_ROLES.iter().position(|&value| value == role)
            .expect("all roles have buckets") * HORDE_PHASES_PER_ROLE;
        (role_offset + phase, key)
    }

    pub(super) fn rebuild(
        &mut self,
        enemies: impl Iterator<Item = CharacterPresentation>,
        presentation_seconds: f64,
        flash: impl Fn(arpg_sim::EntityId) -> f32,
        recoil: impl Fn(arpg_sim::EntityId) -> Option<crate::enemy_feedback::Recoil>,
        attacks: impl Iterator<Item = arpg_game::ActionStatus>,
        alpha: Alpha,
    ) {
        for bucket in &mut self.buckets {
            bucket.instances.clear();
        }

        let mut committed = crate::attack_effects::visible_actions(attacks);
        let attacks: [Option<arpg_game::ActionStatus>; crate::attack_effects::MAX_VISIBLE_ACTIONS] = std::array::from_fn(|_| committed.next());
        self.rebuild_id = self.rebuild_id.wrapping_add(1);
        for enemy in enemies {
            let (mut bucket, key) = Self::bucket(enemy);
            let displacement = enemy.displacement();
            let heading = self
                .headings
                .entry(enemy.id())
                .or_insert(RememberedHeading {
                    yaw: 0.0,
                    seen_in_rebuild: self.rebuild_id,
                });
            if displacement.length_squared() > 1.0e-8 {
                heading.yaw = displacement.x.atan2(displacement.y);
            }
            heading.seen_in_rebuild = self.rebuild_id;
            let mut pitch = 0.0;
            let mut height = 0.0;
            if let Some(attack) = attacks.iter().flatten().find(|a| a.body == enemy.id()) {
                heading.yaw = attack.direction.x.atan2(attack.direction.y);
                let progress = ((attack.elapsed as f32 + alpha.get()) / attack.duration as f32).clamp(0.0, 1.0);
                if attack.kind == arpg_game::ActionKind::Leap {
                    match attack.phase {
                        AttackPhase::Startup => pitch = -0.4 * progress,
                        AttackPhase::Active => {
                            height = 4.0 * 1.8 * progress * (1.0 - progress);
                            pitch = -0.4 + 0.9 * progress;
                        }
                        AttackPhase::Recovery => pitch = 0.5 * (1.0 - progress),
                        AttackPhase::Idle => {}
                    }
                } else {
                    pitch = match attack.phase {
                        AttackPhase::Startup => -0.25 * progress,
                        AttackPhase::Active => -0.25 + 0.8 * (progress * 3.0).min(1.0),
                        AttackPhase::Recovery => 0.55 * (1.0 - progress),
                        AttackPhase::Idle => 0.0,
                    };
                }
            }
            let mut yaw = heading.yaw;
            if let Some(recoil) = recoil(enemy.id()) {
                bucket = RECOIL_START + recoil.stage();
                yaw = recoil.yaw();
                pitch += -0.12 * recoil.strength();
            }
            let shade = 0.72 + ((key >> 8) & 0xff) as f32 / 255.0 * 0.28;
            self.buckets[bucket].instances.push(
                Instance::new(
                    enemy.ground_position() + Vec3::Y * height,
                    Vec3::splat(HORDE_SCALE),
                    LinearRgb::splat(shade),
                )
                .with_yaw(yaw)
                .with_white_mix(flash(enemy.id()))
                .with_pitch(pitch),
            );
        }
        self.headings
            .retain(|_, heading| heading.seen_in_rebuild == self.rebuild_id);

        for (index, bucket) in self.buckets[..RECOIL_START].iter_mut().enumerate() {
            if bucket.instances.is_empty() {
                continue;
            }
            let role = HORDE_ROLES[index / HORDE_PHASES_PER_ROLE];
            let clip = self.clips.for_role(role);
            let metadata = self.asset.clip(clip).expect("resolved clip remains live");
            let phase = index % HORDE_PHASES_PER_ROLE;
            let offset = f64::from(metadata.duration_seconds()) * phase as f64
                / HORDE_PHASES_PER_ROLE as f64;
            let time = metadata.loop_time(presentation_seconds * role.playback_rate() + offset);
            let sampled = self.asset.sample(clip, time, &mut bucket.pose);
            debug_assert!(sampled, "resolved clip remains live");
        }
    }

    /// One bent-limb death pose supports tick-derived falls and settled remains.
    pub(super) fn rebuild_corpses(&mut self, corpses: &[arpg_game::Corpse], alpha: Alpha) {
        let bucket = &mut self.buckets[CORPSE_BUCKET];
        bucket.instances.clear();
        for corpse in corpses {
            let key = Self::phase_key(corpse.id());
            let yaw = (key & 0xffff) as f32 / 65535.0 * std::f32::consts::TAU;
            let position = corpse.position();
            let age = (arpg_game::CORPSE_LIFETIME_TICKS - corpse.remaining_ticks()) as f32 + alpha.get();
            let fall = (age / 15.0).clamp(0.0, 1.0);
            let fall = fall * fall * (3.0 - 2.0 * fall);
            let pitch = std::f32::consts::FRAC_PI_2 * fall;
            let height = (self.corpse_front * pitch.sin() - self.corpse_min_height * pitch.cos()).max(0.0);
            bucket.instances.push(Instance::new(
                Vec3::new(position.x, height, position.y),
                Vec3::splat(HORDE_SCALE), LinearRgb::splat(0.6),
            ).with_yaw(yaw).with_pitch(pitch));
        }
    }

    pub(super) fn clear(&mut self) {
        for bucket in &mut self.buckets {
            bucket.instances.clear();
        }
        self.headings.clear();
    }
}

impl LoadedHorde<CharacterMesh> {
    pub(super) fn draw(&self) -> CharacterHorde<'_> {
        let buckets = std::array::from_fn(|index| {
            let bucket = &self.buckets[index];
            CharacterBucket::new(&bucket.pose, &bucket.instances)
        });
        CharacterHorde::new(&self.gpu, buckets)
    }
}

pub(super) fn presentation_seconds(tick: u64, alpha: Alpha) -> f64 {
    (tick as f64 + f64::from(alpha.get())) / f64::from(TICK_HZ)
}

pub(super) fn report_character_preview<T>(preview: Option<&LoadedCharacter<T>>, out: &mut Report) {
    report_character_asset(
        preview.map(|preview| (preview.path.as_path(), &preview.asset)),
        out,
    );
    if let Some(preview) = preview {
        let clip = preview.clips.for_role(preview.sampled_role);
        let clip = preview
            .asset
            .clip(clip)
            .expect("resolved clip remains live");
        out.int("clips", preview.asset.clip_count() as u64);
        out.text("role", preview.sampled_role.label());
        // During locomotion this is the gait clock; the actual blend is reported below.
        out.text("clip", clip.name());
        out.num("clip_duration", clip.duration_seconds());
        out.int("channels", clip.channel_count() as u64);
        out.num("sample_seconds", preview.sampled_seconds);
        out.text("locomotion_base", preview.asset.clip(preview.locomotion.base).expect("resolved clip").name());
        out.text("locomotion_overlay", preview.asset.clip(preview.locomotion.overlay).expect("resolved clip").name());
        out.num("locomotion_weight", preview.locomotion.weight);

        out.num("channel_weight", preview.channel_weight);
    } else {
        out.int("clips", 0);
        out.text("role", "");
        out.text("clip", "");
        out.num("clip_duration", 0.0);
        out.int("channels", 0);
        out.num("sample_seconds", 0.0);
        out.text("locomotion_base", "");
        out.text("locomotion_overlay", "");
        out.num("locomotion_weight", 0.0);
        out.num("channel_weight", 0.0);
    }
}

pub(super) fn report_horde_preview<T>(preview: Option<&LoadedHorde<T>>, out: &mut Report) {
    report_character_asset(
        preview.map(|preview| (preview.path.as_path(), &preview.asset)),
        out,
    );
    out.object("shadow_mesh", |out| {
        let proxy = preview.and_then(|horde| horde.asset.shadow_mesh());
        out.bool("coarse", proxy.is_some());
        out.text("path", &preview.filter(|_| proxy.is_some()).map_or_else(String::new,
            |horde| horde.path.with_extension("shadow.glb").to_string_lossy().into_owned()));
        out.int("vertices", preview.map_or(0, |horde| proxy.map_or(horde.asset.vertex_count(), |p| p.vertices().len())) as u64);
        out.int("indices", preview.map_or(0, |horde| proxy.map_or(horde.asset.index_count(), |p| p.indices().len())) as u64);
    });
    if let Some(preview) = preview {
        out.text(
            "idle_clip",
            preview
                .asset
                .clip(preview.clips.idle)
                .expect("resolved clip remains live")
                .name(),
        );
        out.text(
            "run_clip",
            preview
                .asset
                .clip(preview.clips.run)
                .expect("resolved clip remains live")
                .name(),
        );
        out.int("pose_buckets", MAX_HORDE_POSE_BUCKETS as u64);
        out.int("occupied_buckets", preview.occupied_buckets() as u64);
        out.int("instances", preview.instance_count() as u64);
        out.int("corpse_instances", preview.buckets[CORPSE_BUCKET].instances.len() as u64);
        out.num("corpse_settled_height", preview.corpse_front);
        out.int("recoiling_instances", preview.buckets[RECOIL_START..CORPSE_BUCKET].iter()
            .map(|bucket| bucket.instances.len() as u64).sum());
        out.int("flashing_instances", preview.buckets.iter()
            .flat_map(|bucket| &bucket.instances)
            .filter(|instance| instance.white_mix() > 0.0).count() as u64);
        out.int(
            "idle_instances",
            preview.buckets[..HORDE_PHASES_PER_ROLE]
                .iter()
                .map(|bucket| bucket.instances.len() as u64)
                .sum(),
        );
        out.int(
            "run_instances",
            preview.buckets[HORDE_PHASES_PER_ROLE..RECOIL_START]
                .iter()
                .map(|bucket| bucket.instances.len() as u64)
                .sum(),
        );
    } else {
        out.text("idle_clip", "");
        out.text("run_clip", "");
        out.int("pose_buckets", 0);
        out.int("occupied_buckets", 0);
        out.int("instances", 0);
        out.int("corpse_instances", 0);
        out.num("corpse_settled_height", 0.0);
        out.int("recoiling_instances", 0);
        out.int("flashing_instances", 0);
        out.int("idle_instances", 0);
        out.int("run_instances", 0);
    }
}

fn report_character_asset(
    selected: Option<(&std::path::Path, &arpg_assets::CharacterAsset)>,
    out: &mut Report,
) {
    let (path, asset) = selected.unzip();
    out.bool("active", asset.is_some());
    out.text(
        "path",
        &path.map_or_else(String::new, |path| path.to_string_lossy().into_owned()),
    );
    out.int(
        "vertices",
        asset.map_or(0, |asset| asset.vertex_count() as u64),
    );
    out.int(
        "indices",
        asset.map_or(0, |asset| asset.index_count() as u64),
    );
    out.int("part_count", asset.map_or(0, |a| a.parts().len() as u64));
    out.int("material_count", asset.map_or(0, |a| a.materials().len() as u64));
    out.int("image_count", asset.map_or(0, |a| a.images().len() as u64));
    out.int("texture_bytes", asset.map_or(0, |a| a.texture_bytes() as u64));
    out.object("parts", |out| {
        if let Some(asset) = asset {
            for (index, part) in asset.parts().iter().enumerate() {
                out.object(&index.to_string(), |out| {
                    out.text("node", part.node_name());
                    out.int("primitive", part.primitive_index() as u64);
                    out.int("material", part.material_index() as u64);
                    out.int("vertices", part.vertices().len() as u64);
                    out.int("indices", u64::from(part.indices().end - part.indices().start));
                });
            }
        }
    });
    out.object("materials", |out| {
        if let Some(asset) = asset {
            for (index, material) in asset.materials().iter().enumerate() {
                out.object(&index.to_string(), |out| out.int("image", material.image_index() as u64));
            }
        }
    });
    out.object("images", |out| {
        if let Some(asset) = asset {
            for (index, image) in asset.images().iter().enumerate() {
                out.object(&index.to_string(), |out| {
                    out.int("width", u64::from(image.width()));
                    out.int("height", u64::from(image.height()));
                });
            }
        }
    });
    out.int("nodes", asset.map_or(0, |asset| asset.node_count() as u64));
    out.int(
        "joints",
        asset.map_or(0, |asset| asset.joint_count() as u64),
    );
}

fn read_glb(path: &std::path::Path) -> Result<Vec<u8>, String> {
    let file = std::fs::File::open(path)
        .map_err(|error| format!("{}: open GLB: {error}", path.display()))?;
    read_glb_file(path, file)
}

fn read_glb_file(path: &std::path::Path, file: std::fs::File) -> Result<Vec<u8>, String> {
    use std::io::Read as _;
    if path.extension() != Some(std::ffi::OsStr::new("glb")) {
        return Err(format!("{}: expected a .glb file", path.display()));
    }
    let metadata = file.metadata()
        .map_err(|error| format!("{}: read metadata: {error}", path.display()))?;
    let bytes = usize::try_from(metadata.len())
        .map_err(|_| format!("{}: file size cannot fit this process", path.display()))?;
    if bytes > arpg_assets::MAX_GLB_BYTES {
        return Err(format!(
            "{}: GLB exceeds asset capacity: file bytes",
            path.display()
        ));
    }
    let mut source = Vec::with_capacity(bytes);
    file.take(arpg_assets::MAX_GLB_BYTES as u64 + 1).read_to_end(&mut source)
        .map_err(|error| format!("{}: read GLB: {error}", path.display()))?;
    // The opened file may grow after metadata was read. Bound the read as well
    // as admission, so external writes cannot create an unbounded allocation.
    if source.len() > arpg_assets::MAX_GLB_BYTES {
        return Err(format!("{}: GLB exceeds asset capacity: file bytes", path.display()));
    }
    Ok(source)
}

fn load_static<T>(
    path: std::path::PathBuf,
    upload: impl FnOnce(&arpg_assets::StaticMesh) -> Result<T, String>,
) -> Result<LoadedMesh<T>, String> {
    let source = read_glb(&path)?;
    let mesh =
        arpg_assets::import_glb(&source).map_err(|error| format!("{}: {error}", path.display()))?;
    let vertex_count = mesh.vertex_count();
    let index_count = mesh.index_count();
    let texture_width = mesh.base_color_texture().width();
    let texture_height = mesh.base_color_texture().height();
    let gpu = upload(&mesh).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(LoadedMesh {
        path,
        vertex_count,
        index_count,
        texture_width,
        texture_height,
        gpu,
    })
}

/// The checked-in playable asset is resolved independently of the launch directory.
pub(super) fn default_character_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/characters/basic-player/basic-player.glb")
}

/// The checked-in enemy asset is resolved independently of the launch directory.
pub(super) fn default_horde_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/characters/basic-enemy/basic-enemy.glb")
}

/// Character roles stay here, outside the generic asset importer.
fn player_upper_mask(asset: &arpg_assets::CharacterAsset) -> Result<arpg_assets::PoseMask, String> {
    asset.pose_mask_parent_aligned(&[("Spine", 1.0), ("Chest", 1.0), ("Neck", 1.0), ("Head", 1.0),
        ("LeftShoulder", 1.0), ("RightShoulder", 1.0), ("LeftArm", 1.0), ("RightArm", 1.0),
        ("LeftForearm", 1.0), ("RightForearm", 1.0), ("LeftHand", 1.0), ("RightHand", 1.0),
        ("LeftFingers", 1.0), ("RightFingers", 1.0), ("LeftThumb", 1.0), ("RightThumb", 1.0), ("Weapon", 1.0)])
        .map_err(str::to_owned)
}

pub(super) fn replace_character<T>(
    slot: &mut Option<LoadedCharacter<T>>,
    path: std::path::PathBuf,
    upload: impl FnOnce(&arpg_assets::CharacterAsset) -> Result<T, String>,
) -> Result<(), String> {
    let source = read_glb(&path)?;
    let asset = arpg_assets::import_character_glb(&source)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let clips =
        CharacterClips::resolve(&asset).map_err(|error| format!("{}: {error}", path.display()))?;
    let beam_origin = asset.joint_named("BeamOrigin").ok_or("missing player sword-tip attachment BeamOrigin")?;
    let upper_mask = player_upper_mask(&asset)?;
    let full_mask = asset.full_mask();
    let scratch = asset.bind_pose();
    let gpu = upload(&asset).map_err(|error| format!("{}: {error}", path.display()))?;
    let pose = asset.bind_pose();
    let locomotion = LocomotionSample::idle(&clips);
    *slot = Some(LoadedCharacter {
        path,
        asset,
        pose,
        clips,
        locomotion,
        scratch,
        full_mask,
        upper_mask,
        beam_origin,
        sampled_role: PresentationRole::Idle,
        sampled_seconds: 0.0,
        channel_weight: 0.0,
        sampled_at: None,
        gpu,
    });
    Ok(())
}

/// Prepares horde clips, poses and GPU resources before replacing `slot`.
pub(super) fn replace_horde<T>(
    slot: &mut Option<LoadedHorde<T>>,
    path: std::path::PathBuf,
    upload: impl FnOnce(&arpg_assets::CharacterAsset) -> Result<T, String>,
) -> Result<(), String> {
    let source = read_glb(&path)?;
    let asset = arpg_assets::import_character_glb(&source)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let clips =
        HordeClips::resolve(&asset).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut asset = asset;
    let shadow_path = path.with_extension("shadow.glb");
    match std::fs::File::open(&shadow_path) {
        Ok(file) => {
            let proxy = arpg_assets::import_character_glb(&read_glb_file(&shadow_path, file)?)
                .map_err(|error| format!("{}: {error}", shadow_path.display()))?;
            asset.attach_shadow(proxy).map_err(|error| format!("{}: {error}", shadow_path.display()))?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("{}: {error}", shadow_path.display())),
    }
    let gpu = upload(&asset).map_err(|error| format!("{}: {error}", path.display()))?;
    *slot = Some(LoadedHorde::new(path, asset, clips, gpu));
    Ok(())
}

#[cfg(test)]
mod tests;
