---
applies_to: ["server"]
authors: ["sachinsharma3191"]
references: ["smithy-rs#4366"]
breaking: false
new_feature: false
bug_fix: true
---
Inline format arguments in generated server code to satisfy the `clippy::uninlined_format_args` lint. All `format!("{}", var)` / `write!(f, "{}", var)` patterns in constraint violation display implementations are now `format!("{var}")` / `write!(f, "{var}")`.
