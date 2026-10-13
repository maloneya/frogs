//! Contracts for asset selection and simulation-driven presentation.

use super::*;
use arpg_core::{AimDir, Intent, MoveDir};
use arpg_game::Game;
use arpg_sim::Accumulator;

fn character_fixture_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/fixtures/blender-bind-pose.glb")
}

fn multipart_fixture_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/fixtures/blender-multipart.glb")
}

#[test]
fn startup_assets_support_their_presentation_roles() {
    let mut player = None;
    let mut horde = None;
    let player_path = default_character_path();
    let horde_path = default_horde_path();
    assert!(player_path.is_absolute());
    assert!(horde_path.is_absolute());
    replace_character(&mut player, player_path.clone(), |_| Ok(())).unwrap();
    replace_horde(&mut horde, horde_path.clone(), |_| Ok(())).unwrap();
    assert!(horde.as_ref().unwrap().asset.shadow_mesh().is_some());
    assert_eq!(player.unwrap().path, player_path);
    assert_eq!(horde.unwrap().path, horde_path);
}

#[test]
fn horde_sidecar_is_atomic_on_refusal_and_is_removed_when_the_asset_changes() {
    let mut slot = None;
    replace_horde(&mut slot, default_horde_path(), |_| Ok(())).unwrap();
    let mut before = Report::default();
    report_horde_preview(slot.as_ref(), &mut before);
    let before = before.finish();
    let dir = std::env::temp_dir().join(format!("arpg-shadow-admission-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("enemy.glb");
    std::fs::write(&path, include_bytes!("../../../../assets/characters/basic-enemy/basic-enemy.glb")).unwrap();
    let sidecar = path.with_extension("shadow.glb");
    for bytes in [&b"not a GLB"[..], &include_bytes!("../../../../assets/fixtures/blender-bind-pose.glb")[..]] {
        std::fs::write(&sidecar, bytes).unwrap();
        let mut uploaded = false;
        assert!(replace_horde(&mut slot, path.clone(), |_| { uploaded = true; Ok(()) }).is_err());
        assert!(!uploaded);
        let mut after = Report::default();
        report_horde_preview(slot.as_ref(), &mut after);
        assert_eq!(before, after.finish());
    }
    let oversized = std::fs::File::create(&sidecar).unwrap();
    oversized.set_len(arpg_assets::MAX_GLB_BYTES as u64 + 1).unwrap();
    drop(oversized);
    let mut uploaded = false;
    assert!(replace_horde(&mut slot, path.clone(), |_| { uploaded = true; Ok(()) })
        .unwrap_err().contains("GLB exceeds asset capacity"));
    assert!(!uploaded);
    let mut after = Report::default();
    report_horde_preview(slot.as_ref(), &mut after);
    assert_eq!(before, after.finish());
    std::fs::remove_file(&sidecar).unwrap();
    replace_horde(&mut slot, path, |_| Ok(())).unwrap();
    assert!(slot.as_ref().unwrap().asset.shadow_mesh().is_none());
    replace_horde(&mut slot, default_horde_path(), |_| Ok(())).unwrap();
    assert!(slot.as_ref().unwrap().asset.shadow_mesh().is_some());
    slot.as_mut().unwrap().clear();
    assert_eq!(slot.as_ref().unwrap().instance_count(), 0);
    std::fs::remove_dir_all(dir).unwrap();
}

fn loaded_character_from(path: std::path::PathBuf) -> LoadedCharacter<()> {
    let mut slot = None;
    replace_character(&mut slot, path, |_| Ok(())).unwrap();
    slot.unwrap()
}

fn weapon_tip_at(
    character: &mut LoadedCharacter<()>,
    profile: AttackProfile,
    phase: AttackPhase,
    elapsed: u32,
    alpha: Alpha,
    resolved: arpg_sim::ResolvedAttack,
) -> Vec3 {
    let clip = character.clips.attack(profile);
    let duration = character.asset.clip(clip).unwrap().duration_seconds();
    let seconds = attack_clip_time(phase, elapsed, alpha, resolved, duration);
    assert!(character.asset.sample(clip, seconds, &mut character.pose));
    let tip = character.asset.joint_named("BeamOrigin").unwrap();
    character.asset.joint_transform(&character.pose, tip).unwrap().transform_point3(Vec3::ZERO)
}

fn loaded_character(path: &str) -> LoadedCharacter<()> {
    let mut character = loaded_character_from(default_character_path());
    character.path = path.into();
    character
}

fn loaded_horde(path: &str) -> LoadedHorde<()> {
    let source = std::fs::read(character_fixture_path()).unwrap();
    let asset = arpg_assets::import_character_glb(&source).unwrap();
    let clips = HordeClips::resolve(&asset).unwrap();
    LoadedHorde::new(path.into(), asset, clips, ())
}

#[test]
fn character_replacement_is_atomic_across_import_and_upload_failure() {
    let mut slot = Some(loaded_character("old.glb"));
    let missing = std::env::temp_dir().join("arpg-character-that-does-not-exist.glb");
    assert!(replace_character(&mut slot, missing, |_| Ok(())).is_err());
    assert_eq!(slot.as_ref().unwrap().path, std::path::Path::new("old.glb"));

    assert!(
        replace_character(&mut slot, default_character_path(), |_| {
            Err("GPU refused it".into())
        })
        .is_err()
    );
    assert_eq!(slot.as_ref().unwrap().path, std::path::Path::new("old.glb"));

    replace_character(&mut slot, default_character_path(), |_| Ok(())).unwrap();
    let current = slot.as_ref().unwrap();
    assert_eq!(
        (current.asset.vertex_count(), current.asset.index_count()),
        (12324, 70722)
    );
    assert_eq!(
        (current.asset.node_count(), current.asset.joint_count()),
        (28, 26)
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
        r#"{"active":false,"path":"","vertices":0,"indices":0,"part_count":0,"material_count":0,"image_count":0,"texture_bytes":0,"parts":{},"materials":{},"images":{},"nodes":0,"joints":0,"clips":0,"role":"","clip":"","clip_duration":0.0000,"channels":0,"sample_seconds":0.0000,"locomotion_base":"","locomotion_overlay":"","locomotion_weight":0.0000,"channel_weight":0.0000}"#
    );

    let mut preview = loaded_character("assets/character.glb");
    let player = Game::empty().player_presentation(Alpha::ONE);
    preview.sample(player, Alpha::ONE, 0.25);
    let mut active = Report::default();
    report_character_preview(Some(&preview), &mut active);
    assert_eq!(
        active.finish(),
        r#"{"active":true,"path":"assets/character.glb","vertices":12324,"indices":70722,"part_count":1,"material_count":1,"image_count":1,"texture_bytes":349524,"parts":{"0":{"node":"Swordsman","primitive":0,"material":0,"vertices":12324,"indices":70722}},"materials":{"0":{"image":0}},"images":{"0":{"width":256,"height":256}},"nodes":28,"joints":26,"clips":8,"role":"idle","clip":"Idle","clip_duration":1.0000,"channels":52,"sample_seconds":0.2500,"locomotion_base":"Idle","locomotion_overlay":"Idle","locomotion_weight":0.0000,"channel_weight":0.0000}"#
    );
    assert_eq!(preview.draw_count(), 1);
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

    replace_horde(&mut slot, multipart_fixture_path(), |_| Ok(())).unwrap();
    let mut game = Game::empty();
    for index in 0..32 {
        game.place(glam::Vec2::new(4.0 + index as f32, 4.0), arpg_game::ActorDefinition::passive_hostile()).unwrap();
    }
    let current = slot.as_mut().unwrap();
    current.rebuild(game.character_presentations(Alpha::ONE), 0.25, |_| 0.0, |_| None, std::iter::empty(), Alpha::ONE);
    assert_eq!(current.instance_count(), 32);
    assert!((1..=HORDE_PHASES_PER_ROLE).contains(&current.occupied_buckets()));

    let mut active = Report::default();
    report_horde_preview(Some(current), &mut active);
    let report = active.finish();
    assert_eq!(current.asset.parts().len(), 10);
    assert_eq!(current.draw_count(), current.occupied_buckets() * 10);
    assert!(report.contains(r#""part_count":10,"material_count":3,"image_count":3,"texture_bytes":1020"#));
    for (index, part) in current.asset.parts().iter().enumerate() {
        assert!(report.contains(&format!("\"{index}\":{{\"node\":\"{}\",\"primitive\":{},\"material\":{},\"vertices\":{},\"indices\":{}}}",
            part.node_name(), part.primitive_index(), part.material_index(), part.vertices().len(), part.indices().end - part.indices().start)));
    }
    assert!(report.contains(r#""active":true"#));
    assert!(report.contains(r#""idle_clip":"Idle""#));
    assert!(report.contains(r#""run_clip":"Run""#));
    assert!(report.contains(r#""pose_buckets":12"#));
    assert!(report.contains(r#""instances":32"#));
    assert!(report.contains(r#""idle_instances":32"#));
    assert!(report.contains(r#""run_instances":0"#));
}

#[test]
fn stable_identity_selects_bounded_reused_horde_pose_buckets() {
    let mut horde = loaded_horde("horde.glb");
    let mut game = Game::empty();
    for index in 0..32 {
        game.place(glam::Vec2::new(4.0 + index as f32, 4.0), arpg_game::ActorDefinition::passive_hostile()).unwrap();
    }
    let enemies: Vec<_> = game.character_presentations(Alpha::ONE).collect();
    let removed = enemies[0].id();
    let survivor = enemies[1];
    let survivor_bucket = LoadedHorde::<()>::bucket(survivor).0;

    horde.rebuild(enemies.into_iter(), 0.0, |_| 0.0, |_| None, std::iter::empty(), Alpha::ONE);
    let capacities = horde
        .buckets
        .each_ref()
        .map(|bucket| bucket.instances.capacity());
    horde.rebuild(game.character_presentations(Alpha::ONE), 0.1, |_| 0.0, |_| None, std::iter::empty(), Alpha::ONE);
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
        .character_presentations(Alpha::ONE)
        .find(|enemy| enemy.id() == survivor.id())
        .unwrap();
    assert_eq!(
        LoadedHorde::<()>::bucket(survivor_after_swap).0,
        survivor_bucket
    );

    let survivors: Vec<_> = game.character_presentations(Alpha::ONE).map(|enemy| enemy.id()).collect();
    for id in survivors {
        game.assign_behavior(id, arpg_game::ActorDefinition::hostile()).unwrap();
    }
    let dt = Accumulator::default()
        .pending(arpg_sim::Dt::SECS)
        .next()
        .unwrap();
    game.step(dt, Intent::new(MoveDir::NONE, AimDir::FORWARD, false));
    horde.rebuild(game.character_presentations(Alpha::ONE), 0.2, |_| 0.0, |_| None, std::iter::empty(), Alpha::ONE);
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
    assert!(horde.occupied_buckets() <= HORDE_PHASES_PER_ROLE);
}

#[test]
fn stopped_horde_members_keep_heading_and_dead_identities_are_forgotten() {
    let mut horde = loaded_horde("horde.glb");
    let mut game = Game::empty();
    let id = game.place(glam::Vec2::new(4.0, 4.0), arpg_game::ActorDefinition::passive_hostile()).unwrap();
    assert!(game.apply_impulse(id, arpg_sim::Impulse::try_from((1.0, 0.0)).unwrap()));
    let dt = Accumulator::default()
        .pending(arpg_sim::Dt::SECS)
        .next()
        .unwrap();
    game.step(dt, Intent::new(MoveDir::NONE, AimDir::FORWARD, false));
    horde.rebuild(game.character_presentations(Alpha::ONE), 0.0, |_| 0.0, |_| None, std::iter::empty(), Alpha::ONE);
    let moving_yaw = horde
        .buckets
        .iter()
        .flat_map(|bucket| &bucket.instances)
        .next()
        .unwrap()
        .yaw();
    assert_ne!(moving_yaw, 0.0);

    // Carried motion decays to rest without changing this body's identity.
    for _ in 0..100 {
        game.step(dt, Intent::new(MoveDir::NONE, AimDir::FORWARD, false));
    }
    assert_eq!(game.character_presentations(Alpha::ONE).next().unwrap().displacement(), glam::Vec2::ZERO);
    horde.rebuild(game.character_presentations(Alpha::ONE), 0.1, |_| 0.0, |_| None, std::iter::empty(), Alpha::ONE);
    let stopped_yaw = horde
        .buckets
        .iter()
        .flat_map(|bucket| &bucket.instances)
        .next()
        .unwrap()
        .yaw();
    assert_eq!(stopped_yaw, moving_yaw);

    let dead = game.character_presentations(Alpha::ONE).next().unwrap().id();
    assert!(game.despawn_body(dead));
    horde.rebuild(game.character_presentations(Alpha::ONE), 0.2, |_| 0.0, |_| None, std::iter::empty(), Alpha::ONE);
    assert!(horde.headings.is_empty());
}

#[test]
fn resetting_horde_presentation_cannot_reuse_a_previous_runs_heading() {
    let mut horde = loaded_horde("horde.glb");
    let mut old = Game::empty();
    let id = old.place(glam::Vec2::new(4.0, 4.0), arpg_game::ActorDefinition::passive_hostile()).unwrap();
    old.assign_behavior(id, arpg_game::ActorDefinition::hostile()).unwrap();
    let dt = Accumulator::default()
        .pending(arpg_sim::Dt::SECS)
        .next()
        .unwrap();
    old.step(dt, Intent::new(MoveDir::NONE, AimDir::FORWARD, false));
    horde.rebuild(old.character_presentations(Alpha::ONE), 1.0, |_| 0.0, |_| None, std::iter::empty(), Alpha::ONE);
    let old_id = old.character_presentations(Alpha::ONE).next().unwrap().id();
    assert_ne!(horde.headings[&old_id].yaw, 0.0);

    let mut fresh = Game::empty();
    fresh.place(glam::Vec2::new(4.0, 4.0), arpg_game::ActorDefinition::passive_hostile()).unwrap();
    assert_eq!(
        fresh.character_presentations(Alpha::ONE).next().unwrap().id(),
        old_id
    );
    horde.clear();
    assert_eq!(horde.instance_count(), 0);
    assert_eq!(horde.occupied_buckets(), 0);
    horde.rebuild(fresh.character_presentations(Alpha::ONE), 0.0, |_| 0.0, |_| None, std::iter::empty(), Alpha::ONE);
    assert_eq!(horde.headings[&old_id].yaw, 0.0);
}

#[test]
fn every_attack_profile_resolves_to_its_own_presentation_clip() {
    let character = loaded_character("character.glb");
    let expected = [
        "AttackCleave",
        "AttackSlam",
        "Disintegrate",
    ];
    for (profile, expected) in AttackProfile::ALL.into_iter().zip(expected) {
        let id = character.clips.for_role(PresentationRole::Attack(profile));
        assert_eq!(character.asset.clip(id).unwrap().name(), expected);
    }
}

#[test]
fn authoritative_player_facts_drive_direct_roles() {
    let mut character = loaded_character("character.glb");
    let mut game = Game::empty();
    let idle = game.player_presentation(Alpha::ONE);
    character.sample(idle, Alpha::ONE, 0.0);
    assert_eq!(character.sampled_role, PresentationRole::Idle);

    let dt = Accumulator::default()
        .pending(arpg_sim::Dt::SECS)
        .next()
        .unwrap();
    game.step(dt, Intent::new(MoveDir::new(Vec3::X), AimDir::new(Vec3::X).unwrap(), false));
    let running = game.player_presentation(Alpha::ONE);
    character.sample(running, Alpha::ONE, 1.0 / 60.0);
    assert_eq!(character.sampled_role, PresentationRole::Run);

    game.set_attack_profile(AttackProfile::Slam);
    let dt = Accumulator::default()
        .pending(arpg_sim::Dt::SECS)
        .next()
        .unwrap();
    game.step(dt, Intent::new(MoveDir::NONE, AimDir::FORWARD, true));
    let attacking = game.player_presentation(Alpha::ONE);
    character.sample(attacking, Alpha::ONE, 0.25);
    assert_eq!(
        character.sampled_role,
        PresentationRole::Attack(AttackProfile::Slam)
    );
    let clip = character.clips.for_role(character.sampled_role);
    assert_eq!(character.asset.clip(clip).unwrap().name(), "AttackSlam");
}

#[test]
fn recovery_tuning_cannot_move_an_active_phase_sample() {
    let mut character = loaded_character_from(default_character_path());
    for profile in AttackProfile::ALL {
        let Some(base) = profile.resolve(arpg_sim::Tuning::default()) else { continue };
        let longer = arpg_sim::ResolvedAttack::try_new(
            base.startup(),
            base.active(),
            arpg_sim::RecoveryTicks::try_from(base.recovery().get() + 20).unwrap(),
            arpg_sim::AttackShape::try_new(
                base.start(),
                base.start_radius(),
                base.end(),
                base.end_radius(),
            )
            .unwrap(),
            base.knockback(),
            base.damage(),
        )
        .unwrap();
        // The complete wind-up and strike must depict the same motion even
        // when this swing takes longer to recover afterward.
        for elapsed in 0..base.startup() + base.active() {
            let phase = if elapsed < base.startup() {
                AttackPhase::Startup
            } else {
                AttackPhase::Active
            };
            for alpha in [Alpha::ZERO, Alpha::ONE] {
                let expected = weapon_tip_at(&mut character, profile, phase, elapsed, alpha, base);
                let actual = weapon_tip_at(&mut character, profile, phase, elapsed, alpha, longer);
                assert_eq!(
                    actual, expected,
                    "{profile:?}: recovery moved the strike at {elapsed}"
                );
            }
        }
    }
}

#[test]
fn checked_in_attack_poses_put_the_strike_inside_the_active_phase() {
    let mut character = loaded_character_from(default_character_path());
    let tip = |character: &mut LoadedCharacter<()>, profile, phase, elapsed, alpha| {
        weapon_tip_at(character, profile, phase, elapsed, alpha, profile.resolve(arpg_sim::Tuning::default()).unwrap())
    };
    for profile in AttackProfile::ALL {
        let Some(resolved) = profile.resolve(arpg_sim::Tuning::default()) else { continue };
        let ready = tip(
            &mut character,
            profile,
            AttackPhase::Startup,
            0,
            Alpha::ZERO,
        );
        let wound = tip(
            &mut character,
            profile,
            AttackPhase::Startup,
            resolved.startup() - 1,
            Alpha::ONE,
        );
        let active_start = tip(
            &mut character,
            profile,
            AttackPhase::Active,
            resolved.startup(),
            Alpha::ZERO,
        );
        let struck = tip(
            &mut character,
            profile,
            AttackPhase::Recovery,
            resolved.startup() + resolved.active(),
            Alpha::ZERO,
        );
        let recovered = tip(
            &mut character,
            profile,
            AttackPhase::Recovery,
            resolved.startup() + resolved.active() + resolved.recovery().get() - 1,
            Alpha::ONE,
        );
        assert!(active_start.abs_diff_eq(wound, 1.0e-4));
        assert!(ready.distance(wound) > 0.12, "{profile:?} never prepares");
        assert!(
            wound.distance(struck) > 0.20,
            "{profile:?} has no active strike"
        );
        assert!(
            recovered.distance(ready) < 1.0e-4,
            "{profile:?} does not recover"
        );
    }

    let profile = AttackProfile::Cleave;
    let resolved = profile.resolve(arpg_sim::Tuning::default()).unwrap();
    let wound = tip(
        &mut character, profile, AttackPhase::Active,
        resolved.startup(), Alpha::ZERO,
    );
    let struck = tip(
        &mut character, profile, AttackPhase::Recovery,
        resolved.startup() + resolved.active(), Alpha::ZERO,
    );
    assert!(
        wound.x < -0.15 && struck.x > 0.55 && struck.x - wound.x > 0.9,
        "cleave must sweep left to right: {wound:?} -> {struck:?}"
    );
    let mut previous = wound;
    for tick in 0..resolved.active() {
        for alpha in [Alpha::ZERO, Alpha::ONE] {
            let current = tip(
                &mut character, profile, AttackPhase::Active,
                resolved.startup() + tick, alpha,
            );
            assert!(current.z > 0.0, "cleave went behind the player: {current:?}");
            assert!(current.x >= previous.x - 1.0e-4, "cleave reversed: {previous:?} -> {current:?}");
            previous = current;
        }
    }

}

#[test]
fn character_time_is_the_continuous_tick_plus_alpha_clock() {
    assert_eq!(presentation_seconds(60, Alpha::ZERO), 1.0);
    assert_eq!(presentation_seconds(59, Alpha::ONE), 1.0);
}

#[test]
fn world_assets_follow_prop_membership_and_authored_geometry() {
    let mut world = WorldAssets::load(|mesh| {
        if mesh.vertex_count() == 4 {
            assert_eq!(mesh.index_count(), 6);
            for vertex in mesh.vertices() {
                assert!(vertex.position().y.abs() < 1e-6);
                assert!((vertex.position().x.abs() - 0.5).abs() < 1e-6);
                assert!((vertex.position().z.abs() - 0.5).abs() < 1e-6);
            }
            let texture = mesh.base_color_texture();
            assert_eq!((texture.width(), texture.height()), (256, 256));
            assert!(texture.rgba8().as_chunks::<4>().0.iter().all(|pixel| pixel[3] == 255));
            let uv_min = mesh.vertices().iter().map(|vertex| vertex.uv())
                .fold(glam::Vec2::splat(f32::INFINITY), glam::Vec2::min);
            let uv_max = mesh.vertices().iter().map(|vertex| vertex.uv())
                .fold(glam::Vec2::splat(f32::NEG_INFINITY), glam::Vec2::max);
            assert_eq!(uv_max - uv_min, glam::Vec2::splat(96.0));
        }
        Ok(())
    }).unwrap();
    assert_eq!(world.ground.vertex_count, 4);
    assert_eq!(world.ground.index_count, 6);
    assert!(world.prop.vertex_count > 24, "the prop is authored bevelled geometry");
    assert_eq!(world.ground_instance[0].pos(), Vec3::ZERO);
    let mut game = Game::empty();
    game.place(glam::Vec2::new(4.0, 0.0), arpg_game::ActorDefinition::passive_hostile()).unwrap();
    let id = game.place(glam::Vec2::new(0.0, 1.0), arpg_game::ActorDefinition::prop().interactive()).unwrap();
    let before = game.hash();
    world.rebuild(game.prop_presentations(Alpha::ONE), |id| game.chest_state_for_body(id));
    assert_eq!(game.hash(), before, "presentation cannot modify gameplay");
    assert_eq!(world.instance_count(), 2);
    assert_eq!(world.draw_count(), 2);
    assert_eq!(world.props.as_slice()[0].pos(), Vec3::new(0.0, 0.0, 1.0));
    assert_eq!(game.prop_presentations(Alpha::ONE).next().unwrap().interaction(), Some(arpg_sim::InteractionState::Ready));
    let mut accumulator = Accumulator::default();
    game.step(accumulator.pending(arpg_sim::Dt::SECS).next().unwrap(), Intent::new(MoveDir::NONE, AimDir::FORWARD, false).with_interact(true));
    let snapshot = game.prop_presentations(Alpha::ONE).next().unwrap();
    assert_eq!(snapshot.id(), id);
    assert_eq!(snapshot.interaction(), Some(arpg_sim::InteractionState::Activated));
    world.rebuild(game.prop_presentations(Alpha::ONE), |id| game.chest_state_for_body(id));
    world.rebuild(Game::empty().prop_presentations(Alpha::ONE), |_| None);
    assert_eq!(world.instance_count(), 1, "old props must not survive a fresh world");
    assert_eq!(world.draw_count(), 1);
}

#[test]
fn panel_presentation_follows_the_hinge_and_survives_breach_without_a_prop_duplicate() {
    let mut scene = arpg_game::GameScene::default();
    scene.engine.bodies.push(arpg_game::Placed { pos: (0.0, 4.0), what: arpg_game::ActorDefinition {
        physical: arpg_sim::Template::BODY, ..arpg_game::ActorDefinition::prop()
    } });
    scene.doors.push(arpg_game::DoorSpec { body: 0,
        panel: arpg_sim::PanelConfig::new(2.0, 0.2, 0.0, -1.5, 1.5, 0.1).unwrap() });
    let mut game = Game::from_scene(&scene).unwrap();
    let owner = game.scene_instances().next().unwrap().0;
    let id = game.scene_bodies(owner).unwrap()[0];
    let mut view = WorldAssets::load(|_| Ok(())).unwrap();
    view.rebuild(game.prop_presentations(Alpha::ONE), |_| None);
    view.rebuild_panels(game.panel_presentations(Alpha::ONE));
    assert!(view.props.as_slice().is_empty());
    assert_eq!(view.panels.len(), 1);
    assert_eq!(view.panels[0].pos(), Vec3::new(0.0, 0.0, 4.0));
    assert_eq!(view.instance_count(), 2);
    assert!(game.apply_impulse(id, arpg_sim::Impulse::try_from((0.0, -1.0)).unwrap()));
    game.step(Accumulator::default().pending(arpg_sim::Dt::SECS).next().unwrap(),
        Intent::new(MoveDir::NONE, AimDir::FORWARD, false));
    assert_eq!(game.panel(id).unwrap().latch(), arpg_sim::LatchState::Broken);
    let before = game.hash();
    let pose = game.panel_presentations(Alpha::ONE).next().unwrap();
    view.rebuild_panels(game.panel_presentations(Alpha::ONE));
    assert_eq!(view.panels[0].pos(), Vec3::new(pose.centre().x, 0.0, pose.centre().y));
    assert!(pose.angle() > 0.0);
    assert_eq!(game.hash(), before);
    assert!(game.evict_scene(owner));
    view.rebuild_panels(game.panel_presentations(Alpha::ONE));
    assert_eq!(view.instance_count(), 1);
}

#[test]
fn moving_attacks_keep_directional_legs_and_committed_weapon_pose() {
    for (direction, clip_name) in [(Vec3::Z, "Run"), (-Vec3::Z, "RunBackward"), (Vec3::X, "RunRight"), (-Vec3::X, "RunLeft")] {
        let mut character = loaded_character_from(default_character_path());
        let mut game = Game::empty();
        for tick in 0..=6 {
            let dt = Accumulator::default().pending(arpg_sim::Dt::SECS).next().unwrap();
            game.step(dt, Intent::new(MoveDir::new(direction), AimDir::FORWARD, tick == 0));
        }
        let player = game.player_presentation(Alpha::ONE);
        character.sample(player, Alpha::ONE, 0.1);
        let mut expected = character.asset.bind_pose();
        character.asset.sample(character.asset.clip_named(clip_name).unwrap(), 0.1, &mut expected);
        for name in ["Root", "LeftLeg", "RightLeg"] {
            let joint = character.asset.joint_named(name).unwrap();
            assert!(character.asset.joint_transform(&character.pose, joint).unwrap()
                .abs_diff_eq(character.asset.joint_transform(&expected, joint).unwrap(), 1e-5), "{clip_name} {name}");
        }
        character.asset.sample(character.clips.attack(AttackProfile::Cleave), character.sampled_seconds, &mut expected);
        let root = character.asset.joint_named("Spine").unwrap();
        // Turning hips keep their gait while the Spine boundary preserves attack aim.
        let weapon = character.asset.joint_named("Weapon").unwrap();
        let relative = |pose: &arpg_assets::CharacterPose| character.asset.joint_transform(pose, root).unwrap().inverse()
            * character.asset.joint_transform(pose, weapon).unwrap();
        assert!(relative(&character.pose).abs_diff_eq(relative(&expected), 1e-5));
        let rotation = |pose| character.asset.joint_transform(pose, weapon).unwrap().to_scale_rotation_translation().1;
        assert!(rotation(&character.pose).abs_diff_eq(rotation(&expected), 1e-5),
            "{clip_name} hips turned the attack away from its committed aim");
    }
}

#[test]
fn locomotion_reads_travel_relative_to_aim_and_keeps_gameplay_unchanged() {
    for aim in [AimDir::FORWARD, AimDir::new(Vec3::X).unwrap()] {
        let facing = aim.as_vec3().x.atan2(aim.as_vec3().z);
        for local in [Vec3::Z, -Vec3::Z, Vec3::X, -Vec3::X, Vec3::new(1.0, 0.0, 1.0), Vec3::new(-1.0, 0.0, -1.0)] {
            let movement = MoveDir::new(glam::Quat::from_rotation_y(facing) * local);
            let mut game = Game::empty();
            let mut character = loaded_character_from(default_character_path());
            let dt = Accumulator::default().pending(arpg_sim::Dt::SECS).next().unwrap();
            // Facing keeps the existing 14 rad/s turn limit; seven ticks settle a quarter turn.
            for _ in 0..7 { game.step(dt, Intent::new(MoveDir::NONE, aim, false)); }
            for _ in 0..12 {
                game.step(dt, Intent::new(movement, aim, false));
                let player = game.player_presentation(Alpha::ONE);
                let hash = game.hash();
                character.sample(player, Alpha::ONE, presentation_seconds(game.tick(), Alpha::ONE));
                assert_eq!(character.sampled_role, PresentationRole::Run);
                assert!((player.facing() - facing).abs() < 1e-5, "visual travel cannot replace mouse aim");
                let expected = local.x.abs() / (local.x.abs() + local.z.abs());
                assert!((character.locomotion.weight - expected).abs() < 1e-4);
                if expected < 1.0 {
                    assert_eq!(character.asset.clip(character.locomotion.base).unwrap().name(),
                        if local.z >= 0.0 { "Run" } else { "RunBackward" });
                }
                if expected > 0.0 {
                    assert_eq!(character.asset.clip(character.locomotion.overlay).unwrap().name(),
                        if local.x >= 0.0 { "RunRight" } else { "RunLeft" });
                }
                assert_eq!(game.hash(), hash);
            }
            game.start_scene(&arpg_game::GameScene::default()).unwrap();
            character.reset();
            let player = game.player_presentation(Alpha::ONE);
            character.sample(player, Alpha::ONE, 0.0);
            assert_eq!(character.sampled_role, PresentationRole::Idle);
        }
    }
}

#[test]
fn chest_presentation_follows_opening_and_eviction_without_drawing_a_plinth() {
    use arpg_game::{ChestSpec, EncounterAction, EncounterObjective, EncounterSpec, EncounterStart, DefeatObjective};
    let mut scene = arpg_game::GameScene::default();
    scene.engine.bodies = vec![
        arpg_game::Placed { pos: (1.2, 0.0), what: arpg_game::ActorDefinition::prop().interactive() },
        arpg_game::Placed { pos: (-1.2, 0.0), what: arpg_game::ActorDefinition::prop() },
    ];
    scene.engine.sources = vec![arpg_game::SourceSpec { pos: (20.0,20.0), radius: 0.0, every: 100,
        when: arpg_sim::Condition::Always, what: arpg_game::ActorDefinition::passive_hostile(), enabled: false }];
    scene.chests = vec![ChestSpec { body: 1 }];
    // A one-tick timeout exercises the same unlock effect without a combat fixture.
    scene.encounters = vec![EncounterSpec { name: "View test".into(), start: EncounterStart::Activated(0),
        objective: EncounterObjective::Defeat(DefeatObjective { sources: vec![0], count: 1.try_into().unwrap(), within_ticks: 1.try_into().unwrap() }),
        on_start: vec![], on_success: vec![], on_failure: vec![EncounterAction::UnlockChest(0)] }];
    let mut game = Game::from_scene(&scene).unwrap();
    let owner = game.scene_instances().next().unwrap().0;
    let mut view = WorldAssets::load(|_| Ok(())).unwrap();
    view.rebuild(game.prop_presentations(Alpha::ONE), |id| game.chest_state_for_body(id));
    assert_eq!(view.props.as_slice().len(), 1);
    assert_eq!(view.closed_chests.len(), 1);
    assert!(view.open_chests.is_empty());
    for interact in [true, false, true, false] {
        game.step(Accumulator::default().pending(arpg_sim::Dt::SECS).next().unwrap(),
            Intent::new(MoveDir::NONE, AimDir::FORWARD, false).with_interact(interact));
    }
    let hash = game.hash();
    view.rebuild(game.prop_presentations(Alpha::ONE), |id| game.chest_state_for_body(id));
    assert_eq!(game.hash(), hash);
    assert!(view.closed_chests.is_empty());
    assert_eq!(view.open_chests.len(), 1);
    assert_eq!(view.open_chests[0].pos(), Vec3::new(-1.2, 0.0, 0.0));
    assert!(game.evict_scene(owner));
    view.rebuild(game.prop_presentations(Alpha::ONE), |id| game.chest_state_for_body(id));
    assert_eq!(view.instance_count(), 1);
}

#[test]
fn contact_flash_reaches_instances_without_changing_pose_buckets() {
    let mut game = Game::empty();
    let id = game.place(glam::Vec2::new(-1.8, 1.2), arpg_game::ActorDefinition::passive_hostile()).unwrap();
    let dt = Accumulator::default().pending(arpg_sim::Dt::SECS).next().unwrap();
    let mut horde = loaded_horde("horde.glb");
    let mut feedback = crate::enemy_feedback::EnemyFeedback::default();
    // Skip drawing across the whole swing: the first visible frame still marks damage.
    for tick in 0..12 {
        game.step(dt, Intent::new(MoveDir::NONE, AimDir::FORWARD, tick == 0));
    }
    feedback.observe(&game, |_, _| {});
    horde.rebuild(game.character_presentations(Alpha::ZERO), 0.2,
        |id| feedback.flash(id, game.tick(), Alpha::ZERO), |_| None, std::iter::empty(), Alpha::ONE);
    let instance = horde.buckets.iter().flat_map(|bucket| &bucket.instances).next().unwrap();
    assert_eq!(game.health(id), Some(1));
    assert_eq!(instance.white_mix(), 1.0);
    assert_eq!(horde.instance_count(), 1);
    assert_eq!(horde.occupied_buckets(), 1);
}

#[test]
fn wall_instances_follow_authoritative_footprints_and_scene_lifetime() {
    let mut view = WorldAssets::load(|_| Ok(())).unwrap();
    let wall = arpg_sim::Wall::new(glam::Vec2::new(3.0, 4.0), glam::Vec2::new(6.0, 0.2), 0.7).unwrap();
    let mut game = Game::from_scene(&arpg_game::Scene { walls: vec![wall], ..Default::default() }.into()).unwrap();
    let owner = game.walls().next().unwrap().0;
    view.rebuild_walls(game.walls());
    assert_eq!(view.walls.len(), 1);
    assert_eq!(view.walls[0].pos(), Vec3::new(3.0, 0.0, 4.0));
    assert_eq!(view.walls[0].yaw(), wall.yaw());
    assert_eq!(view.instance_count(), 2);
    assert_eq!(view.draw_count(), 2);
    assert!(game.evict_scene(owner));
    view.rebuild_walls(game.walls());
    assert!(view.walls.is_empty());
    assert_eq!(view.draw_count(), 1);
    game.restart().unwrap();
    view.rebuild_walls(game.walls());
    assert_eq!(view.walls.len(), 1);
}

#[test]
fn missing_required_joint_refuses_replacement_before_upload() {
    let mut source = std::fs::read(default_character_path()).unwrap();
    let offset = source.windows(b"Spine".len()).position(|w| w == b"Spine").unwrap();
    source[offset..offset + b"Spine".len()].copy_from_slice(b"Wrong");
    let path = std::env::temp_dir().join(format!("arpg-missing-spine-{}.glb", std::process::id()));
    std::fs::write(&path, source).unwrap();
    let mut slot = Some(loaded_character("current.glb"));
    let mut uploaded = false;
    let result = replace_character(&mut slot, path.clone(), |_| { uploaded = true; Ok(()) });
    std::fs::remove_file(path).unwrap();
    assert!(result.unwrap_err().contains("missing mask joint"));
    assert!(!uploaded);
    assert_eq!(slot.unwrap().path, std::path::Path::new("current.glb"));
}

#[test]
fn interrupted_upper_hierarchy_refuses_replacement_before_upload() {
    let source = std::fs::read(default_character_path()).unwrap();
    let json_len = u32::from_le_bytes(source[12..16].try_into().unwrap()) as usize;
    let mut document: serde_json::Value = serde_json::from_slice(&source[20..20 + json_len]).unwrap();
    let nodes = document["nodes"].as_array_mut().unwrap();
    let spine = nodes.iter().position(|node| node["name"] == "Spine").unwrap();
    let chest = nodes.iter().position(|node| node["name"] == "Chest").unwrap();
    let bridge = nodes.len();
    let child = nodes[spine]["children"].as_array_mut().unwrap().iter_mut()
        .find(|child| child.as_u64() == Some(chest as u64)).unwrap();
    *child = serde_json::json!(bridge);
    nodes.push(serde_json::json!({ "name": "UpperSpineBridge", "children": [chest] }));
    let mut json = serde_json::to_vec(&document).unwrap();
    while !json.len().is_multiple_of(4) { json.push(b' '); }
    let mut candidate = source[..20].to_vec();
    candidate[12..16].copy_from_slice(&(json.len() as u32).to_le_bytes());
    candidate.extend_from_slice(&json);
    candidate.extend_from_slice(&source[20 + json_len..]);
    let length = candidate.len() as u32;
    candidate[8..12].copy_from_slice(&length.to_le_bytes());
    // A static identity helper is valid for the importer, but splits the layer.
    arpg_assets::import_character_glb(&candidate).unwrap();
    let path = std::env::temp_dir().join(format!("arpg-interrupted-upper-{}.glb", std::process::id()));
    std::fs::write(&path, candidate).unwrap();
    let mut slot = Some(loaded_character("current.glb"));
    let mut uploaded = false;
    let result = replace_character(&mut slot, path.clone(), |_| { uploaded = true; Ok(()) });
    std::fs::remove_file(path).unwrap();
    assert!(result.unwrap_err().contains("unmasked parent"));
    assert!(!uploaded);
    assert_eq!(slot.unwrap().path, std::path::Path::new("current.glb"));
}

#[test]
fn malformed_last_image_preserves_every_live_part_before_upload() {
    let mut source = std::fs::read(multipart_fixture_path()).unwrap();
    let signature = b"\x89PNG\r\n\x1a\n";
    let offset = source.windows(signature.len()).rposition(|w| w == signature).unwrap();
    source[offset] = 0;
    let path = std::env::temp_dir().join(format!("arpg-invalid-last-image-{}.glb", std::process::id()));
    std::fs::write(&path, source).unwrap();
    let mut slot = None;
    replace_horde(&mut slot, multipart_fixture_path(), |_| Ok(())).unwrap();
    let mut before = Report::default();
    report_horde_preview(slot.as_ref(), &mut before);
    let mut uploaded = false;
    let result = replace_horde(&mut slot, path.clone(), |_| { uploaded = true; Ok(()) });
    std::fs::remove_file(path).unwrap();
    assert!(result.is_err());
    assert!(!uploaded);
    let mut after = Report::default();
    report_horde_preview(slot.as_ref(), &mut after);
    assert_eq!(before.finish(), after.finish());
    assert_eq!(slot.unwrap().asset.parts().len(), 10);
}

#[test]
fn channel_attachment_matches_the_extended_sword_tip_and_follows_world_aim() {
    let mut character = loaded_character("channel.glb");
    let mut game = Game::empty();
    game.set_attack_profile(AttackProfile::Disintegrate);
    let dt = arpg_sim::Accumulator::default().pending(arpg_sim::Dt::SECS).next().unwrap();
    character.sample(game.player_presentation(Alpha::ONE), Alpha::ONE, 0.0);
    for direction in [Vec3::Z, Vec3::X, Vec3::NEG_Z] {
        game.step(dt, arpg_core::Intent::new(arpg_core::MoveDir::NONE,
            arpg_core::AimDir::new(direction).unwrap(), false).with_attack_held(true));
        let player = game.player_presentation(Alpha::ONE);
        character.sample(player, Alpha::ONE, 1.0);
        let weapon = character.asset.joint_named("Weapon").unwrap();
        let socket = character.asset.joint_named("BeamOrigin").unwrap();
        let tip = character.asset.joint_transform(&character.pose, socket).unwrap().transform_point3(Vec3::ZERO);
        let blade = character.asset.joint_transform(&character.pose, weapon).unwrap().transform_vector3(Vec3::Y).normalize();
        assert!(blade.dot(Vec3::Z) > 0.999);
        assert!(tip.z > 1.5 && tip.y > 1.3 && tip.y < 1.6, "extended horizontal sword: {tip:?}");
        let expected = player.ground_position() + glam::Quat::from_rotation_y(player.facing()) * tip;
        assert!(character.beam_emitter(player).tip.abs_diff_eq(expected, 0.0001));
    }
}

#[test]
fn channel_pose_blends_by_elapsed_time_on_entry_release_and_restart() {
    let mut game = Game::empty();
    let mut whole = loaded_character("whole.glb");
    let mut split = loaded_character("split.glb");
    let  reported_weight = |character: &LoadedCharacter<()>| {
        let mut report = Report::default();
        report_character_preview(Some(character), &mut report);
        report.finish()
    };
    let idle = game.player_presentation(Alpha::ONE);
    whole.sample(idle, Alpha::ONE, 0.0);
    assert!(reported_weight(&whole).contains("\"channel_weight\":0.0000"));
    split.sample(idle, Alpha::ONE, 0.0);
    game.set_attack_profile(AttackProfile::Disintegrate);
    let dt = arpg_sim::Accumulator::default().pending(arpg_sim::Dt::SECS).next().unwrap();
    game.step(dt, arpg_core::Intent::new(arpg_core::MoveDir::NONE,
        arpg_core::AimDir::FORWARD, false).with_attack_held(true));
    let channel = game.player_presentation(Alpha::ONE);
    let hash = game.hash();
    whole.sample(channel, Alpha::ONE, 0.045);
    assert!((whole.channel_weight - 0.5).abs() < 0.00001, "half-raised after 45ms"
    );
    assert!(reported_weight(&whole).contains("\"channel_weight\":0.5000"));
    for time in [0.015, 0.030, 0.045] { split.sample(channel, Alpha::ONE, time); }
    assert!((split.channel_weight - whole.channel_weight).abs() < 0.00001);
    assert!(split.beam_emitter(channel).tip.abs_diff_eq(whole.beam_emitter(channel).tip, 0.00001));
    whole.sample(channel, Alpha::ONE, 0.045);
    assert!((whole.channel_weight - 0.5).abs() < 0.00001, "duplicate redraw cannot advance blend");
    assert_eq!(game.hash(), hash, "pose interpolation cannot change damage");
    game.step(dt, arpg_core::Intent::new(arpg_core::MoveDir::NONE, arpg_core::AimDir::FORWARD, false));
    assert!(game.beam().is_none(), "release ends gameplay immediately");
    whole.sample(game.player_presentation(Alpha::ONE), Alpha::ONE, 0.105);
    assert!((whole.channel_weight - 0.25).abs() < 0.00001, "arm eases down after release");

    assert!(reported_weight(&whole).contains("\"channel_weight\":0.2500"));
    whole.reset();
    assert!(reported_weight(&whole).contains("\"channel_weight\":0.0000"));
    assert_eq!(whole.channel_weight, 0.0);
    assert!(whole.sampled_at.is_none());
}

#[test]
fn channel_geometry_clips_the_interpolated_sword_path_when_the_damage_ray_misses() {
    // Both +X and +Z damage rays miss this off-axis wall. Immediately after
    // turning, the displayed sword still points +X; its connector toward +Z
    // crosses the wall before z=3. The visual must stop there too.
    let wall = arpg_sim::Wall::new(glam::Vec2::new(1.3, 3.0), glam::Vec2::splat(0.4), 0.0).unwrap();
    let mut game = Game::from_scene(
        &arpg_game::Scene {
            walls: vec![wall],
            ..Default::default()
        }
        .into(),
    )
    .unwrap();
    game.set_attack_profile(AttackProfile::Disintegrate);
    let dt = arpg_sim::Accumulator::default()
        .pending(arpg_sim::Dt::SECS)
        .next()
        .unwrap();
    let mut character = loaded_character("wall-channel.glb");
    character.sample(game.player_presentation(Alpha::ONE), Alpha::ONE, 0.0);
    let mut effects = crate::attack_effects::AttackEffects::default();
    game.step(
        dt,
        Intent::new(MoveDir::NONE, AimDir::new(Vec3::X).unwrap(), false).with_attack_held(true),
    );
    character.sample(game.player_presentation(Alpha::ONE), Alpha::ONE, 1.0);
    effects.observe(&game);
    game.step(
        dt,
        Intent::new(MoveDir::NONE, AimDir::FORWARD, false).with_attack_held(true),
    );
    effects.observe(&game);
    assert!(game.beam().unwrap().end_normal().is_none());
    let player = game.player_presentation(Alpha::ZERO);
    character.sample(player, Alpha::ZERO, 1.0);
    let emitter = character.beam_emitter(player);
    let hash = game.hash();
    effects.rebuild(&game, Alpha::ZERO, Some(emitter));
    assert!(!effects.vertices().is_empty());
    assert!(
        effects
            .vertices()
            .iter()
            .all(|vertex| vertex.position().z <= 2.8 + 0.0001),
        "displayed connector and glow must stop at the first wall they actually cross"
    );
    assert_eq!(
        game.hash(),
        hash,
        "visual obstruction cannot feed back into damage"
    );
}

#[test]
fn varied_ground_speeds_use_three_cadences_within_the_existing_pose_budget() {
    let mut horde = loaded_horde("horde.glb");
    let mut game = Game::empty();
    for (pos, speed) in [(glam::Vec2::new(20.0, 0.0), 1.5), (glam::Vec2::new(-20.0, 0.0), 2.5), (glam::Vec2::new(0.0, 20.0), 4.0)] {
        let mut actor = arpg_game::ActorDefinition::hostile();
        let arpg_game::ActionDefinition::Move(movement) = &mut actor.actions[0] else { unreachable!() };
        movement.speed = speed;
        game.place(pos, actor).unwrap();
    }
    game.place(glam::Vec2::new(0.0, -20.0), arpg_game::ActorDefinition::passive_hostile()).unwrap();
    for _ in 0..24 {
        game.step(Accumulator::default().pending(arpg_sim::Dt::SECS).next().unwrap(),
            Intent::new(MoveDir::NONE, AimDir::FORWARD, false));
    }
    horde.rebuild(game.character_presentations(Alpha::ONE), 0.37, |_| 0.0, |_| None, std::iter::empty(), Alpha::ONE);
    assert_eq!(horde.instance_count(), 4);
    assert_eq!(horde.occupied_buckets(), 4);
    for role in 0..HORDE_ROLES.len() {
        assert_eq!(horde.buckets[role * HORDE_PHASES_PER_ROLE..(role + 1) * HORDE_PHASES_PER_ROLE]
            .iter().map(|bucket| bucket.instances.len()).sum::<usize>(), 1);
    }
    assert_eq!(horde.buckets.len(), 12);
    assert!(HordeRole::Walk.playback_rate() < HordeRole::Run.playback_rate());
    assert!(HordeRole::Run.playback_rate() < HordeRole::Sprint.playback_rate());
}

#[test]
fn tree_batches_follow_scene_lifetime_and_keep_game_unchanged() {
    let mut view = WorldAssets::load(|_| Ok(())).unwrap();
    let mut game = Game::empty();
    let scene = game.load_scene(&arpg_game::GameScene {
        trees: vec![
            arpg_game::TreeSpec::new(arpg_game::TreeKind::Pine, glam::Vec2::new(12.0, 3.0), 0.5, 1.2).unwrap(),
            arpg_game::TreeSpec::new(arpg_game::TreeKind::Bare, glam::Vec2::new(-12.0, 0.0), 0.0, 0.8).unwrap(),
        ],
        ..arpg_game::GameScene::default()
    }).unwrap();
    let hash = game.hash();
    for _ in 0..2 {
        view.rebuild_trees(game.trees());
        view.rebuild_walls(game.visible_walls());
        assert!(view.walls.is_empty(), "trunk colliders must not draw wall boxes");
        assert_eq!(view.instance_count(), 3);
        assert_eq!(view.draw_count(), 3);
        assert_eq!(view.trees[0].2[0].pos(), Vec3::new(12.0, 0.0, 3.0));
        assert!(view.trees[1].2.is_empty());
        assert_eq!(view.trees[2].2[0].pos(), Vec3::new(-12.0, 0.0, 0.0));
        assert_eq!(game.hash(), hash);
    }
    assert!(game.evict_scene(scene));
    view.rebuild_trees(game.trees());
    view.rebuild_walls(game.visible_walls());
    assert_eq!(view.instance_count(), 1);
    assert_eq!(view.draw_count(), 1);
}

#[test]
fn corpses_use_one_frozen_horizontal_pose_and_clear_with_the_game() {
    let mut game = arpg_game::Game::empty();
    game.place(glam::Vec2::new(0.0, 3.0), arpg_game::ActorDefinition::passive_hostile()).unwrap();
    game.set_attack_profile(AttackProfile::Disintegrate);
    let dt = Accumulator::default().pending(arpg_sim::Dt::SECS).next().unwrap();
    for _ in 0..25 {
        game.step(dt, Intent::new(MoveDir::NONE, AimDir::FORWARD, false).with_attack_held(true));
    }
    let mut slot = None;
    replace_horde(&mut slot, default_horde_path(), |_| Ok(())).unwrap();
    let mut horde = slot.unwrap();
    horde.rebuild(game.character_presentations(Alpha::ONE), 1.0, |_| 1.0, |_| None, std::iter::empty(), Alpha::ONE);
    horde.rebuild_corpses(game.corpses(), Alpha::ONE);
    assert_eq!(horde.instance_count(), 1);
    assert_eq!(horde.occupied_buckets(), 1);
    let instance = horde.buckets[CORPSE_BUCKET].instances[0];
    assert!(instance.pitch() > 0.0 && instance.pitch() < 0.03);
    assert_eq!(instance.white_mix(), 0.0);
    assert_eq!(instance.pos().z, game.corpses()[0].position().y);
    let pose = corpse_joint_matrices(&horde);
    for _ in 0..7 { game.step(dt, Intent::new(MoveDir::NONE, AimDir::FORWARD, false)); }
    horde.rebuild_corpses(game.corpses(), Alpha::ZERO);
    let halfway = horde.buckets[CORPSE_BUCKET].instances[0].pitch();
    assert!(halfway > 0.6 && halfway < 0.8);
    for _ in 0..8 { game.step(dt, Intent::new(MoveDir::NONE, AimDir::FORWARD, false)); }
    horde.rebuild_corpses(game.corpses(), Alpha::ZERO);
    assert_eq!(horde.buckets[CORPSE_BUCKET].instances[0].pitch(), std::f32::consts::FRAC_PI_2);
    let instance = horde.buckets[CORPSE_BUCKET].instances[0];
    let palette = horde.buckets[CORPSE_BUCKET].pose.joint_matrices();
    let rotation = glam::Quat::from_rotation_y(instance.yaw()) * glam::Quat::from_rotation_x(instance.pitch());
    for vertex in horde.asset.vertices() {
        let skinned = vertex.joints().into_iter().zip(vertex.weights())
            .fold(Vec3::ZERO, |sum, (joint, weight)| sum
                + palette[usize::from(joint)].matrix().transform_point3(vertex.position()) * weight);
        let point = instance.pos() + rotation * (skinned * instance.scale());
        assert!(point.y >= -1e-5, "corpse penetrates the floor: {point:?}");
    }
    horde.rebuild(game.character_presentations(Alpha::ONE), 9.0, |_| 0.0, |_| None, std::iter::empty(), Alpha::ONE);
    horde.rebuild_corpses(game.corpses(), Alpha::ONE);
    assert_eq!(pose, corpse_joint_matrices(&horde));
    game.start_scene(&arpg_game::GameScene::default()).unwrap();
    horde.rebuild_corpses(game.corpses(), Alpha::ONE);
    assert_eq!(horde.instance_count(), 0);
}

fn corpse_joint_matrices(horde: &LoadedHorde<()>) -> Vec<glam::Mat4> {
    horde.buckets[CORPSE_BUCKET].pose.joint_matrices().iter().map(|joint| joint.matrix()).collect()
}

#[test]
fn committed_attacker_faces_its_strike_and_tilts_without_extra_pose_buckets() {
    let mut game = arpg_game::Game::from_scene(&arpg_game::GameScene {
        engine: arpg_game::Scene { bodies: vec![arpg_game::Placed {
            pos: (0.0, 1.4), what: arpg_game::ActorDefinition::attacker(),
        }], ..Default::default() },
        ..Default::default()
    }).unwrap();
    let mut horde = loaded_horde("melee.glb");
    let dt = Accumulator::default().pending(arpg_sim::Dt::SECS).next().unwrap();
    for tick in 0..84 {
        game.step(dt, Intent::new(MoveDir::NONE, AimDir::FORWARD, false));
        let hash = game.hash();
        horde.rebuild(game.character_presentations(Alpha::ONE), 0.0, |_| 0.0, |_| None,
            game.action_statuses(), Alpha::ONE);
        assert_eq!(horde.occupied_buckets(), 1);
        let instance = horde.buckets.iter().flat_map(|b| &b.instances).next().unwrap();
        assert!((instance.yaw().abs() - std::f32::consts::PI).abs() < 0.0001);
        if tick == 35 { assert!((instance.pitch() + 0.25).abs() < 0.0001); }
        if tick == 44 { assert!((instance.pitch() - 0.55).abs() < 0.0001); }
        if tick == 83 { assert!(instance.pitch().abs() < 0.0001); }
        assert_eq!(game.hash(), hash);
    }
}

#[test]
fn leaping_enemy_rises_and_lands_without_expanding_shared_pose_count() {
    let mut game = arpg_game::Game::from_scene(&arpg_game::GameScene {
        engine: arpg_game::Scene { bodies: vec![arpg_game::Placed {
            pos: (0.0, 4.0), what: arpg_game::ActorDefinition::attacker(),
        }], ..Default::default() },
        ..Default::default()
    }).unwrap();
    let mut horde = loaded_horde("leap.glb");
    let dt = Accumulator::default().pending(arpg_sim::Dt::SECS).next().unwrap();
    for tick in 0..132 {
        game.step(dt, Intent::new(MoveDir::NONE, AimDir::FORWARD, false));
        let hash = game.hash();
        horde.rebuild(game.character_presentations(Alpha::ONE), 0.0, |_| 0.0, |_| None,
            game.action_statuses(), Alpha::ONE);
        assert_eq!(horde.occupied_buckets(), 1);
        let instance = horde.buckets.iter().flat_map(|b| &b.instances).next().unwrap();
        if !(42..77).contains(&tick) { assert_eq!(instance.pos().y, 0.0); }
        if tick == 59 { assert!((instance.pos().y - 1.8).abs() < 0.0001); }
        assert_eq!(game.hash(), hash);
    }
}

#[test]
fn damage_uses_three_reaction_stages_then_returns_to_gait_without_mutating_game() {
    let mut game = Game::empty();
    let target = game.place(glam::Vec2::new(-1.8, 1.2), arpg_game::ActorDefinition::passive_hostile()).unwrap();
    let missed = game.place(glam::Vec2::new(8.0, 8.0), arpg_game::ActorDefinition::passive_hostile()).unwrap();
    let mut feedback = crate::enemy_feedback::EnemyFeedback::default();
    let mut horde = loaded_horde("recoil.glb");
    let dt = Accumulator::default().pending(arpg_sim::Dt::SECS).next().unwrap();
    let mut contacts = 0;
    for step in 1..=25 {
        game.step(dt, Intent::new(MoveDir::NONE, AimDir::FORWARD, step == 1));
        let hash = game.hash();
        feedback.observe(&game, |_, _| contacts += 1);
        feedback.observe(&game, |_, _| contacts += 1);
        let reaction = feedback.recoil(target, game.tick(), Alpha::ZERO);
        let expected = if !(7..25).contains(&step) { None } else { Some((step - 7) / 6) };
        assert_eq!(reaction.map(|r| r.stage()), expected);
        assert!(feedback.recoil(missed, game.tick(), Alpha::ZERO).is_none());
        horde.rebuild(game.character_presentations(Alpha::ZERO), presentation_seconds(game.tick(), Alpha::ZERO),
            |id| feedback.flash(id, game.tick(), Alpha::ZERO),
            |id| feedback.recoil(id, game.tick(), Alpha::ZERO), std::iter::empty(), Alpha::ZERO);
        assert_eq!(horde.buckets[RECOIL_START..CORPSE_BUCKET].iter().map(|b| b.instances.len()).sum::<usize>(),
            usize::from(expected.is_some()));
        assert_eq!(horde.instance_count(), 2);
        assert_eq!(game.hash(), hash);
    }
    assert_eq!(contacts, 1, "duplicate redraws cannot emit another impact");
    assert!(game.despawn_body(target));
    let replacement = game.place(glam::Vec2::new(-1.8, 1.2), arpg_game::ActorDefinition::passive_hostile()).unwrap();
    feedback.observe(&game, |_, _| contacts += 1);
    assert!(feedback.recoil(target, game.tick(), Alpha::ZERO).is_none());
    assert!(feedback.recoil(replacement, game.tick(), Alpha::ZERO).is_none());
    feedback.clear();
    assert!(feedback.recoil(missed, game.tick(), Alpha::ZERO).is_none());
}

#[test]
fn stationary_slam_braces_the_knees_and_enemy_height_is_close_to_player_height() {
    let mut game = Game::empty();
    game.set_attack_profile(AttackProfile::Slam);
    let dt = Accumulator::default().pending(arpg_sim::Dt::SECS).next().unwrap();
    for tick in 0..9 { game.step(dt, Intent::new(MoveDir::NONE, AimDir::FORWARD, tick == 0)); }
    let mut player = loaded_character_from(default_character_path());
    player.sample(game.player_presentation(Alpha::ONE), Alpha::ONE, presentation_seconds(game.tick(), Alpha::ONE));
    let mut expected = player.asset.bind_pose();
    player.asset.sample(player.clips.attack(AttackProfile::Slam), player.sampled_seconds, &mut expected);
    for name in ["Pelvis", "LeftLeg", "RightLeg", "LeftShin", "RightShin", "Weapon"] {
        let joint = player.asset.joint_named(name).unwrap();
        let actual = player.asset.joint_transform(&player.pose, joint).unwrap();
        assert!(actual.abs_diff_eq(player.asset.joint_transform(&expected, joint).unwrap(), 1e-5), "{name}");
    }
    let enemy = arpg_assets::import_character_glb(include_bytes!("../../../../assets/characters/basic-enemy/basic-enemy.glb")).unwrap();
    let height = |asset: &arpg_assets::CharacterAsset| {
        let min = asset.vertices().iter().map(|v| v.position().y).fold(f32::INFINITY, f32::min);
        let max = asset.vertices().iter().map(|v| v.position().y).fold(f32::NEG_INFINITY, f32::max);
        max - min
    };
    let ratio = height(&enemy) * HORDE_SCALE / height(&player.asset);
    assert!((0.80..=0.90).contains(&ratio), "enemy/player height {ratio}");
}

#[test]
fn slam_loads_overhead_releases_low_and_keeps_the_sword_in_its_hand_through_game() {
    for movement in [MoveDir::NONE, MoveDir::new(Vec3::X)] {
        let mut game = Game::empty();
        game.set_attack_profile(AttackProfile::Slam);
        let mut player = loaded_character_from(default_character_path());
        let dt = Accumulator::default().pending(arpg_sim::Dt::SECS).next().unwrap();
        let mut interpolation = Accumulator::default();
        assert_eq!(interpolation.pending(arpg_sim::Dt::SECS * 0.5).count(), 0);
        let mut grip = None;
        for elapsed in 0..31 {
            game.step(dt, Intent::new(movement, AimDir::FORWARD, elapsed == 0));
            let hash = game.hash();
            for alpha in [Alpha::ZERO, interpolation.alpha(), Alpha::ONE] {
                player.sample(game.player_presentation(alpha), alpha, presentation_seconds(game.tick(), alpha));
                let transform = |name| player.asset.joint_transform(&player.pose, player.asset.joint_named(name).unwrap()).unwrap();
                let tip = transform("BeamOrigin").transform_point3(Vec3::ZERO);
                let hand = transform("RightHand");
                let weapon = transform("Weapon");
                assert!(hand.transform_point3(Vec3::ZERO).distance(weapon.transform_point3(Vec3::ZERO)) < 0.001);
                if (9..24).contains(&elapsed) {
                    let relative = hand.inverse() * weapon;
                    if let Some(previous) = grip {
                        assert!(relative.abs_diff_eq(previous, 1e-4), "sword rotated inside the hand");
                    } else { grip = Some(relative); }
                }
                if elapsed == 8 && alpha == Alpha::ONE { assert!(tip.y > 2.0, "slam must load above the head"); }
                if elapsed == 13 && alpha == Alpha::ONE {
                    // Walking retains its pelvis/leg gait beneath the upper-body
                    // strike; its low release is relative to that moving torso.
                    let low = if movement == MoveDir::NONE { tip.y < 0.3 }
                        else { tip.y < transform("Chest").transform_point3(Vec3::ZERO).y - 0.25 };
                    assert!(low && tip.z > 0.8, "slam must release low in front during contact: {tip:?}");
                }
            }
            assert_eq!(hash, game.hash());
        }
    }
}

#[test]
fn missing_horde_reaction_clip_preserves_the_complete_accepted_asset_before_upload() {
    let mut slot = None;
    replace_horde(&mut slot, default_horde_path(), |_| Ok(())).unwrap();
    let mut before = Report::default();
    report_horde_preview(slot.as_ref(), &mut before);
    let before = before.finish();
    for (name, replacement) in [(b"\"Hit\"".as_slice(), b"\"Bad\"".as_slice()),
        (b"\"Death\"".as_slice(), b"\"Wrong\"".as_slice())] {
        let mut source = std::fs::read(default_horde_path()).unwrap();
        let offset = source.windows(name.len()).position(|w| w == name).unwrap();
        source[offset..offset + name.len()].copy_from_slice(replacement);
        let path = std::env::temp_dir().join(format!("arpg-missing-reaction-{}.glb", std::process::id()));
        std::fs::write(&path, source).unwrap();
        let mut uploaded = false;
        let result = replace_horde(&mut slot, path.clone(), |_| { uploaded = true; Ok(()) });
        std::fs::remove_file(path).unwrap();
        assert!(result.unwrap_err().contains("missing horde presentation clip"));
        assert!(!uploaded);
        let mut after = Report::default();
        report_horde_preview(slot.as_ref(), &mut after);
        assert_eq!(before, after.finish());
    }
}

#[test]
fn cleave_coils_before_damage_then_accelerates_across_the_front_through_game() {
    let mut game = Game::empty();
    let target = game.place(glam::Vec2::new(-1.8, 1.2), arpg_game::ActorDefinition::passive_hostile()).unwrap();
    let mut character = loaded_character_from(default_character_path());
    let dt = Accumulator::default().pending(arpg_sim::Dt::SECS).next().unwrap();
    let socket = character.asset.joint_named("BeamOrigin").unwrap();
    let mut active = Vec::new();
    for tick in 0..=12 {
        game.step(dt, Intent::new(MoveDir::NONE, AimDir::FORWARD, tick == 0));
        let hash = game.hash();
        let player = game.player_presentation(Alpha::ZERO);
        character.sample(player, Alpha::ZERO, presentation_seconds(game.tick(), Alpha::ZERO));
        let tip = character.asset.joint_transform(&character.pose, socket).unwrap().transform_point3(Vec3::ZERO);
        if tick < 6 {
            assert_eq!(game.health(target), Some(3));
            if tick >= 4 { assert!(tip.x < -1.0, "cleave never coils fully: {tip:?}"); }
        } else if tick < 12 {
            assert_eq!(game.health(target), Some(1));
            assert!(tip.z > 0.0, "strike went behind the player");
            active.push(tip.x);
        }
        assert_eq!(hash, game.hash());
    }
    let entry = active[1] - active[0];
    let release = active[3] - active[1];
    assert!(release > entry * 3.0, "cleave has no acceleration: {active:?}");
    assert!(active.windows(2).all(|p| p[1] >= p[0]));
    assert!(active.last().unwrap() - active[0] > 2.5);
}

#[test]
fn cleave_arm_and_wrist_articulation_survives_game_phase_sampling() {
    let mut game = Game::empty();
    let mut character = loaded_character_from(default_character_path());
    let dt = Accumulator::default().pending(arpg_sim::Dt::SECS).next().unwrap();
    let mut samples = Vec::new();
    for tick in 0..=14 {
        game.step(dt, Intent::new(MoveDir::NONE, AimDir::FORWARD, tick == 0));
        let hash = game.hash();
        character.sample(game.player_presentation(Alpha::ZERO), Alpha::ZERO,
            presentation_seconds(game.tick(), Alpha::ZERO));
        if [0, 5, 12, 14].contains(&tick) {
            let transform = |name| character.asset.joint_transform(&character.pose,
                character.asset.joint_named(name).unwrap()).unwrap();
            samples.push([transform("Chest"), transform("RightForearm"), transform("LeftArm"),
                transform("RightHand"), transform("LeftHand"), transform("Weapon"), transform("Pelvis"), transform("Head")]);
        }
        assert_eq!(hash, game.hash());
    }
    let elbow = |sample: &[glam::Mat4; 8]| (sample[0].inverse() * sample[1]).transform_point3(Vec3::ZERO);
    assert!(elbow(&samples[0]).distance(elbow(&samples[1])) > 0.25,
        "sword arm stayed fixed to the chest during wind-up");
    let axis = |matrix: glam::Mat4| matrix.transform_vector3(Vec3::Y).normalize();
    assert!(axis(samples[0][0].inverse() * samples[0][2])
        .angle_between(axis(samples[1][0].inverse() * samples[1][2])) > 0.2,
        "free arm stayed fixed to the chest during wind-up");
    assert!(axis(samples[0][1].inverse() * samples[0][5])
        .angle_between(axis(samples[2][1].inverse() * samples[2][5])) > 0.7,
        "sword did not release at the wrist");
    let point = |matrix: glam::Mat4| matrix.transform_point3(Vec3::ZERO);
    assert!(point(samples[0][6]).y - point(samples[1][6]).y > 0.25, "hips stayed upright instead of loading the supporting leg");
    assert!(point(samples[1][7]).z - point(samples[1][6]).z > 0.15, "body arches away from the cut");
    assert!(!samples[2][5].abs_diff_eq(samples[3][5], 0.1), "sword freezes at the start of recovery");
    assert!((samples[1][3].inverse() * samples[1][5]).abs_diff_eq(samples[2][3].inverse() * samples[2][5], 1e-5), "sword twists inside the hand");
    assert!(samples[2][4].transform_point3(Vec3::ZERO)
        .distance(samples[3][4].transform_point3(Vec3::ZERO)) > 0.07,
        "free arm has no follow-through after contact");
    for sample in samples {
        assert!(sample[3].transform_point3(Vec3::ZERO)
            .distance(sample[5].transform_point3(Vec3::ZERO)) < 0.001);
    }
}
