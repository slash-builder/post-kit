//! The `post` command. See the crate docs and README.

#![allow(clippy::print_stdout, clippy::print_stderr)]

use clap::{Parser, Subcommand, ValueEnum};
use post_kit::guard::{self, Guard};
use post_kit::model::ProcessModel;
use post_kit::profile::{Profile, PublishMode};
use post_kit::publish::{publish, Credentials, CurlGithub};
use post_kit::run::{apply, build_plan, calibrate, stage, StageOptions};
use post_kit::state::State;
use post_kit::Error;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// Personas that write blog posts: plan, run, apply, publish.
///
/// Exit codes: 0 ok (or skipped), 3 attention (flagged), 73 a check failed, 64 usage, 65 a proposal is already open, 1 other.
#[derive(Parser)]
#[command(name = "post", version, about)]
struct Cli {
    /// The profile JSON file.
    #[arg(long)]
    profile: PathBuf,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Clone, Copy, ValueEnum)]
enum ModeArg {
    Stage,
    Pr,
    Direct,
}

#[derive(Subcommand)]
enum Cmd {
    /// Show which topic would be written about next (no model call).
    Plan {
        /// Force a topic key.
        #[arg(long)]
        topic: Option<String>,
        /// Override today's date.
        #[arg(long)]
        date: Option<String>,
    },
    /// Write one post into a folder (calls the model); changes nothing else.
    Run {
        /// Folder to write.
        #[arg(long)]
        out: PathBuf,
        /// Force a topic key.
        #[arg(long)]
        topic: Option<String>,
        /// Override today's date.
        #[arg(long)]
        date: Option<String>,
        /// Also copy the result into the working tree.
        #[arg(long)]
        apply: bool,
        /// Ignore the daily cap and overwrite an existing file.
        #[arg(long)]
        force: bool,
    },
    /// Copy a staged post into the working tree.
    Apply {
        /// The folder written by `run`.
        #[arg(long)]
        out: PathBuf,
    },
    /// Commit/push or open a pull request, per the profile. A flagged post is held.
    Publish {
        /// The folder written by `run`.
        #[arg(long)]
        out: PathBuf,
        /// Override the profile's mode.
        #[arg(long, value_enum)]
        mode: Option<ModeArg>,
    },
    /// Manage the term blocklist (hashes only).
    Guard {
        #[command(subcommand)]
        cmd: GuardCmd,
    },
    /// Show mode, pause state, daily cap and history.
    Status,
    /// Measure how often the review flags known-good pages (calls the model).
    Calibrate {
        /// Reviews per page.
        #[arg(long, default_value_t = 3)]
        samples: usize,
    },
}

#[derive(Subcommand)]
enum GuardCmd {
    /// Add terms to the blocklist.
    Add {
        /// Terms to list.
        #[arg(required = true)]
        terms: Vec<String>,
    },
    /// Check files against the blocklist.
    Check {
        /// Files to check.
        #[arg(required = true)]
        files: Vec<PathBuf>,
    },
    /// Scan a repository.
    Audit {
        /// Folder to scan; default the repository root.
        #[arg(long)]
        root: Option<PathBuf>,
    },
}

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .init();
    let cli = Cli::parse();
    match go(&cli) {
        Ok(code) => ExitCode::from(u8::try_from(code).unwrap_or(1)),
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(u8::try_from(e.exit_code()).unwrap_or(1))
        }
    }
}

fn go(cli: &Cli) -> Result<i32, Error> {
    let profile = Profile::load(&cli.profile)?;
    match &cli.cmd {
        Cmd::Plan { topic, date } => {
            let state = State::load(&profile)?;
            let date = date.clone().unwrap_or_else(today);
            let mut plan = build_plan(&profile, &state, &date, topic.as_deref())?;
            let short: String = plan.topic.excerpt.chars().take(300).collect();
            plan.topic.excerpt = format!("{short} [...]");
            println!(
                "{}",
                serde_json::to_string_pretty(&plan).map_err(|e| Error::Usage(e.to_string()))?
            );
            Ok(0)
        }
        Cmd::Status => {
            let st = State::load(&profile)?;
            let p = &profile.data.publish;
            let v = json!({
                "profile": profile.data.name,
                "format": format!("{:?}", profile.data.format).to_lowercase(),
                "mode": format!("{:?}", p.mode).to_lowercase(),
                "on_attention": format!("{:?}", p.on_attention).to_lowercase(),
                "paused": profile.paused(),
                "posts_total": st.entries.len(),
                "posts_today": st.posts_on(&today()),
                "daily_cap": p.max_per_day,
                "last": st.entries.last().map(|e| e.title.clone()),
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&v).map_err(|e| Error::Usage(e.to_string()))?
            );
            Ok(0)
        }
        Cmd::Run {
            out,
            topic,
            date,
            apply: apply_after,
            force,
        } => {
            let backend = ProcessModel::from_env()?;
            let mut opts = StageOptions::new(out);
            opts.topic.clone_from(topic);
            opts.date.clone_from(date);
            opts.force = *force;
            let staged = stage(&profile, &backend, &opts)?;
            println!("{}", staged.digest);
            if *apply_after {
                for rel in apply(&profile, out)? {
                    println!("applied {rel}");
                }
            }
            Ok(staged.exit_code())
        }
        Cmd::Apply { out } => {
            for rel in apply(&profile, out)? {
                println!("applied {rel}");
            }
            Ok(0)
        }
        Cmd::Publish { out, mode } => {
            let creds = Credentials::from_env();
            let api = std::env::var("POST_KIT_API_URL")
                .unwrap_or_else(|_| "https://api.github.com".into());
            let prs = CurlGithub::new(api, creds.token.clone().unwrap_or_default());
            let mode = mode.map(|m| match m {
                ModeArg::Stage => PublishMode::Stage,
                ModeArg::Pr => PublishMode::Pr,
                ModeArg::Direct => PublishMode::Direct,
            });
            println!("{}", publish(&profile, out, mode, &creds, &prs)?);
            Ok(0)
        }
        Cmd::Guard { cmd } => guard_cmd(&profile, cmd),
        Cmd::Calibrate { samples } => {
            let backend = ProcessModel::from_env()?;
            let cal = calibrate(&profile, &backend, (*samples).max(1))?;
            for r in cal.rows.iter().filter(|r| r.flagged > 0) {
                println!(
                    "{}/{} flagged: {} {:?}",
                    r.flagged, r.samples, r.key, r.kinds
                );
            }
            println!(
                "{}/{} reviews of {} known-good pages flagged ({:.0}%); spend at list price ${:.2}",
                cal.flagged,
                cal.reviews,
                cal.rows.len(),
                cal.flag_rate() * 100.0,
                cal.spend_usd
            );
            Ok(0)
        }
    }
}

fn today() -> String {
    std::env::var("POST_KIT_DATE")
        .ok()
        .filter(|d| !d.is_empty())
        .unwrap_or_else(|| {
            let secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());
            post_kit::run::date_from_unix(secs)
        })
}

fn guard_cmd(profile: &Profile, cmd: &GuardCmd) -> Result<i32, Error> {
    let list = profile.blocklist();
    match cmd {
        GuardCmd::Add { terms } => {
            let path =
                list.ok_or_else(|| Error::Profile("the profile has no guard.blocklist".into()))?;
            println!("wrote {} hash line(s)", guard::add_terms(terms, &path)?);
            Ok(0)
        }
        GuardCmd::Check { files } => {
            let g = Guard::load(list.as_deref())?;
            if g.is_empty() {
                eprintln!("no blocklist; nothing to check against");
                return Ok(2);
            }
            let mut bad = 0;
            for f in files {
                let text = std::fs::read_to_string(f)
                    .map_err(|e| Error::Usage(format!("{}: {e}", f.display())))?;
                let hits = g.scan(&text);
                for h in &hits {
                    println!(
                        "{}: listed term at normalised offset {}: {}",
                        f.display(),
                        h.offset,
                        h.context
                    );
                }
                bad += i32::from(!hits.is_empty());
            }
            println!(
                "{}",
                if bad == 0 {
                    "clean: no listed term found".to_string()
                } else {
                    format!("{bad} file(s) hit")
                }
            );
            Ok(i32::from(bad != 0))
        }
        GuardCmd::Audit { root } => {
            let g = Guard::load(list.as_deref())?;
            if g.is_empty() {
                eprintln!("no blocklist; nothing to check against");
                return Ok(2);
            }
            let root: &Path = root.as_deref().unwrap_or(&profile.root);
            let (lines, clean) = guard::audit_report(root, &g)?;
            println!("{}", lines.join("\n"));
            Ok(i32::from(!clean))
        }
    }
}
