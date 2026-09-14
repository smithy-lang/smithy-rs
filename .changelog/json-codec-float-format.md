---
applies_to: ["client", "server"]
authors: ["drganjoo"]
references: []
breaking: false
new_feature: false
bug_fix: true
---
Fix schema-based JSON serialization to preserve decimal points for integral float and double values, including negative zero, without widening float values.
