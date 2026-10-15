---
applies_to: ["server"]
authors: ["fahadzub"]
references: []
breaking: false
new_feature: false
bug_fix: true
---

Schema server serializers use the existing integer-list helper for dense lists
with plain integer elements. CBOR writes these elements directly through its
encoder, avoiding per-element dynamic dispatch while preserving the configured
collection encoding. Sparse lists and constrained integer elements retain their
generic serialization paths.
