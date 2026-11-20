---
applies_to: ["aws-sdk-rust"]
authors: ["HDauven"]
references: ["aws-sdk-rust#1478"]
breaking: false
new_feature: false
bug_fix: true
---
The STS, SSO, SSO OIDC and Sign-In clients that credential providers create internally now honor `AWS_IGNORE_CONFIGURED_ENDPOINT_URLS` and the `ignore_configured_endpoint_urls` profile key. Previously, an endpoint URL from the environment or config file still reached them when the flag was set.
