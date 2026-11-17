//! # Post Kit
//!
//! Personas that write **blog posts**: a persona (a prompt file) plus a topic becomes a checked,
//! rendered post or multi-voice exchange, and is published under rules you set.
//!
//! The model gets **no tools** and writes plain text into a JSON schema. Code writes all markup,
//! decides whether each answer passes, and publishes. A person can be in the loop or not; that is a
//! setting, and the Kit says plainly when nobody reviewed a post.
//!
//! | Module | What it does |
//! |---|---|
//! | [`model`] | One tool-less, schema-checked model call behind a trait, with a spend meter. |
//! | [`guard`] | Keeps listed terms out of text while storing only hashes. |
//! | [`checks`] | Word limits, plain-text rules, footnote bookkeeping, invented references. |
//! | [`text`] | Page-to-text and text-to-HTML helpers. |
//! | [`profile`] | The JSON file that describes one blog setup. |
//! | [`state`] | Post history and counters, kept in the repository. |
//! | [`topics`] | What to write about: pages, a list, or the last post's open question. |
//! | [`run`] | Plan, compose, review, render, stage. Publishes nothing. |
//! | [`publish`] | Commit directly or open a pull request, with a pause switch, daily cap and hold-on-flag. |
//!
//! ```
//! use post_kit::guard::Guard;
//!
//! // The guard stores hashes, never the term itself.
//! let dir = tempfile::tempdir()?;
//! let list = dir.path().join("blocklist.sha256");
//! post_kit::guard::add_terms(&["Examplecorp"], &list)?;
//! let guard = Guard::from_file(&list)?;
//! assert!(!guard.scan("Written for EXAMPLE-CORP Ltd.").is_empty());
//! assert!(guard.scan("Nothing to see here.").is_empty());
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod checks;
mod error;
mod format;
pub mod guard;
pub mod model;
pub mod profile;
pub mod publish;
pub mod run;
pub mod state;
pub mod text;
pub mod topics;
mod util;

pub use error::{Error, Result};
