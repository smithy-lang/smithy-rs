---
applies_to: ["server"]
authors: ["franco2002lu"]
references: ["smithy-rs#2472"]
breaking: false
new_feature: false
bug_fix: true
---
Exclude `@internal` enum values from the enum value set in server constraint violation messages. Previously, rejecting an invalid enum value listed every value, including internal ones that should not be exposed to callers. Internal values are still accepted as valid input.
