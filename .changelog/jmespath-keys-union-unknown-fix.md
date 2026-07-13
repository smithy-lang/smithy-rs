---
applies_to:
- client
authors:
- mark-creamer-amazon
references: ["smithy-rs#4741"]
breaking: false
new_feature: false
bug_fix: true
---

Fix `keys()` JMESPath codegen for union shapes by skipping an explicit match arm for any `Unknown` union member. These collide with the synthetic `Unknown` variant, which is a unit variant and cannot be matched with a tuple destructure pattern.
