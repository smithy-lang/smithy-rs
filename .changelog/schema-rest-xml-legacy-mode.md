---
applies_to: ["server"]
authors: ["drganjoo"]
references: []
breaking: false
new_feature: true
bug_fix: false
---

Schema server JSON and XML parsers now use legacy generated-server behavior by default.
JSON ignores invalid escapes in skipped values. XML checks the modeled root and collection
item names, recovers from malformed documents, and accepts `application/xml` request bodies.
Codec validation and request media-type aliases have independent settings; the bundled
`legacyMode` setting has been removed.
