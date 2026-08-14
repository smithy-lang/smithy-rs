---
applies_to: ["aws-sdk-rust"]
authors: ["lnj"]
references: []
breaking: false
new_feature: false
bug_fix: true
---
Fix `credential_process` on Windows when the executable path contains spaces. The
command was passed to `cmd.exe /C` as a single ordinary argument, whose default
escaping combined with `cmd.exe`'s quote-stripping rules to mangle a quoted first
token containing spaces (for example an executable installed under
`C:\Program Files\...`, such as AWS AppStream 2.0's `appstream_machine_role`
provider). The command is now appended to the `cmd.exe` command line verbatim
(via `raw_arg`) and wrapped in an extra pair of quotes as `cmd.exe` requires, so a
quoted path containing spaces is preserved. Behavior on non-Windows platforms is
unchanged.
