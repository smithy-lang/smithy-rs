---
applies_to: ["client", "server"]
authors: ["drganjoo"]
references: []
breaking: false
new_feature: true
bug_fix: true
---

Fix signed-exponent parsing in schema-based JSON documents. Add independent
validation controls and opt-in number allowances, null-member dispatch, and
arbitrary-precision normalization for legacy server compatibility. Align float
serialization with legacy behavior.
