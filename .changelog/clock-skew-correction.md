---
applies_to: ["aws-sdk-rust"]
authors: ["ysaito1001"]
references: ["smithy-rs#4800", "aws-sdk-rust#1033"]
breaking: false
new_feature: true
bug_fix: false
---
The SDK now corrects for clock skew automatically. Requests are signed with a timestamp, and
if the clock on the machine running the SDK drifts from the service's clock, the service can
reject the signature (commonly seen as `SignatureDoesNotMatch`, `InvalidSignatureException`,
or `RequestTimeTooSkewed`). The SDK measures the offset between the two clocks from the
response `Date` header, and when that offset exceeds 4 minutes it treats the failure as clock
skew: it adjusts the timestamp it signs with, retries the request, and remembers the offset on
the client so later requests are signed correctly the first time. This is enabled by default
and needs no code changes. Presigned requests are unaffected: a presigned URL is always signed
at the `start_time` you give it, so it stays a function of its inputs.

If you need to turn it off, the correction can be disabled three ways (highest precedence first):

- In code, on the config loader (or a service client's config builder):
  ```rust
  let sdk_config = aws_config::defaults(aws_config::BehaviorVersion::latest())
      .disable_clock_skew_correction(true)
      .load()
      .await;
  ```
- Environment variable:
  ```
  AWS_DISABLE_CLOCK_SKEW_CORRECTION=true
  ```
- Shared config profile:
  ```ini
  [default]
  disable_clock_skew_correction = true
  ```
