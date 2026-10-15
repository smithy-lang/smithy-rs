---
applies_to: ["server"]
authors: ["fahadzub"]
references: []
breaking: false
new_feature: true
bug_fix: true
---

`CborCodecSettings::use_definite_lengths(true)` encodes collections with known
element or entry counts using definite CBOR lengths. Generated schema serializers
provide these counts, including null entries in sparse collections. The setting
defaults to false; callbacks without a count retain indefinite encoding.

Schema RPC v2 CBOR servers enable the setting to match legacy generated collection
encoding, including structured event-stream payloads.
