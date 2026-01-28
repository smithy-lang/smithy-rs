---
applies_to:
  - aws-sdk-rust
  - client
  - server
authors:
  - landonxjames
  - drganjoo
references:
  - smithy-rs#4484
breaking: false
new_feature: true
bug_fix: false
---

All Smithy-rs crates, for both servers and clients, now use the 1.x version of
the `http` crate for all internal processing. Utility methods are still provided
for users to convert between SDK types and both of the `http` 0.x and 1.x types.
