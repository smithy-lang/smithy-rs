---
applies_to: ["client", "aws-sdk-rust"]
authors: ["vcjana"]
references: []
breaking: false
new_feature: true
bug_fix: false
---
Record captured telemetry attributes and transfer sizes on the built-in client metrics. Values
captured via `always_record_attributes([...])` are now merged onto `smithy.client.call.duration` and
`smithy.client.call.attempt.duration` as attributes. The operation-duration metric additionally
carries the outcome as `error.type` (a coarse category, set only on failure) and the raw
`http.response.status_code` when a response was received, plus real transferred-byte counts as
`http.request.body.size`/`http.response.body.size` (counted per frame, so streaming bodies are
measured correctly rather than reported as content-length `0`).

```rust,ignore
// The same opt-in that captures a member now also records it on the built-in metrics.
let config = aws_sdk_s3::Config::builder()
    .always_record_attributes(["Bucket"])
    // ...
    .build();

// `smithy.client.call.duration` is now emitted with, e.g.:
//   rpc.service="S3", rpc.method="GetObject", Bucket="my-bucket",
//   error.type="connector" (on failure), http.response.status_code=200,
//   http.request.body.size=0, http.response.body.size=1024
```
