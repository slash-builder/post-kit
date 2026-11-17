# Security policy

## Reporting a vulnerability

**Do not open a public issue for a vulnerability.** Report it privately, by either of these:

1. **GitHub private vulnerability reporting:** use the "Report a vulnerability" button on the [Security tab of this repository](https://github.com/slash-builder/post-kit/security/advisories/new). Only the maintainer and you see the report.
2. **Email:** security@slashbuilder.com, for when GitHub's form is not an option for you.

Please include:

* what you found and where (file, command or page);
* the commit of `main` you looked at (`git rev-parse HEAD`) and your operating system;
* steps to reproduce it, or a short proof of concept;
* what you think the impact is, and whether anyone else knows.

Do not put a token or a private term list in a report.

## What to expect

You get an **acknowledgement within 7 days**, from a person. Nothing else is promised: this project has one maintainer, no paid support and no fixed fix time. If the report is valid, the reply says what happens next; if it is not, it says why.

## Supported versions

This project is pre-1.0. **Only the latest release and the latest commit of `main` are supported.** Older versions get no fixes; update to the latest release.

## Scope

In scope: the code, the contract (`spec/POST.md`) and the documentation in **this repository**: the Rust crate (library and `post` command). For example, anything that:

* lets generated text carry a listed term, a link, markup or a path past the checks;
* lets a proposal write outside the allowed paths or through a symlink;
* publishes a post the profile says to hold, publishes while the pause switch is on, or publishes past the daily cap;
* leaks the git token or the model token into a command line, a log, a file or a commit;
* opens a pull request without the draft marker when the post was flagged.

Out of scope (documented limits, see "Known limits" in the README): a hash list is not a secret; the model client may add account details to the
model's own context; the advisory review is a model and can be wrong; and a post that passes every check can still be unsuitable to publish without a person reading it.
Third-party services and software (GitHub, model providers, Rust and their packages) belong with their owners. If you are not sure where something belongs, report it here, privately.
