---
applies_to:
- aws-sdk-rust
- client
authors:
- lnj
references:
- smithy-rs#4614
breaking: false
new_feature: false
bug_fix: true
---

Fix `ProfileFileCredentialsProvider` so that profile-level `use_fips_endpoint` and `use_dualstack_endpoint` settings are propagated to the internal STS client used during assume-role credential chaining. Previously these settings were only applied when the provider was built through `aws_config::ConfigLoader::load`, so users constructing `ProfileFileCredentialsProvider` directly via its builder would see STS requests go to non-FIPS / non-dual-stack endpoints even when the selected profile enabled them.
