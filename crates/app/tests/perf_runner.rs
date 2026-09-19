//! The evidence policy is part of the ordinary workspace gate, even without a window.

#[test]
fn performance_evidence_policy() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = std::process::Command::new("python3")
        .args(["-B", "-m", "unittest", "discover", "-s", "scripts", "-p", "test_perf.py"])
        .current_dir(root)
        .output()
        .expect("Python 3 is required by the benchmark runner");
    assert!(output.status.success(), "benchmark policy tests failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
}
