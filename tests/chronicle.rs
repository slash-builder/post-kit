#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Chronicling an event log: one topic per day, oldest first, with listed terms removed before the model sees them.

mod common;

use common::{Env, Fake, Mode};
use post_kit::run::{apply, build_plan, stage, StageOptions};
use post_kit::state::State;
use post_kit::topics;

fn staged_topic(env: &Env, name: &str, date: &str) -> String {
    let mut o = StageOptions::new(env.out(name));
    o.date = Some(date.into());
    stage(&env.profile(), &Fake::new(Mode::Ok), &o).unwrap();
    apply(&env.profile(), &env.out(name)).unwrap();
    let t: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(env.out(name).join("transcript.json")).unwrap(),
    )
    .unwrap();
    t["plan"]["topic"]["key"].as_str().unwrap().to_string()
}

#[test]
fn the_log_becomes_one_topic_per_day_oldest_first() {
    let env = Env::new("chronicle");
    let cands = topics::candidates(&env.profile()).unwrap();
    let keys: Vec<&str> = cands.iter().map(|c| c.key.as_str()).collect();
    assert_eq!(keys, ["log/2026-11-01", "log/2026-11-02"]);
    assert_eq!(cands[0].title, "Records for 2026-11-01");
    assert_eq!((cands[0].order, cands[1].order), (Some(0), Some(1)));
    assert!(cands[0].text.contains("Ada: mine item=oak_log count=4"));
    assert!(cands[0].text.contains("Bram: death"));
}

#[test]
fn a_line_with_a_listed_term_never_reaches_the_model() {
    let env = Env::new("chronicle");
    let cands = topics::candidates(&env.profile()).unwrap();
    assert!(cands[0].text.contains("[line removed]"));
    assert!(!cands[0].text.to_lowercase().contains("examplecorp"));
    assert!(!cands[0].text.contains("supplied the build plan"));
    // the plan (what the model is shown) is clean too
    let plan = build_plan(
        &env.profile(),
        &State::load(&env.profile()).unwrap(),
        "2026-11-01",
        None,
    )
    .unwrap();
    assert!(!plan.topic.excerpt.to_lowercase().contains("examplecorp"));
}

#[test]
fn runs_walk_forward_through_the_log_and_then_start_over() {
    let env = Env::new("chronicle");
    assert_eq!(staged_topic(&env, "o1", "2026-11-03"), "log/2026-11-01");
    assert_eq!(staged_topic(&env, "o2", "2026-11-04"), "log/2026-11-02");
    assert_eq!(staged_topic(&env, "o3", "2026-11-05"), "log/2026-11-01");
}

#[test]
fn a_chronicle_post_says_who_wrote_it_and_that_nobody_reviewed_it() {
    let env = Env::new("chronicle");
    let mut o = StageOptions::new(env.out("out"));
    o.date = Some("2026-11-03".into());
    stage(&env.profile(), &Fake::new(Mode::Ok), &o).unwrap();
    let post = std::fs::read_to_string(common::first_file(
        &env.out("out").join("files/_posts"),
        "2026",
    ))
    .unwrap();
    for needle in [
        "author: \"the Annalist\"",
        "reviewed: false",
        "published without human review",
        "topic: \"log/2026-11-01\"",
    ] {
        assert!(post.contains(needle), "{needle}\n{post}");
    }
}
