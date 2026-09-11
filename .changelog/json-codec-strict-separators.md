---
applies_to: ["client", "server"]
authors: ["fahadzub"]
references: []
breaking: false
new_feature: false
bug_fix: true
---
The schema-based JSON deserializer now rejects malformed JSON that it previously accepted: commas were skipped as whitespace, so `{"int": 10,}`, `[1,,2]` and `{,"a": 1}` parsed successfully; bytes after the top-level value such as `{"int": 10}abc` were ignored; and values inside unknown members were skipped without validating number grammar or string escapes. These correspond to the Smithy protocol tests `RestJsonInvalidJsonBody_case1` and `RestJsonInvalidJsonBody_case7`.
