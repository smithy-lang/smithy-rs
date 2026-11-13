---
applies_to: ["client"]
authors: ["yychen23"]
references: ["smithy-rs#5004"]
breaking: false
new_feature: true
bug_fix: false
---
Added `BehaviorVersion::v2026_11_17`, which is now what `BehaviorVersion::latest()` returns. `BehaviorVersion::v2026_11_10` is deprecated in favor of it.

For generic Smithy clients this changes no behavior: the new version exists to gate a default read timeout that applies only to AWS SDK clients. If you call `v2026_11_10()` explicitly you will see a deprecation warning, which you can resolve by moving to `latest()` or by naming the new version directly.
