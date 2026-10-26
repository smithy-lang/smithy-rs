---
applies_to: ["client", "server"]
authors: ["fahadzub"]
references: []
breaking: false
new_feature: true
bug_fix: false
---

Add schema-based JSON codec settings for server compatibility.
`normalize_arbitrary_precision(true)` rewrites decimal text such as `+007`,
`.5`, and `1.` into valid JSON numbers when serializing big integers and
decimals. It defaults to false and does not affect string-based
arbitrary-precision serialization.

`validate_skipped_values(false)` disables escape validation inside discarded
strings while retaining container syntax, number and literal grammar,
truncation checks, and nesting limits. Escape validation remains enabled by
default. `validate_skipped_string_encoding(true)` independently checks UTF-8
and unescaped control characters in discarded strings.

`dispatch_null_members(true)` passes null structure members to the consumer
instead of skipping them, allowing server builders to distinguish explicit
nulls from omitted members. This setting defaults to false.
