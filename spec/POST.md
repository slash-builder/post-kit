# Post Kit contract

Version 0.1 (alpha). This describes the files Post Kit reads and writes, so another tool can read them or another implementation can match them. The Rust types and the tests in this repository are the reference.

## Profile

One JSON file. Paths inside it are relative to the profile file, except where a section says "repository root". The repository root is `root` (default `.`), relative to the profile.

| Section | Required | Meaning |
|---|---|---|
| `name` | yes | A short name. |
| `format` | yes | `post` or `conversation`. |
| `root` | no | Repository root. |
| `guard.blocklist` | no | Blocklist file (see below). A run **refuses to start** without a non-empty blocklist. |
| `brief` | no | Rules every persona receives. |
| `personas` | yes | Object of id to `{file, label?, model}`. `file` is the prompt; its front matter is dropped. |
| `turns` | yes | List of `{voice, kind, ask?}`. |
| `topics` | no | `collections` (name to folder, repository root), `list_file`, `log`, `skip`, `permalinks`, `plain_rules`, `drift.continue_probability`. |
| `limits` | no | Word limits, `retries_per_turn`, `call_timeout_s`, `max_cost_usd`. Unknown keys are an error. |
| `checks` | no | `signature_regex`, `reference_regex`. |
| `render` | no | `layout`, `attribution`, `author`, `byline_markdown`, `reply_class`, `tokens`, `redaction_html`, `signature_html`, `footnotes {config_file, key}`. |
| `disclosure.front_matter` | no | Key/value pairs added to every post's front matter. |
| `output` | yes | `directory`, `state_file`, `file_prefix` (conversation). Repository root. |
| `publish` | no | `mode` (`stage`, `pr`, `direct`; default `stage`), `on_attention` (`hold`, `publish`; default `hold`), `max_per_day` (default 1; 0 = none), `pause_file`, `branch_prefix`, `trailer`, `allowed_paths`, `base_branch`. Unknown keys are an error. |

## Formats

**`post`.** Exactly one turn, kind `post`. The persona answers `{title, summary, paragraphs, footnotes, tags, held_open}`. Output is a markdown file `directory/DATE-slug.md` with front matter (`layout`, `title`, `date`, `description`, `tags`, optional `author`, the disclosure keys, `post_id`, `topic`, optional `continued_from`), the paragraphs, an optional byline, and `[^n]: note` footnotes.

**`conversation`.** Turns in this shape: opener `open`; one `margin` per margin voice (distinct, not the opener); opener `close`; then any number of `last` turns by margin voices.

| Kind | Answer |
|---|---|
| `open` | `{title, paragraphs, footnotes}` |
| `margin` | `{margin}` |
| `close` | `{paragraphs, footnotes, reply_to_<voice>..., summary, held_open}` (a voice id's `-` becomes `_`) |
| `last` | `{margin}` |

Output is `directory/PREFIX-ID.md`: HTML with `<aside class="VOICE">` margin notes, replies inside them, a signature, and a footnote list numbered from the repository counter.

## Answers and checks

The model writes plain text into a JSON schema and has no tools. Code decides pass or fail. A failing answer is retried with the reasons up to `retries_per_turn` times, then the run fails.

An answer fails if any string is empty; is over its word limit; contains a link, email address, file path, domain name, combining mark or angle bracket; starts a line with markdown structure; ends with a sign-off (a dash line, or a match for `signature_regex`); or contains a **listed term**. Footnote markers `[^1]..[^n]` must each appear once. The finished page is scanned for listed terms again.

A run is `ATTENTION` if the advisory review (a second model pass) returns concerns or fails to run, or if the text cites a reference matching `reference_regex` that is absent from the supplied material. Neither can fail a run; they only decide `STATUS`.

## Listed terms

A blocklist file has lines `length:sha256`, where the hash is of a term normalised to lowercase ASCII letters and digits (accents and combining marks removed), plus its reversed and rot13 forms. Matching hashes every window of the normalised text. It hides a term from a text search; it does not stop someone who suspects the term and can hash it.

## Run output

`run --out DIR` empties and writes `DIR`:

| File | Content |
|---|---|
| `digest.md` | Human summary. The last non-empty line is `STATUS: OK`, `ATTENTION`, `SKIPPED` or `FAILED`. |
| `manifest.json` | `{id, status, publishable, files, title?, topic?, cost_usd?, calls?, reason?}`. `files` are repository-relative paths under `files/`. |
| `transcript.json` | `{plan, turns, review}`: every turn, its attempts and rejection reasons. |
| `files/` | The post, the history file and (conversation) the counter file, laid out as in the repository. |

`publishable` is true when the status is `OK`, or `ATTENTION` with `on_attention: publish`.

## History

`output.state_file`: `{"entries": [{id, date, topic, title, file, summary, held_open, footnotes}]}`, oldest first. Later runs show the last `prior_entries` to the model and use it for topic rotation, drift and the daily cap.

## Publishing

`publish --out DIR` reads `manifest.json` and does nothing if the pause switch is on (`pause_file` exists in the repository, or `POST_KIT_PAUSE` is set), the status is `SKIPPED` or `FAILED`, the post is not publishable (**held**), the mode is `stage`, or the daily cap is reached. Every path in `files` must be inside `allowed_paths` (default: the output paths) and inside the repository. A symlink in the proposal is refused. The working tree must be clean in those paths.

`direct`: commit on the base branch and push. The message says no person reviewed the post. `pr`: push `branch_prefix` + id and open a pull request (a draft when `ATTENTION`); refuses if another branch with the prefix exists on the remote. Both end the message with `trailer`.

## Event logs

`topics.log` turns a log into topics: `{file, group_by, count, title, fields}`. `file` is relative to the profile and is JSON lines, or one JSON document that is a list or has an `events` list. Each event is an object; the time, who, kind and text are read from the first present key of `fields.time` (default `ts`, `t`, `time`, `timestamp`), `fields.who` (`who`, `agent`, `name`, `player`), `fields.kind` (`kind`, `type`, `event`) and `fields.text` (`text`, `msg`, `message`). Other scalar keys are appended as `key=value`. A line is `who: kind text key=value`. Lines that are not JSON objects are skipped.

`group_by: day` groups by calendar day (ISO text, or epoch seconds or milliseconds; anything else is `undated`); `group_by: count` groups every `count` events. Each group is one topic, `log/ID`, titled from `title` with `{group}`. Log topics are taken **oldest first** and a group is used once before any is repeated.

A line that contains a listed term is replaced by `[line removed]` before the model sees it.

## Calibration

`calibrate --samples N` reviews every page of the profile's collections (not list briefs, and not pages the bot wrote) `N` times with the
advisory review and prints the pages that were flagged, as `flagged/samples: key [kinds]`, then the overall rate and the spend. A post is
held when its review flags it, so the rate is roughly the share of good posts that would be held. A page flagged on every sample points to
a systematic cause (tune `review.notes`); a page flagged on some samples points to noise. The spend cap applies.

## Exit codes

| Code | Meaning |
|---|---|
| 0 | OK, or skipped |
| 1 | any other failure |
| 3 | ATTENTION: flagged |
| 64 | usage or profile error |
| 65 | a proposal branch is already open |
| 73 | a check failed after retries, or the spend cap was hit |

## Environment

`POST_KIT_LLM` (model command), `POST_KIT_TOKEN_FILE`, `POST_KIT_BLOCKLIST` (replaces the profile's), `POST_KIT_GIT_TOKEN`, `POST_KIT_REPOSITORY`, `POST_KIT_API_URL`, `POST_KIT_PAUSE`, `POST_KIT_DATE`.
