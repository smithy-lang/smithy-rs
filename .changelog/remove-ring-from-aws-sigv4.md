---
applies_to:
- aws-sdk-rust
- client
authors:
- suzak
references:
- smithy-rs#4650
breaking: false
new_feature: false
bug_fix: false
---

Remove the `ring` dependency from `aws-sigv4`. The `sigv4a` feature previously pulled in `ring` solely for HMAC-SHA256 in the SigV4a signing-key derivation; this is now done with the `hmac`/`sha2` (RustCrypto) crates that were already used by the SigV4 signer. `ring` is in maintenance hibernation, and the original reason for using it (a version conflict with the older `p256` crate at the time SigV4a was introduced) no longer applies. Users of the `sigv4a` feature will see `ring` removed from their dependency tree.
