---
applies_to: ["server"]
authors: ["drganjoo"]
references: []
breaking: false
new_feature: true
bug_fix: true
---

JSON schema codecs support independent `allow_leading_zeros` and
`allow_trailing_decimal_point` settings. Both default to disabled and can be
enabled while retaining `enforce_strictness(true)`. AWS JSON and restJson1 schema
servers enable both to preserve legacy number parsing, including integral
values such as `0147483648` and `214748364.`. Other strict grammar and integer
range checks remain enabled.
