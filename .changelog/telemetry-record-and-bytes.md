---
applies_to: ["client", "aws-sdk-rust"]
authors: ["vcjana"]
references: []
breaking: false
new_feature: true
bug_fix: false
---
Emit captured telemetry attributes and transfer sizes on the built-in client metrics. Values
selected via `emit_input_attributes([...])` are now attached as attributes on
`smithy.client.call.duration` and `smithy.client.call.attempt.duration`. The operation-duration metric additionally
carries the outcome as `error.type` (a coarse category, set only on failure) and the raw
`http.status_code` when a response was received. Real transferred-byte counts are recorded on their
own histograms, `smithy.client.call.request.size` / `smithy.client.call.response.size`, counted per
frame and emitted when the body completes — so a streaming body reports its true size rather than the
`0` that content-length reports for it.

```rust,ignore
// The same opt-in that captures a member now also emits it on the built-in metrics.
let config = aws_sdk_s3::Config::builder()
    .emit_input_attributes(["Bucket"])
    // ...
    .build();

// `smithy.client.call.duration` is now emitted with, e.g.:
//   rpc.service="S3", rpc.method="GetObject", Bucket="my-bucket",
//   error.type="connector" (on failure), http.status_code=200
//
// and body sizes on their own instruments, e.g.:
//   smithy.client.call.response.size { rpc.service="S3", rpc.method="GetObject" } = 1024
```
