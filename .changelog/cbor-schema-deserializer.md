---
applies_to: ["client", "server"]
authors: ["drganjoo"]
references: []
breaking: false
new_feature: true
bug_fix: true
---

Bound schema-based CBOR container capacity hints and list/map preallocation by
remaining input bytes, retaining the allocation cap for truncated containers.
Expose the current byte offset from schema-based CBOR deserializers without
advancing decoding.
