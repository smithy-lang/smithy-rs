---
applies_to: ["client", "aws-sdk-rust"]
authors: ["xiazhvera"]
references: ["smithy-rs#TODO"]
breaking: false
new_feature: false
bug_fix: true
---
Fix `RequestChecksumCalculation::WhenRequired` ignoring a caller-set checksum algorithm. With `RequestChecksumCalculation::WhenRequired` and trait `requestChecksumRequired: false`, the SDK now calculates a request checksum when the caller sets the modeled request algorithm member. If the member is not set, no checksum is calculated as before.
