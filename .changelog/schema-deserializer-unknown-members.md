---
applies_to: ["client", "server"]
authors: ["drganjoo"]
references: []
breaking: false
new_feature: true
bug_fix: false
---

`ShapeDeserializer::read_struct` in the JSON and CBOR codecs now reports a key
that names no member of the structure or union being read, rather than skipping
it silently. Generated structure code ignores the call; union code uses it to
decide how an unknown variant is handled. The `__type` discriminator and
`null`-valued keys are not reported.
