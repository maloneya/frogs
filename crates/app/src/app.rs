use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

use glam::{Mat4, Vec2, Vec3};

use arpg_core::{Action, Instance, InstanceBuffer, Intent, MoveDir, Report};
use arpg_game::{Game, GameScene as Scene};
use arpg_gfx::{
    CharacterBucket, CharacterHorde, CharacterMesh, CharacterPreview, MAX_HORDE_POSE_BUCKETS,
    MeshAsset, MeshPreview, OrthoCamera, QuadBuffer, Renderer,
};
use arpg_sim::{
    Accumulator, Alpha, AttackPhase, AttackProfile, EnemyPresentation, Fnv, PlayerPresentation,
    TICK_HZ,
};

use crate::harness::{self, Command, Request};
use crate::hud;
use crate::input::Controls;
use crate::time::Clock;
use crate::ui::{MenuRequest, SceneChoice};

/// Owns everything and wires it together. Deliberately the only place that
/// knows about all the subsystems at once — `gfx` and `game` stay ignorant of
/// each other, and this is where they meet.
#[derive(Default)]
pub(crate) struct App {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    /// App-global presentation selected independently of playable scene state.
    asset_preview: Option<AssetPreview<MeshAsset>>,
    /// Hierarchical character presentation, also independent of simulation.
    character_preview: Option<LoadedCharacter<CharacterMesh>>,
    /// Shared-pose horde presentation, independent of playable scene state.
    horde_preview: Option<LoadedHorde<CharacterMesh>>,
    camera: Option<OrthoCamera>,
    game: Game,
    /// Game ids and ticks are scoped to this playtest generation.
    run_id: u64,
    /// Numbers the screenshots, so repeated captures do not overwrite.
    captures: u32,
    /// Frames skipped because the surface had none to give — occluded,
    /// resized behind our back, or lost. Reported alongside `frames` so a
    /// suspiciously fast run is self-diagnosing rather than mysterious.
    skipped: u64,
    /// Frames presented since launch.
    ///
    /// Reported by the harness so throughput can be *counted* over a known
    /// interval rather than inferred from `Clock`'s smoothed average — which is
    /// an EMA, and so cannot distinguish a steady 60Hz from a mixture that
    /// averages to it. It is also the only way to notice the app being throttled
    /// while it sits in the background.
    frames: u64,
    /// Present only when `ARPG_HARNESS` asked for a control socket.
    harness: Option<std::sync::mpsc::Receiver<Request>>,
    /// Keys to release, and replies to send, once their deadline passes.
    scheduled: Vec<Deferred>,
    /// Replies owed to callers waiting on a frame to be captured.
    awaiting_frame: Vec<std::sync::mpsc::Sender<String>>,
    /// Frames skipped since the oldest pending screenshot was asked for.
    ///
    /// A capture is recorded between drawing a frame and presenting it, so a
    /// frame that is never drawn never captures, and an occluded window skips
    /// every frame. Counting them is what lets the reply say "your window is
    /// covered" rather than a silent `ok` — see [`App::capture_has_stalled`].
    capture_stall: u32,
    /// Reused every frame so a steady state allocates nothing.
    instances: InstanceBuffer,
    /// The overlay's own staging buffer, on exactly the same terms — and
    /// separate from `instances` because the two are drawn by different
    /// pipelines in different spaces. Merging them would mean a sort.
    quads: QuadBuffer,
    input: Controls,
    clock: Clock,
    /// Turns the frame's elapsed seconds into whole simulation ticks.
    ///
    /// Owned by `app` because the frame loop is here, but defined in `sim`,
    /// which is the only thing that may mint a `Dt`. That split is the point:
    /// this crate measures wall clock and is structurally unable to hand any of
    /// it to the simulation.
    accumulator: Accumulator,
}

/// Metadata and GPU ownership committed together after a complete load.
struct AssetPreview<T> {
    path: std::path::PathBuf,
    vertex_count: usize,
    index_count: usize,
    texture_width: u32,
    texture_height: u32,
    gpu: T,
}

impl<T> AssetPreview<T> {
    fn report(&self, out: &mut Report) {
        out.bool("active", true);
        out.text("path", &self.path.to_string_lossy());
        out.int("vertices", self.vertex_count as u64);
        out.int("indices", self.index_count as u64);
        out.int("texture_width", u64::from(self.texture_width));
        out.int("texture_height", u64::from(self.texture_height));
    }
}

fn report_asset_preview<T>(preview: Option<&AssetPreview<T>>, out: &mut Report) {
    if let Some(preview) = preview {
        preview.report(out);
    } else {
        out.bool("active", false);
        out.text("path", "");
        out.int("vertices", 0);
        out.int("indices", 0);
        out.int("texture_width", 0);
        out.int("texture_height", 0);
    }
}

const CROSS_FADE_SECONDS: f64 = 0.10;
const WEAPON_JOINT: &str = "Weapon";
const WEAPON_LENGTH: f32 = 0.85;
const WEAPON_THICKNESS: f32 = 0.07;

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
            Self::Attack(AttackProfile::Basic) => "attack_basic",
            Self::Attack(AttackProfile::Thrust) => "attack_thrust",
            Self::Attack(AttackProfile::Sweep) => "attack_sweep",
            Self::Attack(AttackProfile::HeavySweep) => "attack_heavy_sweep",
            Self::Attack(AttackProfile::Cleave) => "attack_cleave",
            Self::Attack(AttackProfile::CrowdBreaker) => "attack_crowd_breaker",
        }
    }
}

#[derive(Clone, Copy)]
struct CharacterClips {
    idle: arpg_assets::ClipId,
    run: arpg_assets::ClipId,
    attacks: [arpg_assets::ClipId; AttackProfile::ALL.len()],
}

impl CharacterClips {
    fn resolve(asset: &arpg_assets::CharacterAsset) -> Result<Self, String> {
        let clip = |name| {
            asset
                .clip_named(name)
                .ok_or_else(|| format!("missing player presentation clip {name}"))
        };
        let idle = clip("Idle")?;
        let mut attacks = [idle; AttackProfile::ALL.len()];
        for (slot, profile) in attacks.iter_mut().zip(AttackProfile::ALL) {
            *slot = clip(Self::attack_name(profile))?;
        }
        Ok(Self {
            idle,
            run: clip("Run")?,
            attacks,
        })
    }

    fn attack_name(profile: AttackProfile) -> &'static str {
        match profile {
            AttackProfile::Basic => "AttackBasic",
            AttackProfile::Thrust => "AttackThrust",
            AttackProfile::Sweep => "AttackSweep",
            AttackProfile::HeavySweep => "AttackHeavySweep",
            AttackProfile::Cleave => "AttackCleave",
            AttackProfile::CrowdBreaker => "AttackCrowdBreaker",
        }
    }

    fn for_role(self, role: PresentationRole) -> arpg_assets::ClipId {
        match role {
            PresentationRole::Idle => self.idle,
            PresentationRole::Run => self.run,
            PresentationRole::Attack(profile) => {
                let index = AttackProfile::ALL
                    .iter()
                    .position(|candidate| *candidate == profile)
                    .expect("every attack profile is present in ALL");
                self.attacks[index]
            }
        }
    }
}

#[derive(Clone, Copy)]
struct Transition {
    role: PresentationRole,
    clip_seconds: f32,
    started_at: f64,
}

struct CharacterPlayback {
    role: PresentationRole,
    transition: Option<Transition>,
    clip_seconds: f32,
    blend: f32,
    last_presentation_seconds: f64,
}

impl Default for CharacterPlayback {
    fn default() -> Self {
        Self {
            role: PresentationRole::Idle,
            transition: None,
            clip_seconds: 0.0,
            blend: 1.0,
            last_presentation_seconds: 0.0,
        }
    }
}

impl CharacterPlayback {
    fn reset(&mut self) {
        *self = Self::default();
    }
}

/// CPU hierarchy, role mapping and GPU resources committed after a complete load.
struct LoadedCharacter<T> {
    path: std::path::PathBuf,
    asset: arpg_assets::CharacterAsset,
    pose: arpg_assets::CharacterPose,
    clips: CharacterClips,
    weapon: arpg_assets::JointId,
    playback: CharacterPlayback,
    gpu: T,
}

impl<T> LoadedCharacter<T> {
    fn role(player: PlayerPresentation) -> PresentationRole {
        let attack = player.attack();
        if attack.phase != AttackPhase::Idle {
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

    fn role_time(
        &self,
        role: PresentationRole,
        player: PlayerPresentation,
        alpha: Alpha,
        presentation_seconds: f64,
    ) -> f32 {
        let clip = self.clips.for_role(role);
        let metadata = self.asset.clip(clip).expect("resolved clip remains live");
        match role {
            PresentationRole::Idle | PresentationRole::Run => {
                metadata.loop_time(presentation_seconds)
            }
            PresentationRole::Attack(_) => {
                let attack = player.attack();
                let resolved = attack
                    .swing_resolved
                    .expect("a non-idle attack retains its committed resolution");
                let total = resolved.startup() + resolved.active() + resolved.recovery().get();
                ((attack.elapsed as f32 + alpha.get()) / total as f32).clamp(0.0, 1.0)
                    * metadata.duration_seconds()
            }
        }
    }

    fn sample(&mut self, player: PlayerPresentation, alpha: Alpha, presentation_seconds: f64) {
        if presentation_seconds < self.playback.last_presentation_seconds {
            self.playback.reset();
        }
        let role = Self::role(player);
        if role != self.playback.role {
            self.playback.transition = Some(Transition {
                role: self.playback.role,
                clip_seconds: self.playback.clip_seconds,
                started_at: presentation_seconds,
            });
            self.playback.role = role;
        }
        let clip_seconds = self.role_time(role, player, alpha, presentation_seconds);
        let clip = self.clips.for_role(role);
        let blend = self.playback.transition.map_or(1.0, |transition| {
            ((presentation_seconds - transition.started_at) / CROSS_FADE_SECONDS).clamp(0.0, 1.0)
                as f32
        });
        if let Some(transition) = self.playback.transition {
            let from = self.clips.for_role(transition.role);
            let _ = self.asset.sample_blended(
                (from, transition.clip_seconds),
                (clip, clip_seconds),
                blend,
                &mut self.pose,
            );
            if blend >= 1.0 {
                self.playback.transition = None;
            }
        } else {
            let _ = self.asset.sample(clip, clip_seconds, &mut self.pose);
        }
        self.playback.clip_seconds = clip_seconds;
        self.playback.blend = blend;
        self.playback.last_presentation_seconds = presentation_seconds;
    }

    fn character_instance(&self, player: PlayerPresentation) -> Instance {
        Instance::new(player.ground_position(), Vec3::ONE, Vec3::ONE).with_yaw(player.facing())
    }

    fn weapon_instance(&self, player: PlayerPresentation) -> Instance {
        let joint = self
            .asset
            .joint_transform(&self.pose, self.weapon)
            .expect("resolved weapon joint remains live");
        let along = joint.transform_vector3(Vec3::Y).normalize_or_zero();
        let centre = joint.transform_point3(Vec3::ZERO) + along * (WEAPON_LENGTH * 0.5);
        let world = Mat4::from_rotation_y(player.facing());
        let centre = player.ground_position() + world.transform_vector3(centre);
        let along = world.transform_vector3(along);
        let flat = Vec2::new(along.x, along.z).normalize_or_zero();
        let yaw = if flat == Vec2::ZERO {
            player.facing()
        } else {
            flat.x.atan2(flat.y)
        };
        Instance::new(
            centre,
            Vec3::new(WEAPON_THICKNESS, WEAPON_THICKNESS, WEAPON_LENGTH),
            Vec3::new(0.82, 0.86, 0.92),
        )
        .with_yaw(yaw)
    }
}

const HORDE_PHASES_PER_ROLE: usize = MAX_HORDE_POSE_BUCKETS / 2;
const HORDE_SCALE: f32 = 0.42;
const _: () = assert!(MAX_HORDE_POSE_BUCKETS > 0 && MAX_HORDE_POSE_BUCKETS.is_multiple_of(2));

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HordeRole {
    Idle,
    Run,
}

#[derive(Clone, Copy)]
struct HordeClips {
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
        })
    }

    fn for_role(self, role: HordeRole) -> arpg_assets::ClipId {
        match role {
            HordeRole::Idle => self.idle,
            HordeRole::Run => self.run,
        }
    }
}

struct HordeBucket {
    pose: arpg_assets::CharacterPose,
    instances: Vec<Instance>,
}

/// One mesh and a bounded set of shared poses for every ordinary enemy.
struct LoadedHorde<T> {
    path: std::path::PathBuf,
    asset: arpg_assets::CharacterAsset,
    clips: HordeClips,
    buckets: [HordeBucket; MAX_HORDE_POSE_BUCKETS],
    occupied_buckets: usize,
    instance_count: usize,
    gpu: T,
}

impl<T> LoadedHorde<T> {
    fn new(
        path: std::path::PathBuf,
        asset: arpg_assets::CharacterAsset,
        clips: HordeClips,
        gpu: T,
    ) -> Self {
        let buckets = std::array::from_fn(|_| HordeBucket {
            pose: asset.bind_pose(),
            instances: Vec::new(),
        });
        Self {
            path,
            asset,
            clips,
            buckets,
            occupied_buckets: 0,
            instance_count: 0,
            gpu,
        }
    }

    fn phase_key(id: arpg_sim::EntityId) -> u64 {
        let mut hash = Fnv::default();
        id.hash_into(&mut hash);
        hash.finish()
    }

    fn bucket(enemy: EnemyPresentation) -> (usize, u64) {
        let role = if enemy.displacement().length_squared() > 1.0e-8 {
            HordeRole::Run
        } else {
            HordeRole::Idle
        };
        let key = Self::phase_key(enemy.id());
        let phase = (key % HORDE_PHASES_PER_ROLE as u64) as usize;
        let role_offset = match role {
            HordeRole::Idle => 0,
            HordeRole::Run => HORDE_PHASES_PER_ROLE,
        };
        (role_offset + phase, key)
    }

    fn rebuild(
        &mut self,
        enemies: impl Iterator<Item = EnemyPresentation>,
        presentation_seconds: f64,
    ) {
        for bucket in &mut self.buckets {
            bucket.instances.clear();
        }

        self.instance_count = 0;
        for enemy in enemies {
            let (bucket, key) = Self::bucket(enemy);
            let displacement = enemy.displacement();
            let yaw = if displacement.length_squared() > 1.0e-8 {
                displacement.x.atan2(displacement.y)
            } else {
                0.0
            };
            let shade = 0.72 + ((key >> 8) & 0xff) as f32 / 255.0 * 0.28;
            self.buckets[bucket].instances.push(
                Instance::new(
                    enemy.ground_position(),
                    Vec3::splat(HORDE_SCALE),
                    Vec3::splat(shade),
                )
                .with_yaw(yaw),
            );
            self.instance_count += 1;
        }

        self.occupied_buckets = 0;
        for (index, bucket) in self.buckets.iter_mut().enumerate() {
            if bucket.instances.is_empty() {
                continue;
            }
            self.occupied_buckets += 1;
            let role = if index < HORDE_PHASES_PER_ROLE {
                HordeRole::Idle
            } else {
                HordeRole::Run
            };
            let clip = self.clips.for_role(role);
            let metadata = self.asset.clip(clip).expect("resolved clip remains live");
            let phase = index % HORDE_PHASES_PER_ROLE;
            let offset = f64::from(metadata.duration_seconds()) * phase as f64
                / HORDE_PHASES_PER_ROLE as f64;
            let time = metadata.loop_time(presentation_seconds + offset);
            let sampled = self.asset.sample(clip, time, &mut bucket.pose);
            debug_assert!(sampled, "resolved clip remains live");
        }
    }

    fn clear(&mut self) {
        for bucket in &mut self.buckets {
            bucket.instances.clear();
        }
        self.occupied_buckets = 0;
        self.instance_count = 0;
    }
}

impl LoadedHorde<CharacterMesh> {
    fn draw(&self) -> CharacterHorde<'_> {
        let buckets = std::array::from_fn(|index| {
            let bucket = &self.buckets[index];
            CharacterBucket::new(&bucket.pose, &bucket.instances)
        });
        CharacterHorde::new(&self.gpu, buckets)
    }
}

fn presentation_seconds(tick: u64, alpha: Alpha) -> f64 {
    (tick as f64 + f64::from(alpha.get())) / f64::from(TICK_HZ)
}

fn report_character_preview<T>(preview: Option<&LoadedCharacter<T>>, out: &mut Report) {
    if let Some(preview) = preview {
        let texture = preview.asset.base_color_texture();
        let clip = preview.clips.for_role(preview.playback.role);
        let clip = preview
            .asset
            .clip(clip)
            .expect("resolved clip remains live");
        out.bool("active", true);
        out.text("path", &preview.path.to_string_lossy());
        out.int("vertices", preview.asset.vertex_count() as u64);
        out.int("indices", preview.asset.index_count() as u64);
        out.int("texture_width", u64::from(texture.width()));
        out.int("texture_height", u64::from(texture.height()));
        out.int("nodes", preview.asset.node_count() as u64);
        out.int("joints", preview.asset.joint_count() as u64);
        out.int("clips", preview.asset.clip_count() as u64);
        out.text("role", preview.playback.role.label());
        out.text("clip", clip.name());
        out.num("clip_duration", clip.duration_seconds());
        out.int("channels", clip.channel_count() as u64);
        out.num("sample_seconds", preview.playback.clip_seconds);
        out.text(
            "blend_from",
            preview
                .playback
                .transition
                .map_or("", |transition| transition.role.label()),
        );
        out.num("blend", preview.playback.blend);
        out.text("weapon_joint", WEAPON_JOINT);
    } else {
        out.bool("active", false);
        out.text("path", "");
        out.int("vertices", 0);
        out.int("indices", 0);
        out.int("texture_width", 0);
        out.int("texture_height", 0);
        out.int("nodes", 0);
        out.int("joints", 0);
        out.int("clips", 0);
        out.text("role", "");
        out.text("clip", "");
        out.num("clip_duration", 0.0);
        out.int("channels", 0);
        out.num("sample_seconds", 0.0);
        out.text("blend_from", "");
        out.num("blend", 0.0);
        out.text("weapon_joint", "");
    }
}

fn report_horde_preview<T>(preview: Option<&LoadedHorde<T>>, out: &mut Report) {
    if let Some(preview) = preview {
        let texture = preview.asset.base_color_texture();
        out.bool("active", true);
        out.text("path", &preview.path.to_string_lossy());
        out.int("vertices", preview.asset.vertex_count() as u64);
        out.int("indices", preview.asset.index_count() as u64);
        out.int("texture_width", u64::from(texture.width()));
        out.int("texture_height", u64::from(texture.height()));
        out.int("nodes", preview.asset.node_count() as u64);
        out.int("joints", preview.asset.joint_count() as u64);
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
        out.int("occupied_buckets", preview.occupied_buckets as u64);
        out.int("instances", preview.instance_count as u64);
        out.int(
            "idle_instances",
            preview.buckets[..HORDE_PHASES_PER_ROLE]
                .iter()
                .map(|bucket| bucket.instances.len() as u64)
                .sum(),
        );
        out.int(
            "run_instances",
            preview.buckets[HORDE_PHASES_PER_ROLE..]
                .iter()
                .map(|bucket| bucket.instances.len() as u64)
                .sum(),
        );
    } else {
        out.bool("active", false);
        out.text("path", "");
        out.int("vertices", 0);
        out.int("indices", 0);
        out.int("texture_width", 0);
        out.int("texture_height", 0);
        out.int("nodes", 0);
        out.int("joints", 0);
        out.text("idle_clip", "");
        out.text("run_clip", "");
        out.int("pose_buckets", 0);
        out.int("occupied_buckets", 0);
        out.int("instances", 0);
        out.int("idle_instances", 0);
        out.int("run_instances", 0);
    }
}

/// World-space placement belongs to app, never to the imported mesh or sim.
fn preview_instance() -> Instance {
    Instance::new(
        glam::Vec3::new(-3.0, 0.0, -3.0),
        glam::Vec3::ONE,
        // White preserves the asset's authored base colour and factor.
        glam::Vec3::ONE,
    )
}

fn read_glb(path: &std::path::Path) -> Result<Vec<u8>, String> {
    if path.extension() != Some(std::ffi::OsStr::new("glb")) {
        return Err(format!("{}: expected a .glb file", path.display()));
    }
    let metadata = std::fs::metadata(path)
        .map_err(|error| format!("{}: read metadata: {error}", path.display()))?;
    let bytes = usize::try_from(metadata.len())
        .map_err(|_| format!("{}: file size cannot fit this process", path.display()))?;
    if bytes > arpg_assets::MAX_GLB_BYTES {
        return Err(format!(
            "{}: GLB exceeds preview capacity: file bytes",
            path.display()
        ));
    }
    std::fs::read(path).map_err(|error| format!("{}: read GLB: {error}", path.display()))
}

/// Prepares every fallible part before replacing `slot`.
fn replace_preview<T>(
    slot: &mut Option<AssetPreview<T>>,
    path: std::path::PathBuf,
    upload: impl FnOnce(&arpg_assets::StaticMesh) -> Result<T, String>,
) -> Result<(), String> {
    let source = read_glb(&path)?;
    let mesh = arpg_assets::import_glb(&source)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let vertex_count = mesh.vertex_count();
    let index_count = mesh.index_count();
    let texture_width = mesh.base_color_texture().width();
    let texture_height = mesh.base_color_texture().height();
    let gpu = upload(&mesh).map_err(|error| format!("{}: {error}", path.display()))?;
    *slot = Some(AssetPreview {
        path,
        vertex_count,
        index_count,
        texture_width,
        texture_height,
        gpu,
    });
    Ok(())
}

fn replace_character<T>(
    slot: &mut Option<LoadedCharacter<T>>,
    path: std::path::PathBuf,
    upload: impl FnOnce(&arpg_assets::CharacterAsset) -> Result<T, String>,
) -> Result<(), String> {
    let source = read_glb(&path)?;
    let asset = arpg_assets::import_character_glb(&source)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let clips =
        CharacterClips::resolve(&asset).map_err(|error| format!("{}: {error}", path.display()))?;
    let weapon = asset.joint_named(WEAPON_JOINT).ok_or_else(|| {
        format!(
            "{}: missing player attachment joint {WEAPON_JOINT}",
            path.display()
        )
    })?;
    let gpu = upload(&asset).map_err(|error| format!("{}: {error}", path.display()))?;
    let pose = asset.bind_pose();
    *slot = Some(LoadedCharacter {
        path,
        asset,
        pose,
        clips,
        weapon,
        playback: CharacterPlayback::default(),
        gpu,
    });
    Ok(())
}

/// Prepares horde clips, poses and GPU resources before replacing `slot`.
fn replace_horde<T>(
    slot: &mut Option<LoadedHorde<T>>,
    path: std::path::PathBuf,
    upload: impl FnOnce(&arpg_assets::CharacterAsset) -> Result<T, String>,
) -> Result<(), String> {
    let source = read_glb(&path)?;
    let asset = arpg_assets::import_character_glb(&source)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let clips =
        HordeClips::resolve(&asset).map_err(|error| format!("{}: {error}", path.display()))?;
    let gpu = upload(&asset).map_err(|error| format!("{}: {error}", path.display()))?;
    *slot = Some(LoadedHorde::new(path, asset, clips, gpu));
    Ok(())
}

/// A key release, a harness reply, or both, owed once `due` passes.
///
/// Named rather than a tuple because two of its three fields are optional and
/// which combination is in play is the whole meaning: `tap` is a release with
/// no reply, `wait` is a reply with no release, `hold` is both.
struct Deferred {
    due: std::time::Instant,
    /// The key to lift, if this deadline releases one.
    release: Option<KeyCode>,
    /// The caller to answer, if one is blocked on this deadline.
    reply: Option<std::sync::mpsc::Sender<String>>,
}

/// One notch of zoom per keypress.
const ZOOM_STEP: f32 = 1.2;
const _: () = assert!(ZOOM_STEP > 1.0, "a step of 1 or less makes the zoom keys inert or inverted");

/// Where `P` writes screenshots. The system temp directory unless told
/// otherwise, so a stray keypress never litters the working tree.
fn capture_dir() -> std::path::PathBuf {
    std::env::var_os("ARPG_CAPTURE_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
}

/// Preserve the file identity in diagnostics from either caller.
fn read_scene(path: &std::path::Path) -> Result<Scene, String> {
    arpg_content::load_scene(path).map_err(|error| format!("{}: {error}", path.display()))
}

/// Discovery happens only when the picker opens. Do not parse here: a broken
/// file remains selectable so its load error can be shown without losing a run.
fn scene_catalog(directory: &std::path::Path) -> Result<Vec<std::path::PathBuf>, String> {
    let read = || -> std::io::Result<Vec<std::path::PathBuf>> {
        let mut paths = Vec::new();
        for entry in std::fs::read_dir(directory)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "ron") && entry.file_type()?.is_file() {
                paths.push(path);
            }
        }
        paths.sort();
        Ok(paths)
    };
    read().map_err(|error| format!("Cannot list {}: {error}", directory.display()))
}

impl App {
    fn show_asset(&mut self, path: std::path::PathBuf) -> Result<String, String> {
        let Self { renderer, asset_preview, .. } = self;
        let renderer = renderer.as_ref().ok_or_else(|| "no renderer yet".to_string())?;
        replace_preview(asset_preview, path, |mesh| {
            renderer.upload_mesh(mesh).map_err(|error| error.to_string())
        })?;
        let preview = asset_preview.as_ref().expect("successful replacement selects a preview");
        Ok(format!(
            "asset preview vertices={} indices={} texture={}x{} path={}",
            preview.vertex_count,
            preview.index_count,
            preview.texture_width,
            preview.texture_height,
            preview.path.display()
        ))
    }

    fn show_character(&mut self, path: std::path::PathBuf) -> Result<String, String> {
        let Self {
            renderer,
            character_preview,
            ..
        } = self;
        let renderer = renderer
            .as_ref()
            .ok_or_else(|| "no renderer yet".to_string())?;
        replace_character(character_preview, path, |character| {
            renderer
                .upload_character(character)
                .map_err(|error| error.to_string())
        })?;
        let preview = character_preview
            .as_ref()
            .expect("successful replacement selects a character");
        let clip = preview
            .asset
            .clip(preview.clips.idle)
            .expect("resolved clip remains live");
        Ok(format!(
            "player character vertices={} indices={} nodes={} joints={} clips={} texture={}x{} idle={} weapon={} path={}",
            preview.asset.vertex_count(),
            preview.asset.index_count(),
            preview.asset.node_count(),
            preview.asset.joint_count(),
            preview.asset.clip_count(),
            preview.asset.base_color_texture().width(),
            preview.asset.base_color_texture().height(),
            clip.name(),
            WEAPON_JOINT,
            preview.path.display()
        ))
    }

    fn show_horde(&mut self, path: std::path::PathBuf) -> Result<String, String> {
        let Self {
            renderer,
            horde_preview,
            ..
        } = self;
        let renderer = renderer
            .as_ref()
            .ok_or_else(|| "no renderer yet".to_string())?;
        replace_horde(horde_preview, path, |character| {
            renderer
                .upload_character(character)
                .map_err(|error| error.to_string())
        })?;
        let preview = horde_preview
            .as_ref()
            .expect("successful replacement selects a horde");
        Ok(format!(
            "horde character vertices={} indices={} joints={} pose_buckets={} idle=Idle run=Run path={}",
            preview.asset.vertex_count(),
            preview.asset.index_count(),
            preview.asset.joint_count(),
            MAX_HORDE_POSE_BUCKETS,
            preview.path.display()
        ))
    }

    /// Both native keys and harness keys execute menu requests at this same
    /// between-tick boundary. No file I/O or game replacement happens in draw.
    fn handle_key(&mut self, key: KeyCode, pressed: bool, repeat: bool) -> bool {
        let consumed = self.input.on_key(key, pressed, repeat, self.game.attack_status().profile);
        match self.input.menu_mut().take_request() {
            Some(MenuRequest::RefreshScenes) => {
                self.input.menu_mut().set_catalog(scene_catalog(std::path::Path::new("scenes")));
            }
            Some(MenuRequest::Start(choice)) => {
                let result = match choice {
                    SceneChoice::Restart => self.restart_playtest(),
                    SceneChoice::Boot => self.start_playtest(Scene::boot()),
                    SceneChoice::File { path, .. } => self.start_scene_file(&path),
                };
                if let Err(error) = result {
                    self.input.menu_mut().set_error(error);
                }
            }
            None => {}
        }
        consumed
    }

    fn start_scene_file(&mut self, path: &std::path::Path) -> Result<(), String> {
        self.start_playtest(read_scene(path)?)
    }

    fn restart_playtest(&mut self) -> Result<(), String> {
        let run_id = self.run_id.checked_add(1).ok_or("playtest identities exhausted")?;
        let old_profile = self.game.attack_status().profile;
        self.game.restart().map_err(|error| error.to_string())?;
        self.finish_playtest_start(run_id, old_profile);
        Ok(())
    }

    /// Prepare before committing: a bad replacement cannot destroy the current
    /// playtest. Construction is synchronous and runs zero simulation ticks.
    fn start_playtest(&mut self, scene: Scene) -> Result<(), String> {
        let run_id = self.run_id.checked_add(1).ok_or("playtest identities exhausted")?;
        let old_profile = self.game.attack_status().profile;
        self.game.start_scene(&scene).map_err(|error| error.to_string())?;
        self.finish_playtest_start(run_id, old_profile);
        Ok(())
    }

    /// Infallible adapter cleanup after Game has installed a complete replacement.
    /// Device input, captures, and camera state belong to the app, not gameplay.
    fn finish_playtest_start(&mut self, run_id: u64, old_profile: arpg_sim::AttackProfile) {
        // Old delayed key releases must not lift a new run's presses. Pending
        // callers receive a cancellation rather than a success for another run.
        for deferred in std::mem::take(&mut self.scheduled) {
            if let Some(key) = deferred.release {
                self.input.on_key(key, false, false, old_profile);
            }
            if let Some(reply) = deferred.reply {
                let _ = reply.send("error: cancelled by playtest restart".into());
            }
        }
        for reply in self.awaiting_frame.drain(..) {
            let _ = reply.send("error: cancelled by playtest restart".into());
        }
        if let Some(renderer) = &mut self.renderer {
            renderer.cancel_capture();
        }
        self.capture_stall = 0;
        self.input.restart();
        self.run_id = run_id;
        self.accumulator = Accumulator::default();
        if let Some(character) = &mut self.character_preview {
            character.playback.reset();
        }
        if let Some(horde) = &mut self.horde_preview {
            horde.clear();
        }
        if let Some(camera) = &mut self.camera {
            camera.snap_to(self.game.player_pos());
        }
        self.game.extract(Alpha::ZERO, self.instances.sink());
        self.quads.sink();
        // Exclude file reading, construction, and the old run's frame remainder.
        self.clock = Clock::default();
    }

    fn ready_reply(&self) -> String {
        format!("ready run={} tick=0 hash={:016x}", self.run_id, self.game.hash())
    }

    /// Debug and meta commands, kept deliberately apart from the action layer.
    ///
    /// These are not things the *character* does — they are things done to the
    /// running program, and the difference is not cosmetic. Game actions are
    /// sampled as state once per tick, need rebinding, and will one day come
    /// from a gamepad or a replay. These are one-shot, fire straight from the
    /// event callback, and are meaningless to a simulation. Funnelling them
    /// through `Action` would put "toggle vsync" in the vocabulary the horde's
    /// AI speaks.
    ///
    /// The two sets must stay disjoint; `BINDINGS` is the list to check against.
    fn on_debug_key(&mut self, key: KeyCode) {
        match key {
            // Doubling rather than stepping: the interesting range spans three
            // orders of magnitude, and the knee is easier to find by bisection
            // than by walking. Both directions clamp inside `set_enemy_count`.
            KeyCode::BracketRight => {
                // `max(1)` because doubling zero is zero: the horde can now
                // be emptied, and without this `]` could not refill it.
                self.game.set_enemy_count((self.game.enemy_count() * 2).max(1));
                log::info!("N = {}", self.game.enemy_count());
            }
            KeyCode::BracketLeft => {
                self.game.set_enemy_count(self.game.enemy_count() / 2);
                log::info!("N = {}", self.game.enemy_count());
            }
            KeyCode::Equal => {
                if let Some(c) = &mut self.camera {
                    c.zoom_by(1.0 / ZOOM_STEP);
                }
            }
            KeyCode::Minus => {
                if let Some(c) = &mut self.camera {
                    c.zoom_by(ZOOM_STEP);
                }
            }
            _ => {}
        }
    }

    /// Applies whatever the control socket has sent since the last frame.
    ///
    /// Runs before input is sampled, so an injected key takes effect on the
    /// very frame it arrives rather than the one after.
    fn drain_harness(&mut self) -> bool {
        // Collected up front so the receiver borrow ends before the loop needs
        // the rest of `self`.
        let Some(rx) = &self.harness else {
            return false;
        };
        let requests: Vec<Request> = rx.try_iter().collect();
        let mut quit = false;

        for Request { command, reply } in requests {
            let now = std::time::Instant::now();
            let answer = match command {
                Command::ShowAsset(path) => match self.show_asset(path) {
                    Ok(answer) => answer,
                    Err(error) => format!("error: {error}"),
                },
                Command::ClearAsset => {
                    self.asset_preview = None;
                    "ok".to_string()
                }
                Command::ShowCharacter(path) => match self.show_character(path) {
                    Ok(answer) => answer,
                    Err(error) => format!("error: {error}"),
                },
                Command::ClearCharacter => {
                    self.character_preview = None;
                    "ok".to_string()
                }
                Command::ShowHorde(path) => match self.show_horde(path) {
                    Ok(answer) => answer,
                    Err(error) => format!("error: {error}"),
                },
                Command::ClearHorde => {
                    self.horde_preview = None;
                    "ok".to_string()
                }
                Command::StartScene(path) => match self.start_scene_file(&path) {
                    Ok(()) => self.ready_reply(),
                    Err(error) => format!("error: {error}"),
                },
                Command::RestartScene => match self.restart_playtest() {
                    Ok(()) => self.ready_reply(),
                    Err(error) => format!("error: {error}"),
                },
                Command::AddScene(path) => match read_scene(&path).and_then(|scene| {
                    self.game.load_scene(&scene).map_err(|error| error.to_string())
                }) {
                    Ok(id) => {
                        format!("ready run={} scene={id} tick={}", self.run_id, self.game.tick())
                    }
                    Err(error) => format!("error: {error}"),
                },
                Command::EvictScene(id) => {
                    if self.game.evict_scene(id) {
                        format!("evicted run={} scene={id}", self.run_id)
                    } else {
                        format!("error: no live scene {id} in run {}", self.run_id)
                    }
                }
                Command::ListScenes => {
                    let mut out = Report::default();
                    out.int("run_id", self.run_id);
                    for (id, name) in self.game.scene_instances() {
                        out.text(&id.to_string(), name);
                    }
                    out.finish()
                }
                Command::Press(key) => {
                    self.handle_key(key, true, false);
                    "ok".to_string()
                }
                Command::Release(key) => {
                    self.handle_key(key, false, false);
                    "ok".to_string()
                }
                Command::Tap(key) => {
                    self.handle_key(key, true, false);
                    self.scheduled.push(Deferred {
                        due: now,
                        release: Some(key),
                        reply: None,
                    });
                    "ok".to_string()
                }
                Command::Hold(key, ms) => {
                    self.handle_key(key, true, false);
                    let due = now + std::time::Duration::from_millis(ms);
                    self.scheduled.push(Deferred {
                        due,
                        release: Some(key),
                        reply: Some(reply),
                    });
                    continue; // replies once the key comes back up
                }
                Command::Wait(ms) => {
                    let due = now + std::time::Duration::from_millis(ms);
                    self.scheduled.push(Deferred {
                        due,
                        release: None,
                        reply: Some(reply),
                    });
                    continue;
                }
                Command::Shot(path) => {
                    if let Some(renderer) = self.renderer.as_mut() {
                        renderer.request_capture(path);
                    }
                    self.awaiting_frame.push(reply);
                    continue; // replies once the file exists
                }
                Command::State => self.report_state(),
                Command::Impulse { target, value } => match self.game.body_named(&target) {
                    Some(id) if self.game.apply_impulse(id, value) => {
                        format!("impulse applied to {id}")
                    }
                    _ => format!("error: no physical body {target}"),
                },
                Command::TraceSince(tick) => self.report_trace(tick),
                Command::SetEnemies(n) => {
                    self.game.set_enemy_count(n);
                    // Respawning retires every name, so it revokes every
                    // behaviour with them. Said in the reply rather than left
                    // to be discovered, because "I set the horde and the
                    // chasing stopped" is otherwise a puzzle.
                    format!(
                        "enemies {} seekers {}",
                        self.game.enemy_count(),
                        self.game.seeker_count()
                    )
                }
                Command::Spawn { x, z, what } => {
                    // The reply says *queued*, not spawned, because that is what
                    // happened: the body appears when the next tick runs. A
                    // reply claiming otherwise would make a `state` taken
                    // immediately afterwards look like a bug.
                    if self.game.request_spawn(Vec2::new(x, z), what) {
                        format!("queued at ({x}, {z}) seeks={}", what.seeks())
                    } else {
                        "error: spawn queue full".to_string()
                    }
                }
                Command::Source(spec) => {
                    // The conversion is where `Placement::around` and the
                    // cadence clamp are applied, so the socket cannot reach a
                    // source that skipped either.
                    let id = self.game.add_source(spec.into());
                    format!("source {id}")
                }
                Command::SetSourceEnabled { id, enabled } => {
                    if self.game.set_source_enabled(id, enabled) {
                        format!("source {id} enabled={enabled}")
                    } else {
                        format!("error: no live source {id}")
                    }
                }
                Command::RemoveSource(id) => match self.game.remove_source(id) {
                    true => format!("removed {id}"),
                    false => format!("error: no live source {id}"),
                },
                Command::SetSeekers(n) => {
                    self.game.set_seeker_count(n);
                    format!("seekers {}", self.game.seeker_count())
                }
                Command::SetVsync(on) => match self.renderer.as_mut() {
                    Some(renderer) => {
                        if renderer.vsync() != on {
                            renderer.toggle_vsync();
                        }
                        format!("vsync {}", renderer.vsync())
                    }
                    None => "error: no renderer yet".to_string(),
                },
                Command::Quit => {
                    quit = true;
                    "ok".to_string()
                }
            };
            let _ = reply.send(answer);
        }
        quit
    }

    /// Everything worth knowing about the running program, as JSON.
    ///
    /// **Derived, not hand-written.** The simulation's half comes from
    /// `Game::report`, which destructures `Game` exhaustively — so a field
    /// added to the game fails to compile until it is observable. What is left
    /// here is the part `sim` genuinely cannot know: how the frame went, what
    /// the camera is doing, whether the renderer is presenting.
    ///
    /// JSON rather than a positional line, because the consumer is usually a
    /// program: `jq -r .sim.tick` does not care what order the fields are in or
    /// how many were added since it was written.
    fn report_state(&self) -> String {
        let mut out = Report::default();

        out.int("run_id", self.run_id);
        out.text(
            "selected_scene",
            self.game.selected_scene_name().unwrap_or(""),
        );
        out.text("sim_hash", &format!("{:016x}", self.game.hash()));
        out.object("sim", |sim| self.game.report(sim));
        out.object("ui", |ui| self.input.menu().report(ui));
        out.object("asset_preview", |asset| {
            report_asset_preview(self.asset_preview.as_ref(), asset);
        });
        out.object("character_preview", |character| {
            report_character_preview(self.character_preview.as_ref(), character);
        });
        out.object("horde_preview", |horde| {
            report_horde_preview(self.horde_preview.as_ref(), horde);
        });

        out.object("render", |r| {
            let target = self
                .camera
                .as_ref()
                .map(OrthoCamera::target)
                .unwrap_or_default();
            r.vec3("camera_target", target);
            let player_instances = u64::from(self.character_preview.is_some());
            let horde_instances = self
                .horde_preview
                .as_ref()
                .map_or(0, |horde| horde.instance_count as u64);
            r.int(
                "instances",
                self.instances.as_slice().len() as u64 + player_instances + horde_instances,
            );
            r.int("cube_instances", self.instances.as_slice().len() as u64);
            r.int("player_character_instances", player_instances);
            r.int("horde_character_instances", horde_instances);
            r.int(
                "horde_pose_draws",
                self.horde_preview
                    .as_ref()
                    .map_or(0, |horde| horde.occupied_buckets as u64),
            );
            r.int("frames", self.frames);
            r.int("skipped", self.skipped);
            r.num("frame_ms", self.clock.frame_ms());
            r.bool("vsync", self.renderer.as_ref().is_some_and(Renderer::vsync));
        });

        out.finish()
    }

    /// Every trace event from `tick` onward, one per line.
    ///
    /// Same rendering as a scenario's golden trace file, because it is the same
    /// function — a second formatter would be a second thing to keep in step,
    /// and the whole point is that what you read here is what a scenario can
    /// assert on.
    fn report_trace(&self, tick: u64) -> String {
        let mut out = format!("# run {}\n", self.run_id);
        let dropped = self.game.trace_dropped();
        if dropped > 0 {
            out.push_str(&format!("# {dropped} event(s) dropped; the buffer wrapped\n"));
        }
        let events = self.game.render_trace_since(tick);
        out.push_str(&events);
        out.push_str(&format!("# {} event(s)", self.game.trace().since(tick).count() + self.game.control_trace().since(tick).count()));
        out
    }

    /// Counts one skipped frame against any pending screenshot, and reports
    /// whether the wait should be abandoned.
    ///
    /// Waiting forever would be honest and useless: the caller blocks on a
    /// socket read with no idea why. Giving up after a bounded run of skipped
    /// frames keeps the harness's promise — every command replies, and the
    /// reply is true — while making the one condition that breaks screenshots
    /// name itself.
    ///
    /// Split from the abandoning itself so the rule is a plain function over a
    /// counter. The condition it fires on is one this environment cannot
    /// produce on demand, and an untested error path is one that has never run.
    fn capture_has_stalled(waiting: bool, stall: &mut u32) -> bool {
        /// Frames of nothing before a screenshot is declared impossible. Only
        /// has to exceed what a healthy run produces, and a healthy run
        /// captures on the very next frame.
        const STALLED_FRAMES: u32 = 240;

        if !waiting {
            *stall = 0;
            return false;
        }

        *stall += 1;
        if *stall < STALLED_FRAMES {
            return false;
        }

        *stall = 0;
        true
    }

    /// Fires the releases and replies whose deadline has passed.
    fn service_schedule(&mut self) {
        let now = std::time::Instant::now();
        let mut still_pending = Vec::new();

        for deferred in std::mem::take(&mut self.scheduled) {
            if now < deferred.due {
                still_pending.push(deferred);
                continue;
            }
            if let Some(key) = deferred.release {
                self.input.on_key(key, false, false, self.game.attack_status().profile);
            }
            if let Some(reply) = deferred.reply {
                let _ = reply.send("ok".to_string());
            }
        }
        self.scheduled = still_pending;
    }

    /// One frame: measure it, run whatever ticks it bought, then draw.
    ///
    /// Everything below the `alpha` line is presentation. Nothing there may
    /// write simulation state, and nothing there may consume an input edge.
    fn redraw(&mut self) {
        let (Some(renderer), Some(camera)) = (self.renderer.as_mut(), self.camera.as_mut()) else {
            return;
        };

        let frame = self.clock.tick();

        // Screen space becomes world space here, and only here. The camera owns
        // the mapping because it owns the angle; `sim` is handed a direction it
        // can integrate without knowing a screen exists.
        let (right, up) = camera.ground_basis();
        let to_world = |axis: Vec2| MoveDir::new(right * axis.x + up * axis.y);

        // Zero, one or several — a frame buys whole ticks and the remainder
        // waits.
        //
        // **Intent is sampled per tick, not per frame.** `sample` clears the
        // latched edges, so sampling once per frame means a frame that runs no
        // ticks consumes a keypress and discards it — and uncapped, most frames
        // run no ticks. Sampling here makes a press wait for a tick and gives it
        // to exactly one. The symptom otherwise is "the attack sometimes does
        // not come out", which points nowhere near the frame loop.
        for dt in self.accumulator.pending(frame) {
            // Requests wait through zero-tick frames and apply before this
            // tick's attack input. Drawing below cannot consume or apply one.
            if let Some(profile) = self.input.take_profile() {
                self.game.set_attack_profile(profile);
            }
            let intent = self.input.sample();
            // `just_pressed`, not `held`: a swing is an edge. Holding the key
            // must not swing every tick, and a tap shorter than a frame must
            // still swing exactly once.
            self.game.step(
                dt,
                Intent::new(
                    to_world(intent.move_axis()),
                    intent.just_pressed(Action::Attack),
                )
                .with_interact(intent.just_pressed(Action::Interact)),
            );
        }

        // How far this frame falls between the tick just run and the next one.
        // Everything below draws; nothing below simulates.
        let alpha = self.accumulator.alpha();
        let player = self.game.player_presentation(alpha);
        let presentation_seconds = presentation_seconds(self.game.tick(), alpha);

        // Presentation, in three ways at once. **After** the step, or it would
        // add a frame of lag on top of the smoothing that is there on purpose.
        // The **drawn** position and the **frame's** delta, not the tick's,
        // because a rig whose whole job is smoothness must not be given a
        // stair-step to follow. And `held`, never `sample` — a camera that
        // consumed a keypress would be the same bug wearing a different hat.
        let facing = to_world(self.input.held().move_axis());
        camera.follow(player.ground_position(), facing, frame);

        if let Some(horde) = &mut self.horde_preview {
            horde.rebuild(self.game.enemy_presentations(alpha), presentation_seconds);
        }
        let has_player_character = self.character_preview.is_some();
        let has_horde_character = self.horde_preview.is_some();
        let weapon = self.character_preview.as_mut().map(|character| {
            character.sample(player, alpha, presentation_seconds);
            character.weapon_instance(player)
        });

        if !has_player_character && !has_horde_character {
            self.game.extract(alpha, self.instances.sink());
        } else {
            let mut sink = self.instances.sink();
            if has_horde_character {
                self.game.extract_without_characters(alpha, &mut sink);
                if !has_player_character {
                    player.extract_fallback(&mut sink);
                }
            } else {
                self.game.extract_without_player(alpha, &mut sink);
            }
            if let Some(weapon) = weapon {
                sink.push(weapon);
            }
        }

        // The overlay is built here rather than inside `render` for the same
        // reason `extract` is: what a readout says is a decision this crate
        // makes, and the renderer's job stops at drawing the rectangles it is
        // handed. Scoped so the sink's borrow ends before the draw.
        {
            let mut sink = self.quads.sink();
            let size = self.window.as_ref().expect("rendering has a window").inner_size();
            hud::draw(
                renderer.glyphs(),
                self.input.menu(),
                self.game.attack_status(),
                self.game.selected_scene_name().unwrap_or("--"),
                Vec2::new(size.width as f32, size.height as f32),
                &mut sink,
            );
        }

        // Counted only when a frame actually reached the screen. An occluded
        // window skips the draw entirely, and counting those would report
        // thousands of frames a second for drawing nothing.
        let preview = self
            .asset_preview
            .as_ref()
            .map(|preview| MeshPreview::new(&preview.gpu, preview_instance()));
        let character = self.character_preview.as_ref().map(|preview| {
            CharacterPreview::new(
                &preview.gpu,
                &preview.pose,
                preview.character_instance(player),
            )
        });
        let horde = self.horde_preview.as_ref().map(LoadedHorde::draw);
        if renderer.render(
            camera,
            self.instances.as_slice(),
            preview,
            character,
            horde,
            self.quads.as_slice(),
        ) {
            self.frames += 1;

            // The capture is written inside `render`, and only on the path that
            // presents — so this is the first moment the file is known to exist,
            // and the only place `ok` is honest.
            self.capture_stall = 0;
            for reply in std::mem::take(&mut self.awaiting_frame) {
                let _ = reply.send("ok".to_string());
            }
        } else {
            self.skipped += 1;

            let waiting = !self.awaiting_frame.is_empty();
            if Self::capture_has_stalled(waiting, &mut self.capture_stall) {
                // Drop the request too, so it cannot fire minutes later and
                // write a file after the caller was told it failed.
                renderer.cancel_capture();
                for reply in std::mem::take(&mut self.awaiting_frame) {
                    let _ = reply.send(
                        "error: no frame was presented, so nothing could be \
                         captured — the window is occluded or minimised"
                            .to_string(),
                    );
                }
            }
        }

        if self.clock.hud_due()
            && let Some(window) = &self.window
        {
            window.set_title(&format!(
                "arpg — {:.2}ms  {:.0}fps  N={}  ({} instances){}",
                self.clock.frame_ms(),
                self.clock.fps(),
                self.game.enemy_count(),
                self.instances.as_slice().len(),
                if renderer.vsync() { "  [vsync]" } else { "  [uncapped]" },
            ));
        }
    }

    pub(crate) fn run() {
        let event_loop = EventLoop::new().expect("create event loop");

        // Poll rather than Wait: never block waiting for input, just keep
        // looping. Vsync in the surface config is what actually paces us.
        event_loop.set_control_flow(ControlFlow::Poll);
        let mut app = App::default();
        app.start_playtest(Scene::boot()).expect("valid built-in boot scene");
        event_loop.run_app(&mut app).expect("run app");
    }
}

impl ApplicationHandler for App {
    /// Window and GPU creation belongs here rather than in `main`, because on
    /// mobile platforms the surface is destroyed and rebuilt as the app moves
    /// in and out of the foreground. winit models that as suspend/resume, and
    /// guarantees `resumed` fires before any window event on every platform.
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.renderer.is_some() {
            return; // Can fire more than once; only build GPU state the first time.
        }

        let attrs = Window::default_attributes()
            .with_title("arpg")
            .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 720.0));

        let window = Arc::new(event_loop.create_window(attrs).expect("create window"));
        let display_handle = event_loop.owned_display_handle();

        // Native-only, so we can simply block until the GPU is ready. The
        // cross-platform examples route this back through the event loop
        // because the browser forbids blocking the main thread.
        self.renderer = Some(pollster::block_on(Renderer::new(
            window.clone(),
            display_handle,
        )));
        let size = window.inner_size();

        // Start framed on the character rather than easing in from the origin.
        // Harmless today, since both begin there — but the moment anything
        // spawns the player elsewhere, the first thing the player would see is
        // the camera flying across the game to catch up.
        let mut camera = OrthoCamera::new(size.width, size.height);
        camera.snap_to(self.game.player_pos());

        self.camera = Some(camera);
        self.window = Some(window);
        self.harness = harness::start();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if self.renderer.is_none() || self.camera.is_none() {
            return;
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),

            // A key-up goes to whoever has focus. Alt-tab while running and the
            // release never arrives, so without this the character keeps going.
            WindowEvent::Focused(focused) => {
                log::debug!("focus: {focused}");
                if !focused {
                    self.input.release_all();
                }
            }

            WindowEvent::KeyboardInput { event: key_event, .. } => {
                log::debug!(
                    "key: physical={:?} state={:?} repeat={}",
                    key_event.physical_key,
                    key_event.state,
                    key_event.repeat
                );

                let PhysicalKey::Code(key) = key_event.physical_key else {
                    return;
                };
                let pressed = key_event.state == ElementState::Pressed;

                // The harness uses this same route. A modal UI event must not
                // also fire a debug command (especially Escape -> quit).
                let consumed = self.handle_key(key, pressed, key_event.repeat);

                if !consumed && pressed && !key_event.repeat {
                    match key {
                        KeyCode::Escape => event_loop.exit(),
                        KeyCode::KeyV => {
                            self.renderer.as_mut().expect("initialized").toggle_vsync();
                        }
                        KeyCode::KeyP => {
                            let path = capture_dir().join(format!("arpg-{:04}.png", self.captures));
                            self.captures += 1;
                            self.renderer.as_mut().expect("initialized").request_capture(path);
                        }
                        _ => self.on_debug_key(key),
                    }
                }
            }

            WindowEvent::Resized(size) => {
                self.renderer.as_mut().expect("initialized").resize(size.width, size.height);
                self.camera.as_mut().expect("initialized").set_viewport(size.width, size.height);
            }

            WindowEvent::RedrawRequested => self.redraw(),

            _ => {}
        }
    }

    /// winit is event-driven by default: with nothing happening, it sleeps. A
    /// game is the opposite — it must produce a frame whether or not anyone
    /// touched the keyboard. Requesting a redraw every time the event queue
    /// drains is what turns this into a continuous loop.
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // The control socket is drained here rather than inside the redraw,
        // because this is the one point in the loop where nothing else is
        // borrowed out of `self` — and it runs immediately before the frame,
        // so an injected key takes effect on the very next one.
        //
        // Expiries are serviced *first*, so a key pressed by this pass survives
        // until the next one and is therefore held for exactly one frame.
        self.service_schedule();
        if self.drain_harness() {
            event_loop.exit();
            return;
        }

        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair() -> Scene {
        arpg_sim::Scene {
            name: "pair".into(),
            bodies: vec![arpg_sim::Placed {
                pos: (20.0, 0.0),
                what: arpg_sim::Template::BODY,
            }],
            grids: vec![],
            sources: vec![],
        }
        .into()
    }

    fn fixture_path() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/fixtures/static-preview.glb")
    }

    fn character_fixture_path() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/fixtures/blender-bind-pose.glb")
    }

    fn loaded_character(path: &str) -> LoadedCharacter<()> {
        let source = std::fs::read(character_fixture_path()).unwrap();
        let asset = arpg_assets::import_character_glb(&source).unwrap();
        let clips = CharacterClips::resolve(&asset).unwrap();
        let weapon = asset.joint_named(WEAPON_JOINT).unwrap();
        let pose = asset.bind_pose();
        LoadedCharacter {
            path: path.into(),
            asset,
            pose,
            clips,
            weapon,
            playback: CharacterPlayback::default(),
            gpu: (),
        }
    }

    fn loaded_horde(path: &str) -> LoadedHorde<()> {
        let source = std::fs::read(character_fixture_path()).unwrap();
        let asset = arpg_assets::import_character_glb(&source).unwrap();
        let clips = HordeClips::resolve(&asset).unwrap();
        LoadedHorde::new(path.into(), asset, clips, ())
    }

    #[test]
    fn preview_replacement_is_atomic_across_import_and_upload_failure() {
        let mut slot = Some(AssetPreview {
            path: "old.glb".into(),
            vertex_count: 3,
            index_count: 3,
            texture_width: 1,
            texture_height: 1,
            gpu: (),
        });
        let missing = std::env::temp_dir().join("arpg-preview-that-does-not-exist.glb");
        assert!(replace_preview(&mut slot, missing, |_| Ok(())).is_err());
        assert_eq!(slot.as_ref().unwrap().path, std::path::Path::new("old.glb"));

        assert!(replace_preview(&mut slot, fixture_path(), |_| Err("GPU refused it".into())).is_err());
        assert_eq!(slot.as_ref().unwrap().path, std::path::Path::new("old.glb"));

        replace_preview(&mut slot, fixture_path(), |_| Ok(())).unwrap();
        let current = slot.as_ref().unwrap();
        assert_eq!((current.vertex_count, current.index_count), (12, 12));
        assert_eq!((current.texture_width, current.texture_height), (4, 4));
    }

    #[test]
    fn asset_preview_report_is_derived_from_the_committed_selection() {
        let mut inactive = Report::default();
        report_asset_preview::<()>(None, &mut inactive);
        assert_eq!(
            inactive.finish(),
            r#"{"active":false,"path":"","vertices":0,"indices":0,"texture_width":0,"texture_height":0}"#
        );

        let preview = AssetPreview {
            path: "assets/fixture.glb".into(),
            vertex_count: 12,
            index_count: 18,
            texture_width: 2,
            texture_height: 4,
            gpu: (),
        };
        let mut active = Report::default();
        report_asset_preview(Some(&preview), &mut active);
        assert_eq!(
            active.finish(),
            r#"{"active":true,"path":"assets/fixture.glb","vertices":12,"indices":18,"texture_width":2,"texture_height":4}"#
        );
    }

    #[test]
    fn character_replacement_is_atomic_across_import_and_upload_failure() {
        let mut slot = Some(loaded_character("old.glb"));
        let missing = std::env::temp_dir().join("arpg-character-that-does-not-exist.glb");
        assert!(replace_character(&mut slot, missing, |_| Ok(())).is_err());
        assert_eq!(slot.as_ref().unwrap().path, std::path::Path::new("old.glb"));

        assert!(
            replace_character(&mut slot, character_fixture_path(), |_| {
                Err("GPU refused it".into())
            })
            .is_err()
        );
        assert_eq!(slot.as_ref().unwrap().path, std::path::Path::new("old.glb"));

        replace_character(&mut slot, character_fixture_path(), |_| Ok(())).unwrap();
        let current = slot.as_ref().unwrap();
        assert_eq!(
            (current.asset.vertex_count(), current.asset.index_count()),
            (144, 216)
        );
        assert_eq!(
            (current.asset.node_count(), current.asset.joint_count()),
            (6, 4)
        );
        assert_eq!(current.asset.clip_count(), 8);
        let current = slot.as_mut().unwrap();
        let player = Game::empty().player_presentation(Alpha::ONE);
        current.sample(player, Alpha::ONE, 0.5);
        assert!(
            current
                .pose
                .joint_matrices()
                .iter()
                .any(|matrix| !matrix.matrix().abs_diff_eq(glam::Mat4::IDENTITY, 1.0e-3))
        );
    }

    #[test]
    fn character_preview_report_is_derived_from_the_committed_selection() {
        let mut inactive = Report::default();
        report_character_preview::<()>(None, &mut inactive);
        assert_eq!(
            inactive.finish(),
            r#"{"active":false,"path":"","vertices":0,"indices":0,"texture_width":0,"texture_height":0,"nodes":0,"joints":0,"clips":0,"role":"","clip":"","clip_duration":0.0000,"channels":0,"sample_seconds":0.0000,"blend_from":"","blend":0.0000,"weapon_joint":""}"#
        );

        let mut preview = loaded_character("assets/character.glb");
        let player = Game::empty().player_presentation(Alpha::ONE);
        preview.sample(player, Alpha::ONE, 0.25);
        let mut active = Report::default();
        report_character_preview(Some(&preview), &mut active);
        assert_eq!(
            active.finish(),
            r#"{"active":true,"path":"assets/character.glb","vertices":144,"indices":216,"texture_width":16,"texture_height":16,"nodes":6,"joints":4,"clips":8,"role":"idle","clip":"Idle","clip_duration":1.0000,"channels":12,"sample_seconds":0.2500,"blend_from":"","blend":1.0000,"weapon_joint":"Weapon"}"#
        );
    }

    #[test]
    fn horde_replacement_is_atomic_and_its_report_is_derived() {
        let mut slot = Some(loaded_horde("old.glb"));
        let missing = std::env::temp_dir().join("arpg-horde-that-does-not-exist.glb");
        assert!(replace_horde(&mut slot, missing, |_| Ok(())).is_err());
        assert_eq!(slot.as_ref().unwrap().path, std::path::Path::new("old.glb"));

        assert!(
            replace_horde(&mut slot, character_fixture_path(), |_| {
                Err("GPU refused it".into())
            })
            .is_err()
        );
        assert_eq!(slot.as_ref().unwrap().path, std::path::Path::new("old.glb"));

        replace_horde(&mut slot, character_fixture_path(), |_| Ok(())).unwrap();
        let mut game = Game::empty();
        game.set_enemy_count(32);
        let current = slot.as_mut().unwrap();
        current.rebuild(game.enemy_presentations(Alpha::ONE), 0.25);
        assert_eq!(current.instance_count, 32);
        assert!((1..=HORDE_PHASES_PER_ROLE).contains(&current.occupied_buckets));

        let mut active = Report::default();
        report_horde_preview(Some(current), &mut active);
        let report = active.finish();
        assert!(report.contains(r#""active":true"#));
        assert!(report.contains(r#""idle_clip":"Idle""#));
        assert!(report.contains(r#""run_clip":"Run""#));
        assert!(report.contains(r#""pose_buckets":8"#));
        assert!(report.contains(r#""instances":32"#));
        assert!(report.contains(r#""idle_instances":32"#));
        assert!(report.contains(r#""run_instances":0"#));
    }

    #[test]
    fn stable_identity_selects_bounded_reused_horde_pose_buckets() {
        let mut horde = loaded_horde("horde.glb");
        let mut game = Game::empty();
        game.set_enemy_count(32);
        let enemies: Vec<_> = game.enemy_presentations(Alpha::ONE).collect();
        let removed = enemies[0].id();
        let survivor = enemies[1];
        let survivor_bucket = LoadedHorde::<()>::bucket(survivor).0;

        horde.rebuild(enemies.into_iter(), 0.0);
        let capacities = horde
            .buckets
            .each_ref()
            .map(|bucket| bucket.instances.capacity());
        horde.rebuild(game.enemy_presentations(Alpha::ONE), 0.1);
        assert_eq!(
            horde
                .buckets
                .each_ref()
                .map(|bucket| bucket.instances.capacity()),
            capacities,
            "a steady horde reallocated its instance buckets"
        );

        assert!(game.despawn_body(removed));
        let survivor_after_swap = game
            .enemy_presentations(Alpha::ONE)
            .find(|enemy| enemy.id() == survivor.id())
            .unwrap();
        assert_eq!(
            LoadedHorde::<()>::bucket(survivor_after_swap).0,
            survivor_bucket
        );

        game.set_seeker_count(31);
        let dt = Accumulator::default()
            .pending(arpg_sim::Dt::SECS)
            .next()
            .unwrap();
        game.step(dt, Intent::default());
        horde.rebuild(game.enemy_presentations(Alpha::ONE), 0.2);
        assert!(
            horde.buckets[..HORDE_PHASES_PER_ROLE]
                .iter()
                .all(|bucket| bucket.instances.is_empty())
        );
        assert_eq!(
            horde.buckets[HORDE_PHASES_PER_ROLE..]
                .iter()
                .map(|bucket| bucket.instances.len())
                .sum::<usize>(),
            31
        );
        assert!(horde.occupied_buckets <= HORDE_PHASES_PER_ROLE);
    }

    #[test]
    fn every_attack_profile_resolves_to_its_own_presentation_clip() {
        let character = loaded_character("character.glb");
        let expected = [
            "AttackBasic",
            "AttackThrust",
            "AttackSweep",
            "AttackHeavySweep",
            "AttackCleave",
            "AttackCrowdBreaker",
        ];
        for (profile, expected) in AttackProfile::ALL.into_iter().zip(expected) {
            let id = character.clips.for_role(PresentationRole::Attack(profile));
            assert_eq!(character.asset.clip(id).unwrap().name(), expected);
        }
    }

    #[test]
    fn authoritative_player_facts_drive_roles_cross_fade_and_weapon() {
        let mut character = loaded_character("character.glb");
        let mut game = Game::empty();
        let idle = game.player_presentation(Alpha::ONE);
        character.sample(idle, Alpha::ONE, 0.0);
        assert_eq!(character.playback.role, PresentationRole::Idle);

        let dt = Accumulator::default()
            .pending(arpg_sim::Dt::SECS)
            .next()
            .unwrap();
        game.step(dt, Intent::new(MoveDir::new(Vec3::X), false));
        let running = game.player_presentation(Alpha::ONE);
        character.sample(running, Alpha::ONE, 1.0 / 60.0);
        assert_eq!(character.playback.role, PresentationRole::Run);
        assert_eq!(character.playback.blend, 0.0);
        assert_eq!(
            character.playback.transition.unwrap().role,
            PresentationRole::Idle
        );

        character.sample(running, Alpha::ONE, 0.2);
        assert_eq!(character.playback.blend, 1.0);
        assert!(character.playback.transition.is_none());
        let weapon = character.weapon_instance(running);
        assert!(weapon.pos().is_finite());
        assert!(weapon.yaw().is_finite());

        game.set_attack_profile(AttackProfile::Sweep);
        let dt = Accumulator::default()
            .pending(arpg_sim::Dt::SECS)
            .next()
            .unwrap();
        game.step(dt, Intent::new(MoveDir::NONE, true));
        let attacking = game.player_presentation(Alpha::ONE);
        character.sample(attacking, Alpha::ONE, 0.25);
        assert_eq!(
            character.playback.role,
            PresentationRole::Attack(AttackProfile::Sweep)
        );
        let clip = character.clips.for_role(character.playback.role);
        assert_eq!(character.asset.clip(clip).unwrap().name(), "AttackSweep");
    }

    #[test]
    fn character_time_is_the_continuous_tick_plus_alpha_clock() {
        assert_eq!(presentation_seconds(60, Alpha::ZERO), 1.0);
        assert_eq!(presentation_seconds(59, Alpha::ONE), 1.0);
    }

    fn tap(app: &mut App, key: KeyCode) {
        app.handle_key(key, true, false);
        app.handle_key(key, false, false);
    }

    #[test]
    fn picker_uses_the_fresh_start_boundary_and_survives_bad_files() {
        let directory = std::env::temp_dir().join(format!("arpg-picker-{}", std::process::id()));
        std::fs::create_dir_all(directory.join("directory.ron")).unwrap();
        let path = directory.join("a scene.ron");
        std::fs::write(&path, "(name: \"first\", bodies: [(pos: (20.0, 0.0))])").unwrap();
        std::fs::write(directory.join("b.ron"), "broken RON").unwrap();
        std::fs::write(directory.join("ignored.txt"), "ignored").unwrap();
        let paths = scene_catalog(&directory).unwrap();
        assert_eq!(paths, vec![path.clone(), directory.join("b.ron")]);
        assert!(scene_catalog(&directory.join("missing")).is_err());

        let mut app = App::default();
        app.start_playtest(pair()).unwrap();
        tap(&mut app, KeyCode::F2);
        app.input.menu_mut().set_catalog(Ok(paths.clone()));
        tap(&mut app, KeyCode::ArrowDown);
        tap(&mut app, KeyCode::ArrowDown);
        tap(&mut app, KeyCode::Enter);
        let initial = app.game.hash();
        let direct = Game::from_scene(&arpg_content::load_scene(&path).unwrap()).unwrap();
        assert_eq!(
            initial,
            direct.hash(),
            "picker and file loader produce identical tick-zero worlds"
        );
        assert_eq!(app.run_id, 2);
        assert!(!app.input.menu().open());
        assert_eq!(app.input.sample().move_axis(), Vec2::ZERO);
        std::fs::write(&path, "(name: \"changed\")").unwrap();
        tap(&mut app, KeyCode::F2);
        tap(&mut app, KeyCode::Enter); // Cached restart remains independent of disk.
        assert_eq!(app.game.hash(), initial);
        assert_eq!(app.run_id, 3);

        tap(&mut app, KeyCode::F2);
        app.input.menu_mut().set_catalog(Ok(paths.clone()));
        for _ in 0..3 {
            tap(&mut app, KeyCode::ArrowDown);
        }
        tap(&mut app, KeyCode::Enter);
        assert!(app.input.menu().picker().error().unwrap().contains("b.ron"));
        assert!(app.input.menu().open());
        assert_eq!(app.game.hash(), initial);
        assert_eq!(app.run_id, 3);
        std::fs::remove_file(&path).unwrap();
        tap(&mut app, KeyCode::ArrowUp);
        tap(&mut app, KeyCode::Enter); // A file disappearing after discovery is safe too.
        assert!(app.input.menu().picker().error().is_some());
        assert_eq!(app.game.hash(), initial);

        std::fs::write(&path, "(name: \"changed\")").unwrap();
        tap(&mut app, KeyCode::Enter); // Retry reads the repaired file.
        assert_ne!(app.game.hash(), initial);
        assert_eq!(app.run_id, 4);
        assert!(!app.input.menu().open());
        tap(&mut app, KeyCode::F2);
        tap(&mut app, KeyCode::ArrowDown);
        tap(&mut app, KeyCode::Enter);
        assert_eq!(app.game.hash(), Game::from_scene(&Scene::boot()).unwrap().hash());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn exhausted_run_ids_preserve_the_game_input_and_pending_capture() {
        let mut app = App::default();
        app.start_playtest(pair()).unwrap();
        app.run_id = u64::MAX;
        let profile = app.game.attack_status().profile;
        app.input.on_key(KeyCode::KeyW, true, false, profile);
        let (reply, response) = std::sync::mpsc::channel();
        app.awaiting_frame.push(reply);
        let before = app.game.hash();
        let held = app.input.held().move_axis();

        assert!(app.start_playtest(Scene::boot()).is_err());
        assert!(app.restart_playtest().is_err());
        assert_eq!(app.game.hash(), before);
        assert_eq!(app.game.selected_scene_name(), Some("pair"));
        assert_eq!(app.run_id, u64::MAX);
        assert_eq!(app.input.held().move_axis(), held);
        assert_eq!(app.awaiting_frame.len(), 1);
        assert!(matches!(response.try_recv(), Err(std::sync::mpsc::TryRecvError::Empty)));
    }

    #[test]
    fn restart_resets_the_complete_playtest_boundary() {
        let mut app = App::default();
        app.start_playtest(pair()).unwrap();
        let initial = app.game.hash();
        let profile = app.game.attack_status().profile;
        app.input.on_key(KeyCode::KeyW, true, false, profile);
        app.input.on_key(KeyCode::Space, true, false, profile);
        for dt in app.accumulator.pending(arpg_sim::Dt::SECS) {
            app.game.step(dt, Intent::new(MoveDir::new(glam::Vec3::X), true));
        }
        app.game.set_attack_recovery(arpg_sim::RecoveryTicks::try_from(1).unwrap());
        assert!(app.game.apply_impulse(
            app.game.player_id(),
            arpg_sim::Impulse::try_from((6.0, 0.0)).unwrap()
        ));
        assert!(app.game.request_spawn(Vec2::ZERO, arpg_sim::Template::BODY));
        app.input.on_key(KeyCode::F1, true, false, profile);
        app.input.on_key(KeyCode::ArrowRight, true, false, profile);
        assert!(app.input.menu().pending().is_some());
        let mut camera = OrthoCamera::new(1280, 720);
        camera.snap_to(glam::Vec3::new(40.0, 0.0, 40.0));
        app.camera = Some(camera);
        assert_eq!(app.accumulator.pending(arpg_sim::Dt::SECS * 0.5).count(), 0);

        app.restart_playtest().unwrap();
        assert_eq!(app.run_id, 2);
        assert_eq!(app.game.hash(), initial, "all simulation state returns to the baseline");
        assert_eq!(app.game.tick(), 0);
        assert_eq!(app.accumulator.alpha().get(), 0.0);
        assert_eq!(app.camera.as_ref().unwrap().target(), glam::Vec3::ZERO);
        assert!(!app.input.menu().open());
        assert!(app.input.take_profile().is_none());
        assert_eq!(app.input.sample().move_axis(), Vec2::ZERO);
        app.input.on_key(KeyCode::KeyW, true, false, profile);
        assert_eq!(app.input.sample().move_axis(), Vec2::ZERO, "held native keys require release");
        app.input.on_key(KeyCode::KeyW, false, false, profile);
        app.input.on_key(KeyCode::KeyW, true, false, profile);
        assert_ne!(app.input.sample().move_axis(), Vec2::ZERO);
    }

    #[test]
    fn restart_cancels_old_delayed_actions_and_capture_replies() {
        let mut app = App::default();
        let (reply, response) = std::sync::mpsc::channel();
        let (shot_reply, shot_response) = std::sync::mpsc::channel();
        app.input.on_key(KeyCode::KeyD, true, false, app.game.attack_status().profile);
        app.scheduled.push(Deferred {
            due: std::time::Instant::now() + std::time::Duration::from_secs(60),
            release: Some(KeyCode::KeyD),
            reply: Some(reply),
        });
        app.awaiting_frame.push(shot_reply);
        app.start_playtest(pair()).unwrap();
        assert!(response.try_recv().unwrap().contains("cancelled"));
        assert!(shot_response.try_recv().unwrap().contains("cancelled"));
        assert!(app.scheduled.is_empty());
        app.input.on_key(KeyCode::KeyD, true, false, app.game.attack_status().profile);
        app.service_schedule();
        assert_ne!(
            app.input.sample().move_axis(),
            Vec2::ZERO,
            "new presses survive cancelled old releases"
        );
    }

    #[test]
    fn rejected_replacement_preserves_the_current_playtest() {
        let mut app = App::default();
        app.start_playtest(pair()).unwrap();
        app.input.on_key(KeyCode::KeyW, true, false, app.game.attack_status().profile);
        let before = app.report_state();
        let trace = app.game.trace().render();
        let mut bad = pair();
        bad.engine.bodies[0].pos.0 = f32::INFINITY;
        assert!(app.start_playtest(bad).is_err());
        assert_eq!(app.report_state(), before);
        assert_eq!(app.game.trace().render(), trace);
        assert_ne!(app.input.sample().move_axis(), Vec2::ZERO);
    }

    #[test]
    fn harness_start_reads_content_but_restart_reuses_the_snapshot() {
        let mut app = App::default();
        let (tx, rx) = std::sync::mpsc::channel();
        app.harness = Some(rx);
        let mut command = |command| {
            let (reply, response) = std::sync::mpsc::channel();
            tx.send(Request { command, reply }).unwrap();
            assert!(!app.drain_harness());
            (response.try_recv().unwrap(), app.game.hash(), app.run_id)
        };
        let path =
            std::env::temp_dir().join(format!("arpg-scene-snapshot-{}.ron", std::process::id()));
        std::fs::write(&path, "(name: \"first\", bodies: [(pos: (20.0, 0.0))])").unwrap();
        let (reply, first, _) = command(Command::StartScene(path.clone()));
        assert!(reply.contains("ready run=1 tick=0"), "{reply}");
        std::fs::write(&path, "(name: \"second\")").unwrap();
        let (_, restarted, run) = command(Command::RestartScene);
        assert_eq!(first, restarted);
        assert_eq!(run, 2);
        let (_, reloaded, run) = command(Command::StartScene(path.clone()));
        assert_ne!(first, reloaded);
        assert_eq!(run, 3);
        std::fs::remove_file(&path).unwrap();
        let (reply, unchanged, run) = command(Command::StartScene(path));
        assert!(reply.starts_with("error:"));
        assert_eq!(unchanged, reloaded);
        assert_eq!(run, 3);
    }

    /// A screenshot that cannot be taken has to *say so*. A reply sent whether
    /// or not the frame was drawn turns "your window is covered" into "your
    /// change did nothing" — the worst failure available to a harness whose
    /// whole contract is that a reply means the effect landed.
    #[test]
    fn a_screenshot_gives_up_only_after_a_long_run_of_skipped_frames() {
        let mut stall = 0;

        // A hiccup is not a stall; the wait has to survive one.
        assert!(!App::capture_has_stalled(true, &mut stall));
        assert!(!App::capture_has_stalled(true, &mut stall));

        let gave_up = (0..10_000).any(|_| App::capture_has_stalled(true, &mut stall));
        assert!(gave_up, "waited forever instead of reporting the failure");
    }

    /// With nothing pending there is nothing to give up on, and the count must
    /// not carry over — otherwise a long occluded stretch would fail the next
    /// screenshot the instant it was asked for.
    #[test]
    fn skipped_frames_with_no_screenshot_pending_do_not_accumulate() {
        let mut stall = 0;

        for _ in 0..10_000 {
            assert!(!App::capture_has_stalled(false, &mut stall));
        }
        assert_eq!(stall, 0);

        assert!(!App::capture_has_stalled(true, &mut stall), "a fresh request must not fail");
    }
}
