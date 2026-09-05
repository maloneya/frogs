//! The write end of a state report.
//!
//! The third sink in this crate, and deliberately the same shape as the other
//! two. `InstanceSink` is how the simulation describes itself to a renderer;
//! `TraceSink` is how a pass describes what it just did; this is how anything
//! describes itself to an agent. In each case the producer can only *push*, and
//! the consumer owns the buffer, the format and the ordering.
//!
//! The reason it exists rather than a `format!` at each call site: a
//! hand-maintained format string is a second list of the world's fields, and a
//! second list is one that can silently disagree with the first. It is the same
//! failure `BINDINGS` was restructured to avoid, and the same one that made
//! `docs/invariants.md` cite three tests that no longer existed.
//!
//! What it does **not** do by itself is guarantee completeness. Writing every
//! field is the producer's job, and the way to make that checkable is to
//! destructure exhaustively on the way in — see `World::report`, where adding a
//! field to the struct fails to compile until it is reported.

use core::fmt::Write as _;

use glam::Vec3;

/// Collects named fields into a JSON object.
///
/// JSON rather than the space-separated line this replaces, because the
/// consumer is usually a program: `jq -r .tick` does not care what order the
/// fields are in or how many were added since it was written, where an `awk
/// '{print $2}'` breaks the day anything is inserted before column two. That
/// brittleness was real — a positional reader of the old format broke the first
/// time `tick` was added to the front of it.
pub struct Report {
    out: String,
    empty: bool,
}

impl Default for Report {
    fn default() -> Self {
        Self { out: String::from("{"), empty: true }
    }
}

impl Report {
    /// A whole number: a tick, a count.
    pub fn int(&mut self, name: &str, value: u64) {
        self.key(name);
        let _ = write!(self.out, "{value}");
    }

    /// A measurement. Four decimals: enough to see a fraction of a tick's
    /// movement (0.15 units), few enough that the noise floor of an `f32` does
    /// not show up as churn in a diff.
    pub fn num(&mut self, name: &str, value: f32) {
        self.key(name);
        let _ = write!(self.out, "{value:.4}");
    }

    /// A world position, as `[x, y, z]`.
    pub fn vec3(&mut self, name: &str, value: Vec3) {
        self.key(name);
        let _ = write!(self.out, "[{:.4},{:.4},{:.4}]", value.x, value.y, value.z);
    }

    /// A flag.
    pub fn bool(&mut self, name: &str, value: bool) {
        self.key(name);
        let _ = write!(self.out, "{value}");
    }

    /// Nests another reporter's fields under one key, so `sim` can describe
    /// itself without knowing what else is in the report.
    pub fn object(&mut self, name: &str, build: impl FnOnce(&mut Report)) {
        self.key(name);
        let mut inner = Report::default();
        build(&mut inner);
        self.out.push_str(&inner.finish());
    }

    /// The finished JSON object.
    #[must_use]
    pub fn finish(mut self) -> String {
        self.out.push('}');
        self.out
    }

    fn key(&mut self, name: &str) {
        if !self.empty {
            self.out.push(',');
        }
        self.empty = false;
        let _ = write!(self.out, "\"{name}\":");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_report_is_an_empty_object() {
        assert_eq!(Report::default().finish(), "{}");
    }

    #[test]
    fn fields_are_comma_separated_and_quoted() {
        let mut r = Report::default();
        r.int("tick", 42);
        r.num("facing", 1.5);
        r.bool("vsync", true);
        assert_eq!(r.finish(), r#"{"tick":42,"facing":1.5000,"vsync":true}"#);
    }

    #[test]
    fn objects_nest() {
        let mut r = Report::default();
        r.object("sim", |s| s.int("tick", 7));
        r.int("frames", 9);
        assert_eq!(r.finish(), r#"{"sim":{"tick":7},"frames":9}"#);
    }

    #[test]
    fn a_position_is_three_numbers() {
        let mut r = Report::default();
        r.vec3("pos", Vec3::new(1.0, -2.5, 0.0));
        assert_eq!(r.finish(), r#"{"pos":[1.0000,-2.5000,0.0000]}"#);
    }
}
