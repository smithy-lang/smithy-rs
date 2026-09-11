---
applies_to: ["client", "server"]
authors: ["drganjoo"]
references: []
breaking: false
new_feature: false
bug_fix: true
---
The schema-based union deserializer now enforces the union rules the token-based parser enforces. A second key in the union object, whatever it names, is an error ("encountered mixed variants in union"); previously the last member won. A key that names no member becomes the unknown variant on a client and an "unexpected union variant" error on a server; previously it was skipped. On a server, the generated `SerializableStruct` impl no longer refers to an unknown variant the union does not have. This makes the restJson1 protocol tests `RestJsonMalformedUnionMultipleFieldsSet` and `RestJsonMalformedUnionKnownAndUnknownFieldsSet` pass on the schema-based path.
