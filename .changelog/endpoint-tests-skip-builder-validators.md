---
applies_to: ["client", "aws-sdk-rust"]
authors: ["vcjana"]
references: ["smithy-rs#4967"]
breaking: false
new_feature: false
bug_fix: true
---

Generated endpoint rule-set tests no longer run the validators in `ParamsBuilder::build()`, so a test case with an
invalid `Region` reaches the rules instead of panicking with `invalid params`. Requests still run the validators.
