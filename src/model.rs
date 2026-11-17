//! Model access: one tool-less, schema-checked call behind a trait, and a spend meter.
//!
//! The default backend ([`ProcessModel`]) runs a command-line model client with tools switched off,
//! in an empty temporary directory, and reads one JSON answer that must match a schema. Anything
//! else can implement [`ModelBackend`] (tests do).

use crate::error::{io_err, Error, Result};
use serde_json::Value;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// One request to the model.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ModelRequest<'a> {
    /// The system prompt (persona, brief, output rules).
    pub system: &'a str,
    /// The user message (quoted data and the task).
    pub payload: &'a str,
    /// JSON schema the answer must match.
    pub schema: &'a Value,
    /// Model alias.
    pub model: &'a str,
    /// Give up after this long.
    pub timeout: Duration,
}

/// The schema-shaped answer and what it cost.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ModelReply {
    /// The structured answer.
    pub output: Value,
    /// Cost at list price. An estimate, not a bill.
    pub cost_usd: f64,
    /// Tokens used.
    pub tokens: u64,
}

impl<'a> ModelRequest<'a> {
    /// A request. All fields are required.
    ///
    /// ```
    /// let schema = serde_json::json!({});
    /// let req = post_kit::model::ModelRequest::new("system", "payload", &schema, "sonnet", std::time::Duration::from_secs(30));
    /// assert_eq!(req.model, "sonnet");
    /// ```
    #[must_use]
    pub fn new(
        system: &'a str,
        payload: &'a str,
        schema: &'a Value,
        model: &'a str,
        timeout: Duration,
    ) -> Self {
        Self {
            system,
            payload,
            schema,
            model,
            timeout,
        }
    }
}

impl ModelReply {
    /// A reply with a cost.
    ///
    /// ```
    /// let r = post_kit::model::ModelReply::new(serde_json::json!({}), 0.02, 7);
    /// assert_eq!((r.cost_usd, r.tokens), (0.02, 7));
    /// ```
    #[must_use]
    pub fn new(output: Value, cost_usd: f64, tokens: u64) -> Self {
        Self {
            output,
            cost_usd,
            tokens,
        }
    }

    /// A reply with no cost, for tests and fakes.
    ///
    /// ```
    /// let r = post_kit::model::ModelReply::free(serde_json::json!({"margin": "Fair."}));
    /// assert_eq!(r.cost_usd, 0.0);
    /// ```
    #[must_use]
    pub fn free(output: Value) -> Self {
        Self {
            output,
            cost_usd: 0.0,
            tokens: 0,
        }
    }
}

/// Anything that can answer a request.
///
/// ```
/// use post_kit::model::{ModelBackend, ModelReply, ModelRequest};
/// struct Echo;
/// impl ModelBackend for Echo {
///     fn call(&self, _req: &ModelRequest<'_>) -> post_kit::Result<ModelReply> {
///         Ok(ModelReply::free(serde_json::json!({"margin": "Fair."})))
///     }
/// }
/// let schema = serde_json::json!({});
/// let req = ModelRequest::new("s", "p", &schema, "m", std::time::Duration::from_secs(1));
/// assert_eq!(Echo.call(&req)?.output["margin"], "Fair.");
/// # Ok::<(), post_kit::Error>(())
/// ```
pub trait ModelBackend {
    /// Answer one request.
    ///
    /// # Errors
    /// Returns an error if the backend fails or answers badly.
    fn call(&self, req: &ModelRequest<'_>) -> Result<ModelReply>;
}

/// Tracks spend across a run and stops it at the cap.
#[derive(Debug, Clone)]
pub struct Meter {
    usd: f64,
    calls: u32,
    tokens: u64,
    cap: f64,
}

impl Meter {
    /// A meter that errors once spend passes `cap` dollars.
    ///
    /// ```
    /// let mut m = post_kit::model::Meter::new(0.05);
    /// let reply = post_kit::model::ModelReply::new(serde_json::json!({}), 0.04, 10);
    /// m.charge(&reply)?;
    /// assert!(m.charge(&reply).is_err());
    /// # Ok::<(), post_kit::Error>(())
    /// ```
    #[must_use]
    pub fn new(cap: f64) -> Self {
        Self {
            usd: 0.0,
            calls: 0,
            tokens: 0,
            cap,
        }
    }

    /// Add a reply's cost.
    ///
    /// # Errors
    /// Returns [`Error::CostCap`] once the total passes the cap.
    pub fn charge(&mut self, reply: &ModelReply) -> Result<()> {
        self.usd += reply.cost_usd;
        self.calls += 1;
        self.tokens += reply.tokens;
        if self.usd > self.cap {
            return Err(Error::CostCap {
                spent: self.usd,
                cap: self.cap,
            });
        }
        Ok(())
    }

    /// Spend so far, at list price.
    #[must_use]
    pub fn usd(&self) -> f64 {
        self.usd
    }

    /// Calls so far.
    #[must_use]
    pub fn calls(&self) -> u32 {
        self.calls
    }

    /// Tokens so far.
    #[must_use]
    pub fn tokens(&self) -> u64 {
        self.tokens
    }
}

/// Runs a model command-line client once per request, with tools switched off.
#[derive(Debug, Clone)]
pub struct ProcessModel {
    command: PathBuf,
    token_file: Option<PathBuf>,
}

impl ProcessModel {
    /// Use `command`. If `token_file` is set, its contents are given to the client as its login token
    /// (the value never enters this process's own environment).
    #[must_use]
    pub fn new(command: PathBuf, token_file: Option<PathBuf>) -> Self {
        Self {
            command,
            token_file,
        }
    }

    /// Find the command: `POST_KIT_LLM`, else `claude` on the path, else `~/.local/bin/claude`.
    /// `POST_KIT_TOKEN_FILE` names the token file.
    ///
    /// # Errors
    /// Returns an error if no command can be found.
    pub fn from_env() -> Result<Self> {
        let token_file = std::env::var_os("POST_KIT_TOKEN_FILE")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from);
        if let Some(cmd) = std::env::var_os("POST_KIT_LLM").filter(|v| !v.is_empty()) {
            return Ok(Self::new(PathBuf::from(cmd), token_file));
        }
        if let Some(found) = which("claude") {
            return Ok(Self::new(found, token_file));
        }
        let home = std::env::var_os("HOME").map(PathBuf::from);
        if let Some(alt) = home
            .map(|h| h.join(".local").join("bin").join("claude"))
            .filter(|p| p.is_file())
        {
            return Ok(Self::new(alt, token_file));
        }
        Err(Error::Model(
            "no model command: install the client or set POST_KIT_LLM".into(),
        ))
    }
}

fn which(name: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .flat_map(|d| [d.join(name), d.join(format!("{name}.exe"))])
        .find(|p| p.is_file())
}

/// The arguments given to the model client for a request.
pub(crate) fn command_args(req: &ModelRequest<'_>) -> Vec<String> {
    [
        "-p",
        "--model",
        req.model,
        "--output-format",
        "json",
        "--json-schema",
        &req.schema.to_string(),
        "--system-prompt",
        req.system,
        "--no-session-persistence",
        "--disable-slash-commands",
        "--strict-mcp-config",
        "--setting-sources",
        "project",
        "--tools",
        "",
    ]
    .iter()
    .map(|s| (*s).to_string())
    .collect()
}

/// Read the client's JSON envelope.
///
/// ```
/// let r = post_kit::model::parse_reply(r#"{"is_error": false, "total_cost_usd": 0.02, "usage": {"input_tokens": 3, "output_tokens": 4}, "structured_output": {"a": 1}}"#)?;
/// assert_eq!((r.cost_usd, r.tokens, r.output["a"].as_i64()), (0.02, 7, Some(1)));
/// # Ok::<(), post_kit::Error>(())
/// ```
///
/// # Errors
/// Returns an error if the text is not JSON, reports an error, or has no structured answer.
pub fn parse_reply(stdout: &str) -> Result<ModelReply> {
    let v: Value =
        serde_json::from_str(stdout).map_err(|e| Error::Model(format!("unreadable reply: {e}")))?;
    if v["is_error"].as_bool().unwrap_or(false) {
        let msg: String = v["result"]
            .as_str()
            .unwrap_or("error")
            .chars()
            .take(300)
            .collect();
        return Err(Error::Model(format!("model error: {msg}")));
    }
    let output = v
        .get("structured_output")
        .filter(|o| !o.is_null())
        .cloned()
        .ok_or_else(|| Error::Model("no structured_output in the model response".into()))?;
    let tokens = [
        "input_tokens",
        "output_tokens",
        "cache_creation_input_tokens",
        "cache_read_input_tokens",
    ]
    .iter()
    .map(|k| v["usage"][k].as_u64().unwrap_or(0))
    .sum();
    Ok(ModelReply {
        output,
        cost_usd: v["total_cost_usd"].as_f64().unwrap_or(0.0),
        tokens,
    })
}

fn read_all(mut r: impl Read + Send + 'static) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut s = String::new();
        let _ = r.read_to_string(&mut s);
        s
    })
}

impl ModelBackend for ProcessModel {
    fn call(&self, req: &ModelRequest<'_>) -> Result<ModelReply> {
        let tmp = tempfile::tempdir().map_err(io_err(Path::new("tempdir")))?;
        let mut cmd = Command::new(&self.command);
        cmd.args(command_args(req))
            .current_dir(tmp.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(tf) = &self.token_file {
            let token = std::fs::read_to_string(tf).map_err(io_err(tf))?;
            cmd.env("CLAUDE_CODE_OAUTH_TOKEN", token.trim());
        }
        let mut child = cmd
            .spawn()
            .map_err(|e| Error::Model(format!("cannot run {}: {e}", self.command.display())))?;
        let stdin = child.stdin.take();
        let payload = req.payload.to_string();
        let writer = std::thread::spawn(move || {
            if let Some(mut s) = stdin {
                let _ = s.write_all(payload.as_bytes());
            }
        });
        let out = child.stdout.take().map(read_all);
        let err = child.stderr.take().map(read_all);
        let deadline = Instant::now() + req.timeout;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() >= deadline => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(Error::Model(format!(
                        "timed out after {}s",
                        req.timeout.as_secs()
                    )));
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(25)),
                Err(e) => return Err(Error::Model(format!("wait failed: {e}"))),
            }
        };
        let _ = writer.join();
        let stdout = out.and_then(|h| h.join().ok()).unwrap_or_default();
        let stderr = err.and_then(|h| h.join().ok()).unwrap_or_default();
        if !status.success() {
            let msg: String = (if stderr.is_empty() { stdout } else { stderr })
                .chars()
                .take(300)
                .collect();
            return Err(Error::Model(format!(
                "model command exited {}: {msg}",
                status.code().unwrap_or(-1)
            )));
        }
        parse_reply(&stdout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn arguments_switch_tools_off_and_isolate_the_client() {
        let schema = json!({"type": "object"});
        let req = ModelRequest {
            system: "SYS",
            payload: "P",
            schema: &schema,
            model: "sonnet",
            timeout: Duration::from_secs(1),
        };
        let a = command_args(&req);
        let pos = a.iter().position(|x| x == "--tools").unwrap();
        assert_eq!(a[pos + 1], "");
        for flag in [
            "--no-session-persistence",
            "--disable-slash-commands",
            "--strict-mcp-config",
            "--json-schema",
            "--system-prompt",
        ] {
            assert!(a.iter().any(|x| x == flag), "{flag}");
        }
        assert!(a.contains(&"SYS".to_string()) && a.contains(&"sonnet".to_string()));
    }

    #[test]
    fn replies() {
        assert!(parse_reply("not json").is_err());
        assert!(parse_reply(r#"{"is_error": true, "result": "boom"}"#)
            .unwrap_err()
            .to_string()
            .contains("boom"));
        assert!(parse_reply(r#"{"is_error": false}"#).is_err());
        assert!(parse_reply(r#"{"is_error": false, "structured_output": null}"#).is_err());
    }

    #[test]
    fn the_meter_stops_at_the_cap() {
        let mut m = Meter::new(0.05);
        let r = ModelReply {
            output: json!({}),
            cost_usd: 0.03,
            tokens: 5,
        };
        m.charge(&r).unwrap();
        assert!(matches!(m.charge(&r), Err(Error::CostCap { .. })));
        assert_eq!((m.calls(), m.tokens()), (2, 10));
    }

    #[cfg(unix)]
    #[test]
    fn a_real_process_is_run_and_a_hung_one_is_killed() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let ok = d.path().join("ok.sh");
        std::fs::write(&ok, "#!/bin/sh\ncat >/dev/null\necho '{\"is_error\":false,\"total_cost_usd\":0.01,\"structured_output\":{\"x\":1}}'\n").unwrap();
        let slow = d.path().join("slow.sh");
        std::fs::write(&slow, "#!/bin/sh\nsleep 30\n").unwrap();
        for p in [&ok, &slow] {
            std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let schema = json!({});
        let req = ModelRequest {
            system: "s",
            payload: "p",
            schema: &schema,
            model: "m",
            timeout: Duration::from_millis(400),
        };
        let r = ProcessModel::new(ok, None).call(&req).unwrap();
        assert_eq!(r.output["x"], 1);
        let e = ProcessModel::new(slow, None).call(&req).unwrap_err();
        assert!(e.to_string().contains("timed out"), "{e}");
    }
}
