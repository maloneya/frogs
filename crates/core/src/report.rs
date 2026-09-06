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
//! second list is one that can silently disagree with the first — the same
//! failure `BINDINGS` was restructured to avoid.
//!
//! What it does **not** do by itself is guarantee completeness. Writing every
//! field is the producer's job, and the way to make that checkable is to
//! destructure exhaustively on the way in — see `World::report`, where adding a
//! field to the struct fails to compile until it is reported.

use core::fmt::Write as _;

use glam::Vec3;

/// Collects named fields into a JSON object.
///
/// JSON rather than a space-separated line, because the consumer is usually a
/// program: `jq -r .tick` does not care what order the fields are in or how
/// many were added since it was written, where `awk '{print $2}'` breaks the
/// day anything is inserted before column two.
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
        self.number(value);
    }

    /// A world position, as `[x, y, z]`.
    pub fn vec3(&mut self, name: &str, value: Vec3) {
        self.key(name);
        self.out.push('[');
        for (i, v) in [value.x, value.y, value.z].into_iter().enumerate() {
            if i > 0 {
                self.out.push(',');
            }
            self.number(v);
        }
        self.out.push(']');
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

    /// **A non-finite value is written as `null`, not as `NaN`.**
    ///
    /// JSON has no NaN or infinity, and `write!` would emit the bare word,
    /// which no parser accepts. That would break this report at exactly the
    /// moment it is most needed: a poisoned position is the failure the
    /// `finite` field exists to announce, and announcing it by making the whole
    /// object unparseable is announcing nothing. `null` is legal, survives
    /// `jq`, and is visibly not a number.
    fn number(&mut self, value: f32) {
        if value.is_finite() {
            let _ = write!(self.out, "{value:.4}");
        } else {
            self.out.push_str("null");
        }
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

    /// JSON has no NaN. Emitting the bare word would make the whole report
    /// unparseable at the one moment it matters most — see [`Report::number`].
    #[test]
    fn a_value_that_is_not_a_number_is_null() {
        let mut r = Report::default();
        r.num("facing", f32::NAN);
        r.vec3("pos", Vec3::new(1.0, f32::INFINITY, 0.0));
        assert_eq!(r.finish(), r#"{"facing":null,"pos":[1.0000,null,0.0000]}"#);
    }

    #[test]
    fn a_position_is_three_numbers() {
        let mut r = Report::default();
        r.vec3("pos", Vec3::new(1.0, -2.5, 0.0));
        assert_eq!(r.finish(), r#"{"pos":[1.0000,-2.5000,0.0000]}"#);
    }
}
