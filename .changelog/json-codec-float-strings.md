---
applies_to: ["client", "server"]
authors: ["fahadzub"]
references: []
breaking: false
new_feature: false
bug_fix: true
---
The schema-based JSON deserializer now rejects quoted finite numbers for float and double members, such as `{"doubleInBody": "123"}`. A string is accepted only when it represents a non-finite value (`"NaN"`, `"Infinity"`, `"-Infinity"`), matching the token-based parser and the Smithy protocol tests `RestJsonBodyFloatMalformedValueRejected_case0` and `RestJsonBodyDoubleMalformedValueRejected_case0`.
