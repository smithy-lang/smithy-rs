---
applies_to: ["client", "server"]
authors: ["fahadzub"]
references: []
breaking: false
new_feature: true
bug_fix: false
---
Add `SerdeError::invalid_input` and `SerdeError::unsupported` convenience constructors to `aws-smithy-schema`, mirroring the existing `SerdeError::custom`. Codec and binding implementations construct these two variants frequently; the constructors replace repeated struct-literal boilerplate.
