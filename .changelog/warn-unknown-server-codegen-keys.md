---
applies_to:
- server
authors:
- lauzadis
references: []
breaking: false
new_feature: false
bug_fix: true
---

Soften the server codegen's handling of unknown `codegen` configuration keys: log a warning instead of throwing `IllegalArgumentException`. This makes server codegen forward-compatible with `smithy-build.json` files that carry keys recognized by other tools or by future server codegen versions, matching the lenient behavior already used for client codegen settings.
