---
applies_to: ["client", "server"]
authors: ["drganjoo"]
references: []
breaking: false
new_feature: true
bug_fix: false
---

Add an opt-in structure-prefix callback to schema-based CBOR serialization.
The callback receives the value's schema and can write fields before modeled
members, enabling RPC v2 CBOR error discriminators while retaining member names.
