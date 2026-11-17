//! Choose what to write about: pages of the site, a list of briefs, or the question the last post left open.

use crate::error::{io_err, json_err, Error, Result};
use crate::guard::Guard;
use crate::profile::LogCfg;
use crate::profile::Profile;
use crate::state::State;
use crate::text::{fm_value, split_front_matter, to_plain, PlainRules};
use crate::util::{civil_from_days, sha_hex};
use serde::Deserialize;
use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// A thing that could be written about.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Candidate {
    /// `collection/name` or `list/key`.
    pub key: String,
    /// Display title.
    pub title: String,
    /// The page's URL, or empty.
    pub url: String,
    /// True for a page marked `stub: true`.
    pub stub: bool,
    /// True for a page the bot itself wrote.
    pub is_entry: bool,
    /// The page text or the brief.
    pub text: String,
    /// Position in an ordered source (a log); such topics are chosen oldest first. `None` for pages and briefs.
    pub order: Option<usize>,
}

/// The topic chosen for a run.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Pick {
    /// What to write about.
    pub candidate: Candidate,
    /// Set when this run continues an earlier open question.
    pub question: Option<String>,
    /// The earlier post being continued.
    pub continued_from: Option<String>,
}

#[derive(Deserialize)]
struct ListItem {
    key: String,
    title: String,
    brief: String,
    #[serde(default)]
    url: String,
}

/// Every candidate the profile offers: pages of the configured collections, then list items.
///
/// # Errors
/// Returns an error if a folder or list file cannot be read.
pub fn candidates(profile: &Profile) -> Result<Vec<Candidate>> {
    let t = &profile.data.topics;
    let rules = PlainRules::new(&t.plain_rules)?;
    let prefix = (!profile.data.output.file_prefix.is_empty())
        .then(|| format!("{}-", profile.data.output.file_prefix));
    let mut out = Vec::new();
    for (coll, folder) in &t.collections {
        let dir = profile.under_root(folder);
        if !dir.is_dir() {
            continue;
        }
        let mut files: Vec<_> = std::fs::read_dir(&dir)
            .map_err(io_err(&dir))?
            .filter_map(std::result::Result::ok)
            .map(|e| e.path())
            .collect();
        files.sort();
        for f in files
            .into_iter()
            .filter(|f| f.extension().is_some_and(|e| e == "md"))
        {
            let stem = f
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let key = format!("{coll}/{stem}");
            if t.skip.contains(&key) {
                continue;
            }
            let text = std::fs::read_to_string(&f).map_err(io_err(&f))?;
            let (fm, body) = split_front_matter(&text);
            out.push(Candidate {
                title: fm_value(fm, "title").unwrap_or_else(|| stem.clone()),
                stub: fm_value(fm, "stub").is_some_and(|v| v.eq_ignore_ascii_case("true")),
                is_entry: prefix.as_deref().is_some_and(|p| stem.starts_with(p)),
                url: t
                    .permalinks
                    .get(coll)
                    .map_or_else(|| format!("/{coll}/{{name}}/"), Clone::clone)
                    .replace("{name}", &stem),
                text: to_plain(body, &rules),
                order: None,
                key,
            });
        }
    }
    if let Some(lf) = &t.list_file {
        let path = profile.under_profile(lf);
        let items: Vec<ListItem> =
            serde_json::from_str(&std::fs::read_to_string(&path).map_err(io_err(&path))?)
                .map_err(json_err(format!("topic list {}", path.display())))?;
        out.extend(items.into_iter().map(|i| Candidate {
            key: format!("list/{}", i.key),
            title: i.title,
            url: i.url,
            stub: false,
            is_entry: false,
            text: i.brief,
            order: None,
        }));
    }
    if let Some(log) = &t.log {
        out.extend(log_candidates(profile, log)?);
    }
    Ok(out)
}

fn scalar(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// The first of `keys` (or `defaults` when none are configured) that holds a non-empty scalar.
fn pick_field(
    obj: &Map<String, Value>,
    keys: &[String],
    defaults: &[&str],
) -> Option<(String, String)> {
    let list: Vec<&str> = if keys.is_empty() {
        defaults.to_vec()
    } else {
        keys.iter().map(String::as_str).collect()
    };
    list.into_iter().find_map(|k| {
        obj.get(k)
            .and_then(scalar)
            .filter(|s| !s.is_empty())
            .map(|s| (k.to_string(), s))
    })
}

/// The calendar day of a time value: ISO text, or epoch seconds or milliseconds. Anything else is `undated`.
fn day_of(s: &str) -> String {
    let b = s.as_bytes();
    if b.len() >= 10 && b[4] == b'-' && b[7] == b'-' {
        return s[..10].to_string();
    }
    match s.parse::<f64>() {
        Ok(n) if n.is_finite() => {
            let secs = if n > 1e11 { n / 1000.0 } else { n };
            if secs >= 86_400.0 {
                civil_from_days((secs / 86_400.0) as i64)
            } else {
                "undated".into()
            }
        }
        _ => "undated".into(),
    }
}

/// One event as a line of text: `who: kind text key=value ...`.
fn event_line(obj: &Map<String, Value>, cfg: &LogCfg) -> (String, String) {
    let f = &cfg.fields;
    let time = pick_field(obj, &f.time, &["ts", "t", "time", "timestamp"]);
    let who = pick_field(obj, &f.who, &["who", "agent", "name", "player"]);
    let kind = pick_field(obj, &f.kind, &["kind", "type", "event"]);
    let text = pick_field(obj, &f.text, &["text", "msg", "message"]);
    let used: Vec<&str> = [&time, &who, &kind, &text]
        .iter()
        .filter_map(|o| o.as_ref().map(|(k, _)| k.as_str()))
        .collect();
    let mut parts = Vec::new();
    if let Some((_, w)) = &who {
        parts.push(format!("{w}:"));
    }
    if let Some((_, k)) = &kind {
        parts.push(k.clone());
    }
    if let Some((_, x)) = &text {
        parts.push(x.clone());
    }
    for (k, v) in obj {
        if !used.contains(&k.as_str()) {
            if let Some(s) = scalar(v) {
                parts.push(format!("{k}={s}"));
            }
        }
    }
    (
        time.map_or_else(|| "undated".into(), |(_, t)| day_of(&t)),
        parts.join(" "),
    )
}

/// Read events from JSON lines, or from a JSON document that is a list or has an `events` list.
fn read_events(text: &str) -> Vec<Map<String, Value>> {
    if let Ok(doc) = serde_json::from_str::<Value>(text) {
        let list = match doc {
            Value::Array(a) => Some(a),
            Value::Object(mut o) => o.remove("events").and_then(|e| {
                if let Value::Array(a) = e {
                    Some(a)
                } else {
                    None
                }
            }),
            _ => None,
        };
        if let Some(a) = list {
            return a
                .into_iter()
                .filter_map(|v| {
                    if let Value::Object(o) = v {
                        Some(o)
                    } else {
                        None
                    }
                })
                .collect();
        }
    }
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| match serde_json::from_str::<Value>(l) {
            Ok(Value::Object(o)) => Some(o),
            _ => {
                tracing::warn!("skipping a log line that is not a JSON object");
                None
            }
        })
        .collect()
}

/// Group `(day, line)` pairs, keeping order. Returns `(group id, lines)`.
fn group_events(events: Vec<(String, String)>, cfg: &LogCfg) -> Vec<(String, Vec<String>)> {
    if cfg.group_by == "day" {
        let mut groups: Vec<(String, Vec<String>)> = Vec::new();
        for (day, line) in events {
            match groups.last_mut() {
                Some((d, lines)) if *d == day => lines.push(line),
                _ => groups.push((day, vec![line])),
            }
        }
        return groups;
    }
    let n = cfg.count.max(1);
    let lines: Vec<String> = events.into_iter().map(|(_, l)| l).collect();
    lines
        .chunks(n)
        .enumerate()
        .map(|(i, c)| {
            (
                format!("{:04}-{:04}", i * n + 1, i * n + c.len()),
                c.to_vec(),
            )
        })
        .collect()
}

/// Topics from an event log, oldest first. A line holding a listed term is replaced, so the model never sees it.
fn log_candidates(profile: &Profile, cfg: &LogCfg) -> Result<Vec<Candidate>> {
    let path = profile.under_profile(&cfg.file);
    let text = std::fs::read_to_string(&path).map_err(io_err(&path))?;
    let guard = Guard::load(profile.blocklist().as_deref())?;
    let events: Vec<(String, String)> = read_events(&text)
        .iter()
        .map(|o| event_line(o, cfg))
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(day, line)| {
            if guard.scan(&line).is_empty() {
                (day, line)
            } else {
                (day, "[line removed]".to_string())
            }
        })
        .collect();
    Ok(group_events(events, cfg)
        .into_iter()
        .enumerate()
        .map(|(i, (id, lines))| Candidate {
            key: format!("log/{id}"),
            title: cfg.title.replace("{group}", &id),
            url: String::new(),
            stub: false,
            is_entry: false,
            text: lines.join("\n"),
            order: Some(i),
        })
        .collect())
}

fn unit(seed: &str, salt: &str) -> f64 {
    let h = sha_hex(&format!("{seed}{salt}"));
    f64::from(u32::from_str_radix(&h[..8], 16).unwrap_or(0)) / f64::from(u32::MAX)
}

/// The least-discussed candidate: unused first, then pages the bot did not write itself, then stubs,
/// then least recently used, with a seeded hash to break ties.
///
/// # Errors
/// Returns an error if `forced` names no candidate, or there are no candidates.
pub fn choose<'a>(
    cands: &'a [Candidate],
    state: &State,
    seed: &str,
    forced: Option<&str>,
) -> Result<&'a Candidate> {
    if let Some(key) = forced {
        return cands.iter().find(|c| c.key == key).ok_or_else(|| {
            Error::Usage(format!(
                "no such topic: {key} (use `plan` to see what is available)"
            ))
        });
    }
    let mut used: BTreeMap<&str, usize> = BTreeMap::new();
    let mut last: BTreeMap<&str, &str> = BTreeMap::new();
    for e in &state.entries {
        *used.entry(e.topic.as_str()).or_default() += 1;
        let slot = last.entry(e.topic.as_str()).or_insert("");
        if e.date.as_str() > *slot {
            *slot = e.date.as_str();
        }
    }
    cands
        .iter()
        .min_by_key(|c| {
            (
                used.get(c.key.as_str()).copied().unwrap_or(0),
                c.is_entry,
                u8::from(!c.stub),
                last.get(c.key.as_str()).copied().unwrap_or("").to_string(),
                // Log topics are taken oldest first; everything else in seeded random order.
                c.order.map_or_else(
                    || sha_hex(&format!("{seed}{}", c.key)),
                    |o| format!("{o:08}"),
                ),
            )
        })
        .ok_or_else(|| {
            Error::Usage("no topics available: check the profile's topics section".into())
        })
}

/// The topic for a run. With drift enabled, sometimes takes up the last post's open question.
///
/// # Errors
/// Returns an error if the candidates cannot be read or none fits.
pub fn pick(profile: &Profile, state: &State, seed: &str, forced: Option<&str>) -> Result<Pick> {
    let cands = candidates(profile)?;
    if forced.is_none() {
        if let (Some(drift), Some(last)) = (&profile.data.topics.drift, state.entries.last()) {
            if !last.held_open.is_empty() && unit(seed, "drift") < drift.continue_probability {
                if let Some(c) = cands.iter().find(|c| c.key == last.topic) {
                    return Ok(Pick {
                        candidate: c.clone(),
                        question: Some(last.held_open.clone()),
                        continued_from: Some(last.id.clone()),
                    });
                }
            }
        }
    }
    Ok(Pick {
        candidate: choose(&cands, state, seed, forced)?.clone(),
        question: None,
        continued_from: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(group_by: &str, count: usize) -> LogCfg {
        serde_json::from_value(
            serde_json::json!({"file": "x", "group_by": group_by, "count": count}),
        )
        .unwrap()
    }

    #[test]
    fn json_lines_and_json_documents_both_read() {
        let lines = "{\"a\":1}\n\nnot json\n{\"b\":2}\n";
        assert_eq!(read_events(lines).len(), 2);
        assert_eq!(read_events("[{\"a\":1},{\"b\":2},3]").len(), 2);
        assert_eq!(read_events("{\"events\":[{\"a\":1}]}").len(), 1);
        assert_eq!(
            read_events("{\"a\":1}").len(),
            1,
            "a single object is one JSON line"
        );
    }

    #[test]
    fn lines_use_the_common_field_names_and_keep_the_rest() {
        let o: Map<String, Value> = serde_json::from_value(serde_json::json!({"ts": "2026-11-01T10:00:00Z", "agent": "Ada", "type": "mine", "item": "iron", "count": 3})).unwrap();
        let (day, line) = event_line(&o, &cfg("day", 1));
        assert_eq!(
            (day.as_str(), line.as_str()),
            ("2026-11-01", "Ada: mine item=iron count=3")
        );
        let o: Map<String, Value> =
            serde_json::from_value(serde_json::json!({"msg": "hello"})).unwrap();
        assert_eq!(
            event_line(&o, &cfg("day", 1)),
            ("undated".to_string(), "hello".to_string())
        );
    }

    #[test]
    fn days_come_from_iso_text_or_epoch() {
        assert_eq!(day_of("2026-11-01T10:00:00Z"), "2026-11-01");
        assert_eq!(day_of("1793491200"), "2026-11-01");
        assert_eq!(day_of("1793491200000"), "2026-11-01");
        assert_eq!(day_of("500"), "undated");
        assert_eq!(day_of("soon"), "undated");
    }

    #[test]
    fn grouping_by_day_and_by_count_keeps_order() {
        let ev = |d: &str, l: &str| (d.to_string(), l.to_string());
        let events = vec![ev("d1", "a"), ev("d1", "b"), ev("d2", "c")];
        let by_day = group_events(events.clone(), &cfg("day", 0));
        assert_eq!(
            by_day,
            vec![
                ("d1".to_string(), vec!["a".to_string(), "b".to_string()]),
                ("d2".to_string(), vec!["c".to_string()])
            ]
        );
        let by_count = group_events(events, &cfg("count", 2));
        assert_eq!(
            by_count
                .iter()
                .map(|(id, l)| (id.as_str(), l.len()))
                .collect::<Vec<_>>(),
            vec![("0001-0002", 2), ("0003-0003", 1)]
        );
    }
}
