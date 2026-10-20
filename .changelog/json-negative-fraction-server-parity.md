---
applies_to: ["server"]
authors: ["fahadzub"]
references: []
breaking: false
new_feature: false
bug_fix: true
---

Restore legacy server acceptance of negative JSON fractions without integer
digits, such as `-.123` and `-.123e2`, for REST JSON and AWS JSON 1.0/1.1.
The allowance applies to known and skipped members. The JSON codec exposes
an additive `allow_empty_integer_part` setting, disabled by default; existing
codec defaults are unchanged. Positive `.123` and incomplete `-.` remain
rejected by the server.
