---
applies_to: ["aws-sdk-rust"]
authors: ["vcjana"]
references: ["aws-sdk-rust#1471"]
breaking: false
new_feature: false
bug_fix: true
---
The `credentials-login` feature of `aws-config` now uses `p256` 0.14, addressing RUSTSEC-2026-0387.
It previously depended on `p256` 0.13, whose `crypto-bigint` 0.5 dependency has no patched release,
so the advisory could not be cleared with a lockfile update. The DPoP proofs sent when refreshing a
login session are unchanged.
