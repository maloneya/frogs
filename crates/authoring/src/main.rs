//! One JSON request on stdin, one structured response on stdout. No game or GPU.
use std::io::{Read, Write};

fn main() -> std::process::ExitCode {
    let result = run();
    let success = result.is_ok();
    let reply = match result {
        Ok(editor) => serde_json::json!({"ok":true,"editor":editor,"error":null}),
        Err(error) => serde_json::json!({"ok":false,"editor":null,"error":error}),
    };
    let _ = writeln!(std::io::stdout().lock(), "{reply}");
    if success { std::process::ExitCode::SUCCESS } else { std::process::ExitCode::FAILURE }
}
fn run() -> Result<serde_json::Value, String> {
    if std::env::args().len() != 1 {
        return Err("Usage: arpg-author < request.json. Actions: new, open(path), edit(document, edit), batch(document, edits), validate(document), save(document, path, revision). See docs/scene-authoring.md.".into());
    }
    const LIMIT: u64 = 8 * 1024 * 1024;
    let mut input = String::new();
    std::io::stdin().take(LIMIT + 1).read_to_string(&mut input).map_err(|e| e.to_string())?;
    if input.len() as u64 > LIMIT { return Err("Authoring request exceeds 8 MiB.".into()); }
    let request = serde_json::from_str(&input).map_err(|e| format!("Invalid authoring request: {e}"))?;
    arpg_authoring::execute(request)
}
