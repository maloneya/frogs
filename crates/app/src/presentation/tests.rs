//! Contracts for asset selection and simulation-driven presentation.

use super::*;
use arpg_core::{Intent, MoveDir};
use arpg_game::Game;
use arpg_sim::Accumulator;

fn fixture_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/fixtures/static-preview.glb")
}

fn character_fixture_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/fixtures/blender-bind-pose.glb")
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
    assert_eq!(player.unwrap().path, player_path);
    assert_eq!(horde.unwrap().path, horde_path);
}

fn loaded_character_from(path: std::path::PathBuf) -> LoadedCharacter<()> {
    let source = std::fs::read(&path).unwrap();
    let asset = arpg_assets::import_character_glb(&source).unwrap();
    let clips = CharacterClips::resolve(&asset).unwrap();
    let pose = asset.bind_pose();
    LoadedCharacter {
        path,
        asset,
        pose,
        clips,
        sampled_role: PresentationRole::Idle,
        sampled_seconds: 0.0,
        gpu: (),
    }
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
    let weapon = character.asset.joint_named("Weapon").unwrap();
    character
        .asset
        .joint_transform(&character.pose, weapon)
        .unwrap()
        .transform_point3(Vec3::Y * 0.96)
}

fn loaded_character(path: &str) -> LoadedCharacter<()> {
    let mut character = loaded_character_from(character_fixture_path());
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
    assert_eq!(current.asset.clip_count(), 4);
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
        r#"{"active":false,"path":"","vertices":0,"indices":0,"texture_width":0,"texture_height":0,"nodes":0,"joints":0,"clips":0,"role":"","clip":"","clip_duration":0.0000,"channels":0,"sample_seconds":0.0000}"#
    );

    let mut preview = loaded_character("assets/character.glb");
    let player = Game::empty().player_presentation(Alpha::ONE);
    preview.sample(player, Alpha::ONE, 0.25);
    let mut active = Report::default();
    report_character_preview(Some(&preview), &mut active);
    assert_eq!(
        active.finish(),
        r#"{"active":true,"path":"assets/character.glb","vertices":144,"indices":216,"texture_width":16,"texture_height":16,"nodes":6,"joints":4,"clips":4,"role":"idle","clip":"Idle","clip_duration":1.0000,"channels":12,"sample_seconds":0.2500}"#
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
    assert_eq!(current.instance_count(), 32);
    assert!((1..=HORDE_PHASES_PER_ROLE).contains(&current.occupied_buckets()));

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
    assert!(horde.occupied_buckets() <= HORDE_PHASES_PER_ROLE);
}

#[test]
fn stopped_horde_members_keep_heading_and_dead_identities_are_forgotten() {
    let mut horde = loaded_horde("horde.glb");
    let mut game = Game::empty();
    game.set_enemy_count(1);
    game.set_seeker_count(1);
    let dt = Accumulator::default()
        .pending(arpg_sim::Dt::SECS)
        .next()
        .unwrap();
    game.step(dt, Intent::default());
    horde.rebuild(game.enemy_presentations(Alpha::ONE), 0.0);
    let moving_yaw = horde
        .buckets
        .iter()
        .flat_map(|bucket| &bucket.instances)
        .next()
        .unwrap()
        .yaw();
    assert_ne!(moving_yaw, 0.0);

    game.set_seeker_count(0);
    game.step(dt, Intent::default());
    horde.rebuild(game.enemy_presentations(Alpha::ONE), 0.1);
    let stopped_yaw = horde
        .buckets
        .iter()
        .flat_map(|bucket| &bucket.instances)
        .next()
        .unwrap()
        .yaw();
    assert_eq!(stopped_yaw, moving_yaw);

    let dead = game.enemy_presentations(Alpha::ONE).next().unwrap().id();
    assert!(game.despawn_body(dead));
    horde.rebuild(game.enemy_presentations(Alpha::ONE), 0.2);
    assert!(horde.headings.is_empty());
}

#[test]
fn resetting_horde_presentation_cannot_reuse_a_previous_runs_heading() {
    let mut horde = loaded_horde("horde.glb");
    let mut old = Game::empty();
    old.set_enemy_count(1);
    old.set_seeker_count(1);
    let dt = Accumulator::default()
        .pending(arpg_sim::Dt::SECS)
        .next()
        .unwrap();
    old.step(dt, Intent::default());
    horde.rebuild(old.enemy_presentations(Alpha::ONE), 1.0);
    let old_id = old.enemy_presentations(Alpha::ONE).next().unwrap().id();
    assert_ne!(horde.headings[&old_id].yaw, 0.0);

    let mut fresh = Game::empty();
    fresh.set_enemy_count(1);
    assert_eq!(
        fresh.enemy_presentations(Alpha::ONE).next().unwrap().id(),
        old_id
    );
    horde.clear();
    assert_eq!(horde.instance_count(), 0);
    assert_eq!(horde.occupied_buckets(), 0);
    horde.rebuild(fresh.enemy_presentations(Alpha::ONE), 0.0);
    assert_eq!(horde.headings[&old_id].yaw, 0.0);
}

#[test]
fn every_attack_profile_resolves_to_its_own_presentation_clip() {
    let character = loaded_character("character.glb");
    let expected = [
        "AttackCleave",
        "AttackSlam",
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
    game.step(dt, Intent::new(MoveDir::new(Vec3::X), false));
    let running = game.player_presentation(Alpha::ONE);
    character.sample(running, Alpha::ONE, 1.0 / 60.0);
    assert_eq!(character.sampled_role, PresentationRole::Run);

    game.set_attack_profile(AttackProfile::Slam);
    let dt = Accumulator::default()
        .pending(arpg_sim::Dt::SECS)
        .next()
        .unwrap();
    game.step(dt, Intent::new(MoveDir::NONE, true));
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
        let base = profile.resolve();
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
        weapon_tip_at(character, profile, phase, elapsed, alpha, profile.resolve())
    };
    for profile in AttackProfile::ALL {
        let resolved = profile.resolve();
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
    let resolved = profile.resolve();
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
            let red = mesh.base_color_texture().rgba8()[0];
            assert!((40..=56).contains(&red), "floor colours must be encoded into sRGB, got {red}");
        }
        Ok(())
    }).unwrap();
    assert_eq!(world.ground.vertex_count, 4);
    assert_eq!(world.ground.index_count, 6);
    assert!(world.prop.vertex_count > 24, "the prop is authored bevelled geometry");
    assert_eq!(world.ground_instance[0].pos(), Vec3::ZERO);
    let mut game = Game::empty();
    game.place(glam::Vec2::new(4.0, 0.0), arpg_sim::Template::BODY).unwrap();
    let id = game.place(glam::Vec2::new(0.0, 1.0), arpg_sim::Template::BLOCK.interactive()).unwrap();
    let before = game.hash();
    world.rebuild(game.prop_presentations(Alpha::ONE));
    assert_eq!(game.hash(), before, "presentation cannot modify gameplay");
    assert_eq!(world.instance_count(), 2);
    assert_eq!(world.draw_count(), 2);
    assert_eq!(world.props.as_slice()[0].pos(), Vec3::new(0.0, 0.0, 1.0));
    assert_eq!(game.prop_presentations(Alpha::ONE).next().unwrap().interaction(), Some(arpg_sim::InteractionState::Ready));
    let mut accumulator = Accumulator::default();
    game.step(accumulator.pending(arpg_sim::Dt::SECS).next().unwrap(), Intent::NONE.with_interact(true));
    let snapshot = game.prop_presentations(Alpha::ONE).next().unwrap();
    assert_eq!(snapshot.id(), id);
    assert_eq!(snapshot.interaction(), Some(arpg_sim::InteractionState::Activated));
    world.rebuild(game.prop_presentations(Alpha::ONE));
    world.rebuild(Game::empty().prop_presentations(Alpha::ONE));
    assert_eq!(world.instance_count(), 1, "old props must not survive a fresh world");
    assert_eq!(world.draw_count(), 1);
}
