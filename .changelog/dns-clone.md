---
applies_to: ["client"]
authors: ["landonxjames"]
references: ["smithy-rs#4274"]
breaking: false
new_feature: false
bug_fix: true
---
The `HickoryDnsResolver` and `TokioDnsResolver` were not `Clone` making it impossible to use them in the http_client builder's `build_with_resolver` method.
