//! Shared test support: a fake model, and throwaway copies of the examples.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use post_kit::model::{ModelBackend, ModelReply, ModelRequest};
use post_kit::profile::Profile;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Ok,
    BadNameOnce,
    AlwaysBad,
    Cost,
    Look,
    Invented,
}

pub struct Fake {
    pub mode: Mode,
    pub calls: AtomicUsize,
}

impl Fake {
    pub fn new(mode: Mode) -> Self {
        Self {
            mode,
            calls: AtomicUsize::new(0),
        }
    }
    pub fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
    fn leaky(&self, n: usize, s: &str) -> String {
        match self.mode {
            Mode::BadNameOnce if n == 0 => format!("{s} Examplecorp Ltd"),
            Mode::AlwaysBad => format!("{s} Examplecorp Ltd"),
            Mode::Invented => format!("{s} See \u{a7}XLII and note 99."),
            _ => s.to_string(),
        }
    }
}

impl ModelBackend for Fake {
    fn call(&self, req: &ModelRequest<'_>) -> post_kit::Result<ModelReply> {
        let n = self.calls.fetch_add(1, Ordering::SeqCst);
        let props = req.schema["properties"]
            .as_object()
            .cloned()
            .unwrap_or_default();
        let block = "\u{2588}".repeat(8);
        let out: Value = if props.contains_key("verdict") {
            if self.mode == Mode::Look {
                json!({"verdict": "look", "concerns": [{"kind": "other", "note": "check the tone"}]})
            } else {
                json!({"verdict": "ok", "concerns": []})
            }
        } else if props.contains_key("tags") {
            json!({"title": "A Short Note", "summary": "A short summary.", "paragraphs": [self.leaky(n, "One paragraph with a note.[^1]")],
                   "footnotes": ["A small note."], "tags": ["notes"], "held_open": "What comes next?"})
        } else if props.contains_key("title") {
            json!({"title": "On a Quiet Margin", "paragraphs": [self.leaky(n, &format!("The Host reads the page with care.[^1] It recurs, as the {block} would wish."))],
                   "footnotes": ["An opening note."]})
        } else if props.keys().any(|k| k.starts_with("reply_to_")) {
            let mut o = json!({"paragraphs": ["I keep the question open.[^1]"], "footnotes": ["A closing note."],
                               "summary": "The Host read the page and was corrected.", "held_open": "Who reads the reader?"});
            for k in props.keys().filter(|k| k.starts_with("reply_to_")) {
                o[k] = json!("Fair.");
            }
            o
        } else {
            json!({"margin": "Reaching. The page says less than the Host wants."})
        };
        let cost = if self.mode == Mode::Cost { 50.0 } else { 0.01 };
        Ok(ModelReply::new(out, cost, 5))
    }
}

pub fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let dest = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &dest);
        } else {
            std::fs::copy(e.path(), dest).unwrap();
        }
    }
}

pub struct Env {
    pub tmp: tempfile::TempDir,
    pub ex: PathBuf,
    pub root: PathBuf,
    pub profile_path: PathBuf,
}

impl Env {
    pub fn new(example: &str) -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let ex = tmp.path().join(example);
        copy_dir(
            &Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("examples")
                .join(example),
            &ex,
        );
        Self {
            root: ex.join("site"),
            profile_path: ex.join("profile.json"),
            ex,
            tmp,
        }
    }

    pub fn with_git(example: &str) -> Self {
        let e = Self::new(example);
        let origin = e.tmp.path().join("origin.git");
        run(
            e.tmp.path(),
            &["git", "init", "-q", "--bare", origin.to_str().unwrap()],
        );
        for args in [
            vec!["git", "init", "-q", "-b", "main"],
            vec!["git", "config", "user.email", "t@example.invalid"],
            vec!["git", "config", "user.name", "t"],
            vec!["git", "add", "-A"],
            vec!["git", "commit", "-q", "-m", "base"],
            vec!["git", "remote", "add", "origin", origin.to_str().unwrap()],
            vec!["git", "push", "-q", "-u", "origin", "main"],
        ] {
            run(&e.root, &args);
        }
        e
    }

    pub fn profile(&self) -> Profile {
        Profile::load(&self.profile_path).unwrap()
    }

    /// Edit the profile JSON in place: set `section.key = value`.
    pub fn set(&self, section: &str, key: &str, value: Value) {
        let mut v: Value =
            serde_json::from_str(&std::fs::read_to_string(&self.profile_path).unwrap()).unwrap();
        v[section][key] = value;
        std::fs::write(
            &self.profile_path,
            serde_json::to_string_pretty(&v).unwrap(),
        )
        .unwrap();
    }

    pub fn out(&self, name: &str) -> PathBuf {
        self.tmp.path().join(name)
    }

    pub fn git(&self, args: &[&str]) -> String {
        run(&self.root, &[&["git"], args].concat())
    }
}

pub fn run(cwd: &Path, args: &[&str]) -> String {
    let out = Command::new(args[0])
        .args(&args[1..])
        .current_dir(cwd)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

pub fn manifest(out: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(out.join("manifest.json")).unwrap()).unwrap()
}

pub fn first_file(dir: &Path, prefix: &str) -> PathBuf {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.file_name().unwrap().to_string_lossy().starts_with(prefix))
        .collect();
    found.sort();
    found.remove(0)
}
