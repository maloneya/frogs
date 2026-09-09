//! App-only interaction state. Drawing borrows it; typed requests cross into
//! the app between ticks, where file I/O and playtest replacement are allowed.

use std::path::PathBuf;

use arpg_core::Report;
use arpg_sim::AttackProfile;

#[derive(Clone, Copy)]
pub(crate) enum MenuKey {
    Toggle,
    Scenes,
    Close,
    Decrease,
    Increase,
    Reset,
    Previous,
    Next,
    Accept,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Mode {
    #[default]
    Closed,
    Attack,
    Scenes,
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
    mode: Mode,
    /// A content selection waiting for the next simulation tick.
    pending: Option<AttackProfile>,
    picker: Picker,
    request: Option<MenuRequest>,
}

impl Menu {
    pub(crate) fn mode(&self) -> Mode {
        self.mode
    }

    pub(crate) fn open(&self) -> bool {
        self.mode != Mode::Closed
    }

    pub(crate) fn picker(&self) -> &Picker {
        &self.picker
    }

    pub(crate) fn pending(&self) -> Option<AttackProfile> {
        self.pending
    }

    pub(crate) fn profile(&self, applied: AttackProfile) -> AttackProfile {
        self.pending.unwrap_or(applied)
    }

    pub(crate) fn on_key(&mut self, key: MenuKey, applied: AttackProfile) {
        match key {
            MenuKey::Toggle | MenuKey::Scenes => {
                let target =
                    if matches!(key, MenuKey::Toggle) { Mode::Attack } else { Mode::Scenes };
                self.mode = if self.mode == target { Mode::Closed } else { target };
                self.request = (self.mode == Mode::Scenes).then_some(MenuRequest::RefreshScenes);
            }
            MenuKey::Close => {
                self.mode = Mode::Closed;
                self.request = None;
            }
            MenuKey::Previous if self.mode == Mode::Scenes => {
                self.picker.selected = self.picker.selected.saturating_sub(1);
            }
            MenuKey::Next if self.mode == Mode::Scenes => {
                self.picker.selected =
                    (self.picker.selected + 1).min(self.picker.entries.len() - 1);
            }
            MenuKey::Previous | MenuKey::Decrease if self.mode == Mode::Attack => {
                let current = profile_index(self.profile(applied));
                self.pending = Some(AttackProfile::ALL[current.saturating_sub(1)]);
            }
            MenuKey::Next | MenuKey::Increase if self.mode == Mode::Attack => {
                let current = profile_index(self.profile(applied));
                self.pending =
                    Some(AttackProfile::ALL[(current + 1).min(AttackProfile::ALL.len() - 1)]);
            }
            MenuKey::Accept if self.mode == Mode::Scenes => {
                self.request =
                    Some(MenuRequest::Start(self.picker.entries[self.picker.selected].clone()));
            }
            MenuKey::Reset if self.mode == Mode::Attack => {
                self.pending = Some(AttackProfile::default());
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

    /// Called only at the input/sim boundary. A zero-tick frame leaves it alone.
    pub(crate) fn take_profile(&mut self) -> Option<AttackProfile> {
        self.pending.take()
    }

    pub(crate) fn report(&self, out: &mut Report) {
        let Self { mode, pending, picker, request } = self;
        out.bool("attack_menu_open", *mode == Mode::Attack);
        out.bool("scene_picker_open", *mode == Mode::Scenes);
        out.bool("captures_gameplay", self.open());
        out.bool("attack_profile_pending", pending.is_some());
        out.text("pending_attack_profile", pending.map_or("", |profile| profile.label()));
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

fn profile_index(profile: AttackProfile) -> usize {
    AttackProfile::ALL
        .iter()
        .position(|candidate| *candidate == profile)
        .expect("every attack profile is in its catalog")
}
