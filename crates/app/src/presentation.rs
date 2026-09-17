//! Asset selection and character presentation; no write access to simulation.

use arpg_core::{Instance, InstanceBuffer, Report};
use arpg_gfx::{
    CharacterBucket, CharacterHorde, CharacterMesh, CharacterPreview, MAX_HORDE_POSE_BUCKETS,
    MeshAsset, MeshBatch,
};
use arpg_sim::{
    Alpha, AttackPhase, AttackProfile, EnemyPresentation, Fnv, PlayerPresentation, TICK_HZ,
};
use glam::Vec3;

// Ground and optional static preview add two placements to an all-prop world.
// This also covers an all-enemy world plus the player in the character buffer.
const _: () = assert!(arpg_sim::MAX_BODIES + 2 <= arpg_core::MAX_INSTANCES);

pub(super) struct AssetPreview<T> {
    path: std::path::PathBuf,
    vertex_count: usize,
    index_count: usize,
    texture_width: u32,
    texture_height: u32,
    gpu: T,
}

impl<T> AssetPreview<T> {
    pub(super) fn summary(&self) -> String {
        format!(
            "asset preview vertices={} indices={} texture={}x{} path={}",
            self.vertex_count,
            self.index_count,
            self.texture_width,
            self.texture_height,
            self.path.display()
        )
    }

    fn report(&self, out: &mut Report) {
        out.bool("active", true);
        out.text("path", &self.path.to_string_lossy());
        out.int("vertices", self.vertex_count as u64);
        out.int("indices", self.index_count as u64);
        out.int("texture_width", u64::from(self.texture_width));
        out.int("texture_height", u64::from(self.texture_height));
    }
}

impl AssetPreview<MeshAsset> {
    pub(super) fn draw<'a>(&'a self, instances: &'a [Instance]) -> MeshBatch<'a> {
        MeshBatch::new(&self.gpu, instances)
    }
}

pub(super) fn report_asset_preview<T>(preview: Option<&AssetPreview<T>>, out: &mut Report) {
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

/// Required static world assets and their reusable placement buffers.
/// Both assets must import and upload before this value can exist.
pub(super) struct WorldAssets<T> {
    ground: AssetPreview<T>,
    prop: AssetPreview<T>,
    ground_instance: [Instance; 1],
    props: InstanceBuffer,
}

impl<T> WorldAssets<T> {
    pub(super) fn load(mut upload: impl FnMut(&arpg_assets::StaticMesh) -> Result<T, String>) -> Result<Self, String> {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/world");
        let ground = load_static(root.join("ground.glb"), &mut upload)?;
        let prop = load_static(root.join("prop.glb"), upload)?;
        Ok(Self {
            ground,
            prop,
            ground_instance: [Instance::new(
                Vec3::ZERO,
                Vec3::new(arpg_sim::ARENA_HALF * 2.0, 1.0, arpg_sim::ARENA_HALF * 2.0),
                Vec3::ONE,
            )],
            props: InstanceBuffer::default(),
        })
    }

    pub(super) fn rebuild(&mut self, props: impl Iterator<Item = arpg_sim::PropPresentation>) {
        let mut sink = self.props.sink();
        for prop in props {
            let tint = if prop.interaction() == Some(arpg_sim::InteractionState::Activated) {
                Vec3::new(0.06, 0.55, 0.12)
            } else {
                Vec3::new(0.04, 0.18, 0.32)
            };
            sink.push(Instance::new(prop.ground_position(), Vec3::ONE, tint));
        }
    }

    pub(super) fn instance_count(&self) -> usize { 1 + self.props.as_slice().len() }

    pub(super) fn draw_count(&self) -> usize { 1 + usize::from(!self.props.as_slice().is_empty()) }

    pub(super) fn report(&self, out: &mut Report) {
        out.object("ground", |out| self.ground.report(out));
        out.object("prop", |out| self.prop.report(out));
        out.int("ground_instances", 1);
        out.int("prop_instances", self.props.as_slice().len() as u64);
        out.int("draws", self.draw_count() as u64);
    }
}

impl WorldAssets<MeshAsset> {
    pub(super) fn draws<'a>(&'a self, preview: Option<&'a AssetPreview<MeshAsset>>, preview_instances: &'a [Instance]) -> [MeshBatch<'a>; 3] {
        [
            self.ground.draw(&self.ground_instance),
            self.prop.draw(self.props.as_slice()),
            preview.map_or_else(|| self.ground.draw(&[]), |preview| preview.draw(preview_instances)),
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
        }
    }
}

struct CharacterClips {
    idle: arpg_assets::ClipId,
    run: arpg_assets::ClipId,
    attacks: Vec<arpg_assets::ClipId>,
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
            attacks,
        })
    }

    fn attack_name(profile: AttackProfile) -> &'static str {
        match profile {
            AttackProfile::Cleave => "AttackCleave",
            AttackProfile::Slam => "AttackSlam",
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

/// CPU hierarchy, role mapping and GPU resources committed after a complete load.
pub(super) struct LoadedCharacter<T> {
    path: std::path::PathBuf,
    asset: arpg_assets::CharacterAsset,
    pose: arpg_assets::CharacterPose,
    clips: CharacterClips,
    sampled_role: PresentationRole,
    sampled_seconds: f32,
    gpu: T,
}

impl<T> LoadedCharacter<T> {
    pub(super) fn summary(&self) -> String {
        let clip = self
            .asset
            .clip(self.clips.idle)
            .expect("resolved clip remains live");
        format!(
            "player character vertices={} indices={} nodes={} joints={} clips={} texture={}x{} idle={} path={}",
            self.asset.vertex_count(),
            self.asset.index_count(),
            self.asset.node_count(),
            self.asset.joint_count(),
            self.asset.clip_count(),
            self.asset.base_color_texture().width(),
            self.asset.base_color_texture().height(),
            clip.name(),
            self.path.display()
        )
    }

    pub(super) fn reset(&mut self) {
        self.sampled_role = PresentationRole::Idle;
        self.sampled_seconds = 0.0;
    }

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

    pub(super) fn sample(
        &mut self,
        player: PlayerPresentation,
        alpha: Alpha,
        presentation_seconds: f64,
    ) {
        let role = Self::role(player);
        let clip = self.clips.for_role(role);
        let metadata = self.asset.clip(clip).expect("resolved clip remains live");
        let clip_seconds = match role {
            PresentationRole::Idle | PresentationRole::Run => {
                metadata.loop_time(presentation_seconds)
            }
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
        let _ = self.asset.sample(clip, clip_seconds, &mut self.pose);
        self.sampled_role = role;
        self.sampled_seconds = clip_seconds;
    }
}

impl LoadedCharacter<CharacterMesh> {
    pub(super) fn draw(&self, player: PlayerPresentation) -> CharacterPreview<'_> {
        let instance =
            Instance::new(player.ground_position(), Vec3::ONE, Vec3::ONE).with_yaw(player.facing());
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
        let buckets = std::array::from_fn(|_| HordeBucket {
            pose: asset.bind_pose(),
            instances: Vec::new(),
        });
        Self {
            path,
            asset,
            clips,
            buckets,
            headings: std::collections::HashMap::new(),
            rebuild_id: 0,
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

    pub(super) fn rebuild(
        &mut self,
        enemies: impl Iterator<Item = EnemyPresentation>,
        presentation_seconds: f64,
    ) {
        for bucket in &mut self.buckets {
            bucket.instances.clear();
        }

        self.rebuild_id = self.rebuild_id.wrapping_add(1);
        for enemy in enemies {
            let (bucket, key) = Self::bucket(enemy);
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
            let yaw = heading.yaw;
            let shade = 0.72 + ((key >> 8) & 0xff) as f32 / 255.0 * 0.28;
            self.buckets[bucket].instances.push(
                Instance::new(
                    enemy.ground_position(),
                    Vec3::splat(HORDE_SCALE),
                    Vec3::splat(shade),
                )
                .with_yaw(yaw),
            );
        }
        self.headings
            .retain(|_, heading| heading.seen_in_rebuild == self.rebuild_id);

        for (index, bucket) in self.buckets.iter_mut().enumerate() {
            if bucket.instances.is_empty() {
                continue;
            }
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
        out.text("clip", clip.name());
        out.num("clip_duration", clip.duration_seconds());
        out.int("channels", clip.channel_count() as u64);
        out.num("sample_seconds", preview.sampled_seconds);
    } else {
        out.int("clips", 0);
        out.text("role", "");
        out.text("clip", "");
        out.num("clip_duration", 0.0);
        out.int("channels", 0);
        out.num("sample_seconds", 0.0);
    }
}

pub(super) fn report_horde_preview<T>(preview: Option<&LoadedHorde<T>>, out: &mut Report) {
    report_character_asset(
        preview.map(|preview| (preview.path.as_path(), &preview.asset)),
        out,
    );
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
        out.text("idle_clip", "");
        out.text("run_clip", "");
        out.int("pose_buckets", 0);
        out.int("occupied_buckets", 0);
        out.int("instances", 0);
        out.int("idle_instances", 0);
        out.int("run_instances", 0);
    }
}

fn report_character_asset(
    selected: Option<(&std::path::Path, &arpg_assets::CharacterAsset)>,
    out: &mut Report,
) {
    let (path, asset) = selected.unzip();
    let texture = asset.map(arpg_assets::CharacterAsset::base_color_texture);
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
    out.int(
        "texture_width",
        texture.map_or(0, |texture| u64::from(texture.width())),
    );
    out.int(
        "texture_height",
        texture.map_or(0, |texture| u64::from(texture.height())),
    );
    out.int("nodes", asset.map_or(0, |asset| asset.node_count() as u64));
    out.int(
        "joints",
        asset.map_or(0, |asset| asset.joint_count() as u64),
    );
}

/// World-space placement belongs to app, never to the imported mesh or sim.
pub(super) fn preview_instance() -> Instance {
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
pub(super) fn replace_preview<T>(
    slot: &mut Option<AssetPreview<T>>,
    path: std::path::PathBuf,
    upload: impl FnOnce(&arpg_assets::StaticMesh) -> Result<T, String>,
) -> Result<(), String> {
    *slot = Some(load_static(path, upload)?);
    Ok(())
}

fn load_static<T>(
    path: std::path::PathBuf,
    upload: impl FnOnce(&arpg_assets::StaticMesh) -> Result<T, String>,
) -> Result<AssetPreview<T>, String> {
    let source = read_glb(&path)?;
    let mesh =
        arpg_assets::import_glb(&source).map_err(|error| format!("{}: {error}", path.display()))?;
    let vertex_count = mesh.vertex_count();
    let index_count = mesh.index_count();
    let texture_width = mesh.base_color_texture().width();
    let texture_height = mesh.base_color_texture().height();
    let gpu = upload(&mesh).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(AssetPreview {
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
    let gpu = upload(&asset).map_err(|error| format!("{}: {error}", path.display()))?;
    let pose = asset.bind_pose();
    *slot = Some(LoadedCharacter {
        path,
        asset,
        pose,
        clips,
        sampled_role: PresentationRole::Idle,
        sampled_seconds: 0.0,
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
    let gpu = upload(&asset).map_err(|error| format!("{}: {error}", path.display()))?;
    *slot = Some(LoadedHorde::new(path, asset, clips, gpu));
    Ok(())
}

#[cfg(test)]
mod tests;
