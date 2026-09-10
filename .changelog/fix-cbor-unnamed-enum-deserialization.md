---
applies_to: ["client"]
authors: ["drganjoo"]
references: []
breaking: false
new_feature: false
bug_fix: true
---
Fix CBOR client deserialization for unnamed enums by removing an ambiguous
`AsRef` conversion from the generated Rust code.
