---
applies_to: ["client", "server"]
authors: ["drganjoo"]
references: []
breaking: false
new_feature: false
bug_fix: true
---

The schema-based union deserializer now enforces the same union rules as the
token-based parser. A second key in a union object is an error. An unknown key
becomes the unknown variant on clients and an `unexpected union variant` error
on servers. This fixes the schema path for malformed restJson1 unions containing
multiple known or mixed known and unknown fields.
