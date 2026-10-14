---
applies_to: ["client", "server", "aws-sdk-rust"]
authors: ["FalkWoldmann"]
references: ["smithy-rs#2410"]
breaking: false
new_feature: false
bug_fix: false
---
Removed the `pin-utils` dependency from `aws-smithy-async`, `aws-smithy-http`, `aws-smithy-legacy-http`, `aws-smithy-runtime` and `aws-smithy-types`. The remaining `pin_mut!` uses now use `std::pin::pin!`, and three of the crates didn't use it at all.
