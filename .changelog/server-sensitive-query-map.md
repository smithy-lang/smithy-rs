---
applies_to: ["server"]
authors: ["fahadzub"]
references: []
breaking: false
new_feature: false
bug_fix: true
---

Generated servers with sensitive query-map values compile with warnings denied
when every query value is sensitive. The generated closure no longer declares
an unused parameter.
