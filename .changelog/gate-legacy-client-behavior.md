---
applies_to:
- client
- aws-sdk-rust
authors:
- jasgin
references: []
breaking: true
new_feature: false
bug_fix: false
---
Legacy client behavior is now gated behind feature flags. Behavior versions prior to `v2026_01_12`
are only available when the `aws-smithy-runtime-api/legacy-client` feature is enabled (transitively
enabled via `aws-smithy-runtime/tls-rustls`), and the legacy HTTP test utilities exposed from
`aws-smithy-runtime::client::http::test_util` now require the `legacy-test-util` feature. Using a
pre-`v2026_01_12` `BehaviorVersion` without the legacy TLS stack (`tls-rustls`) would result in no
HTTP client being configured at runtime, so gating access to those older versions prevents users
from silently ending up with no HTTP client. In addition, `test-util` no longer implicitly enables
`legacy-test-util`; enable `legacy-test-util` explicitly if you rely on the legacy hyper 0.14 HTTP
test utilities.
