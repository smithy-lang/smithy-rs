---
applies_to: ["client", "server", "aws-sdk-rust"]
authors: ["franco2002lu"]
references: ["smithy-rs#4960"]
breaking: false
new_feature: false
bug_fix: true
---
Fix `DateTime` epoch-seconds formatting and parsing of timestamps before 1970 that have fractional seconds. Formatting -0.75 seconds produced `-1.25`, and parsing `-0.75` produced 0.75. Timestamps at or after 1970 are unaffected.
