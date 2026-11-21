# Post Kit

Personas that write **blog posts**, with the checks and the publishing rules to run them unattended.

A persona is a prompt file. Post Kit gives it a topic, asks the model for a schema-checked answer with **no tools**, checks the answer in code, renders the post, and publishes it under rules you set. A person can be in the loop or not; that is a setting, and the Kit says plainly when nobody reviewed a post.

> **Status: alpha (0.1).** The profile format and the library API may change before 1.0. Minimum supported Rust version: 1.85.

## What it owns

| Piece | What it does |
|---|---|
| Model call | One tool-less, schema-checked call behind a trait, with a spend cap. |
| Term guard | Keeps listed terms (a withheld name, a codename) out of text. Stores hashes, not the terms. |
| Checks | Word limits, plain-text rules, sign-offs, footnotes, invented references. Code decides, never a model. |
| Formats | `post` (one voice, markdown) and `conversation` (several voices with margin notes, HTML). |
| Topics | Pages of a site, a list of briefs, an event log chronicled in order, or the question the last post left open. |
| History | Kept in the repository, so it is reviewable and revertible. |
| Publishing | Stage only, a pull request, or a direct commit. Pause switch, daily cap, path allowlist, hold-on-flag. |

## What it does not own

Model hosting, accounts, memory across projects, site themes, hosting, or deciding whether a post should exist. It does not make an unreviewed post safe: the checks narrow the risk and do not remove it.

## Install

The crate is published as `slashbuilder-post-kit`. The library keeps the short name `post_kit`, and the command is `post`.

```sh
cargo install slashbuilder-post-kit     # installs the `post` command
cargo add slashbuilder-post-kit         # the library: `use post_kit::...`
```

From a checkout: `cargo install --path .`

The default model backend runs a command-line model client found on `PATH` (or named by `POST_KIT_LLM`). Any other backend can implement `post_kit::model::ModelBackend`.

## Use

```sh
post --profile examples/field-notes/profile.json plan                 # what would be written next; no model call
post --profile examples/field-notes/profile.json run --out /tmp/out   # write one post into a folder; changes nothing else
post --profile examples/field-notes/profile.json status
post --profile examples/chronicle/profile.json plan                    # chronicle a log of events, a day at a time
post --profile PROFILE calibrate --samples 3                          # how often the review flags text known to be fine
post --profile PROFILE guard add TERM...                              # hashes only are stored
post --profile PROFILE apply   --out /tmp/out                         # copy the staged post into the working tree
post --profile PROFILE publish --out /tmp/out                         # commit/push or pull request, per the profile
```

Read `digest.md` first. It ends `STATUS: OK`, `ATTENTION`, `SKIPPED` or `FAILED`. The contract is in `spec/POST.md`.

From Rust:

```rust
use post_kit::guard::Guard;

let dir = tempfile::tempdir()?;
let list = dir.path().join("blocklist.sha256");
post_kit::guard::add_terms(&["Examplecorp"], &list)?;
let guard = Guard::from_file(&list)?;
assert!(!guard.scan("Written for EXAMPLE-CORP Ltd.").is_empty());
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Who decides what

| Decision | Made by |
|---|---|
| Did the answer fit the schema and limits | code |
| Does it hold a listed term, a link, an address, markup, a sign-off | code |
| Does it cite a section or note that was not supplied | code (raises `ATTENTION`) |
| Is anything else worrying | a second, smaller model pass; advisory |
| Is a flagged post published | the profile: `on_attention` is `hold` (default) or `publish` |
| Is a post published at all | the profile: `mode` is `stage` (default), `pr` or `direct` |

## Exit codes

| Code | Meaning |
|---|---|
| 0 | OK, or skipped |
| 1 | any other failure |
| 3 | ATTENTION: flagged |
| 64 | usage or profile error |
| 65 | a proposal branch is already open |
| 73 | a check failed after retries, or the spend cap was hit |

## Disclosure

Posts carry front matter from `disclosure.front_matter`. When no person reviews posts, set `reviewed: false` and say so on the page and in the site footer. Commits made by `direct` mode say that no person reviewed the post. Do not claim review that did not happen.

## Known limits

* The guard hides a term from a text search. It does not stop someone who suspects the term and can hash it. Keep the list outside the repository (`POST_KIT_BLOCKLIST`) for more.
* The model client may add account details to the model's context. The guard and the review catch what it writes, not what it sees.
* The advisory review is a model and can be wrong in both directions. Because a flagged post is held, a jumpy reviewer quietly stops a blog.
  Run `post calibrate` before a profile goes on a schedule: it reviews your known-good pages several times and reports the false-hold rate.
  On the first profile it flagged about 1 page in 10, mostly the same pages every time (pages that legitimately cite real people), which
  points to a rule to tune in `review.notes`, not to noise.
* Pull request mode allows one open proposal at a time, because posts share a history file and, for conversations, a footnote counter.
* Topic drift feeds the model its own earlier output as quoted data. Expect repetition over many posts.
* Pull requests are opened by running `curl`; there is no built-in HTTP client.

## License

Source code under [Apache-2.0](LICENSE). Contributions need a sign-off (DCO); see [CONTRIBUTING.md](CONTRIBUTING.md).

## About

Post Kit is part of the **SlashBuilder Kit ecosystem**, a set of reusable Rust and Dart libraries built around the Kit pattern: each Kit owns one
self-contained domain, and products are composed from Kits.

Maintained by [SlashBuilder](https://slashbuilder.com) / Douglas Lockamy.
