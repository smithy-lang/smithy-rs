---
applies_to: ["client", "server"]
authors: ["drganjoo"]
references: []
breaking: false
new_feature: true
bug_fix: true
---

Fix schema-based JSON document decoding of numbers with signed exponents. Add
independent controls for escape validation and UTF-8/control-character validation
in discarded strings, plus opt-in allowances for leading zeros and trailing decimal
points in read and skipped numbers. Container syntax, truncation, and depth checks
remain enforced when discarded-string validation is disabled. Use the shared
number encoder for floating-point serialization while preserving negative zero
and quoted non-finite values.
