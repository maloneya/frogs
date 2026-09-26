//! Headless scene authoring shared by the native editor and agent CLI.
//! Operations consume complete documents; rejected work never changes the caller's
//! accepted document or a running game. Only Save writes to disk.
mod editor;
pub use editor::{Edit, Request, edit, EncounterDraft, PropKind};
pub use arpg_content::{SceneDocument, SceneObjectKind, SceneObjectRef, SceneObjectLabel};
use serde_json::Value;

/// Apply one request and derive its inspector/map representation from the result.
/// Validation uses the same Game admission as runtime and scenarios, without stepping.
pub fn execute(request: Request) -> Result<Value, String> {
    use editor::document;
    match request {
        Request::New => {
            let mut scene = SceneDocument::default();
            scene.scene.engine.name = "Untitled scene".into();
            document(&scene, None, None, None)
        }
        Request::Open { path } => {
            let path = std::path::absolute(path).map_err(|e| e.to_string())?;
            let (scene, revision) = arpg_content::read_scene_document(&path)?;
            document(&scene, None, Some(path), Some(revision))
        }
        Request::Edit { document: scene, edit: operation } => {
            let (scene, selected) = edit(scene, operation)?;
            document(&scene, selected, None, None)
        }
        Request::Batch { document: mut scene, edits } => {
            if edits.is_empty() { validate(&scene)?; }
            let mut selected = None;
            for operation in edits { (scene, selected) = edit(scene, operation)?; }
            document(&scene, selected, None, None)
        }
        Request::Validate { document: scene } => {
            validate(&scene)?;
            document(&scene, None, None, None)
        }
        Request::Save { document: scene, path, revision } => {
            let path = std::path::absolute(path).map_err(|e| e.to_string())?;
            validate(&scene)?;
            let mut result = document(&scene, None, Some(path.clone()), None)?;
            let revision = arpg_content::save_scene_document(&path, &scene, revision.as_deref())?;
            result["revision"] = Value::String(revision);
            Ok(result)
        }
    }
}

/// Validate authoring metadata and the complete playable scene without a renderer.
pub fn validate(document: &SceneDocument) -> Result<(), String> {
    document.validate()
}
