//! Golden fixtures: the exact files a run writes for each example, under the fake model.
//!
//! Regenerate with `POST_KIT_UPDATE_FIXTURES=1 cargo test --test fixtures` and read the diff.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{Env, Fake, Mode};
use post_kit::run::{stage, StageOptions};
use std::path::{Path, PathBuf};

fn files_under(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    fn walk(dir: &Path, root: &Path, out: &mut Vec<PathBuf>) {
        let mut entries: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        entries.sort();
        for p in entries {
            if p.is_dir() {
                walk(&p, root, out);
            } else {
                out.push(p.strip_prefix(root).unwrap().to_path_buf());
            }
        }
    }
    walk(root, root, &mut out);
    out
}

fn check(example: &str, topic: Option<&str>) {
    let env = Env::new(example);
    let mut o = StageOptions::new(env.out("out"));
    o.date = Some("2026-11-01".into());
    o.topic = topic.map(str::to_string);
    stage(&env.profile(), &Fake::new(Mode::Ok), &o).unwrap();
    let produced = env.out("out");
    let golden = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(example);
    let wanted = ["digest.md", "manifest.json"];
    let mut rels: Vec<PathBuf> = wanted.iter().map(PathBuf::from).collect();
    rels.extend(
        files_under(&produced.join("files"))
            .into_iter()
            .map(|p| Path::new("files").join(p)),
    );
    if std::env::var("POST_KIT_UPDATE_FIXTURES").is_ok() {
        let _ = std::fs::remove_dir_all(&golden);
        for rel in &rels {
            let dest = golden.join(rel);
            std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
            std::fs::copy(produced.join(rel), dest).unwrap();
        }
        return;
    }
    let mut have = files_under(&golden);
    have.sort();
    let mut want = rels.clone();
    want.sort();
    assert_eq!(
        have, want,
        "the set of files differs from the fixtures for {example}"
    );
    for rel in &rels {
        let got = std::fs::read_to_string(produced.join(rel)).unwrap();
        let exp = std::fs::read_to_string(golden.join(rel)).unwrap();
        assert_eq!(
            got,
            exp,
            "{example}: {} differs from its fixture",
            rel.display()
        );
    }
}

#[test]
fn duet_matches_its_fixture() {
    check("duet", None);
}

#[test]
fn field_notes_matches_its_fixture() {
    check("field-notes", Some("list/calibration"));
}

#[test]
fn chronicle_matches_its_fixture() {
    check("chronicle", None);
}
