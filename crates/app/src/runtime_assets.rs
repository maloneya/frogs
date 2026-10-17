//! Runtime files are rooted in the launch directory, shared by game and companion.
//! Both launchers select the repository or extracted release directory before launch.

pub(crate) fn root() -> &'static std::path::Path {
    std::path::Path::new("assets")
}

#[cfg(test)]
pub(crate) mod tests {
    use std::{path::{Path, PathBuf}, process::Command, sync::atomic::{AtomicU64, Ordering}};

    pub(crate) struct ExtractedAssets {
        temporary: PathBuf,
        pub(crate) directory: PathBuf,
    }

    impl Drop for ExtractedAssets {
        fn drop(&mut self) { std::fs::remove_dir_all(&self.temporary).unwrap(); }
    }

    /// Package fixtures use the same relative tree without changing process cwd.
    pub(crate) fn extract(directories: &[&Path]) -> ExtractedAssets {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let temporary = std::env::temp_dir().join(format!("arpg-runtime-assets-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir(&temporary).unwrap();
        let directory = temporary.join("relocated");
        std::fs::create_dir(&directory).unwrap();
        let archive = temporary.join("assets.tar");
        let checkout = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        assert!(Command::new("tar").arg("-cf").arg(&archive).arg("-C").arg(checkout)
            .args(directories).status().unwrap().success());
        assert!(Command::new("tar").arg("-xf").arg(archive).arg("-C").arg(&directory)
            .status().unwrap().success());
        ExtractedAssets { temporary, directory }
    }

    #[test]
    fn relocated_package_admits_all_default_meshes() {
        use crate::presentation::{WorldAssets, default_character_path, default_horde_path,
            replace_character, replace_horde};
        let player = default_character_path();
        let horde = default_horde_path();
        let world = super::root().join("world");
        let fixture = extract(&[&world, player.parent().unwrap(), horde.parent().unwrap()]);
        WorldAssets::load(&fixture.directory.join(&world), |_| Ok(())).unwrap();
        let mut character = None;
        replace_character(&mut character, fixture.directory.join(player), |_| Ok(())).unwrap();
        let mut crowd = None;
        replace_horde(&mut crowd, fixture.directory.join(horde), |_| Ok(())).unwrap();
        assert!(crowd.is_some());
    }
}
