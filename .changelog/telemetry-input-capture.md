---
applies_to: ["client", "aws-sdk-rust"]
authors: ["vcjana"]
references: []
breaking: false
new_feature: true
bug_fix: false
---
Add opt-in capture of selected operation-input members for telemetry. Naming input members via
`Config::builder().emit_input_attributes([...])` makes the SDK capture those members' values into
the `ConfigBag` (as `CapturedTelemetryAttributes`) during `read_before_execution`, before the input
is consumed by serialization. Any interceptor can then read a value via
`cfg.load::<CapturedTelemetryAttributes>()`, without threading it through a `task_local!`. Only
string-valued, non-`@sensitive` members are eligible; capture is off by default.

```rust,ignore
use aws_smithy_types::telemetry::CapturedTelemetryAttributes;

// Opt in to capturing the S3 `Bucket` input member.
let config = aws_sdk_s3::Config::builder()
    .emit_input_attributes(["Bucket"])
    // ...
    .build();

// In any interceptor, read the captured value off the config bag — no `task_local!`.
if let Some(bucket) = cfg
    .load::<CapturedTelemetryAttributes>()
    .and_then(|a| a.get("Bucket"))
{
    // use `bucket`
}
```
