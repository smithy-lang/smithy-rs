---
applies_to:
- client
- aws-sdk-rust
authors:
- vcjana
references:
- aws-sdk-rust#146
breaking: false
new_feature: true
bug_fix: false
---

Add support for third-party libraries to self-identify in the SDK user agent via framework metadata, addressing the long-standing request to customize the user agent ([aws-sdk-rust#146](https://github.com/awslabs/aws-sdk-rust/issues/146)).

A new public `FrameworkMetadata` type (re-exported as `aws_config::FrameworkMetadata` and on each client's `config` module) can be set on the client config builder, on `SdkConfig`, and via `aws_config::ConfigLoader::framework_metadata`:

```rust
let config = aws_config::from_env()
    .framework_metadata(FrameworkMetadata::new("some-framework", Some("1.0"))?)
    .load()
    .await;
```

Framework metadata is additive — multiple libraries (and the application) can each self-identify without clobbering one another. The name/version are validated against the same charset as `AppName` (rejecting, not sanitizing, invalid characters to prevent header injection). The `UserAgentInterceptor` de-duplicates entries on `(name, version)` preserving first-seen order, caps the total at 10 unique entries, and renders each as `lib/{name}/{version}` in the `x-amz-user-agent` header.
