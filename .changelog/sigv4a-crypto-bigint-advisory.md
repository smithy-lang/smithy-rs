---
applies_to: ["aws-sdk-rust"]
authors: ["jratliff79"]
references: ["smithy-rs#4958"]
breaking: false
new_feature: false
bug_fix: true
---
The SigV4a signer now requires `crypto-bigint` 0.7.10 or later, addressing
RUSTSEC-2026-0387. SigV4a and S3 Multi-Region Access Point support remain
available, with unchanged key derivation, request authentication and signature
encoding.
