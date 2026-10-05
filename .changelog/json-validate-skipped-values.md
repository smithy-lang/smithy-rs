---
applies_to: ["client", "server"]
authors: ["fahadzub"]
references: []
breaking: false
new_feature: true
bug_fix: false
---

The schema-based JSON codec has a new `validate_skipped_values` setting that
controls how strings inside discarded values are checked: unknown members,
values passed to `skip_value`, and the keys of skipped objects. It is enabled
by default and checks escape syntax without decoding the string. When disabled,
discarded strings are scanned without validating escapes. Container syntax,
number and literal grammar, truncation, and depth limits are checked either way.

`enforce_strictness` now applies only to values that are read. Strings in
discarded values are no longer rejected for raw control characters, invalid
UTF-8, or unpaired surrogate escapes. Keys of skipped objects are checked the
same way as skipped string values; they were previously parsed in full, with or
without strictness.
