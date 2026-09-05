//! Every identifier the documentation names must exist in the source.
//!
//! `docs/invariants.md` opens by admitting it is "a snapshot, not the source of
//! truth" and that "an entry here can silently disagree with the code". On the
//! first run of this check it disagreed three ways: two tests cited by name had
//! been renamed when their constants moved into pass modules, and one helper
//! had been renamed when it was shared with the render blend. Nothing failed;
//! the docs simply described a program that no longer existed.
//!
//! That is the exact failure the `BINDINGS` table was restructured to avoid —
//! a second list that drifts from the first — and it had reappeared in the file
//! whose job is to describe how such drift is prevented.
//!
//! **What this cannot check.** It verifies that a cited name *exists*, not that
//! the claim about it is *true*. The same audit turned up a row asserting
//! "`follow` is the only writer" of the camera target when `snap_to` writes it
//! too — both names exist, and the sentence was still wrong. Semantic claims
//! stay a reading job. This catches renames and deletions, which is the bulk of
//! real drift and all of the silent kind.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Files whose prose is checked.
const DOCS: &[&str] = &["CLAUDE.md", "docs", ".claude/skills"];

/// Where a cited name may live. Deliberately wider than `crates/`: the hooks in
/// `settings.json` are as much a part of how this project works as any function,
/// and `docs` cites their JSON keys.
const SOURCES: &[&str] = &["crates", "scenarios", ".claude/settings.json", "Cargo.toml"];

/// Snake-case words that are prose, or another language's vocabulary, rather
/// than something this repository defines.
///
/// **Empty, and that was checked rather than assumed.** It was first written
/// with three entries — `half_life`, `opt_level`, `rerun_if_changed` — guessed
/// at before running anything. Removing them changed nothing: every one was a
/// hole punched for a problem that did not exist.
///
/// Every entry here is such a hole, so a name belongs on this list only when it
/// is genuinely not ours *and* the check has actually flagged it — never
/// because adding it is easier than fixing the citation.
const NOT_OURS: &[&str] = &[];

#[test]
fn every_identifier_the_docs_cite_exists_in_the_source() {
    let root = repo_root();
    let haystack = read_all(&root, SOURCES).to_lowercase();

    // name -> the documents that cite it, so a failure says where to go.
    let mut cited: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for doc in collect(&root, DOCS, &["md"]) {
        let text = std::fs::read_to_string(&doc).expect("read a doc");
        let where_ = doc.strip_prefix(&root).unwrap_or(&doc).display().to_string();

        for name in backticked_identifiers(&text) {
            cited.entry(name).or_default().push(where_.clone());
        }
    }

    assert!(cited.len() > 20, "found only {} cited identifiers — the scan is broken", cited.len());

    let mut missing = Vec::new();
    for (name, docs) in &cited {
        if NOT_OURS.contains(&name.as_str()) {
            continue;
        }
        if !haystack.contains(&name.to_lowercase()) {
            let mut docs = docs.clone();
            docs.dedup();
            missing.push(format!("  `{name}` — cited in {}", docs.join(", ")));
        }
    }

    assert!(
        missing.is_empty(),
        "the documentation names {} identifier(s) that do not exist in the source:\n{}\n\n\
         Either the code was renamed and the docs were not, or the citation is a typo. \
         Fix the docs — they are the copy that drifts.",
        missing.len(),
        missing.join("\n"),
    );
}

/// Backticked words shaped like a Rust identifier: lowercase, with at least one
/// underscore.
///
/// The underscore requirement is what keeps this from flagging every ordinary
/// backticked word (`sim`, `state`, `tick`) while still catching function and
/// test names, which is where renames actually bite.
fn backticked_identifiers(text: &str) -> Vec<String> {
    let mut found = Vec::new();

    for chunk in text.split('`').skip(1).step_by(2) {
        // A citation may be qualified (`angle::wrap`) or a call (`damp_vec3()`).
        for word in chunk.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
            let ok = word.contains('_')
                && word.len() >= 4
                && word.starts_with(|c: char| c.is_ascii_lowercase())
                && word.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
            if ok {
                found.push(word.to_string());
            }
        }
    }

    found
}

fn repo_root() -> PathBuf {
    // tests run from the crate directory; the workspace is two levels up.
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().expect("find the repo root")
}

fn read_all(root: &Path, roots: &[&str]) -> String {
    let files = collect(root, roots, &["rs", "toml", "wgsl", "json", "ron", "trace"]);

    // Paths as well as contents. A doc naming `walk_east` is citing
    // `scenarios/walk_east.ron`, and that file existing is exactly the thing
    // being asserted — the first run flagged both scenario filenames for this.
    let names = files.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join("\n");

    let bodies =
        files.iter().filter_map(|p| std::fs::read_to_string(p).ok()).collect::<Vec<_>>().join("\n");

    format!("{names}\n{bodies}")
}

/// Every file under `roots` with one of `extensions`, skipping `target`.
fn collect(root: &Path, roots: &[&str], extensions: &[&str]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack: Vec<PathBuf> = roots.iter().map(|r| root.join(r)).collect();

    while let Some(path) = stack.pop() {
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == "target") {
                continue;
            }
            let Ok(entries) = std::fs::read_dir(&path) else { continue };
            stack.extend(entries.filter_map(|e| e.ok()).map(|e| e.path()));
        } else if path.extension().is_some_and(|e| extensions.iter().any(|x| e == *x)) {
            out.push(path);
        }
    }

    out.sort();
    out
}
