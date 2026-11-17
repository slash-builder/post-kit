//! What a run asks of the personas and how the result is written down.
//!
//! Two formats: `post` (one voice, markdown) and `conversation` (several voices, HTML with margin
//! notes). Each provides the same functions; this module dispatches on the profile's format.

mod conversation;
mod post;

use crate::error::Result;
use crate::guard::Guard;
use crate::profile::{FormatKind, Profile, Turn};
use crate::run::{Plan, TurnResult};
use serde_json::{Map, Value};

/// A rendered post: where it goes, its text, and the last footnote number used.
pub(crate) struct Rendered {
    pub path: String,
    pub text: String,
    pub last_note: u32,
}

/// What the history remembers about a post.
pub(crate) struct Record {
    pub title: String,
    pub summary: String,
    pub held_open: String,
}

pub(crate) fn validate(p: &Profile) -> Result<()> {
    match p.data.format {
        FormatKind::Conversation => conversation::validate(p),
        FormatKind::Post => post::validate(p),
    }
}

pub(crate) fn schema(p: &Profile, kind: &str) -> Value {
    match p.data.format {
        FormatKind::Conversation => conversation::schema(p, kind),
        FormatKind::Post => post::schema(p),
    }
}

pub(crate) fn instruction(p: &Profile, spec: &Turn, plan: &Plan) -> String {
    match p.data.format {
        FormatKind::Conversation => conversation::instruction(p, spec, plan),
        FormatKind::Post => post::instruction(p, spec, plan),
    }
}

pub(crate) fn check(
    p: &Profile,
    kind: &str,
    obj: &Map<String, Value>,
    guard: &Guard,
) -> Result<Vec<String>> {
    match p.data.format {
        FormatKind::Conversation => conversation::check(p, kind, obj, guard),
        FormatKind::Post => post::check(p, obj, guard),
    }
}

pub(crate) fn predicted_path(p: &Profile, plan: &Plan) -> Option<String> {
    match p.data.format {
        FormatKind::Conversation => Some(conversation::path(p, plan)),
        FormatKind::Post => None,
    }
}

pub(crate) fn render(p: &Profile, plan: &Plan, turns: &[TurnResult]) -> Result<Rendered> {
    match p.data.format {
        FormatKind::Conversation => conversation::render(p, plan, turns),
        FormatKind::Post => post::render(p, plan, turns),
    }
}

pub(crate) fn record(p: &Profile, turns: &[TurnResult]) -> Record {
    match p.data.format {
        FormatKind::Conversation => conversation::record(turns),
        FormatKind::Post => post::record(turns),
    }
}

pub(crate) fn review_text(p: &Profile, turns: &[TurnResult]) -> String {
    turns
        .iter()
        .map(|t| format!("{}: {}", p.label(&t.voice), crate::text::flatten(&t.obj)))
        .collect::<Vec<_>>()
        .join("\n\n")
}

// ---- helpers shared by both formats

pub(crate) fn str_field<'a>(obj: &'a Map<String, Value>, key: &str) -> &'a str {
    obj.get(key).and_then(Value::as_str).unwrap_or("")
}

pub(crate) fn list_field(obj: &Map<String, Value>, key: &str) -> Vec<String> {
    obj.get(key)
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn string_array(lo: usize, hi: usize) -> Value {
    serde_json::json!({"type": "array", "items": {"type": "string"}, "minItems": lo, "maxItems": hi})
}

pub(crate) fn object_schema(props: Map<String, Value>) -> Value {
    let required: Vec<Value> = props.keys().map(|k| Value::String(k.clone())).collect();
    serde_json::json!({"type": "object", "properties": props, "required": required, "additionalProperties": false})
}

pub(crate) fn required_fields(schema: &Value) -> Vec<String> {
    schema["required"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn signature_regex(p: &Profile) -> Result<Option<regex::Regex>> {
    p.data
        .checks
        .signature_regex
        .as_deref()
        .map(|s| {
            regex::Regex::new(&format!("(?i){s}"))
                .map_err(|e| crate::Error::Profile(format!("bad signature_regex: {e}")))
        })
        .transpose()
}
