---
applies_to: ["server"]
authors: ["drganjoo"]
references: ["smithy-rs#4811"]
breaking: true
new_feature: false
bug_fix: false
---

The HTTP 1.x RPC v2 CBOR router borrows its lookup key directly from the request URI instead
of allocating a `Service.Operation` string. The path parser preserves prefixed
URLs, namespaced services, and Smithy identifier validation. Header validation
and error classification are unchanged.

Generated and schema-based route registrations now use `Service/operation/Operation`.
Handwritten `RpcV2CborRouter` registrations must use this format too, including
registrations for the legacy HTTP runtime. Regenerate server code when updating
the runtime. Request URLs and capitalization-alias settings are unchanged.
