---
applies_to: ["client", "server"]
authors: ["drganjoo"]
references: []
breaking: false
new_feature: false
bug_fix: false
---

Re-export `Uri` from `aws_smithy_runtime_api::http`. The type was already public — it is the type
of `RequestParts::uri` and of `Request::uri_mut` — but was not nameable.
