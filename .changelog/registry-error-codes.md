---
applies_to: ["client", "aws-sdk-rust"]
authors: ["landonxjames"]
references: ["smithy-rs#4853"]
breaking: false
new_feature: false
bug_fix: true
---

The error registries of clients generated with schema serde now match error
codes the way the operation's own error dispatch does, including
`@awsQueryError` codes in awsQueryCompatible services. Each entry whose code
differs from its shape name sets it with the new
`RegistryEntry::with_error_code`, and `TypeRegistry::entry_for_error_code`
matches these codes before shape names. When several entries match the same way,
it picks the smallest shape ID instead of depending on map order. An operation
that models no errors now also uses the service-wide error registry: when a
response's error code matches an error modeled elsewhere in the service, the
unhandled error's `source()` is that error.
