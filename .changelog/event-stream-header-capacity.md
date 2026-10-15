---
applies_to: ["server"]
authors: ["fahadzub"]
references: []
breaking: false
new_feature: false
bug_fix: true
---

Schema event-stream framing reserves four header slots up front, matching the
legacy generated marshaller's initial capacity and avoiding a vector growth
for common event frames.
