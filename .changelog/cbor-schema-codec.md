---
applies_to: ["client", "server"]
authors: ["drganjoo"]
references: []
breaking: false
new_feature: true
bug_fix: true
---

Add an opt-in structure-prefix callback to schema-based CBOR serialization.
The callback receives the value's schema and can write fields before modeled
members, enabling RPC v2 CBOR error discriminators while retaining member names.
Bound list and map preallocation by the remaining input bytes to avoid excessive
allocation from truncated containers with large declared lengths.
