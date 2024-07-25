RFC: File-per-change changelog
==============================

> Status: Implemented
>
> Applies to: client and server

For a summarized list of proposed changes, see the [Changes Checklist] section.

Historically, the smithy-rs and AWS SDK for Rust's changelogs and release notes have been
generated from the `changelogger` tool in `tools/ci-build/changelogger`. This is a tool built
specifically for development and release of smithy-rs, and it requires developers to add
changelog entries to a root `CHANGELOG.next.toml` file. Upon release, the `[[smithy-rs]]` entries
in this file go into the smithy-rs release notes, and the `[[aws-sdk-rust]]` entries are associated
with a smithy-rs release commit hash, and added to the `aws/SDK_CHANGELOG.next.json` for
incorporation into the AWS SDK's changelog when it releases.

This system has gotten us far, but it has always made merging PRs into main more difficult
since the central `CHANGELOG.next.toml` file is almost always a merge conflict for two PRs
with changelog entries.

This RFC proposes a new approach to change logging that will remedy the merge conflict issue,
and explains how this can be done without disrupting the current release process.

The proposed developer experience
---------------------------------

There will be a `changelog/` directory in the smithy-rs root where
developers can add changelog entry Markdown files. Any file name can be picked
for these entries. Suggestions are the development branch name for the
change, or the PR number.

The changelog entry format will change to make it easier to duplicate entries
across both smithy-rs and aws-sdk-rust, a common use-case.

This new format will make use of Markdown front matter in the YAML format.
This change in format has a couple benefits:
- It's easier to write change entries in Markdown than in a TOML string.
- There's no way to escape special characters (such as quotes) in a TOML string,
  so the text that can be a part of the message will be expanded.

While it would be preferable to use TOML for the front matter (and there are libraries
that support that), it will use YAML so that GitHub's Markdown renderer will recognize it.

A changelog entry file will look as follows:

```markdown
---
# Adding `aws-sdk-rust` here duplicates this entry into the SDK changelog.
applies_to: ["client", "server", "aws-sdk-rust"]
authors: ["author1", "author2"]
references: ["smithy-rs#1234", "aws-sdk-rust#1234"]
# The previous `meta` section is broken up into its constituents:
breaking: false
# This replaces "tada":
new_feature: false
bug_fix: false
---

Some message for the change.
```

Implementation
--------------

When a release is performed, the release script will generate the release notes,
update the `CHANGELOG.md` file, copy SDK changelog entries into the SDK,
and delete all the files in `changelog/`.

### SDK Entries

The SDK changelog entries currently end up in `aws/SDK_CHANGELOG.next.json`, and each entry
is given `age` and `since_commit` entries. The age is a number that starts at zero, and gets
incremented with every smithy-rs release. When it reaches a hardcoded threshold, that entry
is removed from `aws/SDK_CHANGELOG.next.json`. The SDK release process uses the `since_commit`
to determine which changelog entries go into the next SDK release's changelog.

The SDK release process doesn't write back to smithy-rs, and history has shown that it
can't since this leads to all sorts of release issues as PRs get merged into smithy-rs
while the release is in progress. Thus, this `age`/`since_commit` dichotomy needs to
stay in place.

The `aws/SDK_CHANGELOG.next.json` will stay in place in its current format without changes.
Its JSON format is capable of escaping characters in the message string, so it will be
compatible with the transition from TOML to Markdown with YAML front matter.

The `SDK_CHANGELOG.next.json` file has had merge conflicts in the past, but this only
happened when the release process wasn't followed correctly. If we're consistent with
our release process, it should never have conflicts.

### Safety requirements

Implementation will be tricky since it needs to be done without disrupting the existing
release process. The biggest area of risk is the SDK sync job that generates individual
commits in the aws-sdk-rust repo for each commit in the smithy-rs release. Fortunately,
the `changelogger` is invoked a single time at the very end of that process, and only
the latest `changelogger` version that is included in the build image. Thus, we can safely
refactor the `changelogger` tool so long as the command-line interface for it remains
backwards compatible. (We _could_ change the CLI interface as well, but it will
require synchronizing the smithy-rs changes with changes to the SDK release scripts.)

At a high level, these requirements must be observed to do this refactor safely:
- The CLI for the `changelogger render` subcommand _MUST_ stay the same, or have minimal
  backwards compatible changes made to it.
- The `SDK_CHANGELOG.next.json` format can change, but _MUST_ remain a single JSON file.
  If it is changed at all, the existing file _MUST_ be transitioned to the new format,
  and a mechanism _MUST_ be in place for making sure it is the correct format after
  merging with other PRs. It's probably better to leave this file alone though, or make
  any changes to it backwards compatible.

Future Improvements
-------------------

After the initial migration, additional niceties could be added such as pulling authors
from git history rather than needing to explicitly state them (at least by default; there
should always be an option to override the author in case a maintainer adds a changelog
entry on behalf of a contributor).

Changes checklist
-----------------

- [x] Refactor changelogger and smithy-rs-tool-common to separate the changelog
      serialization format from the internal representation used for rendering and splitting.
- [x] Implement deserialization for the new Markdown entry format
- [x] Incorporate new format into the `changelogger render` subcommand
- [x] Incorporate new format into the `changelogger split` subcommand
- [x] Port existing `CHANGELOG.next.toml` to individual entries
- [x] Update `sdk-lints` to fail if `CHANGELOG.next.toml` exists at all to avoid losing
      changelog entries during merges.
- [x] Dry-run test against the smithy-rs release process.
- [x] Dry-run test against the SDK release process.

[Changes Checklist]: #changes-checklist
