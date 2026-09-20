//! App-only interaction state. Drawing borrows it; typed requests cross into
//! the app between ticks, where file I/O and playtest replacement are allowed.

use std::path::PathBuf;

use arpg_core::Report;

#[derive(Clone, Copy)]
pub(crate) enum MenuKey {
    Scenes,
    Close,
    Previous,
    Next,
    Accept,
}

#[derive(Clone)]
pub(crate) enum SceneChoice {
    Restart,
    Boot,
    File { path: PathBuf, label: String },
}

impl SceneChoice {
    pub(crate) fn label(&self) -> &str {
        match self {
            Self::Restart => "Restart current (cached snapshot)",
            Self::Boot => "Default horde",
            Self::File { label, .. } => label,
        }
    }
}

pub(crate) enum MenuRequest {
    RefreshScenes,
    Start(SceneChoice),
}

pub(crate) struct Picker {
    entries: Vec<SceneChoice>,
    selected: usize,
    error: Option<String>,
}

impl Default for Picker {
    fn default() -> Self {
        Self { entries: vec![SceneChoice::Restart, SceneChoice::Boot], selected: 0, error: None }
    }
}

impl Picker {
    pub(crate) fn entries(&self) -> &[SceneChoice] {
        &self.entries
    }

    pub(crate) fn selected(&self) -> usize {
        self.selected
    }

    pub(crate) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// The selected row is always visible, even after resizing the window.
    pub(crate) fn visible(&self, capacity: usize) -> std::ops::Range<usize> {
        let start = self.selected.saturating_sub(capacity.saturating_sub(1));
        start..(start + capacity).min(self.entries.len())
    }
}

#[derive(Default)]
pub(crate) struct Menu {
    open: bool,
    picker: Picker,
    request: Option<MenuRequest>,
}

impl Menu {
    pub(crate) fn open(&self) -> bool {
        self.open
    }

    pub(crate) fn picker(&self) -> &Picker {
        &self.picker
    }

    pub(crate) fn on_key(&mut self, key: MenuKey) {
        match key {
            MenuKey::Scenes => {
                self.open = !self.open;
                self.request = self.open.then_some(MenuRequest::RefreshScenes);
            }
            MenuKey::Close => {
                self.open = false;
                self.request = None;
            }
            MenuKey::Previous if self.open => {
                self.picker.selected = self.picker.selected.saturating_sub(1);
            }
            MenuKey::Next if self.open => {
                self.picker.selected = (self.picker.selected + 1).min(self.picker.entries.len() - 1);
            }
            MenuKey::Accept if self.open => {
                self.request = Some(MenuRequest::Start(self.picker.entries[self.picker.selected].clone()));
            }
            _ => {}
        }
    }

    pub(crate) fn set_catalog(&mut self, paths: Result<Vec<PathBuf>, String>) {
        self.picker = Picker::default();
        match paths {
            Ok(paths) => self.picker.entries.extend(paths.into_iter().map(|path| {
                let label = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
                SceneChoice::File { path, label }
            })),
            Err(error) => self.picker.error = Some(error),
        }
    }

    pub(crate) fn set_error(&mut self, error: String) {
        self.picker.error = Some(error);
    }

    pub(crate) fn take_request(&mut self) -> Option<MenuRequest> {
        self.request.take()
    }

    pub(crate) fn report(&self, out: &mut Report) {
        let Self { open, picker, request } = self;
        out.bool("scene_picker_open", *open);
        out.bool("captures_gameplay", self.open());
        out.bool("scene_request_pending", request.is_some());
        out.int("scene_picker_selected", picker.selected as u64);
        out.text("scene_picker_error", picker.error().unwrap_or(""));
        out.object("scene_picker_entries", |out| {
            for (index, entry) in picker.entries.iter().enumerate() {
                out.text(&index.to_string(), entry.label());
            }
        });
    }
}
