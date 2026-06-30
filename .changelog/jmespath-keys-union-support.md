---
applies_to:
- client
authors:
- mark-creamer-amazon
references: ["smithy-rs#4726"]
breaking: false
new_feature: true
bug_fix: false
---

Add `keys()` JMESPath function support for union shapes in waiter matchers. This enables waiters to match on the active variant of a union using `keys(unionField)` with the `allStringEquals` or `anyStringEquals` comparators.
