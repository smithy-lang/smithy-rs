---
applies_to:
  - client
authors:
  - landonxjames
references:
  - smithy-rs#4274
breaking: false
new_feature: true
bug_fix: false
---
Add a new `CachingDnsResolver` to `aws_smithy_runtime::client::dns`. This wraps a `hickory_resolver::Resolver` and provides some minimal configuration options (timeouts, retries, etc.) Instructions for overriding the DNS resolver on your HTTP client can be found in our documentation at https://docs.aws.amazon.com/sdk-for-rust/latest/dg/http.html#overrideDns
