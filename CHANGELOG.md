# Changelog

All notable changes to this crate are documented here. The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/)
and the crate follows [Semantic Versioning](https://semver.org/) (pre-1.0: a breaking change bumps the minor version).

## [0.1.0] - unreleased

### Added
- Profiles, with strict parsing of the `limits` and `publish` sections.
- The `post` and `conversation` formats, with schemas, instructions, checks and renderers.
- A tool-less, schema-checked model call behind a trait, a process backend, and a spend cap.
- A hashed term guard (plain, reversed and rot13 forms), plain-text structure rules, and a repository audit.
- Deterministic checks: word limits, sign-offs, markdown, footnote markers, invented references.
- Topic selection from site pages or a list, rotation, and drift (continue the last open question).
- History and a footnote counter kept in the repository.
- Staging (`run`), `apply`, and `publish` with stage, pull request and direct modes, a pause switch, a daily cap, an allowed-path list,
  and hold-on-flag.
- The `post` command, the contract (`spec/POST.md`), two example profiles, and golden fixtures.
- Event logs as a topic source (`topics.log`): grouped by day or by count, chronicled oldest first, with listed terms removed from the lines the model sees. Example profile `examples/chronicle`.
- `post calibrate`: reviews known-good pages several times and reports how often the advisory review would hold a good post.

### Fixed
- Crossing the spend cap on the advisory review call now stops the run with a `FAILED` digest. It was previously reported as a review that
  "did not run", so a run could overspend its cap on its last call.
- A tag with capitals or spaces no longer rejects a post: tags are normalised to lowercase hyphenated words in code. Found when a live run
  rejected one entry three times for the tag "game log".
