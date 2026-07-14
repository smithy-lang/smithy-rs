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

Fix `keys()` JMESPath codegen for union shapes by using the symbol provider's resolved variant names instead of raw member names. This correctly handles modeled Unknown members (which get renamed to avoid colliding with the synthetic Unknown unit variant) and preserves the original wire name in the match output.
