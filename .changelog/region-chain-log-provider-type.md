---
applies_to: ["aws-sdk-rust"]
authors: ["Mattbusel"]
references: ["aws-sdk-rust#1184"]
breaking: false
new_feature: false
bug_fix: true
---
The `region_provider_chain` tracing span in `aws-config` now records the provider's type name instead of its full `Debug` output. With the default region chain, that output was more than a kilobyte and was repeated on every log line emitted while the region was loading.
