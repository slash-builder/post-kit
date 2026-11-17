//! Post history and counters. Kept in the repository, so history is reviewable and revertible.

use crate::error::{io_err, json_err, Error, Result};
use crate::profile::Profile;
use serde::{Deserialize, Serialize};

/// One published (or staged) post.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct Entry {
    /// `date-topic` identifier.
    pub id: String,
    /// `YYYY-MM-DD`.
    pub date: String,
    /// The topic key.
    pub topic: String,
    /// The post title.
    pub title: String,
    /// The file, relative to the repository root.
    pub file: String,
    /// One-sentence summary.
    pub summary: String,
    /// The question the post leaves open.
    pub held_open: String,
    /// First and last footnote numbers used, or empty.
    pub footnotes: Vec<u32>,
}

/// The history file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct State {
    /// Posts, oldest first.
    pub entries: Vec<Entry>,
}

impl State {
    /// Read the profile's history file; a missing file is an empty history.
    ///
    /// # Errors
    /// Returns an error if the file exists but cannot be read or parsed.
    pub fn load(profile: &Profile) -> Result<Self> {
        let path = profile.state_file();
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(&path).map_err(io_err(&path))?;
        serde_json::from_str(&text).map_err(json_err(format!("history {}", path.display())))
    }

    /// The history as the text written to disk.
    ///
    /// ```
    /// let s = post_kit::state::State::default();
    /// assert_eq!(s.dump(), "{\n  \"entries\": []\n}\n");
    /// ```
    #[must_use]
    pub fn dump(&self) -> String {
        let mut s = serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".into());
        s.push('\n');
        s
    }

    /// How many posts are dated `date`.
    ///
    /// ```
    /// let mut s = post_kit::state::State::default();
    /// assert_eq!(s.posts_on("2026-11-01"), 0);
    /// ```
    #[must_use]
    pub fn posts_on(&self, date: &str) -> usize {
        self.entries.iter().filter(|e| e.date == date).count()
    }
}

fn counter_regex(key: &str) -> Result<regex::Regex> {
    regex::Regex::new(&format!(r"(?m)^(\s*{}:\s*)(\d+)", regex::escape(key)))
        .map_err(|e| Error::Profile(format!("bad counter key: {e}")))
}

/// The footnote counter currently stored in the profile's config file.
///
/// # Errors
/// Returns an error if the profile has no footnote counter, or the file or key is missing.
pub fn counter_get(profile: &Profile) -> Result<u32> {
    let fc = profile
        .data
        .render
        .footnotes
        .as_ref()
        .ok_or_else(|| Error::Profile("profile has no render.footnotes".into()))?;
    let path = profile.under_root(&fc.config_file);
    let text = std::fs::read_to_string(&path).map_err(io_err(&path))?;
    counter_regex(&fc.key)?
        .captures(&text)
        .and_then(|c| c[2].parse().ok())
        .ok_or_else(|| Error::Profile(format!("cannot find {} in {}", fc.key, fc.config_file)))
}

/// The config file's text with the counter set to `value`.
///
/// # Errors
/// Returns an error if the profile has no footnote counter or the file cannot be read.
pub fn counter_set_text(profile: &Profile, value: u32) -> Result<String> {
    let fc = profile
        .data
        .render
        .footnotes
        .as_ref()
        .ok_or_else(|| Error::Profile("profile has no render.footnotes".into()))?;
    let path = profile.under_root(&fc.config_file);
    let text = std::fs::read_to_string(&path).map_err(io_err(&path))?;
    let re = counter_regex(&fc.key)?;
    Ok(re.replacen(&text, 1, format!("${{1}}{value}")).into_owned())
}
