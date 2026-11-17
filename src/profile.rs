//! A profile is one JSON file describing one blog setup: personas, format, turns, topics, limits,
//! rendering, output and publishing.
//!
//! The `limits` and `publish` sections reject unknown keys, because a typo there (a daily cap, say)
//! would otherwise silently fall back to a default.

use crate::error::{io_err, json_err, Error, Result};
use serde::Deserialize;
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Which shape of post the profile produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum FormatKind {
    /// One voice, one post, written as markdown.
    Post,
    /// Several voices on one page, with margin notes, written as HTML.
    Conversation,
}

/// One voice.
#[derive(Debug, Clone, Deserialize)]
#[non_exhaustive]
pub struct Persona {
    /// The prompt file, relative to the profile.
    pub file: String,
    /// Display name; defaults to the id.
    pub label: Option<String>,
    /// The model alias to ask.
    pub model: String,
}

/// One model call in a run.
#[derive(Debug, Clone, Deserialize)]
#[non_exhaustive]
pub struct Turn {
    /// A persona id.
    pub voice: String,
    /// `post` for the post format; `open`, `margin`, `close` or `last` for conversations.
    pub kind: String,
    /// Extra instruction for this turn.
    #[serde(default)]
    pub ask: String,
}

/// Where the term blocklist lives.
#[derive(Debug, Clone, Default, Deserialize)]
#[non_exhaustive]
pub struct GuardCfg {
    /// Path relative to the profile.
    pub blocklist: Option<String>,
}

/// The advisory review.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct ReviewCfg {
    /// Model alias for the review pass.
    pub model: String,
    /// Extra things for the reviewer to look for.
    pub notes: String,
}

impl Default for ReviewCfg {
    fn default() -> Self {
        Self {
            model: "haiku".into(),
            notes: String::new(),
        }
    }
}

/// Drift: sometimes take up the last post's open question instead of a fresh topic.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct Drift {
    /// Chance (0 to 1) that a run continues the last post's question.
    pub continue_probability: f64,
}

/// Which JSON keys of a log line carry the time, who, kind and text. The first key present wins; the defaults
/// cover common shapes (`ts`/`t`/`time`, `who`/`agent`/`name`/`player`, `kind`/`type`/`event`, `text`/`msg`/`message`).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct LogFields {
    /// Keys that may hold the time (ISO text, or epoch seconds or milliseconds).
    pub time: Vec<String>,
    /// Keys that may hold who did it.
    pub who: Vec<String>,
    /// Keys that may hold what kind of event it was.
    pub kind: Vec<String>,
    /// Keys that may hold the text.
    pub text: Vec<String>,
}

/// A log of events to write about, as JSON lines or a JSON file holding a list (or an `events` list).
#[derive(Debug, Clone, Deserialize)]
#[non_exhaustive]
pub struct LogCfg {
    /// The log file, relative to the profile.
    pub file: String,
    /// `count` (every `count` events) or `day` (by calendar day of the time field).
    #[serde(default = "default_group_by")]
    pub group_by: String,
    /// Events per group when grouping by `count`.
    #[serde(default = "default_count")]
    pub count: usize,
    /// Topic title, with `{group}`.
    #[serde(default = "default_log_title")]
    pub title: String,
    /// Key names.
    #[serde(default)]
    pub fields: LogFields,
}

fn default_group_by() -> String {
    "count".into()
}
fn default_count() -> usize {
    40
}
fn default_log_title() -> String {
    "Records for {group}".into()
}

/// What to write about.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct TopicsCfg {
    /// An event log, chronicled in order.
    pub log: Option<LogCfg>,
    /// Site folders of markdown pages, by collection name.
    pub collections: BTreeMap<String, String>,
    /// A JSON file of `{key, title, brief}` items, relative to the profile.
    pub list_file: Option<String>,
    /// Topic keys never chosen.
    pub skip: Vec<String>,
    /// URL pattern per collection, with `{name}`.
    pub permalinks: BTreeMap<String, String>,
    /// Extra page-to-text rules: `[pattern, replacement]`.
    pub plain_rules: Vec<(String, String)>,
    /// Continue-the-last-question setting.
    pub drift: Option<Drift>,
}

/// Limits for answers and runs.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[non_exhaustive]
pub struct Limits {
    /// Characters of a topic page shown to the model.
    pub anchor_chars: usize,
    /// Words per paragraph.
    pub paragraph_words: usize,
    /// Words per margin note.
    pub margin_words: usize,
    /// Words per last line.
    pub last_words: usize,
    /// Words per footnote.
    pub footnote_words: usize,
    /// Words per reply inside a margin.
    pub reply_words: usize,
    /// Words in a title.
    pub title_words: usize,
    /// Words in a summary.
    pub summary_words: usize,
    /// Words in the open question.
    pub held_open_words: usize,
    /// Paragraphs in an opening.
    pub max_paragraphs: usize,
    /// Footnotes in an opening.
    pub max_footnotes: usize,
    /// Tags on a post.
    pub max_tags: usize,
    /// Earlier posts shown to the model for continuity.
    pub prior_entries: usize,
    /// Retries per turn after a failed check.
    pub retries_per_turn: u32,
    /// Seconds before a model call is abandoned.
    pub call_timeout_s: u64,
    /// Spend cap per run, at list price.
    pub max_cost_usd: f64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            anchor_chars: 6000,
            paragraph_words: 170,
            margin_words: 60,
            last_words: 35,
            footnote_words: 70,
            reply_words: 30,
            title_words: 12,
            summary_words: 40,
            held_open_words: 25,
            max_paragraphs: 3,
            max_footnotes: 2,
            max_tags: 5,
            prior_entries: 4,
            retries_per_turn: 2,
            call_timeout_s: 600,
            max_cost_usd: 3.0,
        }
    }
}

/// Detector settings.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct ChecksCfg {
    /// Regex for a sign-off the persona should not write (the script signs).
    pub signature_regex: Option<String>,
    /// Regex with groups naming references (sections, notes) that must come from the supplied text.
    pub reference_regex: Option<String>,
}

/// The running footnote counter kept in a config file.
#[derive(Debug, Clone, Deserialize)]
#[non_exhaustive]
pub struct FootnoteCfg {
    /// File relative to the repository root.
    pub config_file: String,
    /// The `key: number` line to read and bump.
    pub key: String,
}

/// How a post is typeset.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct RenderCfg {
    /// Site layout name.
    pub layout: Option<String>,
    /// Attribution line (conversation).
    pub attribution: Option<String>,
    /// Author name (post).
    pub author: Option<String>,
    /// A line appended to every post (post).
    pub byline_markdown: Option<String>,
    /// CSS class of replies inside margins (conversation).
    pub reply_class: Option<String>,
    /// Exact-case words set in a coloured span: `[word, class]`.
    pub tokens: Vec<(String, String)>,
    /// Markup for runs of `█`.
    pub redaction_html: Option<String>,
    /// Sign-off markup with `{year}` (conversation).
    pub signature_html: Option<String>,
    /// Running footnote counter (conversation).
    pub footnotes: Option<FootnoteCfg>,
}

/// Front matter every post carries.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct DisclosureCfg {
    /// For example `ai_generated: true, reviewed: false`.
    pub front_matter: Map<String, Value>,
}

/// Where output goes.
#[derive(Debug, Clone, Deserialize)]
#[non_exhaustive]
pub struct OutputCfg {
    /// Folder of posts, relative to the repository root.
    pub directory: String,
    /// File-name prefix (conversation).
    #[serde(default)]
    pub file_prefix: String,
    /// The history file, relative to the repository root.
    pub state_file: String,
}

/// How a staged post is published.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum PublishMode {
    /// Write files only.
    Stage,
    /// Push a branch and open a pull request.
    Pr,
    /// Commit and push to the base branch.
    Direct,
}

/// What to do with a flagged post.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum OnAttention {
    /// Archive it and do not publish.
    Hold,
    /// Publish it anyway.
    Publish,
}

/// Publishing rules.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[non_exhaustive]
pub struct PublishCfg {
    /// Stage, pull request or direct commit.
    pub mode: PublishMode,
    /// Hold or publish a flagged post.
    pub on_attention: OnAttention,
    /// Posts per day; 0 means no cap.
    pub max_per_day: u32,
    /// If this file exists in the repository, nothing runs or publishes.
    pub pause_file: Option<String>,
    /// Prefix of proposal branches.
    pub branch_prefix: String,
    /// Commit and pull request trailer.
    pub trailer: String,
    /// Paths a proposal may touch; empty means the output paths only.
    pub allowed_paths: Vec<String>,
    /// Branch to publish to; default the checked-out branch.
    pub base_branch: Option<String>,
}

impl Default for PublishCfg {
    fn default() -> Self {
        Self {
            mode: PublishMode::Stage,
            on_attention: OnAttention::Hold,
            max_per_day: 1,
            pause_file: None,
            branch_prefix: "bots/".into(),
            trailer: "Assisted-by: LLM".into(),
            allowed_paths: Vec::new(),
            base_branch: None,
        }
    }
}

/// The deserialised profile file.
#[derive(Debug, Clone, Deserialize)]
#[non_exhaustive]
pub struct ProfileData {
    /// A short name.
    pub name: String,
    /// The post shape.
    pub format: FormatKind,
    /// Repository root, relative to the profile.
    #[serde(default = "dot")]
    pub root: String,
    /// Blocklist location.
    #[serde(default)]
    pub guard: GuardCfg,
    /// Shared rules every persona receives, relative to the profile.
    pub brief: Option<String>,
    /// The voices, by id.
    pub personas: Map<String, Value>,
    /// Advisory review settings.
    #[serde(default)]
    pub review: ReviewCfg,
    /// The calls in a run.
    pub turns: Vec<Turn>,
    /// Topic sources.
    #[serde(default)]
    pub topics: TopicsCfg,
    /// Limits.
    #[serde(default)]
    pub limits: Limits,
    /// Detector settings.
    #[serde(default)]
    pub checks: ChecksCfg,
    /// Typesetting.
    #[serde(default)]
    pub render: RenderCfg,
    /// Disclosure front matter.
    #[serde(default)]
    pub disclosure: DisclosureCfg,
    /// Output locations.
    pub output: OutputCfg,
    /// Publishing rules.
    #[serde(default)]
    pub publish: PublishCfg,
}

fn dot() -> String {
    ".".into()
}

/// A loaded profile with its resolved paths.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Profile {
    /// The settings.
    pub data: ProfileData,
    /// The profile file.
    pub file: PathBuf,
    /// The folder holding the profile file.
    pub dir: PathBuf,
    /// The repository root.
    pub root: PathBuf,
    personas: BTreeMap<String, Persona>,
    persona_order: Vec<String>,
}

impl Profile {
    /// Read and check a profile file.
    ///
    /// ```no_run
    /// let profile = post_kit::profile::Profile::load("examples/field-notes/profile.json".as_ref())?;
    /// assert_eq!(profile.data.name, "field-notes");
    /// # Ok::<(), post_kit::Error>(())
    /// ```
    ///
    /// # Errors
    /// Returns an error if the file cannot be read or parsed, or names a persona that does not exist.
    pub fn load(path: &Path) -> Result<Self> {
        let file = path.canonicalize().map_err(io_err(path))?;
        let text = std::fs::read_to_string(&file).map_err(io_err(&file))?;
        let data: ProfileData =
            serde_json::from_str(&text).map_err(json_err(format!("profile {}", file.display())))?;
        let dir = file
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
        let root = dir
            .join(&data.root)
            .canonicalize()
            .map_err(io_err(dir.join(&data.root)))?;
        let mut personas = BTreeMap::new();
        let mut persona_order = Vec::new();
        for (id, v) in &data.personas {
            let p: Persona =
                serde_json::from_value(v.clone()).map_err(json_err(format!("persona {id}")))?;
            personas.insert(id.clone(), p);
            persona_order.push(id.clone());
        }
        if personas.is_empty() || data.turns.is_empty() {
            return Err(Error::Profile(
                "a profile needs at least one persona and one turn".into(),
            ));
        }
        for t in &data.turns {
            if !personas.contains_key(&t.voice) {
                return Err(Error::Profile(format!(
                    "turns mention '{}' which is not in personas",
                    t.voice
                )));
            }
        }
        Ok(Self {
            data,
            file,
            dir,
            root,
            personas,
            persona_order,
        })
    }

    /// A path under the repository root.
    #[must_use]
    pub fn under_root(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    /// A path next to the profile file.
    #[must_use]
    pub fn under_profile(&self, rel: &str) -> PathBuf {
        self.dir.join(rel)
    }

    /// The blocklist file named by the profile, if any.
    #[must_use]
    pub fn blocklist(&self) -> Option<PathBuf> {
        self.data
            .guard
            .blocklist
            .as_deref()
            .map(|r| self.under_profile(r))
    }

    /// The history file.
    #[must_use]
    pub fn state_file(&self) -> PathBuf {
        self.under_root(&self.data.output.state_file)
    }

    /// Persona ids in file order.
    #[must_use]
    pub fn persona_ids(&self) -> &[String] {
        &self.persona_order
    }

    /// A persona's display name.
    #[must_use]
    pub fn label(&self, id: &str) -> String {
        self.personas
            .get(id)
            .and_then(|p| p.label.clone())
            .unwrap_or_else(|| id.to_string())
    }

    /// A persona's prompt file name without folders or `.md`, for listing in front matter.
    #[must_use]
    pub fn persona_stem(&self, id: &str) -> String {
        let file = self.personas.get(id).map_or(id, |p| p.file.as_str());
        let name = file.rsplit('/').next().unwrap_or(file);
        name.strip_suffix(".md").unwrap_or(name).to_string()
    }

    /// A persona's model alias.
    #[must_use]
    pub fn model(&self, id: &str) -> String {
        self.personas
            .get(id)
            .map(|p| p.model.clone())
            .unwrap_or_default()
    }

    /// A persona's prompt, without front matter.
    ///
    /// # Errors
    /// Returns an error if the file cannot be read.
    pub fn persona_text(&self, id: &str) -> Result<String> {
        let p = self
            .personas
            .get(id)
            .ok_or_else(|| Error::Profile(format!("no persona '{id}'")))?;
        let path = self.under_profile(&p.file);
        let text = std::fs::read_to_string(&path).map_err(io_err(&path))?;
        Ok(crate::text::split_front_matter(&text).1.trim().to_string())
    }

    /// The shared brief, or an empty string.
    ///
    /// # Errors
    /// Returns an error if the file cannot be read.
    pub fn brief_text(&self) -> Result<String> {
        match &self.data.brief {
            Some(rel) => {
                let path = self.under_profile(rel);
                Ok(std::fs::read_to_string(&path)
                    .map_err(io_err(&path))?
                    .trim()
                    .to_string())
            }
            None => Ok(String::new()),
        }
    }

    /// Paths a proposal may touch.
    #[must_use]
    pub fn allowed_paths(&self) -> Vec<String> {
        let p = &self.data.publish;
        if !p.allowed_paths.is_empty() {
            return p.allowed_paths.clone();
        }
        let mut v = vec![
            self.data.output.directory.clone(),
            self.data.output.state_file.clone(),
        ];
        if let Some(f) = &self.data.render.footnotes {
            v.push(f.config_file.clone());
        }
        v
    }

    /// True if the pause switch is on: `POST_KIT_PAUSE` is set, or the pause file exists.
    #[must_use]
    pub fn paused(&self) -> bool {
        if std::env::var("POST_KIT_PAUSE").is_ok_and(|v| !v.is_empty()) {
            return true;
        }
        self.data
            .publish
            .pause_file
            .as_deref()
            .is_some_and(|f| self.under_root(f).exists())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, text: &str) {
        std::fs::write(dir.join(name), text).unwrap();
    }

    #[test]
    fn loads_and_applies_defaults() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "p.md", "---\nname: x\n---\nYou are X.");
        write(
            d.path(),
            "profile.json",
            r#"{"name":"t","format":"post","personas":{"x":{"file":"p.md","model":"m"}},"turns":[{"voice":"x","kind":"post"}],"output":{"directory":"posts","state_file":"s.json"}}"#,
        );
        let p = Profile::load(&d.path().join("profile.json")).unwrap();
        assert_eq!(p.data.publish.mode, PublishMode::Stage);
        assert_eq!(p.data.publish.on_attention, OnAttention::Hold);
        assert_eq!(p.data.publish.max_per_day, 1);
        assert_eq!(p.persona_text("x").unwrap(), "You are X.");
        assert_eq!(p.label("x"), "x");
        assert_eq!(
            p.allowed_paths(),
            vec!["posts".to_string(), "s.json".to_string()]
        );
        assert!(!p.paused());
    }

    #[test]
    fn a_typo_in_a_safety_section_is_an_error() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "p.md", "x");
        let base = |extra: &str| {
            format!(
                r#"{{"name":"t","format":"post","personas":{{"x":{{"file":"p.md","model":"m"}}}},"turns":[{{"voice":"x","kind":"post"}}],"output":{{"directory":"a","state_file":"s"}},{extra}}}"#
            )
        };
        write(
            d.path(),
            "profile.json",
            &base(r#""publish":{"max_per_dya":3}"#),
        );
        assert!(Profile::load(&d.path().join("profile.json")).is_err());
        write(
            d.path(),
            "profile.json",
            &base(r#""limits":{"max_cost":3}"#),
        );
        assert!(Profile::load(&d.path().join("profile.json")).is_err());
    }

    #[test]
    fn an_unknown_persona_in_a_turn_is_an_error() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "p.md", "x");
        write(
            d.path(),
            "profile.json",
            r#"{"name":"t","format":"post","personas":{"x":{"file":"p.md","model":"m"}},"turns":[{"voice":"y","kind":"post"}],"output":{"directory":"a","state_file":"s"}}"#,
        );
        assert!(Profile::load(&d.path().join("profile.json")).is_err());
    }

    #[test]
    fn the_pause_file_pauses() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "p.md", "x");
        write(
            d.path(),
            "profile.json",
            r#"{"name":"t","format":"post","personas":{"x":{"file":"p.md","model":"m"}},"turns":[{"voice":"x","kind":"post"}],"output":{"directory":"a","state_file":"s"},"publish":{"pause_file":"PAUSE"}}"#,
        );
        let p = Profile::load(&d.path().join("profile.json")).unwrap();
        assert!(!p.paused());
        write(d.path(), "PAUSE", "");
        assert!(p.paused());
    }
}
