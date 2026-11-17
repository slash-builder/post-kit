//! Publishing: direct, pull request, hold, pause, paths and credentials.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{manifest, Env, Fake, Mode};
use post_kit::profile::PublishMode;
use post_kit::publish::{publish, Credentials, Outcome, PullRequest, PullRequests};
use post_kit::run::{stage, StageOptions};
use post_kit::Error;
use std::cell::RefCell;

struct FakePrs(RefCell<Vec<PullRequest>>);
impl FakePrs {
    fn new() -> Self {
        Self(RefCell::new(Vec::new()))
    }
}
impl PullRequests for FakePrs {
    fn open(&self, repository: &str, req: &PullRequest) -> post_kit::Result<String> {
        assert_eq!(repository, "o/r");
        self.0.borrow_mut().push(req.clone());
        Ok("http://example.invalid/pull/1".into())
    }
}

fn creds() -> Credentials {
    Credentials::new(Some("tok".into()), Some("o/r".into()))
}

fn staged(env: &Env, mode: Mode) {
    let mut o = StageOptions::new(env.out("out"));
    o.date = Some("2026-11-01".into());
    stage(&env.profile(), &Fake::new(mode), &o).unwrap();
}

fn log(env: &Env, rev: &str) -> Vec<String> {
    env.git(&["log", "--format=%s", rev])
        .lines()
        .map(str::to_string)
        .collect()
}

#[test]
fn direct_publish_commits_and_pushes_with_an_honest_message() {
    let env = Env::with_git("duet");
    staged(&env, Mode::Ok);
    let out = publish(
        &env.profile(),
        &env.out("out"),
        Some(PublishMode::Direct),
        &creds(),
        &FakePrs::new(),
    )
    .unwrap();
    assert!(matches!(out, Outcome::Published { .. }), "{out}");
    env.git(&["fetch", "-q", "origin"]);
    let msg = env.git(&["log", "-1", "--format=%B", "origin/main"]);
    assert!(msg.contains("No person reviewed this post"), "{msg}");
    assert!(msg.contains("Assisted-by: LLM"));
    let mut changed: Vec<String> = env
        .git(&["show", "--name-only", "--format=", "origin/main"])
        .lines()
        .map(str::to_string)
        .collect();
    changed.sort();
    let mut want: Vec<String> = manifest(&env.out("out"))["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    want.sort();
    assert_eq!(changed, want);
    // the next run in the same checkout sees the history, and the cap
    let mut o = StageOptions::new(env.out("o2"));
    o.date = Some("2026-11-01".into());
    assert!(stage(&env.profile(), &Fake::new(Mode::Ok), &o)
        .unwrap()
        .digest
        .contains("daily cap"));
}

#[test]
fn a_flagged_post_is_held_and_nothing_is_pushed() {
    let env = Env::with_git("duet");
    staged(&env, Mode::Look);
    let out = publish(
        &env.profile(),
        &env.out("out"),
        Some(PublishMode::Direct),
        &creds(),
        &FakePrs::new(),
    )
    .unwrap();
    assert_eq!(out, Outcome::Held);
    assert_eq!(log(&env, "origin/main"), ["base"]);
}

#[test]
fn the_pause_switch_stops_publishing_too() {
    let env = Env::with_git("duet");
    staged(&env, Mode::Ok);
    std::fs::write(env.root.join("PAUSE"), "").unwrap();
    let out = publish(
        &env.profile(),
        &env.out("out"),
        Some(PublishMode::Direct),
        &creds(),
        &FakePrs::new(),
    )
    .unwrap();
    assert_eq!(out, Outcome::Paused);
    assert_eq!(log(&env, "origin/main"), ["base"]);
}

#[test]
fn a_tampered_manifest_cannot_reach_outside_the_allowed_paths() {
    let env = Env::with_git("duet");
    staged(&env, Mode::Ok);
    for bad in ["../escape.txt", "Gemfile"] {
        let mut man = manifest(&env.out("out"));
        man["files"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!(bad));
        std::fs::write(env.out("out").join("manifest.json"), man.to_string()).unwrap();
        let err = publish(
            &env.profile(),
            &env.out("out"),
            Some(PublishMode::Direct),
            &creds(),
            &FakePrs::new(),
        )
        .unwrap_err();
        assert!(matches!(err, Error::Publish(_)), "{bad}: {err}");
    }
    assert_eq!(log(&env, "origin/main"), ["base"]);
}

#[test]
fn publishing_needs_a_token() {
    let env = Env::with_git("duet");
    staged(&env, Mode::Ok);
    let err = publish(
        &env.profile(),
        &env.out("out"),
        Some(PublishMode::Direct),
        &Credentials::new(None, None),
        &FakePrs::new(),
    )
    .unwrap_err();
    assert!(err.to_string().contains("POST_KIT_GIT_TOKEN"));
}

#[test]
fn stage_mode_never_pushes() {
    let env = Env::with_git("duet");
    staged(&env, Mode::Ok);
    let out = publish(
        &env.profile(),
        &env.out("out"),
        None,
        &creds(),
        &FakePrs::new(),
    )
    .unwrap();
    assert_eq!(out, Outcome::StagedOnly);
    assert_eq!(log(&env, "origin/main"), ["base"]);
}

#[test]
fn skipped_and_failed_runs_publish_nothing() {
    let env = Env::with_git("duet");
    std::fs::write(env.root.join("PAUSE"), "").unwrap();
    let mut o = StageOptions::new(env.out("out"));
    o.date = Some("2026-11-01".into());
    stage(&env.profile(), &Fake::new(Mode::Ok), &o).unwrap();
    std::fs::remove_file(env.root.join("PAUSE")).unwrap();
    let out = publish(
        &env.profile(),
        &env.out("out"),
        Some(PublishMode::Direct),
        &creds(),
        &FakePrs::new(),
    )
    .unwrap();
    assert!(matches!(out, Outcome::Nothing(m) if m.starts_with("skipped")));
}

#[test]
fn pull_request_mode_opens_a_draft_when_flagged_and_allowed_and_refuses_a_second() {
    let env = Env::with_git("duet");
    env.set("publish", "on_attention", serde_json::json!("publish"));
    staged(&env, Mode::Look);
    let prs = FakePrs::new();
    let out = publish(
        &env.profile(),
        &env.out("out"),
        Some(PublishMode::Pr),
        &creds(),
        &prs,
    )
    .unwrap();
    assert_eq!(
        out,
        Outcome::PullRequest("http://example.invalid/pull/1".into())
    );
    let req = prs.0.borrow()[0].clone();
    assert!(req.draft && req.base == "main" && req.head.starts_with("bots/2026-11-01"));
    assert!(req.body.trim_end().ends_with("Assisted-by: LLM"));
    assert_eq!(log(&env, "origin/main"), ["base"], "main is untouched");
    assert!(env
        .git(&["ls-remote", "--heads", "origin"])
        .contains("refs/heads/bots/"));
    assert_eq!(
        env.git(&["rev-parse", "--abbrev-ref", "HEAD"]).trim(),
        "main"
    );
    let err = publish(
        &env.profile(),
        &env.out("out"),
        Some(PublishMode::Pr),
        &creds(),
        &prs,
    )
    .unwrap_err();
    assert!(matches!(err, Error::ProposalOpen));
    assert_eq!(err.exit_code(), 65);
}
