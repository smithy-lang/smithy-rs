---
applies_to: ["server"]
authors: ["drganjoo"]
references: ["smithy-rs#4811"]
breaking: false
new_feature: false
bug_fix: false
---

The HTTP 1.x RPC v2 CBOR router validates paths and protocol headers without regex
allocation. It preserves the existing `Service.Operation` registration keys,
prefixed URLs, namespaced services, capitalization aliases, and routing errors.
Generated servers and handwritten registrations retain their existing key format.
