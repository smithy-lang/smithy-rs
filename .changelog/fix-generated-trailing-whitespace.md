---
applies_to:
- server
authors:
- jlizen
references:
- smithy-rs#4634
breaking: false
new_feature: false
bug_fix: true
---

Strip trailing whitespace from generated Rust code. Smithy's `AbstractCodeWriter` adds indentation to blank lines, producing whitespace-only lines that cause `cargo fmt` to fail with `error[internal]: left behind trailing whitespace`.
