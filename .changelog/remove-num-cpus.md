---
applies_to: ["server"]
authors: ["trivikr"]
references: ["smithy-rs#4980"]
breaking: false
new_feature: false
bug_fix: false
---
Remove the `num_cpus` dependency from `aws-smithy-http-server-python`. When `workers` isn't set, the default worker count now comes from `std::thread::available_parallelism`.
