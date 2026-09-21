//! Scene-owned finite timelines. Only typed light pulses are supported today.
use arpg_core::{DirectionalLight, MAX_DIRECTIONAL_LIGHTS, Report};
use arpg_sim::{Fnv, SceneId, Trace, TraceSink};
use serde::Deserialize;

/// Maximum end tick of an authored timeline (one minute at the current fixed rate).
pub const MAX_SET_PIECE_TICKS: u32 = 3600;

/// A validated light pulse with a sharp onset and a linear fade.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(try_from = "RawPulse")]
pub struct LightPulse {
    light: DirectionalLight,
    duration_ticks: u32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPulse {
    toward_source: (f32, f32, f32),
    colour: (f32, f32, f32),
    intensity: f32,
    fill: f32,
    duration_ticks: u32,
}
impl TryFrom<RawPulse> for LightPulse {
    type Error = &'static str;
    fn try_from(raw: RawPulse) -> Result<Self, Self::Error> {
        Self::new(
            DirectionalLight::try_new(
                raw.toward_source.into(),
                raw.colour.into(),
                raw.intensity,
                raw.fill,
            )?,
            raw.duration_ticks,
        )
    }
}
impl LightPulse {
    /// A pulse lasts 1..=MAX_SET_PIECE_TICKS fixed ticks.
    pub fn new(light: DirectionalLight, duration_ticks: u32) -> Result<Self, &'static str> {
        if !(1..=MAX_SET_PIECE_TICKS).contains(&duration_ticks) {
            return Err("invalid light pulse duration");
        }
        Ok(Self {
            light,
            duration_ticks,
        })
    }
}

/// Explicitly supported timeline actions; no arbitrary property mutation.
#[derive(Clone, Copy, Debug, Deserialize)]
pub enum SetPieceAction {
    /// Illuminate surfaces for a bounded duration, then contribute no light.
    LightPulse(LightPulse),
}
impl SetPieceAction {
    fn duration(self) -> u32 {
        match self {
            Self::LightPulse(pulse) => pulse.duration_ticks,
        }
    }
}

/// An action and its start offset from the set piece's first active tick.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(try_from = "RawCue")]
pub struct SetPieceCue {
    at: u32,
    action: SetPieceAction,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCue {
    at: u32,
    action: SetPieceAction,
}
impl TryFrom<RawCue> for SetPieceCue {
    type Error = &'static str;
    fn try_from(raw: RawCue) -> Result<Self, Self::Error> {
        Self::new(raw.at, raw.action)
    }
}
impl SetPieceCue {
    /// Rejects overflow or a cue extending beyond the maximum timeline length.
    pub fn new(at: u32, action: SetPieceAction) -> Result<Self, &'static str> {
        if at
            .checked_add(action.duration())
            .is_none_or(|end| end > MAX_SET_PIECE_TICKS)
        {
            return Err("set piece cue exceeds the timeline limit");
        }
        Ok(Self { at, action })
    }
    fn end(self) -> u32 {
        self.at + self.action.duration()
    }
}

/// A named scene-local moment. Triggering is explicit; installation never starts it.
#[derive(Clone, Debug, Deserialize)]
#[serde(try_from = "RawSetPiece")]
pub struct SetPieceSpec {
    name: String,
    cues: Vec<SetPieceCue>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSetPiece {
    name: String,
    cues: Vec<SetPieceCue>,
}
impl TryFrom<RawSetPiece> for SetPieceSpec {
    type Error = &'static str;
    fn try_from(raw: RawSetPiece) -> Result<Self, Self::Error> {
        Self::new(raw.name, raw.cues)
    }
}
impl SetPieceSpec {
    /// Names are 1..=48 ASCII letters, digits, underscores or hyphens.
    /// Cues must be nonempty, bounded and ordered by start tick; ties use file order.
    pub fn new(name: String, cues: Vec<SetPieceCue>) -> Result<Self, &'static str> {
        if name.is_empty()
            || name.len() > 48
            || !name
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
        {
            return Err("invalid set piece name");
        }
        if cues.is_empty()
            || cues.len() > MAX_DIRECTIONAL_LIGHTS
            || cues.windows(2).any(|pair| pair[0].at > pair[1].at)
        {
            return Err("set piece needs bounded cues in timeline order");
        }
        Ok(Self { name, cues })
    }
    /// Stable authored label within a scene instance.
    pub fn name(&self) -> &str {
        &self.name
    }
    fn end(&self) -> u32 {
        self.cues
            .iter()
            .map(|cue| cue.end())
            .max()
            .expect("nonempty validated timeline")
    }
}

/// Authoritative timeline phase, also used by scenario assertions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum SetPieceState {
    /// Installed but not playing, including after completion.
    Inactive,
    /// An explicit trigger will start or restart the timeline on the next tick.
    Queued,
    /// The most recently completed tick's timeline offset.
    Running {
        /// Zero on the first active tick.
        elapsed_ticks: u32,
    },
}

/// Diagnostic transitions keyed by stable scene and authored index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetPieceEvent {
    /// Started or restarted at offset zero.
    Started {
        /// Owning scene.
        scene: SceneId,
        /// Authored index.
        index: usize,
    },
    /// All actions have ended.
    Finished {
        /// Owning scene.
        scene: SceneId,
        /// Authored index.
        index: usize,
    },
}
impl std::fmt::Display for SetPieceEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Started { scene, index } => write!(f, "SetPieceStarted {scene}/{index}"),
            Self::Finished { scene, index } => write!(f, "SetPieceFinished {scene}/{index}"),
        }
    }
}
struct SetPiece {
    owner: SceneId,
    index: usize,
    spec: SetPieceSpec,
    state: SetPieceState,
}
#[derive(Default)]
pub(crate) struct SetPieces {
    rows: Vec<SetPiece>,
    trace: Trace<SetPieceEvent>,
}
impl SetPieces {
    pub(crate) fn validate_add(&self, specs: &[SetPieceSpec]) -> Result<(), crate::SceneError> {
        // Reserve every authored pulse, even non-overlapping ones. Then arbitrary
        // simultaneous triggers cannot overflow GPU capacity or lose effects.
        let reserved = self
            .rows
            .iter()
            .map(|row| row.spec.cues.len())
            .sum::<usize>();
        let mut remaining = MAX_DIRECTIONAL_LIGHTS - reserved;
        for (index, spec) in specs.iter().enumerate() {
            if specs[..index].iter().any(|other| other.name == spec.name) {
                return Err(crate::SceneError::SetPiece(
                    "duplicate scene-local set piece name",
                ));
            }
            remaining =
                remaining
                    .checked_sub(spec.cues.len())
                    .ok_or(crate::SceneError::SetPiece(
                        "scene exceeds the live directional-light reservation limit",
                    ))?;
        }
        Ok(())
    }
    pub(crate) fn install(&mut self, owner: SceneId, specs: &[SetPieceSpec]) {
        self.validate_add(specs)
            .expect("validated before physical admission");
        self.rows
            .extend(specs.iter().enumerate().map(|(index, spec)| SetPiece {
                owner,
                index,
                spec: spec.clone(),
                state: SetPieceState::Inactive,
            }));
    }
    pub(crate) fn evict(&mut self, owner: SceneId) {
        self.rows.retain(|row| row.owner != owner);
    }
    pub(crate) fn trigger(&mut self, owner: SceneId, name: &str) -> bool {
        let Some(row) = self
            .rows
            .iter_mut()
            .find(|row| row.owner == owner && row.spec.name == name)
        else {
            return false;
        };
        row.state = SetPieceState::Queued;
        true
    }
    pub(crate) fn state(&self, owner: SceneId, name: &str) -> Option<SetPieceState> {
        self.rows
            .iter()
            .find(|row| row.owner == owner && row.spec.name == name)
            .map(|row| row.state)
    }
    pub(crate) fn iter(&self) -> impl Iterator<Item = (SceneId, &str, SetPieceState)> {
        self.rows
            .iter()
            .map(|row| (row.owner, row.spec.name.as_str(), row.state))
    }
    pub(crate) fn lights(&self) -> impl Iterator<Item = DirectionalLight> + '_ {
        self.rows.iter().flat_map(|row| {
            row.spec.cues.iter().filter_map(|cue| {
                let SetPieceState::Running { elapsed_ticks } = row.state else {
                    return None;
                };
                if elapsed_ticks < cue.at || elapsed_ticks >= cue.end() {
                    return None;
                }
                match cue.action {
                    SetPieceAction::LightPulse(pulse) => Some(pulse.light.faded(
                        1.0 - (elapsed_ticks - cue.at) as f32 / pulse.duration_ticks as f32,
                    )),
                }
            })
        })
    }
    pub(crate) fn trace(&self) -> &Trace<SetPieceEvent> {
        &self.trace
    }
    pub(crate) fn clear_trace(&mut self) {
        self.trace.clear();
    }
    pub(crate) fn hash(&self, out: &mut Fnv) {
        out.usize(self.rows.len());
        for row in &self.rows {
            row.owner.hash_into(out);
            out.usize(row.index);
            out.usize(row.spec.name.len());
            for byte in row.spec.name.bytes() {
                out.u64(u64::from(byte));
            }
            out.usize(row.spec.cues.len());
            for cue in &row.spec.cues {
                out.u64(u64::from(cue.at));
                match cue.action {
                    SetPieceAction::LightPulse(pulse) => {
                        out.u64(0);
                        out.u64(u64::from(pulse.duration_ticks));
                        for value in pulse
                            .light
                            .toward_source()
                            .to_array()
                            .into_iter()
                            .chain(pulse.light.colour().to_array())
                            .chain([pulse.light.intensity(), pulse.light.fill()])
                        {
                            out.f32(value);
                        }
                    }
                }
            }
            match row.state {
                SetPieceState::Inactive => out.u64(0),
                SetPieceState::Queued => out.u64(1),
                SetPieceState::Running { elapsed_ticks } => {
                    out.u64(2);
                    out.u64(u64::from(elapsed_ticks));
                }
            }
        }
    }
    pub(crate) fn report(&self, out: &mut Report) {
        out.int("set_piece_events_dropped", self.trace.dropped() as u64);
        out.object("set_pieces", |out| {
            for row in &self.rows {
                out.object(&format!("{}/{}", row.owner, row.spec.name), |out| {
                    out.text("scene", &row.owner.to_string());
                    out.text("name", &row.spec.name);
                    out.int("duration_ticks", u64::from(row.spec.end()));
                    match row.state {
                        SetPieceState::Inactive => out.text("phase", "Inactive"),
                        SetPieceState::Queued => out.text("phase", "Queued"),
                        SetPieceState::Running { elapsed_ticks } => {
                            out.text("phase", "Running");
                            out.int("elapsed_ticks", u64::from(elapsed_ticks));
                        }
                    }
                });
            }
        });
        out.object("directional_lights", |out| {
            for (index, light) in self.lights().enumerate() {
                out.object(&index.to_string(), |out| {
                    out.vec3("toward_source", light.toward_source());
                    out.vec3("colour", light.colour());
                    out.num("intensity", light.intensity());
                    out.num("fill", light.fill());
                });
            }
        });
    }
}

/// Before the engine tick: consumes queued triggers, advances timelines and records
/// transitions. Reads/writes only set-piece state; owns no physical structural edits.
pub(crate) fn advance(pieces: &mut SetPieces, tick: u64) {
    let SetPieces { rows, trace } = pieces;
    let mut trace = trace.sink(tick);
    for row in rows {
        advance_one(row, &mut trace);
    }
}
fn advance_one(row: &mut SetPiece, trace: &mut TraceSink<'_, SetPieceEvent>) {
    row.state = match row.state {
        SetPieceState::Inactive => SetPieceState::Inactive,
        SetPieceState::Queued => {
            trace.emit(SetPieceEvent::Started {
                scene: row.owner,
                index: row.index,
            });
            SetPieceState::Running { elapsed_ticks: 0 }
        }
        SetPieceState::Running { elapsed_ticks } if elapsed_ticks + 1 < row.spec.end() => {
            SetPieceState::Running {
                elapsed_ticks: elapsed_ticks + 1,
            }
        }
        SetPieceState::Running { .. } => {
            trace.emit(SetPieceEvent::Finished {
                scene: row.owner,
                index: row.index,
            });
            SetPieceState::Inactive
        }
    };
}
