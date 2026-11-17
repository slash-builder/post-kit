//! Conversation format: staging, retries, gates, caps, holds.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{first_file, manifest, Env, Fake, Mode};
use post_kit::run::{apply, stage, StageOptions, Status};
use post_kit::Error;

fn opts(env: &Env, name: &str, date: &str) -> StageOptions {
    let mut o = StageOptions::new(env.out(name));
    o.date = Some(date.into());
    o
}

#[test]
fn a_full_run_stages_files_and_changes_nothing_else() {
    let env = Env::new("duet");
    let fake = Fake::new(Mode::Ok);
    let before = std::fs::read_to_string(env.root.join("_config.yml")).unwrap();
    let staged = stage(&env.profile(), &fake, &opts(&env, "out", "2026-11-01")).unwrap();
    assert_eq!(staged.status, Status::Ok);
    assert!(staged.digest.contains("STATUS: OK"));
    let out = env.out("out");
    let entry = std::fs::read_to_string(first_file(&out.join("files/_wiki"), "exchange-")).unwrap();
    for needle in [
        "ai_generated: true",
        "reviewed: false",
        "<aside class=\"critic\">",
        "title=\"See note 5.\"",
        "title=\"See note 6.\"",
    ] {
        assert!(entry.contains(needle), "{needle}");
    }
    assert!(!entry.contains("<script"));
    assert!(std::fs::read_to_string(out.join("files/_config.yml"))
        .unwrap()
        .contains("current_note: 6"));
    let man = manifest(&out);
    assert_eq!(man["publishable"], true);
    let id = man["id"].as_str().unwrap();
    let mut want = vec![
        "_config.yml".to_string(),
        "_data/post-kit-state.json".to_string(),
        format!("_wiki/exchange-{id}.md"),
    ];
    want.sort();
    let got: Vec<String> = man["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert_eq!(got, want);
    assert_eq!(
        std::fs::read_to_string(env.root.join("_config.yml")).unwrap(),
        before,
        "nothing is applied until `apply`"
    );
    assert_eq!(fake.calls(), 5, "4 turns + 1 review");
}

#[test]
fn apply_then_next_run_moves_on_and_the_daily_cap_applies() {
    let env = Env::new("duet");
    let fake = Fake::new(Mode::Ok);
    stage(&env.profile(), &fake, &opts(&env, "o1", "2026-11-01")).unwrap();
    apply(&env.profile(), &env.out("o1")).unwrap();
    let second = stage(&env.profile(), &fake, &opts(&env, "o2", "2026-11-01")).unwrap();
    assert_eq!(second.status, Status::Skipped);
    assert!(second.digest.contains("daily cap"));
    stage(&env.profile(), &fake, &opts(&env, "o3", "2026-11-04")).unwrap();
    let state: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(env.out("o3").join("files/_data/post-kit-state.json")).unwrap(),
    )
    .unwrap();
    let e = state["entries"].as_array().unwrap();
    assert_eq!(e.len(), 2);
    assert_ne!(e[0]["topic"], e[1]["topic"]);
}

#[test]
fn the_pause_file_stops_a_run_before_any_model_call() {
    let env = Env::new("duet");
    std::fs::write(env.root.join("PAUSE"), "").unwrap();
    let fake = Fake::new(Mode::Ok);
    let staged = stage(&env.profile(), &fake, &opts(&env, "out", "2026-11-01")).unwrap();
    assert_eq!(staged.status, Status::Skipped);
    assert!(staged.digest.contains("paused"));
    assert_eq!(fake.calls(), 0);
}

#[test]
fn a_leak_is_retried_then_passes() {
    let env = Env::new("duet");
    let fake = Fake::new(Mode::BadNameOnce);
    let staged = stage(&env.profile(), &fake, &opts(&env, "out", "2026-11-01")).unwrap();
    assert_eq!(staged.status, Status::Ok);
    assert!(staged.digest.contains("retries: 1"));
    let t: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(env.out("out").join("transcript.json")).unwrap(),
    )
    .unwrap();
    assert!(t["turns"][0]["rejected_for"][0][0]
        .as_str()
        .unwrap()
        .contains("listed term"));
}

#[test]
fn a_persistent_leak_fails_the_gate_and_writes_no_post() {
    let env = Env::new("duet");
    let err = stage(
        &env.profile(),
        &Fake::new(Mode::AlwaysBad),
        &opts(&env, "out", "2026-11-01"),
    )
    .unwrap_err();
    assert!(matches!(err, Error::Gate(_)));
    assert_eq!(err.exit_code(), 73);
    assert_eq!(manifest(&env.out("out"))["status"], "FAILED");
    assert!(!env.out("out").join("files/_wiki").exists());
}

#[test]
fn the_cost_cap_stops_the_run() {
    let env = Env::new("duet");
    let err = stage(
        &env.profile(),
        &Fake::new(Mode::Cost),
        &opts(&env, "out", "2026-11-01"),
    )
    .unwrap_err();
    assert!(matches!(err, Error::CostCap { .. }), "{err}");
}

#[test]
fn flagged_posts_are_held_by_default_and_publishable_when_allowed() {
    let env = Env::new("duet");
    let staged = stage(
        &env.profile(),
        &Fake::new(Mode::Look),
        &opts(&env, "out", "2026-11-01"),
    )
    .unwrap();
    assert_eq!(staged.status, Status::Attention);
    assert_eq!(staged.exit_code(), 3);
    assert_eq!(manifest(&env.out("out"))["publishable"], false);
    env.set("publish", "on_attention", serde_json::json!("publish"));
    stage(
        &env.profile(),
        &Fake::new(Mode::Look),
        &opts(&env, "out", "2026-11-01"),
    )
    .unwrap();
    assert_eq!(manifest(&env.out("out"))["publishable"], true);
}

#[test]
fn invented_references_raise_attention() {
    let env = Env::new("duet");
    let staged = stage(
        &env.profile(),
        &Fake::new(Mode::Invented),
        &opts(&env, "out", "2026-11-01"),
    )
    .unwrap();
    assert_eq!(staged.status, Status::Attention);
    assert!(staged.digest.contains("references not found"));
}

#[test]
fn a_missing_blocklist_refuses_to_run() {
    let env = Env::new("duet");
    std::fs::remove_file(env.ex.join("blocklist.sha256")).unwrap();
    let err = stage(
        &env.profile(),
        &Fake::new(Mode::Ok),
        &opts(&env, "out", "2026-11-01"),
    )
    .unwrap_err();
    assert!(matches!(err, Error::Usage(_)));
    assert_eq!(err.exit_code(), 64);
}

#[test]
fn an_existing_target_is_not_overwritten_without_force() {
    let env = Env::new("duet");
    let fake = Fake::new(Mode::Ok);
    let mut first = opts(&env, "o1", "2026-11-01");
    first.topic = Some("wiki/alpha".into());
    stage(&env.profile(), &fake, &first).unwrap();
    apply(&env.profile(), &env.out("o1")).unwrap();
    env.set("publish", "max_per_day", serde_json::json!(0)); // lift the cap so only the existing file stands in the way
    let mut again = opts(&env, "o2", "2026-11-01");
    again.topic = Some("wiki/alpha".into());
    let err = stage(&env.profile(), &fake, &again).unwrap_err();
    assert!(
        matches!(&err, Error::Usage(m) if m.contains("already exists")),
        "{err}"
    );
    again.force = true;
    assert!(stage(&env.profile(), &fake, &again).is_ok());
}

#[test]
fn a_proposal_cannot_touch_paths_the_profile_does_not_allow() {
    let env = Env::new("duet");
    stage(
        &env.profile(),
        &Fake::new(Mode::Ok),
        &opts(&env, "out", "2026-11-01"),
    )
    .unwrap();
    std::fs::write(env.out("out").join("files/Gemfile"), "gem 'x'").unwrap();
    assert!(apply(&env.profile(), &env.out("out")).is_err());
    assert!(!env.root.join("Gemfile").exists());
}

#[cfg(unix)]
#[test]
fn a_symlink_in_a_proposal_is_refused() {
    let env = Env::new("duet");
    stage(
        &env.profile(),
        &Fake::new(Mode::Ok),
        &opts(&env, "out", "2026-11-01"),
    )
    .unwrap();
    std::os::unix::fs::symlink("/etc/passwd", env.out("out").join("files/_wiki/link.md")).unwrap();
    let err = apply(&env.profile(), &env.out("out")).unwrap_err();
    assert!(err.to_string().contains("symlink"));
}

#[test]
fn calibrate_counts_how_often_known_good_pages_are_flagged() {
    use post_kit::run::calibrate;
    let env = Env::new("duet");
    let quiet = Fake::new(Mode::Ok);
    let cal = calibrate(&env.profile(), &quiet, 3).unwrap();
    assert_eq!(
        (cal.rows.len(), cal.reviews, cal.flagged),
        (2, 6, 0),
        "two pages, three reviews each"
    );
    assert_eq!(quiet.calls(), 6);
    assert_eq!(cal.flag_rate(), 0.0);
    let jumpy = Fake::new(Mode::Look);
    let cal = calibrate(&env.profile(), &jumpy, 2).unwrap();
    assert_eq!((cal.reviews, cal.flagged), (4, 4));
    assert!(cal
        .rows
        .iter()
        .all(|r| r.flagged == 2 && r.kinds.len() == 2));
    assert!((cal.flag_rate() - 1.0).abs() < f64::EPSILON);
    // the spend cap applies here too
    assert!(matches!(
        calibrate(&env.profile(), &Fake::new(Mode::Cost), 1),
        Err(Error::CostCap { .. })
    ));
}

#[test]
fn the_cap_is_enforced_on_the_review_call_too() {
    // Four turns at $0.01 stay under $0.045; the fifth call (the review) crosses it.
    let env = Env::new("duet");
    env.set("limits", "max_cost_usd", serde_json::json!(0.045));
    let err = stage(
        &env.profile(),
        &Fake::new(Mode::Ok),
        &opts(&env, "out", "2026-11-01"),
    )
    .unwrap_err();
    assert!(matches!(err, Error::CostCap { .. }), "{err}");
    assert_eq!(manifest(&env.out("out"))["status"], "FAILED");
    assert!(!env.out("out").join("files/_wiki").exists());
}
