---
applies_to: ["client", "server"]
authors: ["ethoman"]
references: ["smithy-rs#4473"]
breaking: false
new_feature: true
bug_fix: false
---
Add CBOR encoding and decoding support for `BigInteger` using CBOR tags 2 (positive bignum) and 3 (negative bignum) as specified by RFC 8949 §3.4.3 and the Smithy RPC v2 CBOR protocol. Values that fit in CBOR major types 0 or 1 use preferred serialization (plain integers) instead of bignum tags.
