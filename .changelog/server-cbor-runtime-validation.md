---
applies_to: ["server"]
authors: ["fahadzub"]
references: []
breaking: false
new_feature: false
bug_fix: true
---

Encode shared runtime validation messages and initial request frame rejection messages
as CBOR maps for rpcv2Cbor, so clients can decode the message. Modeled validation errors,
including custom validation types, retain their own schema and serialized fields.
