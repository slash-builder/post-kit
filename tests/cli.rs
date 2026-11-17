//! End to end through the real binary and a real child process standing in for the model client.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::Env;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

const FAKE_CLIENT: &str = r#"#!/bin/sh
# Stands in for the model client: answers by the shape of the schema it is given.
schema=""
while [ $# -gt 0 ]; do [ "$1" = "--json-schema" ] && schema="$2"; shift; done
cat >/dev/null
case "$schema" in
  *'"verdict"'*) out='{"verdict":"ok","concerns":[]}' ;;
  *'"tags"'*) out='{"title":"A Short Note","summary":"A short summary.","paragraphs":["One paragraph.[^1]"],"footnotes":["A note."],"tags":["notes"],"held_open":"What next?"}' ;;
  *) out='{}' ;;
esac
printf '{"is_error":false,"total_cost_usd":0.01,"usage":{"input_tokens":1,"output_tokens":1},"structured_output":%s}\n' "$out"
"#;

fn post(env: &Env, args: &[&str]) -> std::process::Output {
    let client = env.tmp.path().join("client.sh");
    std::fs::write(&client, FAKE_CLIENT).unwrap();
    std::fs::set_permissions(&client, std::fs::Permissions::from_mode(0o755)).unwrap();
    Command::new(env!("CARGO_BIN_EXE_post"))
        .arg("--profile")
        .arg(&env.profile_path)
        .args(args)
        .env("POST_KIT_LLM", &client)
        .env("POST_KIT_DATE", "2026-11-01")
        .env_remove("POST_KIT_BLOCKLIST")
        .output()
        .unwrap()
}

#[test]
fn run_plan_status_and_guard_through_the_real_binary() {
    let env = Env::new("field-notes");
    let out = env.out("out");
    let r = post(
        &env,
        &[
            "run",
            "--out",
            out.to_str().unwrap(),
            "--topic",
            "list/calibration",
        ],
    );
    assert_eq!(
        r.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&r.stderr)
    );
    assert!(String::from_utf8_lossy(&r.stdout).contains("STATUS: OK"));
    assert!(out.join("files/_posts/2026-11-01-a-short-note.md").exists());

    let r = post(&env, &["apply", "--out", out.to_str().unwrap()]);
    assert_eq!(r.status.code(), Some(0));
    let status = post(&env, &["status"]);
    let v: serde_json::Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(
        (
            v["format"].as_str(),
            v["posts_total"].as_u64(),
            v["paused"].as_bool()
        ),
        (Some("post"), Some(1), Some(false))
    );

    let plan = post(&env, &["plan"]);
    assert_eq!(plan.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&plan.stdout).contains("\"sequence\": 2"));

    let doc = env.tmp.path().join("doc.md");
    std::fs::write(&doc, "Written for Example Corp.").unwrap();
    let r = post(&env, &["guard", "check", doc.to_str().unwrap()]);
    assert_eq!(r.status.code(), Some(1));
    std::fs::write(&doc, "Nothing here.").unwrap();
    assert_eq!(
        post(&env, &["guard", "check", doc.to_str().unwrap()])
            .status
            .code(),
        Some(0)
    );
}

#[test]
fn usage_errors_have_the_documented_exit_codes() {
    let env = Env::new("field-notes");
    let r = post(
        &env,
        &[
            "run",
            "--out",
            env.out("o").to_str().unwrap(),
            "--topic",
            "list/nope",
        ],
    );
    assert_eq!(r.status.code(), Some(64));
    let r = Command::new(env!("CARGO_BIN_EXE_post"))
        .args(["--profile", "/no/such/profile.json", "status"])
        .output()
        .unwrap();
    assert_ne!(r.status.code(), Some(0));
}
