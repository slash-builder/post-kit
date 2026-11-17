//! Several voices on one page: an opening body voice, margin voices, a reply, and last lines.
//!
//! The turn order has a fixed shape: the opener opens; each margin voice writes a margin note; the
//! opener closes with a reply to each margin; then margin voices may add last lines.

use super::{
    list_field, object_schema, required_fields, signature_regex, str_field, string_array, Record,
    Rendered,
};
use crate::checks::{check_footnote_markers, check_text_fields};
use crate::error::{Error, Result};
use crate::guard::Guard;
use crate::profile::{Profile, Turn};
use crate::run::{Plan, TurnResult};
use crate::text::{to_html, yaml_scalar};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

fn roles(p: &Profile) -> (String, Vec<String>) {
    let turns = &p.data.turns;
    let opener = turns[0].voice.clone();
    let margins = turns
        .iter()
        .skip(1)
        .filter(|t| t.kind == "margin")
        .map(|t| t.voice.clone())
        .collect();
    (opener, margins)
}

fn reply_key(voice: &str) -> String {
    format!("reply_to_{}", voice.replace('-', "_"))
}

pub(super) fn validate(p: &Profile) -> Result<()> {
    let seq: Vec<(&str, &str)> = p
        .data
        .turns
        .iter()
        .map(|t| (t.voice.as_str(), t.kind.as_str()))
        .collect();
    let opener = seq[0].0;
    let mut i = 1;
    let mut margins = Vec::new();
    while i < seq.len() && seq[i].1 == "margin" {
        margins.push(seq[i].0);
        i += 1;
    }
    let distinct = margins
        .iter()
        .collect::<std::collections::BTreeSet<_>>()
        .len()
        == margins.len();
    let ok = seq[0].1 == "open"
        && !margins.is_empty()
        && distinct
        && !margins.contains(&opener)
        && i < seq.len()
        && seq[i] == (opener, "close")
        && seq[i + 1..]
            .iter()
            .all(|(v, k)| *k == "last" && margins.contains(v));
    if ok {
        Ok(())
    } else {
        Err(Error::Profile("conversation turns must be: opener open; one margin per margin voice; opener close; then any 'last' margins".into()))
    }
}

pub(super) fn schema(p: &Profile, kind: &str) -> Value {
    let lim = &p.data.limits;
    let s = || serde_json::json!({"type": "string"});
    let mut props = Map::new();
    match kind {
        "open" => {
            props.insert("title".into(), s());
            props.insert("paragraphs".into(), string_array(1, lim.max_paragraphs));
            props.insert("footnotes".into(), string_array(0, lim.max_footnotes));
        }
        "close" => {
            props.insert("paragraphs".into(), string_array(1, 2));
            props.insert("footnotes".into(), string_array(0, 1));
            for v in roles(p).1 {
                props.insert(reply_key(&v), s());
            }
            props.insert("summary".into(), s());
            props.insert("held_open".into(), s());
        }
        _ => {
            props.insert("margin".into(), s());
        }
    }
    object_schema(props)
}

pub(super) fn instruction(p: &Profile, spec: &Turn, plan: &Plan) -> String {
    let lim = &p.data.limits;
    let base = match spec.kind.as_str() {
        "open" => {
            let mut b = format!(
                "Open the exchange on the topic passage. Write 1 to {} paragraphs of at most {} words each, and a title of at most {} words (no quotation marks). You may add up to {} footnotes of at most {} words each; mark each in a paragraph exactly once with [^1], [^2] in order.",
                lim.max_paragraphs, lim.paragraph_words, lim.title_words, lim.max_footnotes, lim.footnote_words
            );
            if let Some(q) = &plan.topic.question {
                b.push_str(&format!(
                    " This exchange continues an earlier one. Take up its open question: {q}"
                ));
            }
            b
        }
        "margin" => format!(
            "Write your margin note on the opening: 1 to 3 sentences, at most {} words.",
            lim.margin_words
        ),
        "close" => {
            let names = roles(p)
                .1
                .iter()
                .map(|v| reply_key(v))
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "Reply to the margins and keep the question open. Write 1 to 2 paragraphs of at most {} words each; up to 1 footnote of at most {} words, marked [^1]. Give a short reply for each margin ({names}; at most {} words each). Give a one-sentence summary of what was argued (at most {} words) and held_open: the question left open (at most {} words).",
                lim.paragraph_words, lim.footnote_words, lim.reply_words, lim.summary_words, lim.held_open_words
            )
        }
        _ => format!(
            "Write your last line: one or two short sentences, at most {} words.",
            lim.last_words
        ),
    };
    format!("{base} {}", spec.ask).trim().to_string()
}

pub(super) fn check(
    p: &Profile,
    kind: &str,
    obj: &Map<String, Value>,
    guard: &Guard,
) -> Result<Vec<String>> {
    let lim = &p.data.limits;
    let want = required_fields(&schema(p, kind));
    let mut have: Vec<&String> = obj.keys().collect();
    have.sort();
    let mut sorted = want.clone();
    sorted.sort();
    if have.iter().map(|s| s.as_str()).collect::<Vec<_>>()
        != sorted.iter().map(String::as_str).collect::<Vec<_>>()
    {
        return Ok(vec![format!("fields must be exactly {want:?}")]);
    }
    let mut limits: BTreeMap<String, usize> = BTreeMap::new();
    for (k, v) in [
        ("paragraphs", lim.paragraph_words),
        ("footnotes", lim.footnote_words),
        ("summary", lim.summary_words),
        ("held_open", lim.held_open_words),
        ("title", lim.title_words),
        (
            "margin",
            if kind == "last" {
                lim.last_words
            } else {
                lim.margin_words
            },
        ),
    ] {
        limits.insert(k.into(), v);
    }
    for v in roles(p).1 {
        limits.insert(reply_key(&v), lim.reply_words);
    }
    let sig = signature_regex(p)?;
    let mut problems = check_text_fields(obj, &limits, guard, sig.as_ref());
    if obj.contains_key("paragraphs") {
        problems.extend(check_footnote_markers(
            &list_field(obj, "paragraphs"),
            &list_field(obj, "footnotes"),
        ));
    }
    if kind == "open"
        && str_field(obj, "title")
            .chars()
            .any(|c| matches!(c, '"' | '\u{201c}' | '\u{201d}'))
    {
        problems.push("title must not contain quotation marks".into());
    }
    Ok(problems)
}

pub(super) fn path(p: &Profile, plan: &Plan) -> String {
    let o = &p.data.output;
    if o.file_prefix.is_empty() {
        format!("{}/{}.md", o.directory, plan.id)
    } else {
        format!("{}/{}-{}.md", o.directory, o.file_prefix, plan.id)
    }
}

fn find<'a>(turns: &'a [TurnResult], kind: &str) -> Result<&'a TurnResult> {
    turns
        .iter()
        .find(|t| t.kind == kind)
        .ok_or_else(|| Error::Gate(format!("no '{kind}' turn was produced")))
}

pub(super) fn render(p: &Profile, plan: &Plan, turns: &[TurnResult]) -> Result<Rendered> {
    let r = &p.data.render;
    let (_, margins) = roles(p);
    let h = |text: &str, fn_map: Option<&BTreeMap<u32, u32>>| {
        to_html(text, &r.tokens, fn_map, r.redaction_html.as_deref())
    };
    let mut n = plan.footnote_start;
    let mut parts: Vec<String> = Vec::new();
    let mut notes: Vec<(u32, String)> = Vec::new();

    let mut paragraphs = |obj: &Map<String, Value>, n: &mut u32| -> Vec<String> {
        let mut map = BTreeMap::new();
        for (i, note) in list_field(obj, "footnotes").into_iter().enumerate() {
            map.insert(u32::try_from(i + 1).unwrap_or(u32::MAX), *n);
            notes.push((*n, note));
            *n += 1;
        }
        list_field(obj, "paragraphs")
            .iter()
            .map(|t| format!("<p>{}</p>", h(t, Some(&map))))
            .collect()
    };

    let first = find(turns, "open")?;
    let close = find(turns, "close")?;
    parts.extend(paragraphs(&first.obj, &mut n));
    let reply_class = r.reply_class.as_deref().unwrap_or("reply");
    for v in &margins {
        let m = turns
            .iter()
            .find(|t| t.kind == "margin" && &t.voice == v)
            .ok_or_else(|| Error::Gate(format!("no margin from {v}")))?;
        parts.push(format!(
            "<aside class=\"{v}\">\n  {}\n  <span class=\"{reply_class}\">{}</span>\n</aside>",
            h(str_field(&m.obj, "margin"), None),
            h(str_field(&close.obj, &reply_key(v)), None)
        ));
    }
    parts.extend(paragraphs(&close.obj, &mut n));
    for t in turns.iter().filter(|t| t.kind == "last") {
        parts.push(format!(
            "<aside class=\"{}\">\n  {}\n</aside>",
            t.voice,
            h(str_field(&t.obj, "margin"), None)
        ));
    }
    if let Some(sig) = &r.signature_html {
        parts.push(sig.replace("{year}", plan.date.get(..4).unwrap_or("")));
    }
    if let Some((first_n, _)) = notes.first() {
        let items: Vec<String> = notes
            .iter()
            .map(|(_, text)| format!("    <li>{}</li>", h(text, None)))
            .collect();
        parts.push(format!(
            "<div class=\"fn-list\">\n  <ol start=\"{first_n}\">\n{}\n  </ol>\n</div>",
            items.join("\n")
        ));
    }

    let t = &plan.topic;
    let mut fm: Vec<String> = vec![
        "---".into(),
        format!("layout: {}", r.layout.as_deref().unwrap_or("post")),
        format!(
            "title: {}",
            yaml_scalar(str_field(&first.obj, "title").trim())
        ),
    ];
    if let Some(a) = &r.attribution {
        fm.push(format!("attribution: {}", yaml_scalar(a)));
    }
    fm.push("stub: false".into());
    for (k, v) in &p.data.disclosure.front_matter {
        fm.push(format!("{k}: {}", yaml_scalar(v)));
    }
    let voices: Vec<String> = p
        .persona_ids()
        .iter()
        .map(|id| p.persona_stem(id))
        .collect();
    fm.extend([
        "exchange:".into(),
        format!("  id: {}", yaml_scalar(&plan.id)),
        format!("  topic: {}", yaml_scalar(&t.key)),
        format!("  voices: {}", yaml_scalar(&voices)),
    ]);
    if let Some(c) = &t.continued_from {
        fm.push(format!("  continued_from: {}", yaml_scalar(c)));
    }
    let occasion = format!(
        "{}{}",
        if t.question.is_some() {
            "a continuation on "
        } else {
            "an exchange on "
        },
        t.title
    );
    fm.extend([
        "entry_meta:".into(),
        "  - k: \"Added\"".into(),
        format!("    v: {}", yaml_scalar(&plan.date)),
        "  - k: \"Occasion\"".into(),
        format!("    v: {}", yaml_scalar(&occasion)),
        "  - k: \"Held open\"".into(),
        format!(
            "    v: {}",
            yaml_scalar(str_field(&close.obj, "held_open").trim())
        ),
    ]);
    if !t.url.is_empty() {
        fm.extend([
            "see_also:".into(),
            format!("  - title: {}", yaml_scalar(&t.title)),
            format!("    url: {}", yaml_scalar(&t.url)),
        ]);
    }
    fm.push("---".into());
    Ok(Rendered {
        path: path(p, plan),
        text: format!("{}\n{}\n", fm.join("\n"), parts.join("\n")),
        last_note: n.saturating_sub(1),
    })
}

pub(super) fn record(turns: &[TurnResult]) -> Record {
    let title = turns
        .iter()
        .find(|t| t.kind == "open")
        .map(|t| str_field(&t.obj, "title").trim().to_string())
        .unwrap_or_default();
    let close = turns.iter().find(|t| t.kind == "close").map(|t| &t.obj);
    Record {
        title,
        summary: close
            .map(|o| str_field(o, "summary").trim().to_string())
            .unwrap_or_default(),
        held_open: close
            .map(|o| str_field(o, "held_open").trim().to_string())
            .unwrap_or_default(),
    }
}
