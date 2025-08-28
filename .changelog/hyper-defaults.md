---
applies_to:
  - client
authors:
  - landonxjames
references:
  - smithy-rs#4282
breaking: false
new_feature: false
bug_fix: true
---
Set the `pool_timer` for the default Hyper client. This is required to allow the `pool_idle_timeout` to work. Now idle connections will be released by the pool after 90 seconds.
