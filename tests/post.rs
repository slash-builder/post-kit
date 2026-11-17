//! Post format, topic lists and drift.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{first_file, Env, Fake, Mode};
use post_kit::run::{stage, StageOptions, Status};

#[test]
fn a_single_voice_post_is_markdown_with_the_unreviewed_marker() {
    let env = Env::new("field-notes");
    let fake = Fake::new(Mode::Ok);
    let mut o = StageOptions::new(env.out("out"));
    o.date = Some("2026-11-01".into());
    o.topic = Some("list/calibration".into());
    let staged = stage(&env.profile(), &fake, &o).unwrap();
    assert_eq!(staged.status, Status::Ok);
    let f = first_file(&env.out("out").join("files/_posts"), "2026-11-01-");
    assert!(f
        .file_name()
        .unwrap()
        .to_string_lossy()
        .starts_with("2026-11-01-a-short-note"));
    let body = std::fs::read_to_string(f).unwrap();
    for needle in [
        "title: \"A Short Note\"",
        "reviewed: false",
        "[^1]: A small note.",
        "published without human review",
        "tags: [\"notes\"]",
    ] {
        assert!(body.contains(needle), "{needle}\n{body}");
    }
    assert_eq!(fake.calls(), 2, "one post turn + one review");
}

#[test]
fn list_topics_come_in_list_order_when_none_is_used() {
    let env = Env::new("field-notes");
    let cands = post_kit::topics::candidates(&env.profile()).unwrap();
    let keys: Vec<&str> = cands.iter().map(|c| c.key.as_str()).collect();
    assert_eq!(keys, ["list/glue", "list/calibration", "list/labels"]);
}

#[test]
fn drift_takes_up_the_last_open_question_only_when_asked() {
    use post_kit::state::{Entry, State};
    let env = Env::new("duet");
    let mut profile = env.profile();
    let mut entry = Entry::default();
    entry.id = "x".into();
    entry.date = "2026-11-01".into();
    entry.topic = "wiki/alpha".into();
    entry.held_open = "Why?".into();
    let mut st = State::default();
    st.entries.push(entry);
    let none = post_kit::topics::pick(&profile, &st, "2026-11-04", None).unwrap();
    assert!(none.question.is_none());
    assert_ne!(none.candidate.key, "wiki/alpha");
    let mut drift_cfg = post_kit::profile::Drift::default();
    drift_cfg.continue_probability = 1.0;
    profile.data.topics.drift = Some(drift_cfg);
    let drift = post_kit::topics::pick(&profile, &st, "2026-11-04", None).unwrap();
    assert_eq!(
        (
            drift.candidate.key.as_str(),
            drift.question.as_deref(),
            drift.continued_from.as_deref()
        ),
        ("wiki/alpha", Some("Why?"), Some("x"))
    );
}
