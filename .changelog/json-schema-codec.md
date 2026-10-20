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

Add opt-in normalization of arbitrary-precision decimal text to valid JSON numbers
while retaining rejection by default. Preserve explicit null document members and
expose the schema JSON deserializer byte position.

Add opt-in dispatch of null structure members to deserializer consumers. Schema
servers use this to preserve legacy duplicate-member semantics: a later null can
clear an earlier optional value. Existing codec callers continue to skip non-
document null members by default.
