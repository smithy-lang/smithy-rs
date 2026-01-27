---
applies_to:
- client
- aws-sdk-rust
authors:
- vcjana
references:
- smithy-rs#4500
breaking: false
new_feature: false
bug_fix: true
---
Fix JMESPath integer literal handling in waiters to support Smithy 1.66.0, which parses integer literals as `Long` instead of `Double`.
