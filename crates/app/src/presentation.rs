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
    doors: Vec<Instance>,
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
            doors: Vec::with_capacity(arpg_sim::MAX_JOINTS),
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

    pub(super) fn rebuild_doors(&mut self, doors: impl Iterator<Item = arpg_game::DoorPose>) {
        self.doors.clear();
        for door in doors {
            assert!(self.doors.len() < arpg_sim::MAX_JOINTS, "physical joint admission bound");
            let (centre, size) = (door.collider.centre(), door.collider.size());
            let colour = match door.state {
                arpg_game::DoorState::Latched => LinearRgb::new(0.35, 0.12, 0.035),
                arpg_game::DoorState::Released => LinearRgb::new(0.25, 0.22, 0.09),
                arpg_game::DoorState::Broken | arpg_game::DoorState::Missing => LinearRgb::new(0.55, 0.22, 0.055),
            };
            self.doors.push(Instance::new(Vec3::new(centre.x, 0.0, centre.y),
                Vec3::new(size.x, 1.0, size.y), colour).with_yaw(door.collider.yaw()));
        }
    }

    pub(super) fn rebuild_trees(&mut self, trees: impl Iterator<Item = arpg_game::TreePose>) {
        for (_, _, instances) in &mut self.trees { instances.clear(); }
        for (count, pose) in trees.enumerate() {
            assert!(count < arpg_game::MAX_TREES, "game tree admission bound");
            let tree = pose.spec;
            let (_, _, instances) = self.trees.iter_mut().find(|(kind, _, _)| *kind == tree.kind()).expect("complete tree catalog");
            let pos = tree.position();
            // Meshes are grounded at their trunk base, which is the fall pivot.
            instances.push(Instance::new(Vec3::new(pos.x, 0.0, pos.y), Vec3::splat(tree.scale()), LinearRgb::WHITE)
                .with_yaw(pose.yaw).with_pitch(pose.pitch));
        }
    }

    pub(super) fn instance_count(&self) -> usize { 1 + self.trees.iter().map(|(_, _, instances)| instances.len()).sum::<usize>() + self.walls.len() + self.doors.len() + self.torches.stand().len() + self.props.as_slice().len() + self.closed_chests.len() + self.open_chests.len() }

    pub(super) fn draw_count(&self) -> usize { 1 + self.trees.iter().filter(|(_, _, instances)| !instances.is_empty()).count() + usize::from(!self.walls.is_empty()) + usize::from(!self.doors.is_empty()) + usize::from(!self.torches.stand().is_empty()) + usize::from(!self.props.as_slice().is_empty()) + usize::from(!self.closed_chests.is_empty()) + usize::from(!self.open_chests.is_empty()) }

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
        out.int("door_instances", self.doors.len() as u64);
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
            self.wall.draw(&self.doors),
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
    weapon: arpg_assets::JointId,
    weapon_butt: Vec3,
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

    pub(super) fn slam_origin(&mut self, player: PlayerPresentation, seconds: f64) -> Vec3 {
        // Sample the completed contact even when several ticks pass without a
        // redraw. Reusing the normal sampler includes moving-attack layering.
        self.sample(player, Alpha::ONE, seconds);
        let socket = self.asset.joint_transform(&self.pose, self.weapon)
            .expect("validated weapon belongs to this character");
        let point = player.ground_position()
            + glam::Quat::from_rotation_y(player.facing()) * socket.transform_point3(self.weapon_butt);
        Vec3::new(point.x, 0.0, point.z)
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
const DEATH_PHASES: usize = 4;
const CORPSE_BUCKET: usize = MAX_HORDE_POSE_BUCKETS - 1;
const DEATH_START: usize = CORPSE_BUCKET - DEATH_PHASES;
const DEATH_BLEND_SECONDS: f32 = 0.15;
const HORDE_PHASES_PER_ROLE: usize = 2;
const ATTACK_START: usize = HORDE_ROLES.len() * HORDE_PHASES_PER_ROLE;
const ATTACK_BUCKETS: usize = arpg_game::MAX_COMBAT_COMMITMENTS;
const RECOIL_START: usize = ATTACK_START + ATTACK_BUCKETS;
const RECOIL_BUCKETS: usize = crate::enemy_feedback::Recoil::POSE_PHASES.len();
const HORDE_SCALE: f32 = 1.0;
const _: () = assert!(RECOIL_START + RECOIL_BUCKETS == DEATH_START);

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

    // Selection follows real displacement, so a blocked sprinter does not
    // gallop in place. Walk has its own four-foot support cycle.
    fn playback_rate(self) -> f64 {
        match self {
            Self::Idle | Self::Walk | Self::Run => 1.0,
            Self::Sprint => f64::from(REFERENCE_GROUND_SPEEDS[2] / REFERENCE_GROUND_SPEEDS[1]),
        }
    }
}

#[derive(Clone, Copy)]
struct HordeClips {
    hit: arpg_assets::ClipId,
    death: arpg_assets::ClipId,
    idle: arpg_assets::ClipId,
    walk: arpg_assets::ClipId,
    run: arpg_assets::ClipId,
    bite: arpg_assets::ClipId,
    leap: arpg_assets::ClipId,
}

impl HordeClips {
    fn resolve(asset: &arpg_assets::CharacterAsset) -> Result<Self, String> {
        let clip = |name| {
            asset
                .clip_named(name)
                .ok_or_else(|| format!("missing horde presentation clip {name}"))
        };
        let death = clip("Death")?;
        if asset.clip(death).expect("resolved death").duration_seconds() <= 0.0 {
            return Err("Death must contain a nonzero collapse duration".into());
        }
        Ok(Self {
            idle: clip("Idle")?,
            walk: clip("Walk")?,
            run: clip("Run")?,
            bite: clip("Bite")?,
            leap: clip("Leap")?,
            hit: clip("Hit")?,
            death,
        })
    }

    fn for_role(self, role: HordeRole) -> arpg_assets::ClipId {
        match role {
            HordeRole::Idle => self.idle,
            HordeRole::Walk => self.walk,
            HordeRole::Run | HordeRole::Sprint => self.run,
        }
    }
}

/// Game bounds simultaneous commitments, so each attack can sample its exact
/// phase while the much larger locomotion crowd continues to share gait poses.
fn horde_attack_time(attack: arpg_game::ActionStatus, alpha: Alpha) -> Option<f32> {
    if !matches!(attack.kind, arpg_game::ActionKind::Swipe | arpg_game::ActionKind::Leap) { return None; }
    let (start, span) = match attack.phase {
        AttackPhase::Startup => (0.0, 0.4),
        AttackPhase::Active => (0.4, 0.2),
        AttackPhase::Recovery => (0.6, 0.4),
        AttackPhase::Idle => return None,
    };
    let progress = ((attack.elapsed as f32 + alpha.get()) / attack.duration as f32).clamp(0.0, 1.0);
    Some(start + span * progress)
}

struct HordeBucket {
    pose: arpg_assets::CharacterPose,
    instances: Vec<Instance>,
}

#[derive(Clone, Copy)]
struct RememberedHeading {
    yaw: f32,
    drawn_yaw: f32,
    seen_in_rebuild: u64,
    bucket: usize,
    height: f32,
}

fn pose_support(asset: &arpg_assets::CharacterAsset, pose: &arpg_assets::CharacterPose) -> f32 {
    let palette = pose.joint_matrices();
    asset.vertices().iter().map(|vertex| vertex.joints().into_iter().zip(vertex.weights())
        .fold(Vec3::ZERO, |sum, (joint, weight)| sum
            + palette[usize::from(joint)].matrix().transform_point3(vertex.position()) * weight).y * HORDE_SCALE)
        .fold(f32::INFINITY, f32::min)
}

pub(super) struct HordeFrame<'a> {
    pub(super) alpha: Alpha,
    pub(super) corpses: &'a [arpg_game::Corpse],
}

struct DeathVisual {
    yaw: f32,
    height: f32,
    pose: arpg_assets::CharacterPose,
    seen_in_rebuild: u64,
}

fn corpse_age(corpse: &arpg_game::Corpse, alpha: Alpha) -> f32 {
    ((arpg_game::CORPSE_LIFETIME_TICKS - corpse.remaining_ticks()) as f32 + alpha.get())
        / arpg_sim::TICK_HZ as f32
}

fn collapse_phase(age: f32, duration: f32) -> usize {
    ((age / duration * DEATH_PHASES as f32) as usize).min(DEATH_PHASES - 1)
}

/// One mesh and a bounded set of shared poses for every ordinary enemy.
pub(super) struct LoadedHorde<T> {
    path: std::path::PathBuf,
    asset: arpg_assets::CharacterAsset,
    clips: HordeClips,
    buckets: [HordeBucket; MAX_HORDE_POSE_BUCKETS],
    corpse_min_height: f32,
    headings: std::collections::HashMap<arpg_sim::EntityId, RememberedHeading>,
    rebuild_id: u64,
    deaths: std::collections::HashMap<arpg_sim::EntityId, DeathVisual>,
    full_mask: arpg_assets::PoseMask,
    gpu: T,
}

impl<T> LoadedHorde<T> {
    pub(super) fn summary(&self) -> String {
        format!(
            "horde character vertices={} indices={} joints={} pose_buckets={} idle=Idle walk=Walk run=Run bite=Bite leap=Leap path={}",
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
        let corpse_min_height = pose_support(&asset, &buckets[CORPSE_BUCKET].pose);
        // Three fixed stages articulate every struck enemy without sampling
        // a separate skeleton per body or increasing cost with hit age variety.
        let hit_duration = asset.clip(clips.hit).expect("resolved hit").duration_seconds();
        for (index, phase) in crate::enemy_feedback::Recoil::POSE_PHASES.into_iter().enumerate() {
            asset.sample(clips.hit, phase * hit_duration, &mut buckets[RECOIL_START + index].pose);
        }
        debug_assert!(sampled, "resolved death clip remains live");
        let full_mask = asset.full_mask();
        Self {
            path,
            asset,
            clips,
            buckets,
            corpse_min_height,
            headings: std::collections::HashMap::new(),
            rebuild_id: 0,
            deaths: std::collections::HashMap::new(),
            full_mask,
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
        frame: HordeFrame<'_>,
    ) {
        let HordeFrame { alpha, corpses } = frame;
        self.rebuild_id = self.rebuild_id.wrapping_add(1);
        self.rebuild_corpses(corpses, alpha);
        for bucket in &mut self.buckets[..DEATH_START] {
            bucket.instances.clear();
        }

        let mut committed = attacks.filter_map(|attack|
            horde_attack_time(attack, alpha).map(|time| (attack, time)));
        let attacks: [Option<_>; ATTACK_BUCKETS] = std::array::from_fn(|_| committed.next());
        debug_assert!(committed.next().is_none(), "Game owns combat commitment capacity");
        for enemy in enemies {
            let (mut bucket, key) = Self::bucket(enemy);
            let displacement = enemy.displacement();
            let heading = self
                .headings
                .entry(enemy.id())
                .or_insert(RememberedHeading {
                    yaw: 0.0,
                    drawn_yaw: 0.0,
                    seen_in_rebuild: self.rebuild_id,
                    bucket, height: 0.0,
                });
            if displacement.length_squared() > 1.0e-8 {
                heading.yaw = displacement.x.atan2(displacement.y);
            }
            heading.seen_in_rebuild = self.rebuild_id;
            let mut pitch = 0.0;
            let mut height = 0.0;
            if let Some((index, (attack, _))) = attacks.iter().enumerate()
                .filter_map(|(index, action)| action.as_ref().map(|action| (index, action)))
                .find(|(_, (attack, _))| attack.body == enemy.id()) {
                heading.yaw = attack.direction.x.atan2(attack.direction.y);
                bucket = ATTACK_START + index;
                let progress = ((attack.elapsed as f32 + alpha.get()) / attack.duration as f32).clamp(0.0, 1.0);
                if attack.kind == arpg_game::ActionKind::Leap && attack.phase == AttackPhase::Active {
                    height = 4.0 * 1.8 * progress * (1.0 - progress);
                }
            }
            let mut yaw = heading.yaw;
            if let Some(recoil) = recoil(enemy.id()) {
                bucket = RECOIL_START + recoil.stage();
                yaw = recoil.yaw();
                pitch += -0.12 * recoil.strength();
            }
            heading.bucket = bucket;
            heading.height = height;
            heading.drawn_yaw = yaw;
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

        for (index, bucket) in self.buckets[..ATTACK_START].iter_mut().enumerate() {
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
        for (index, action) in attacks.into_iter().enumerate() {
            let Some((attack, normalized)) = action else { continue; };
            let bucket = &mut self.buckets[ATTACK_START + index];
            if bucket.instances.is_empty() { continue; }
            let clip = if attack.kind == arpg_game::ActionKind::Leap { self.clips.leap } else { self.clips.bite };
            let duration = self.asset.clip(clip).expect("resolved attack").duration_seconds();
            let sampled = self.asset.sample(clip, normalized * duration, &mut bucket.pose);
            debug_assert!(sampled, "resolved attack remains live");
        }
    }

    /// Capture the visible handoff before live buckets are rebuilt. Moving and
    /// stationary corpses share the same death clock; velocity never selects a gait.
    fn rebuild_corpses(&mut self, corpses: &[arpg_game::Corpse], alpha: Alpha) {
        for corpse in corpses {
            let death = self.deaths.entry(corpse.id()).or_insert_with(|| {
                if let Some(heading) = self.headings.get(&corpse.id()) {
                    DeathVisual { yaw: heading.drawn_yaw, height: heading.height,
                        pose: self.buckets[heading.bucket].pose.clone(), seen_in_rebuild: self.rebuild_id }
                } else {
                    let mut pose = self.asset.bind_pose();
                    self.asset.sample(self.clips.death, 0.0, &mut pose);
                    let travel = corpse.position() - corpse.drawn_position(Alpha::ZERO);
                    DeathVisual { yaw: travel.x.atan2(travel.y), height: 0.0, pose,
                        seen_in_rebuild: self.rebuild_id }
                }
            });
            death.seen_in_rebuild = self.rebuild_id;
        }
        self.deaths.retain(|_, death| death.seen_in_rebuild == self.rebuild_id);
        for bucket in &mut self.buckets[DEATH_START..] { bucket.instances.clear(); }
        let duration = self.asset.clip(self.clips.death).expect("resolved death").duration_seconds();
        // A representative age per phase keeps lone deaths smooth and many
        // different death times bounded by four shared animated palettes.
        let mut samples: [Option<(f32, arpg_sim::EntityId)>; DEATH_PHASES] = [None; DEATH_PHASES];
        for corpse in corpses {
            let age = corpse_age(corpse, alpha);
            if age < duration {
                let phase = collapse_phase(age, duration);
                samples[phase].get_or_insert((age, corpse.id()));
            }
        }
        let mut supports = [0.0; DEATH_PHASES];
        for (phase, sample) in samples.into_iter().enumerate() {
            let Some((age, id)) = sample else { continue; };
            let pose = &mut self.buckets[DEATH_START + phase].pose;
            self.asset.sample(self.clips.death, age, pose);
            let blend = (age / DEATH_BLEND_SECONDS).clamp(0.0, 1.0);
            if blend < 1.0 {
                self.asset.blend(pose, &self.deaths[&id].pose, &self.full_mask, 1.0 - blend);
            }
            supports[phase] = pose_support(&self.asset, pose);
        }
        for corpse in corpses {
            let age = corpse_age(corpse, alpha);
            let death = &self.deaths[&corpse.id()];
            let (bucket, support) = if age < duration {
                let phase = collapse_phase(age, duration);
                (DEATH_START + phase, supports[phase])
            } else { (CORPSE_BUCKET, self.corpse_min_height) };
            let progress = (age / duration).clamp(0.0, 1.0);
            let height = -support + death.height * (1.0 - progress).powi(2);
            let position = corpse.drawn_position(alpha);
            self.buckets[bucket].instances.push(Instance::new(
                Vec3::new(position.x, height, position.y),
                Vec3::splat(HORDE_SCALE), LinearRgb::splat(0.6),
            ).with_yaw(death.yaw));
        }
    }

    pub(super) fn clear(&mut self) {
        for bucket in &mut self.buckets {
            bucket.instances.clear();
        }
        self.headings.clear();
        self.deaths.clear();
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
        out.int("corpse_instances", preview.buckets[DEATH_START..].iter().map(|bucket| bucket.instances.len()).sum::<usize>() as u64);
        out.int("collapsing_instances", preview.buckets[DEATH_START..CORPSE_BUCKET].iter().map(|bucket| bucket.instances.len() as u64).sum());
        out.num("corpse_settled_height", -preview.corpse_min_height);
        out.int("recoiling_instances", preview.buckets[RECOIL_START..DEATH_START].iter()
            .map(|bucket| bucket.instances.len() as u64).sum());
        out.int("attack_instances", preview.buckets[ATTACK_START..RECOIL_START].iter()
            .map(|bucket| bucket.instances.len() as u64).sum());
        for (label, clip) in [("walk_clip", preview.clips.walk), ("bite_clip", preview.clips.bite), ("leap_clip", preview.clips.leap)] {
            out.text(label, preview.asset.clip(clip).expect("resolved clip").name());
        }
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
            preview.buckets[HORDE_PHASES_PER_ROLE..ATTACK_START]
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
        out.int("collapsing_instances", 0);
        out.num("corpse_settled_height", 0.0);
        out.int("recoiling_instances", 0);
        out.int("attack_instances", 0);
        out.text("walk_clip", "");
        out.text("bite_clip", "");
        out.text("leap_clip", "");
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
        .join("../../assets/weapons/thornbound-staff/equipped/sword-glaive/player.glb")
}

/// The checked-in enemy asset is resolved independently of the launch directory.
pub(super) fn default_horde_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/characters/wolf/wolf.glb")
}

const PLAYER_DIGITS: [&str; 5] = ["Little", "Ring", "Middle", "Index", "Thumb"];

fn player_digit_joints(side: &str, digit: &str) -> [String; 3] {
    [1, 2, 3].map(|segment| format!("{side}{digit}{segment}"))
}

/// Character roles stay here, outside the generic asset importer.
fn player_upper_mask(asset: &arpg_assets::CharacterAsset) -> Result<arpg_assets::PoseMask, String> {
    let mut joints = vec![("Spine", 1.0), ("Chest", 1.0), ("Neck", 1.0), ("Head", 1.0),
        ("LeftShoulder", 1.0), ("RightShoulder", 1.0), ("LeftArm", 1.0), ("RightArm", 1.0),
        ("LeftForearm", 1.0), ("RightForearm", 1.0), ("LeftHand", 1.0), ("RightHand", 1.0),
        ("LeftGrip", 1.0), ("RightGrip", 1.0), ("Weapon", 1.0)];
    let fingers: Vec<_> = ["Left", "Right"].into_iter().flat_map(|side|
        PLAYER_DIGITS.into_iter().flat_map(move |digit| player_digit_joints(side, digit))).collect();
    joints.extend(fingers.iter().map(|name| (name.as_str(), 1.0)));
    asset.pose_mask_parent_aligned(&joints)
        .map_err(str::to_owned)
}

fn validate_player_grasp(asset: &arpg_assets::CharacterAsset, idle: arpg_assets::ClipId) -> Result<(), String> {
    asset.validate_rigid_attachment("RightHand", "RightGrip", idle).map_err(str::to_owned)?;
    asset.validate_rigid_attachment("RightGrip", "Weapon", idle).map_err(str::to_owned)?;
    for digit in PLAYER_DIGITS {
        let names = player_digit_joints("Right", digit);
        for (index, name) in names.iter().enumerate() {
            let parent = if index == 0 { "RightHand" } else { &names[index-1] };
            asset.validate_rigid_attachment(parent, name, idle).map_err(str::to_owned)?;
        }
    }
    let mut pose = asset.bind_pose();
    asset.sample(idle, 0.0, &mut pose);
    validate_player_grasp_at(asset, &pose)
}

fn validate_player_grasp_at(asset: &arpg_assets::CharacterAsset, pose: &arpg_assets::CharacterPose) -> Result<(), String> {
    let transform = |name| asset.joint_transform(pose, asset.joint_named(name).expect("validated grasp joint"))
        .expect("owned grasp pose");
    let origin = transform("RightGrip").transform_point3(Vec3::ZERO);
    let axis = transform("Weapon").transform_vector3(Vec3::Y).normalize();
    if origin.distance(transform("RightHand").transform_point3(Vec3::ZERO)) < 0.035 {
        return Err("player palm grasp must be distinct from the wrist".into());
    }
    let skin = |vertex: &arpg_assets::CharacterVertex| vertex.joints().into_iter().zip(vertex.weights())
        .fold(Vec3::ZERO, |sum, (joint, weight)| sum + pose.joint_matrices()[usize::from(joint)]
            .matrix().transform_point3(vertex.position()) * weight);
    let radial = |point: Vec3| { let delta = point - origin; (delta - axis * delta.dot(axis)).length() };
    let weapon = asset.joint_named("Weapon").expect("validated weapon");
    let weapon_lane = asset.joint_index(weapon).expect("owned weapon joint");
    let owns = |vertex: &arpg_assets::CharacterVertex, lanes: &[usize]| vertex.joints().into_iter().zip(vertex.weights())
        .filter_map(|(joint, weight)| lanes.contains(&usize::from(joint)).then_some(weight)).sum::<f32>() > 0.5;
    let radius = asset.vertices().iter().filter(|vertex| owns(vertex, &[weapon_lane])).filter_map(|vertex| {
        let point = skin(vertex); let along = (point-origin).dot(axis);
        (-0.20..0.035).contains(&along).then(|| radial(point))
    }).fold(0.0_f32, f32::max);
    if !(0.01..0.04).contains(&radius) { return Err("invalid player handle radius".into()); }
    for digit in PLAYER_DIGITS {
        let names = player_digit_joints("Right", digit);
        let lanes: Vec<_> = names.iter().map(|name| asset.joint_index(asset.joint_named(name).expect("validated finger"))
            .expect("owned finger joint")).collect();
        let (minimum_gap, closest_contact) = asset.vertices().iter().filter(|vertex| owns(vertex, &lanes))
            .map(|vertex| radial(skin(vertex)) - radius)
            .fold((f32::INFINITY, f32::INFINITY), |(minimum, closest), gap|
                (minimum.min(gap), closest.min(gap.abs())));
        if minimum_gap < -0.006 || closest_contact > 0.006 {
            return Err(format!("player {digit} does not enclose the handle without penetration"));
        }
    }
    Ok(())
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
    validate_player_grasp(&asset, clips.idle)?;
    let weapon = asset.joint_named("Weapon").expect("validated weapon");
    let lane = asset.joint_index(weapon).expect("owned weapon joint");
    let bind = asset.bind_pose();
    let inverse = asset.joint_transform(&bind, weapon).expect("owned weapon pose").inverse();
    // Derive the shaft endpoint from the admitted rigid mesh, so alternate
    // staff lengths share the contact relationship without an app offset.
    let butt_y = asset.vertices().iter().filter(|vertex|
        vertex.joints().into_iter().zip(vertex.weights())
            .any(|(joint, weight)| usize::from(joint) == lane && weight > 0.99))
        .map(|vertex| inverse.transform_point3(vertex.position()).y)
        .reduce(f32::min).ok_or("player weapon has no rigid geometry")?;
    let weapon_butt = Vec3::Y * butt_y;
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
        weapon,
        weapon_butt,
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
