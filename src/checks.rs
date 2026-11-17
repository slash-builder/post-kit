//! Deterministic checks. The verdict on a model answer is made here, never by a model.

use crate::error::{Error, Result};
use crate::guard::{structure_problems, Guard};
use crate::util::const_regex;
use regex::Regex;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

static RE_MARKDOWN: LazyLock<Regex> =
    LazyLock::new(|| const_regex(r"(?m)^\s*(#|```|[-*] |\d+\. )"));
static RE_SIGNOFF: LazyLock<Regex> =
    LazyLock::new(|| const_regex(r"(?m)^\s*(\x{2014}|\x{2013}|--|&mdash;)\s*\S"));
static RE_FN_MARK: LazyLock<Regex> = LazyLock::new(|| const_regex(r"\[\^(\d+)\]"));

/// Number of whitespace-separated words.
///
/// ```
/// assert_eq!(post_kit::checks::words("one two  three"), 3);
/// ```
#[must_use]
pub fn words(s: &str) -> usize {
    s.split_whitespace().count()
}

/// Check every string field of a model answer.
///
/// A field fails if it is empty or not text, is over its word limit (`limits` maps field name to
/// words), is not plain text (links, addresses, paths, markup, markdown headings or lists), contains
/// a listed term, or ends with a sign-off (a dash line, or a match for `signature`).
///
/// ```
/// use post_kit::{checks::check_text_fields, guard::Guard};
/// let obj = serde_json::json!({"margin": "Fine."});
/// let limits = [("margin".to_string(), 5)].into();
/// assert!(check_text_fields(obj.as_object().unwrap(), &limits, &Guard::empty(), None).is_empty());
/// let long = serde_json::json!({"margin": "far too many words in this one"});
/// assert_eq!(check_text_fields(long.as_object().unwrap(), &limits, &Guard::empty(), None).len(), 1);
/// ```
#[must_use]
pub fn check_text_fields(
    obj: &Map<String, Value>,
    limits: &BTreeMap<String, usize>,
    guard: &Guard,
    signature: Option<&Regex>,
) -> Vec<String> {
    let mut problems = Vec::new();
    for (key, value) in obj {
        let items: Vec<&Value> = match value {
            Value::Array(a) => a.iter().collect(),
            other => vec![other],
        };
        for item in items {
            let Some(s) = item.as_str().filter(|s| !s.trim().is_empty()) else {
                problems.push(format!("{key} has an empty or non-text item"));
                continue;
            };
            if let Some(&n) = limits.get(key) {
                if words(s) > n {
                    problems.push(format!("{key} is {} words; the limit is {n}", words(s)));
                }
            }
            for p in structure_problems(s) {
                problems.push(format!("{key} contains {p}"));
            }
            if !guard.scan(s).is_empty() {
                problems.push(format!(
                    "{key} contains a listed term (never write, hint at or distort it)"
                ));
            }
            if RE_MARKDOWN.is_match(s) {
                problems.push(format!(
                    "{key} uses markdown structure; write plain sentences"
                ));
            }
            if RE_SIGNOFF.is_match(s) || signature.is_some_and(|re| re.is_match(s.trim())) {
                problems.push(format!(
                    "{key} ends with a signature; do not sign, the script adds it"
                ));
            }
        }
    }
    problems
}

/// Footnote markers `[^1]..[^n]` must each appear once, in the paragraphs, for `n` footnotes.
///
/// ```
/// use post_kit::checks::check_footnote_markers;
/// assert!(check_footnote_markers(&["a[^1] b[^2]".to_string()], &["x".to_string(), "y".to_string()]).is_empty());
/// assert_eq!(check_footnote_markers(&["a".to_string()], &["x".to_string()]).len(), 1);
/// ```
#[must_use]
pub fn check_footnote_markers(paragraphs: &[String], footnotes: &[String]) -> Vec<String> {
    let mut markers: Vec<u32> = paragraphs
        .iter()
        .flat_map(|p| {
            RE_FN_MARK
                .captures_iter(p)
                .filter_map(|c| c[1].parse().ok())
                .collect::<Vec<u32>>()
        })
        .collect();
    markers.sort_unstable();
    let n = u32::try_from(footnotes.len()).unwrap_or(u32::MAX);
    let want: Vec<u32> = (1..=n).collect();
    if markers == want {
        return Vec::new();
    }
    let shown = if markers.is_empty() {
        "none".to_string()
    } else {
        format!("{markers:?}")
    };
    vec![format!(
        "footnote markers {shown} must be [^1]..[^{n}] each used once ({n} footnotes given)"
    )]
}

/// References (section or note numbers, say) that appear in `texts` but not in the supplied material.
///
/// `pattern` is a regular expression with one or more capture groups; the first group that matches is
/// the reference. `extra_allowed` lists references that are fine even if absent from `allowed_text`.
/// With no pattern nothing is checked.
///
/// ```
/// use post_kit::checks::invented_references;
/// let pat = Some(r"§\s?([IVXLC]+)|\bnotes?\s+(\d+)");
/// let found = invented_references(pat, "see §VI", &["§VI and §XLII, note 99".to_string()], &[]).unwrap();
/// assert_eq!(found, vec!["§XLII".to_string(), "note 99".to_string()]);
/// ```
///
/// # Errors
/// Returns an error if the pattern is not a valid regular expression.
pub fn invented_references(
    pattern: Option<&str>,
    allowed_text: &str,
    texts: &[String],
    extra_allowed: &[String],
) -> Result<Vec<String>> {
    let Some(pattern) = pattern else {
        return Ok(Vec::new());
    };
    let re = Regex::new(&format!("(?i){pattern}"))
        .map_err(|e| Error::Profile(format!("bad reference_regex: {e}")))?;
    let key = |c: &regex::Captures<'_>| {
        c.iter()
            .skip(1)
            .flatten()
            .next()
            .map(|m| m.as_str().to_uppercase())
    };
    let mut allowed: BTreeSet<String> = re
        .captures_iter(allowed_text)
        .filter_map(|c| key(&c))
        .collect();
    allowed.extend(extra_allowed.iter().map(|s| s.to_uppercase()));
    let mut seen: Vec<String> = Vec::new();
    for text in texts {
        for c in re.captures_iter(text) {
            if let Some(k) = key(&c) {
                let whole = c[0].to_string();
                if !allowed.contains(&k) && !seen.contains(&whole) {
                    seen.push(whole);
                }
            }
        }
    }
    Ok(seen)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn problems(v: &Value, limits: &[(&str, usize)], sig: Option<&Regex>) -> Vec<String> {
        let l = limits.iter().map(|(k, n)| ((*k).to_string(), *n)).collect();
        check_text_fields(v.as_object().unwrap(), &l, &Guard::empty(), sig)
    }

    #[test]
    fn limits_markdown_signoff_and_empties() {
        assert!(problems(&json!({"m": "Fine."}), &[("m", 5)], None).is_empty());
        assert!(!problems(
            &json!({"m": "word word word word word word"}),
            &[("m", 5)],
            None
        )
        .is_empty());
        assert!(!problems(&json!({"m": "# head"}), &[], None).is_empty());
        assert!(!problems(&json!({"m": "- item"}), &[], None).is_empty());
        assert!(!problems(&json!({"m": "Text.\n\n\u{2014} Someone"}), &[], None).is_empty());
        let sig = Regex::new(r"who is no one\W*$").unwrap();
        assert!(!problems(
            &json!({"m": "Text, a Reader, who is no one."}),
            &[],
            Some(&sig)
        )
        .is_empty());
        assert!(!problems(&json!({"m": ""}), &[], None).is_empty());
        assert!(!problems(&json!({"m": 3}), &[], None).is_empty());
        assert!(!problems(&json!({"m": ["ok", ""]}), &[], None).is_empty());
    }

    #[test]
    fn a_listed_term_fails() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bl");
        crate::guard::add_terms(&["examplecorp"], &path).unwrap();
        let g = Guard::from_file(&path).unwrap();
        let obj = json!({"m": "Written for Example Corp."});
        assert!(
            !check_text_fields(obj.as_object().unwrap(), &BTreeMap::new(), &g, None).is_empty()
        );
    }

    #[test]
    fn footnote_markers() {
        let s = |x: &str| x.to_string();
        assert!(check_footnote_markers(&[s("a[^1] b[^2]")], &[s("x"), s("y")]).is_empty());
        assert!(!check_footnote_markers(&[s("a")], &[s("x")]).is_empty());
        assert!(!check_footnote_markers(&[s("a[^1] b[^1]")], &[s("x")]).is_empty());
        assert!(check_footnote_markers(&[s("a")], &[]).is_empty());
    }

    #[test]
    fn references() {
        let pat = Some(r"§\s?([IVXLC]+)|\bnotes?\s+(\d+)");
        let s = |x: &str| x.to_string();
        assert!(invented_references(
            pat,
            "see §VI and note 13",
            &[s("recall §VI, note 13")],
            &[s("26")]
        )
        .unwrap()
        .is_empty());
        assert_eq!(
            invented_references(pat, "see §VI", &[s("§XLII and note 99")], &[]).unwrap(),
            vec![s("§XLII"), s("note 99")]
        );
        assert!(invented_references(None, "", &[s("§XLII")], &[])
            .unwrap()
            .is_empty());
        assert!(invented_references(Some("("), "", &[], &[]).is_err());
    }
}
