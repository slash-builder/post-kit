//! One persona, one post. Writes a markdown file with front matter, ready for a Jekyll `_posts` folder.

use super::{
    list_field, object_schema, required_fields, signature_regex, str_field, string_array, Record,
    Rendered,
};
use crate::checks::{check_footnote_markers, check_text_fields};
use crate::error::{Error, Result};
use crate::guard::Guard;
use crate::profile::{Profile, Turn};
use crate::run::{Plan, TurnResult};
use crate::text::{slug, yaml_scalar};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

pub(super) fn validate(p: &Profile) -> Result<()> {
    let t = &p.data.turns;
    if t.len() == 1 && t[0].kind == "post" {
        Ok(())
    } else {
        Err(Error::Profile(
            "the post format needs exactly one turn: {\"voice\": ..., \"kind\": \"post\"}".into(),
        ))
    }
}

pub(super) fn schema(p: &Profile) -> Value {
    let lim = &p.data.limits;
    let s = || serde_json::json!({"type": "string"});
    let mut props = Map::new();
    props.insert("title".into(), s());
    props.insert("summary".into(), s());
    props.insert("paragraphs".into(), string_array(1, lim.max_paragraphs));
    props.insert("footnotes".into(), string_array(0, lim.max_footnotes));
    props.insert("tags".into(), string_array(0, lim.max_tags));
    props.insert("held_open".into(), s());
    object_schema(props)
}

pub(super) fn instruction(p: &Profile, spec: &Turn, plan: &Plan) -> String {
    let lim = &p.data.limits;
    let mut base = format!(
        "Write a blog post on the topic. Give a title of at most {} words (no quotation marks), a summary of at most {} words, 1 to {} paragraphs of at most {} words each, up to {} footnotes of at most {} words (mark each in a paragraph exactly once with [^1], [^2] in order), up to {} short lowercase tags, and held_open: a question this post leaves for a later post (at most {} words).",
        lim.title_words, lim.summary_words, lim.max_paragraphs, lim.paragraph_words, lim.max_footnotes, lim.footnote_words, lim.max_tags, lim.held_open_words
    );
    if let Some(q) = &plan.topic.question {
        base.push_str(&format!(
            " This continues an earlier post. Take up its open question: {q}"
        ));
    }
    format!("{base} {}", spec.ask).trim().to_string()
}

pub(super) fn check(p: &Profile, obj: &Map<String, Value>, guard: &Guard) -> Result<Vec<String>> {
    let lim = &p.data.limits;
    let want = required_fields(&schema(p));
    let mut have: Vec<&str> = obj.keys().map(String::as_str).collect();
    have.sort_unstable();
    let mut sorted: Vec<&str> = want.iter().map(String::as_str).collect();
    sorted.sort_unstable();
    if have != sorted {
        return Ok(vec![format!("fields must be exactly {want:?}")]);
    }
    let limits: BTreeMap<String, usize> = [
        ("paragraphs", lim.paragraph_words),
        ("footnotes", lim.footnote_words),
        ("summary", lim.summary_words),
        ("held_open", lim.held_open_words),
        ("title", lim.title_words),
        ("tags", 3),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();
    let sig = signature_regex(p)?;
    let mut problems = check_text_fields(obj, &limits, guard, sig.as_ref());
    problems.extend(check_footnote_markers(
        &list_field(obj, "paragraphs"),
        &list_field(obj, "footnotes"),
    ));
    if str_field(obj, "title")
        .chars()
        .any(|c| matches!(c, '"' | '\u{201c}' | '\u{201d}'))
    {
        problems.push("title must not contain quotation marks".into());
    }
    Ok(problems)
}

pub(super) fn render(p: &Profile, plan: &Plan, turns: &[TurnResult]) -> Result<Rendered> {
    let r = &p.data.render;
    let obj = &turns
        .first()
        .ok_or_else(|| Error::Gate("no turn was produced".into()))?
        .obj;
    let title = str_field(obj, "title").trim().to_string();
    let mut fm: Vec<String> = vec![
        "---".into(),
        format!("layout: {}", r.layout.as_deref().unwrap_or("post")),
        format!("title: {}", yaml_scalar(&title)),
        format!("date: {}", plan.date),
        format!(
            "description: {}",
            yaml_scalar(str_field(obj, "summary").trim())
        ),
        format!(
            "tags: {}",
            yaml_scalar(&normalised_tags(&list_field(obj, "tags")))
        ),
    ];
    if let Some(a) = &r.author {
        fm.push(format!("author: {}", yaml_scalar(a)));
    }
    for (k, v) in &p.data.disclosure.front_matter {
        fm.push(format!("{k}: {}", yaml_scalar(v)));
    }
    fm.push(format!("post_id: {}", yaml_scalar(&plan.id)));
    fm.push(format!("topic: {}", yaml_scalar(&plan.topic.key)));
    if let Some(c) = &plan.topic.continued_from {
        fm.push(format!("continued_from: {}", yaml_scalar(c)));
    }
    fm.push("---".into());
    let mut body = list_field(obj, "paragraphs").join("\n\n");
    if let Some(b) = &r.byline_markdown {
        body.push_str("\n\n");
        body.push_str(&b.replace("{year}", plan.date.get(..4).unwrap_or("")));
    }
    let notes = list_field(obj, "footnotes");
    if !notes.is_empty() {
        body.push_str("\n\n");
        body.push_str(
            &notes
                .iter()
                .enumerate()
                .map(|(i, n)| format!("[^{}]: {n}", i + 1))
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
    let path = format!(
        "{}/{}-{}.md",
        p.data.output.directory,
        plan.date,
        slug(&title, 60)
    );
    Ok(Rendered {
        path,
        text: format!("{}\n{body}\n", fm.join("\n")),
        last_note: 0,
    })
}

pub(super) fn record(turns: &[TurnResult]) -> Record {
    let o = turns.first().map(|t| &t.obj);
    Record {
        title: o
            .map(|o| str_field(o, "title").trim().to_string())
            .unwrap_or_default(),
        summary: o
            .map(|o| str_field(o, "summary").trim().to_string())
            .unwrap_or_default(),
        held_open: o
            .map(|o| str_field(o, "held_open").trim().to_string())
            .unwrap_or_default(),
    }
}

/// Tags as a site wants them: lowercase words joined by hyphens, no empties or repeats. Code fixes this; a post is
/// never rejected for it.
fn normalised_tags(tags: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for tag in tags.iter().filter(|t| t.chars().any(char::is_alphanumeric)) {
        let t = slug(tag, 30);
        if !out.contains(&t) {
            out.push(t);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::normalised_tags;

    #[test]
    fn tags_are_normalised_not_rejected() {
        let t =
            |v: &[&str]| normalised_tags(&v.iter().map(|s| (*s).to_string()).collect::<Vec<_>>());
        assert_eq!(
            t(&["Game Log", "game-log", "  Notes  ", "", "WEREWOLF"]),
            ["game-log", "notes", "werewolf"]
        );
    }
}
