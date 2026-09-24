//! Reused verification must never turn missing, changed or partial evidence into a pass.
#[test]
fn verification_cache_and_coverage_policy() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = std::process::Command::new("python3")
        .args(["-B", "-m", "unittest", "discover", "-s", "scripts", "-p", "test_verify.py"])
        .current_dir(root).output().expect("Python 3 is required by the verification runner");
    assert!(output.status.success(), "verification policy tests failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
}
