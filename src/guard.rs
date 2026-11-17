//! Term guard: keeps listed terms (a withheld name, a client, a codename) out of generated text
//! without holding them in plain text.
//!
//! A checker that stores the terms in plain text would itself leak them once the repository is
//! public, so this one stores only SHA-256 hashes of normalised terms (and of their reversed and
//! rot13 forms) in a blocklist file. Each line is `length:hash`.
//!
//! **Honest limit:** a hash list hides a term from a text search, not from someone who already
//! suspects it and can hash it. Keep the list outside the repository (`POST_KIT_BLOCKLIST`) for more.
//!
//! Matching ignores case, spacing, punctuation and combining marks, and looks at every window of
//! the text, so a term inside a longer word is caught too.

use crate::error::{io_err, Result};
use crate::util::{const_regex, sha_hex};
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::LazyLock;
use unicode_normalization::{char::canonical_combining_class, UnicodeNormalization};

const HEADER: &str = "# length:sha256 of a normalised, withheld term (plain, reversed, rot13).\n# Written by `post guard add`. See the module docs of post_kit::guard for what this does and does not protect.\n";

/// A listed term was found. The match itself is never returned, only where it was.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[non_exhaustive]
pub struct Hit {
    /// Offset in the *normalised* text.
    pub offset: usize,
    /// A little surrounding normalised text with the match blanked out.
    pub context: String,
}

/// A set of hashed terms.
#[derive(Debug, Clone, Default)]
pub struct Guard {
    table: BTreeMap<usize, BTreeSet<String>>,
}

/// Lowercase letters and digits only, with accents and combining marks removed.
///
/// ```
/// assert_eq!(post_kit::guard::normalise("Zéphyr-ine Q.u"), "zephyrinequ");
/// ```
#[must_use]
pub fn normalise(text: &str) -> String {
    text.nfkd()
        .filter(|c| canonical_combining_class(*c) == 0)
        .flat_map(char::to_lowercase)
        .filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        .collect()
}

fn rot13(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'a'..='m' => char::from(c as u8 + 13),
            'n'..='z' => char::from(c as u8 - 13),
            _ => c,
        })
        .collect()
}

/// The normalised term plus its reversed and rot13 forms.
#[must_use]
pub fn variants(term: &str) -> BTreeSet<String> {
    let base = normalise(term);
    let mut set = BTreeSet::new();
    if !base.is_empty() {
        set.insert(base.chars().rev().collect());
        set.insert(rot13(&base));
        set.insert(base);
    }
    set
}

/// Add terms to a blocklist file (created if missing). Returns how many lines were written.
///
/// # Errors
/// Returns an error if the file cannot be read or written.
pub fn add_terms<S: AsRef<str>>(terms: &[S], path: &Path) -> Result<usize> {
    let env = std::env::var("POST_KIT_BLOCKLIST")
        .ok()
        .filter(|v| !v.is_empty());
    let path = env.as_deref().map_or(path, Path::new);
    let existing = if path.exists() {
        std::fs::read_to_string(path).map_err(io_err(path))?
    } else {
        HEADER.to_string()
    };
    let have: BTreeSet<&str> = existing.lines().collect();
    let mut new = Vec::new();
    for term in terms {
        for v in variants(term.as_ref()) {
            let line = format!("{}:{}", v.len(), sha_hex(&v));
            if !have.contains(line.as_str()) && !new.contains(&line) {
                new.push(line);
            }
        }
    }
    if !new.is_empty() {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(io_err(parent))?;
        }
        let mut text = existing.trim_end_matches('\n').to_string();
        text.push('\n');
        text.push_str(&new.join("\n"));
        text.push('\n');
        std::fs::write(path, text).map_err(io_err(path))?;
    }
    Ok(new.len())
}

impl Guard {
    /// A guard that matches nothing.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Read a blocklist file. `POST_KIT_BLOCKLIST`, if set, replaces the path.
    ///
    /// # Errors
    /// Returns an error if the file exists but cannot be read.
    pub fn from_file(path: &Path) -> Result<Self> {
        Self::load(Some(path))
    }

    /// Read the blocklist named by the profile, honouring the `POST_KIT_BLOCKLIST` override.
    /// A missing file gives an empty guard.
    ///
    /// # Errors
    /// Returns an error if the file exists but cannot be read.
    pub fn load(path: Option<&Path>) -> Result<Self> {
        let env = std::env::var("POST_KIT_BLOCKLIST")
            .ok()
            .filter(|v| !v.is_empty());
        let Some(path) = env.as_deref().map(Path::new).or(path) else {
            return Ok(Self::empty());
        };
        if !path.exists() {
            return Ok(Self::empty());
        }
        let text = std::fs::read_to_string(path).map_err(io_err(path))?;
        let mut table: BTreeMap<usize, BTreeSet<String>> = BTreeMap::new();
        for line in text.lines().map(str::trim) {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((len, digest)) = line.split_once(':') {
                if let (Ok(len), 64) = (len.parse::<usize>(), digest.len()) {
                    table.entry(len).or_default().insert(digest.to_string());
                }
            }
        }
        Ok(Self { table })
    }

    /// True when no terms are listed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.table.is_empty()
    }

    /// Find listed terms in `text`.
    #[must_use]
    pub fn scan(&self, text: &str) -> Vec<Hit> {
        if self.table.is_empty() {
            return Vec::new();
        }
        let norm = normalise(text);
        let bytes = norm.as_bytes();
        let mut hits = BTreeSet::new();
        for (&len, digests) in &self.table {
            if len == 0 || len > bytes.len() {
                continue;
            }
            for i in 0..=(bytes.len() - len) {
                // `norm` is ASCII, so every window is valid UTF-8.
                let Ok(window) = std::str::from_utf8(&bytes[i..i + len]) else {
                    continue;
                };
                if digests.contains(&sha_hex(window)) {
                    let left = &norm[i.saturating_sub(12)..i];
                    let right_end = (i + len + 12).min(norm.len());
                    let right = &norm[i + len..right_end];
                    hits.insert(Hit {
                        offset: i,
                        context: format!("...{left}[{}]{right}...", "#".repeat(len)),
                    });
                }
            }
        }
        hits.into_iter().collect()
    }
}

static RE_ZALGO: LazyLock<Regex> = LazyLock::new(|| {
    const_regex(
        "[\u{0300}-\u{036f}\u{0483}-\u{0489}\u{1dc0}-\u{1dff}\u{20d0}-\u{20ff}\u{fe20}-\u{fe2f}]",
    )
});
static RE_EMAIL: LazyLock<Regex> =
    LazyLock::new(|| const_regex(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}"));
static RE_URL: LazyLock<Regex> = LazyLock::new(|| const_regex(r"(?i)\b(?:https?://|www\.)\S+"));
static RE_PATH: LazyLock<Regex> =
    LazyLock::new(|| const_regex(r"(?:/Users/|/home/|C:\\Users\\|~/)\S*"));
static RE_DOMAIN: LazyLock<Regex> = LazyLock::new(|| {
    const_regex(r"(?i)\b[a-z0-9-]+\.(?:com|net|org|io|me|systems|dev|app|co|us|xyz)\b")
});

/// Plain-text rules for generated output: no combining-mark corruption, addresses, links, paths or markup.
///
/// ```
/// use post_kit::guard::structure_problems;
/// assert!(structure_problems("see https://example.invalid/x").contains(&"a link"));
/// assert!(structure_problems("A plain sentence, with *italics* and a [^1] marker.").is_empty());
/// ```
#[must_use]
pub fn structure_problems(text: &str) -> Vec<&'static str> {
    let mut out = Vec::new();
    if RE_ZALGO.is_match(text) {
        out.push("combining marks (distorted text is not allowed)");
    }
    if RE_EMAIL.is_match(text) {
        out.push("an email address");
    }
    if RE_URL.is_match(text) {
        out.push("a link");
    }
    if RE_PATH.is_match(text) {
        out.push("a file path");
    }
    if RE_DOMAIN.is_match(text) {
        out.push("a domain name");
    }
    if text.contains('<') || text.contains('>') {
        out.push("angle brackets (output must be plain text)");
    }
    out
}

const SKIP_DIRS: [&str; 8] = [
    ".git",
    "_site",
    "vendor",
    ".bundle",
    ".jekyll-cache",
    ".sass-cache",
    "node_modules",
    "__pycache__",
];
const TEXT_SUFFIXES: [&str; 18] = [
    "md", "html", "htm", "yml", "yaml", "json", "scss", "css", "txt", "py", "sh", "rb", "xml",
    "toml", "rs", "lock", "cfg", "",
];
const NAMED_FILES: [&str; 3] = ["Makefile", "Gemfile", "Dockerfile"];

/// Scan the text files under `root` (skipping build output, `.git` and the blocklist itself).
/// Returns one line per file with hits, then a summary line, and whether the tree is clean.
///
/// # Errors
/// Returns an error if a directory cannot be listed.
pub fn audit_report(root: &Path, guard: &Guard) -> Result<(Vec<String>, bool)> {
    let mut per_file: BTreeMap<String, Vec<Hit>> = BTreeMap::new();
    walk(root, root, guard, &mut per_file)?;
    let mut lines: Vec<String> = per_file
        .iter()
        .map(|(f, h)| {
            format!(
                "{f}: {} overlapping hit(s); first at normalised offset {}: {}",
                h.len(),
                h[0].offset,
                h[0].context
            )
        })
        .collect();
    let clean = per_file.is_empty();
    lines.push(if clean {
        "clean: no listed term found".to_string()
    } else {
        format!("{} file(s) hit", per_file.len())
    });
    Ok((lines, clean))
}

fn walk(
    root: &Path,
    dir: &Path,
    guard: &Guard,
    out: &mut BTreeMap<String, Vec<Hit>>,
) -> Result<()> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .map_err(io_err(dir))?
        .filter_map(std::result::Result::ok)
        .collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            if !SKIP_DIRS.contains(&name.as_str()) {
                walk(root, &path, guard, out)?;
            }
        } else if kind.is_file() {
            let suffix = path
                .extension()
                .map(|e| e.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            if name == "blocklist.sha256"
                || !(TEXT_SUFFIXES.contains(&suffix.as_str())
                    || NAMED_FILES.contains(&name.as_str()))
            {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .into_owned();
            let hits = guard.scan(&format!("{rel}\n{text}"));
            if !hits.is_empty() {
                out.insert(rel, hits);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guard_for(term: &str) -> (tempfile::TempDir, Guard) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bl");
        add_terms(&[term], &path).unwrap();
        let g = Guard::from_file(&path).unwrap();
        (dir, g)
    }

    #[test]
    fn the_list_holds_no_plain_term() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bl");
        add_terms(&["zephyrine quillon"], &path).unwrap();
        let text = std::fs::read_to_string(path).unwrap();
        assert!(!text.contains("zephyrine") && !text.contains("quillon"));
    }

    #[test]
    fn catches_disguised_forms() {
        let (_d, g) = guard_for("zephyrine quillon");
        for s in [
            "Zephyrine Quillon",
            "ZEPHYRINEQUILLON",
            "z.e.p.h.y.r.i.n.e q u i l l o n",
            "Zéphyrine Quillon",
            "mr-zephyrinequillon42",
            "Z̷e̷p̷h̷y̷r̷i̷n̷e̷ Q̶u̶i̶l̶l̶o̶n̶",
        ] {
            assert!(!g.scan(s).is_empty(), "{s}");
        }
        assert!(!g.scan(&rot13("zephyrinequillon")).is_empty());
        assert!(!g
            .scan(&"zephyrinequillon".chars().rev().collect::<String>())
            .is_empty());
    }

    #[test]
    fn clean_text_passes_and_the_match_is_never_returned() {
        let (_d, g) = guard_for("zephyrine quillon");
        assert!(g.scan("Nothing to see here.").is_empty());
        let ctx: String = g
            .scan("before Zephyrine Quillon after")
            .iter()
            .map(|h| h.context.clone())
            .collect();
        assert!(!ctx.to_lowercase().contains("zephyrine"));
    }

    #[test]
    fn adding_twice_adds_nothing_the_second_time() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bl");
        assert_eq!(add_terms(&["alpha"], &path).unwrap(), 3);
        assert_eq!(add_terms(&["alpha"], &path).unwrap(), 0);
    }

    #[test]
    fn structure_rules() {
        for bad in [
            "a@b.com",
            "https://x.y/z",
            "/Users/someone/f",
            "a <b>tag</b>",
            "Ze\u{335}\u{336}lgo",
            "see example.com now",
        ] {
            assert!(!structure_problems(bad).is_empty(), "{bad}");
        }
        assert!(
            structure_problems("A plain sentence, with *italics* and a [^1] marker.").is_empty()
        );
    }

    #[test]
    fn audit_finds_a_term_in_a_file_and_in_a_file_name() {
        let (_d, g) = guard_for("secretword");
        let tree = tempfile::tempdir().unwrap();
        std::fs::write(tree.path().join("a.md"), "has the secretword inside").unwrap();
        std::fs::write(tree.path().join("secretword.md"), "clean body").unwrap();
        std::fs::write(tree.path().join("c.md"), "clean").unwrap();
        let (lines, clean) = audit_report(tree.path(), &g).unwrap();
        assert!(!clean);
        assert_eq!(lines.len(), 3, "{lines:?}");
        assert!(lines.last().unwrap().contains("2 file(s) hit"));
    }
}
