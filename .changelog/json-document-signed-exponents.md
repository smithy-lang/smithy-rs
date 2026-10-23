---
applies_to: ["client", "server", "aws-sdk-rust"]
authors: ["fahadzub"]
references: []
breaking: false
new_feature: false
bug_fix: true
---

The schema-based JSON codec now reads document numbers with signed exponents,
such as `1e-5` and `-2E-1`, correctly. Number boundaries are determined by the
existing number lexer, preserving strict JSON validation and negative zero.
