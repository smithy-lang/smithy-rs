---
applies_to: ["server"]
authors: ["fahadzub"]
references: []
breaking: false
new_feature: false
bug_fix: true
---

Servers using `schemaSerde` now honor `alwaysSendEventStreamInitialResponse` for
restJson1 and restXml, matching legacy servers. When enabled, the initial frame
arrives before application events, with `{}` for restJson1 and an empty payload
for restXml. Non-stream output members retain their HTTP bindings.
