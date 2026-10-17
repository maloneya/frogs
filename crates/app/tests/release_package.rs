//! Release packaging preserves the launch-directory runtime contract.
#[test]
fn relocated_release_contains_assets_and_independent_launchers() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = std::process::Command::new("python3")
        .args(["-B", "-m", "unittest", "discover", "-s", "scripts", "-p", "test_release.py"])
        .current_dir(root).output().expect("Python 3 is required by release packaging");
    assert!(output.status.success(), "release packaging tests failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
}
