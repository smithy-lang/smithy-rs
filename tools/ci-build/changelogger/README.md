# ChangeLogger

The ChangeLogger tool generates public facing `.md` changelogs from the individual changelog entry Markdown files stored in the `.changelog` directory. The changelogger runs during smithy-rs releases to generate the smithy-rs specific changelog as well as during SDK releases to generate the SDK changelog. The smithy-rs changelog generation duplicates the AWS changelog entries into a separate file so that they can later be consumed.

[smithy-rs-maintainers.txt](./smithy-rs-maintainers.txt) controls the set of users that is **not** acknowledged for their contributions in changelogs.

## Commands
### Ls
Display a preview of pending changelog entries (either for `smithy-rs` or for `aws-sdk-rust`) since the last release to standard output.

This command is intended for developer use.

### New
Create a new changelog entry Markdown file in the `.changelog` directory.

This command is intended for developer use.

### Render
Render a `.md` format changelog from changelog entry Markdown files stored in the `.changelog` directory.

This command is intended for automation and invoked as part of `smithy-rs-release`.

### Split
Duplicate `aws-sdk-rust` entries from those in the `.changelog` directory into `aws/SDK_CHANGELOG.next.json`, which is a prestep to full changelog generation.

This command is intended for automation and invoked as part of `smithy-rs-release`.

## Installation
To install, run the following from this README path:
```
$ cargo install --locked --path .
```
Confirm the installation:
```
$ changelogger --version
```

## How to Use
### Ls
To preview changelog entries for an upcoming release in `smithy-rs`:
```
$ changelogger ls --change-set smithy-rs
```
To preview changelog entries for an upcoming release in `aws-sdk-rust`:
```
$ changelogger ls --change-set aws-sdk
```

### New
An example usage:
```
$ changelogger new \
  --applies-to client \
  --applies-to aws-sdk-rust \
  --references smithy-rs#1234 \
  --authors someone \
  --bug-fix \
  --message "Some changelog for \`foo\`"

The following changelog entry has been written to "<smithy-rs root>/.changelog/8814816.md":
---
applies_to:
- aws-sdk-rust
- client
authors:
- someone
references:
- smithy-rs#1234
breaking: false
new_feature: false
bug_fix: true
---
Some changelog for `foo`
```

The following CLI arguments are "logically" required
- `--applies-to`
- `--authors`
- `--references`
- `--message`

If any of the above is not passed a value at command line, then the user's editor is opened for further edit (which editor to open can be configured per the [edit crate](https://docs.rs/edit/0.1.5/edit/)).
