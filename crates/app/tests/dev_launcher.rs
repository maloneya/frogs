//! Development process ownership stays outside the game.
#[test]
fn launcher_checks_readiness_and_owns_only_its_startup_process() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = std::process::Command::new("python3")
        .args(["-B", "-m", "unittest", "discover", "-s", "scripts", "-p", "test_dev.py"])
        .current_dir(root).output().expect("Python 3 is required by the development launcher");
    assert!(output.status.success(), "launcher tests failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
}
