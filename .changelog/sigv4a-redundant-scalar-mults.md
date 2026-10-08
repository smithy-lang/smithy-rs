---
applies_to: ["client", "aws-sdk-rust"]
authors: ["yychen23"]
references: ["smithy-rs#4892"]
breaking: false
new_feature: false
bug_fix: false
---
SigV4a signing is about 2.6x faster. It performed three P-256 scalar multiplications per request where the mathematics needs one: `generate_signing_key` built an ECDSA `SigningKey` only to serialize it straight back to bytes, and `calculate_signature` rebuilt one from those bytes. Constructing a `SigningKey` derives the verifying key, which costs a scalar multiplication as expensive as the signature itself and which signing never reads.

The derived key is now returned as the raw private scalar and signed with directly. Signatures are unchanged: the new path is `ecdsa`'s own `SigningKey` signing routine with that derivation left out, so the RFC 6979 nonce and the output bytes are identical, which a test asserts against the previous path. No public signature changes, since `generate_signing_key` already returned an opaque `impl AsRef<[u8]>`.

Measured with the crate's `sigv4a` benchmark on a Xeon Platinum 8375C:

| | before | after |
|---|---|---|
| `generate_signing_key` | 145 µs | 0.47 µs |
| `calculate_signature` | 316 µs | 171 µs |
| full `http_request::sign` | 465 µs | 178 µs |

What remains is the signature's own scalar multiplication.
