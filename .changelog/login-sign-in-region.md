---
applies_to: ["aws-sdk-rust"]
authors: ["osrs-agent"]
references: []
breaking: false
new_feature: false
bug_fix: true
---
The Login credentials provider now refreshes cached tokens against the `signInRegion` recorded in the token cache, when present.
