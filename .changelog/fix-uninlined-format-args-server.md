---
applies_to: ["server"]
authors: ["sachinsharma3191"]
references: ["smithy-rs#4366"]
breaking: false
new_feature: false
bug_fix: true
---
Inline format arguments in generated server code to satisfy the `clippy::uninlined_format_args` lint and remove the `allow(clippy::uninlined_format_args)` workaround from `ServerRequiredCustomizations`. All positional format arguments in constraint violation display implementations, validation error messages, and validation exception decorators are now inlined into their format strings.
