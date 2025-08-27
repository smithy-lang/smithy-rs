---
applies_to:
  - client
authors:
  - landonxjames
references:
  - smithy-rs#4274
breaking: false
new_feature: false
bug_fix: true
---
Set the `pool_idle_timeout` for the default Hyper client to 90 seconds. This aligns with the behavior of the hyper 0.14.x client that was previously the default.
https://github.com/smithy-lang/smithy-rs/issues/4282
