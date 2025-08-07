---
applies_to:
  - aws-sdk-rust
  - client
authors:
  - joshtriplett
breaking: false
new_feature: true
bug_fix: false
---
Enable `hints.mostly-unused = true` for `aws-sdk-lambda` (taking a release
build from 57s to 40s) and `aws-sdk-rds` (taking a release build from 1m34s to
49s).
