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
