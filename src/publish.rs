//! Publish a staged post: commit directly, or open a pull request. Never silently.
//!
//! Safety, in order:
//! - the pause switch (a file in the repository, or `POST_KIT_PAUSE`) stops publishing as well as runs;
//! - only a post whose manifest says it is publishable goes out (a flagged post is **held** by default);
//! - a proposal may only touch the profile's allowed paths;
//! - the git token reaches git through the process environment as an HTTP header, never on a command
//!   line, in a URL or in a file the repository can see;
//! - the daily cap is checked again here, against the repository's own history.

use crate::error::{io_err, json_err, Error, Result};
use crate::profile::{Profile, PublishMode};
use crate::run::{apply, check_paths};
use crate::state::State;
use crate::util::base64;
use serde::Deserialize;
use serde_json::{json, Value};
use std::io::Write;
use std::path::Path;
use std::process::Command;

/// Credentials for publishing, normally from the environment.
#[derive(Clone, Default)]
#[non_exhaustive]
pub struct Credentials {
    /// A git token that may push (and open pull requests).
    pub token: Option<String>,
    /// `owner/name`, for pull requests.
    pub repository: Option<String>,
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credentials")
            .field("token", &self.token.as_ref().map(|_| "<set>"))
            .field("repository", &self.repository)
            .finish()
    }
}

impl Credentials {
    /// Credentials from values you already hold.
    ///
    /// ```
    /// let c = post_kit::publish::Credentials::new(Some("token".into()), Some("owner/name".into()));
    /// assert_eq!(c.repository.as_deref(), Some("owner/name"));
    /// ```
    #[must_use]
    pub fn new(token: Option<String>, repository: Option<String>) -> Self {
        Self { token, repository }
    }

    /// Read `POST_KIT_GIT_TOKEN` and `POST_KIT_REPOSITORY`.
    #[must_use]
    pub fn from_env() -> Self {
        let get = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        Self {
            token: get("POST_KIT_GIT_TOKEN"),
            repository: get("POST_KIT_REPOSITORY"),
        }
    }
}

/// A pull request to open.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct PullRequest {
    /// Title.
    pub title: String,
    /// The branch holding the change.
    pub head: String,
    /// The branch to merge into.
    pub base: String,
    /// Description.
    pub body: String,
    /// Open as a draft.
    pub draft: bool,
}

/// Something that can open a pull request.
pub trait PullRequests {
    /// Open `req` in `repository` (`owner/name`) and return its URL.
    ///
    /// # Errors
    /// Returns an error if the request fails.
    fn open(&self, repository: &str, req: &PullRequest) -> Result<String>;
}

/// Opens pull requests on GitHub by running `curl`, with the token in a private header file
/// (never on the command line).
#[derive(Debug, Clone)]
pub struct CurlGithub {
    api_url: String,
    token: String,
}

impl CurlGithub {
    /// A client for `api_url` (for example `https://api.github.com`).
    #[must_use]
    pub fn new(api_url: impl Into<String>, token: impl Into<String>) -> Self {
        Self {
            api_url: api_url.into(),
            token: token.into(),
        }
    }
}

impl PullRequests for CurlGithub {
    fn open(&self, repository: &str, req: &PullRequest) -> Result<String> {
        let mut headers = tempfile::NamedTempFile::new().map_err(io_err("header file"))?;
        write!(headers, "Authorization: Bearer {}\nAccept: application/vnd.github+json\nContent-Type: application/json\n", self.token).map_err(io_err("header file"))?;
        let mut body = tempfile::NamedTempFile::new().map_err(io_err("body file"))?;
        let payload = json!({"title": req.title, "head": req.head, "base": req.base, "body": req.body, "draft": req.draft});
        body.write_all(payload.to_string().as_bytes())
            .map_err(io_err("body file"))?;
        let out = Command::new("curl")
            .args([
                "-sS",
                "-f",
                "-X",
                "POST",
                &format!("{}/repos/{repository}/pulls", self.api_url),
                "-H",
            ])
            .arg(format!("@{}", headers.path().display()))
            .arg("--data")
            .arg(format!("@{}", body.path().display()))
            .output()
            .map_err(|e| Error::Publish(format!("cannot run curl: {e}")))?;
        if !out.status.success() {
            return Err(Error::Publish(format!(
                "the pull request was refused: {}",
                String::from_utf8_lossy(&out.stderr)
                    .chars()
                    .take(300)
                    .collect::<String>()
            )));
        }
        let v: Value =
            serde_json::from_slice(&out.stdout).map_err(json_err("pull request response"))?;
        v["html_url"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| Error::Publish("the response had no html_url".into()))
    }
}

#[derive(Deserialize)]
struct Manifest {
    id: String,
    status: String,
    publishable: bool,
    #[serde(default)]
    files: Vec<String>,
    #[serde(default)]
    title: String,
    #[serde(default)]
    reason: String,
}

/// What [`publish`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Outcome {
    /// The pause switch is on; nothing was published.
    Paused,
    /// The run was skipped or failed, so there is nothing to publish.
    Nothing(String),
    /// The post was flagged and the profile holds flagged posts.
    Held,
    /// Mode is `stage`: nothing to do.
    StagedOnly,
    /// The daily cap was already reached.
    CapReached(String),
    /// Committed and pushed to the base branch.
    Published {
        /// The branch pushed to.
        branch: String,
        /// The post title.
        title: String,
    },
    /// A pull request was opened.
    PullRequest(String),
}

impl std::fmt::Display for Outcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Outcome::Paused => write!(f, "paused: nothing published"),
            Outcome::Nothing(why) => write!(f, "{why}"),
            Outcome::Held => write!(f, "HELD: this post was flagged and the profile holds flagged posts. It is archived with the run and not published."),
            Outcome::StagedOnly => write!(f, "mode is stage: files are staged only"),
            Outcome::CapReached(m) => write!(f, "{m}"),
            Outcome::Published { branch, title } => write!(f, "published directly to {branch}: {title}"),
            Outcome::PullRequest(url) => write!(f, "opened: {url}"),
        }
    }
}

fn git(repo: &Path, args: &[&str], envs: &[(String, String)]) -> Result<std::process::Output> {
    let out = Command::new("git")
        .args(args)
        .current_dir(repo)
        .envs(envs.iter().map(|(k, v)| (k, v)))
        .output()
        .map_err(|e| Error::Git(format!("cannot run git: {e}")))?;
    if !out.status.success() {
        return Err(Error::Git(format!(
            "{} failed: {}",
            args.iter().take(2).copied().collect::<Vec<_>>().join(" "),
            String::from_utf8_lossy(&out.stderr)
                .trim()
                .chars()
                .take(300)
                .collect::<String>()
        )));
    }
    Ok(out)
}

fn auth_env(token: &str) -> Vec<(String, String)> {
    vec![
        ("GIT_TERMINAL_PROMPT".into(), "0".into()),
        ("GIT_CONFIG_COUNT".into(), "1".into()),
        ("GIT_CONFIG_KEY_0".into(), "http.extraHeader".into()),
        (
            "GIT_CONFIG_VALUE_0".into(),
            format!(
                "Authorization: Basic {}",
                base64(format!("x-access-token:{token}").as_bytes())
            ),
        ),
    ]
}

/// Publish the post staged in `out`, under the profile's rules.
///
/// `mode` overrides the profile's mode. `prs` opens pull requests (use [`CurlGithub`]).
///
/// # Errors
/// - [`Error::Publish`] for a disallowed path, a symlink, a dirty working tree, a missing token, or a refused request;
/// - [`Error::ProposalOpen`] in `pr` mode when a proposal branch already exists on the remote;
/// - [`Error::Git`] when git fails.
pub fn publish(
    profile: &Profile,
    out: &Path,
    mode: Option<PublishMode>,
    creds: &Credentials,
    prs: &dyn PullRequests,
) -> Result<Outcome> {
    let path = out.join("manifest.json");
    let man: Manifest =
        serde_json::from_str(&std::fs::read_to_string(&path).map_err(io_err(&path))?)
            .map_err(json_err("manifest.json"))?;
    let mode = mode.unwrap_or(profile.data.publish.mode);
    if profile.paused() {
        return Ok(Outcome::Paused);
    }
    if man.status == "SKIPPED" || man.status == "FAILED" {
        return Ok(Outcome::Nothing(format!(
            "{}: nothing to publish ({})",
            man.status.to_lowercase(),
            man.reason
        )));
    }
    if !man.publishable {
        return Ok(Outcome::Held);
    }
    if mode == PublishMode::Stage {
        return Ok(Outcome::StagedOnly);
    }
    check_paths(profile, &man.files)?;

    let token = creds
        .token
        .as_deref()
        .ok_or_else(|| Error::Publish("set POST_KIT_GIT_TOKEN to publish".into()))?;
    let repo = profile.root.as_path();
    let base = match &profile.data.publish.base_branch {
        Some(b) => b.clone(),
        None => {
            String::from_utf8_lossy(&git(repo, &["rev-parse", "--abbrev-ref", "HEAD"], &[])?.stdout)
                .trim()
                .to_string()
        }
    };
    let mut status_args = vec!["status", "--porcelain", "--"];
    status_args.extend(man.files.iter().map(String::as_str));
    if !git(repo, &status_args, &[])?.stdout.is_empty() {
        return Err(Error::Publish("the working tree has uncommitted changes in the paths this post touches; refusing to publish over them".into()));
    }
    let date = man.id.get(..10).unwrap_or("");
    let cap = profile.data.publish.max_per_day;
    if cap > 0 && State::load(profile)?.posts_on(date) >= cap as usize {
        return Ok(Outcome::CapReached(format!(
            "daily cap of {cap} already reached for {date}: nothing published"
        )));
    }

    let envs = auth_env(token);
    let trailer = profile.data.publish.trailer.as_str();
    let mut add_args = vec!["add", "--"];
    add_args.extend(man.files.iter().map(String::as_str));
    if mode == PublishMode::Direct {
        git(repo, &["switch", "-q", &base], &[])?;
        apply(profile, out)?;
        git(repo, &add_args, &[])?;
        git(
            repo,
            &[
                "commit",
                "-q",
                "-m",
                &format!("Add post {}", man.id),
                "-m",
                "Written and published automatically by Post Kit. No person reviewed this post.",
                "-m",
                trailer,
            ],
            &[],
        )?;
        git(
            repo,
            &["push", "-q", "origin", &format!("HEAD:{base}")],
            &envs,
        )?;
        return Ok(Outcome::Published {
            branch: base,
            title: man.title,
        });
    }

    let repository = creds
        .repository
        .as_deref()
        .ok_or_else(|| Error::Publish("set POST_KIT_REPOSITORY (owner/name) for pr mode".into()))?;
    let prefix = &profile.data.publish.branch_prefix;
    let open = Command::new("git")
        .args([
            "ls-remote",
            "--exit-code",
            "--heads",
            "origin",
            &format!("refs/heads/{prefix}*"),
        ])
        .current_dir(repo)
        .output()
        .map_err(|e| Error::Git(format!("cannot run git: {e}")))?;
    if open.status.success() {
        return Err(Error::ProposalOpen);
    }
    let branch = format!("{prefix}{}", man.id);
    git(repo, &["switch", "-q", "-c", &branch], &[])?;
    apply(profile, out)?;
    git(repo, &add_args, &[])?;
    git(
        repo,
        &[
            "commit",
            "-q",
            "-m",
            &format!("Add post {}", man.id),
            "-m",
            "Proposed by Post Kit. A person reviews this pull request before it is merged.",
            "-m",
            trailer,
        ],
        &[],
    )?;
    git(repo, &["push", "-q", "-u", "origin", &branch], &envs)?;
    let digest = std::fs::read_to_string(out.join("digest.md")).unwrap_or_default();
    let body = format!(
        "{}\n\n## Before merging\n\n- [ ] Read the new page end to end.\n- [ ] Look at anything listed under Flagged above.\n- [ ] Approve this pull request. The approval is the record that a person reviewed it.\n\n{trailer}\n",
        digest.trim()
    );
    let url = prs.open(
        repository,
        &PullRequest {
            title: format!("Add post {}", man.id),
            head: branch,
            base: base.clone(),
            body,
            draft: man.status == "ATTENTION",
        },
    )?;
    git(repo, &["switch", "-q", &base], &[])?;
    Ok(Outcome::PullRequest(url))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_token_is_not_in_debug_output() {
        let c = Credentials {
            token: Some("sekrit".into()),
            repository: Some("o/r".into()),
        };
        assert!(!format!("{c:?}").contains("sekrit"));
    }

    #[test]
    fn the_auth_header_is_basic_and_goes_through_the_environment() {
        let env = auth_env("tok");
        let value = &env
            .iter()
            .find(|(k, _)| k == "GIT_CONFIG_VALUE_0")
            .unwrap()
            .1;
        assert_eq!(
            value,
            &format!("Authorization: Basic {}", base64(b"x-access-token:tok"))
        );
        assert!(env
            .iter()
            .any(|(k, v)| k == "GIT_TERMINAL_PROMPT" && v == "0"));
    }

    #[test]
    fn outcomes_read_plainly() {
        assert!(Outcome::Held.to_string().starts_with("HELD"));
        assert_eq!(Outcome::Paused.to_string(), "paused: nothing published");
    }
}
