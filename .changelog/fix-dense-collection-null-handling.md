---
applies_to: ["client", "aws-sdk-rust"]
authors: ["vcjana"]
references: []
breaking: false
new_feature: false
bug_fix: true
---
Fix null value handling in dense collections: SDK now correctly rejects null values in non-sparse collections instead of silently dropping them.
