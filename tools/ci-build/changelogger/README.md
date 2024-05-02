# ChangeLogger

The Changelogger tool generates public facing `.md` changelogs from the structured `CHANGELOG.next.toml` formats that developers modify. The changelogger runs during smithy-rs releases to generate the smithy-rs specific changelog as well as during SDK releases to generate the SDK changelog. The smithy-rs changelog generation moves the AWS changelog entries into a separate file so that they can later be consumed.

[smithy-rs-maintainers.txt](./smithy-rs-maintainers.txt) controls the set of users that is **not** acknowledged for their contributions in changelogs.

## Commands
### Split
Splits changelog entries into two `json` files, splitting up `aws-sdk-rust` entries from `smithy-rs` entries. This is a prestep to full changelog generation. The end result is `aws/SDK_CHANGELOG.next.json`.

This command is invoked as part of `smithy-rs-release`.

### Render
Render a `.md` format changelog from a structured changelog.
