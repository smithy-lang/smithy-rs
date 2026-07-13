---
applies_to:
- server
authors:
- lauzadis
references: []
breaking: false
new_feature: true
bug_fix: false
---

Add a `codegen.allowMissingUnionVariant` configuration (boolean, default `false`). When `true`, a union JSON body whose object did not set any recognized variant (e.g. `{}` or `{"unknownKey": ...}`) parses to `Ok(None)` rather than returning a deserialization error. Opt in only for services that have shipped clients depending on the lenient behavior. Client codegen is unaffected.
