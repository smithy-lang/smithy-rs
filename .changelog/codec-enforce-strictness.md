---
applies_to: ["client", "server"]
authors: ["drganjoo"]
references: []
breaking: false
new_feature: true
bug_fix: false
---

The schema-based JSON, XML, and CBOR codecs gained an opt-in `enforce_strictness` setting for request validation. JSON validates string contents and number syntax and rejects whitespace-only structure bodies and out-of-range floating-point epoch timestamps. XML validates the document root against the schema. CBOR rejects trailing bytes after top-level containers and truncated nested structures, and skips a `null` structure-member value as if the member were absent (union variants still see nulls and reject them). Defaults remain unchanged. JSON also gained `allow_integral_float_numbers` to accept exactly integral decimal and exponent numbers for integer members without losing precision through floating-point conversion.
