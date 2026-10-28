---
applies_to: ["server"]
authors: ["fahadzub"]
references: ["smithy-rs#4894", "smithy-rs#4906"]
breaking: true
new_feature: true
bug_fix: false
---

Upgrade `aws-smithy-http-server` to 0.69.0 with schema-based serialization and
multi-protocol routing for HTTP 1.x servers. Enable schema serde with
`codegen.schemaSerde: true` and regenerate your server SDK. Review these migration
changes when upgrading:

- Protocol types such as `RestJson1` now carry configuration. Replace unit-struct
  construction with `RestJson1::default()`. For request-specific serialization,
  use the protocol selected by routing so its configuration is preserved.
- The HTTP 1.x server runtime uses Tower 0.5. Update application dependencies
  and middleware that use Tower utility types to Tower 0.5.
  `aws-smithy-http-server-metrics` and `aws-smithy-http-server-metrics-macro` 0.4.0
  target server 0.69; upgrade the server and metrics crates together.
- Schema serde calls `IntoResponse<P>::into_response_with_protocol` to frame
  errors using the selected protocol. Its default preserves the existing
  `into_response` conversion and the `FromParts<P>::Rejection: IntoResponse<P>`
  bound. Override the contextual method when an error needs protocol-specific
  framing. Generated-serde callers continue using `into_response`. Use
  `response::ModeledRejection<E>` for modeled extractor errors and the shared
  `runtime_error::RuntimeError` for framework errors.
- AWS JSON routing uses the modeled operation name in both serde modes:
  `getFoo` uses `X-Amz-Target: Service.getFoo`. To also accept the former generated
  spelling `Service.GetFoo`, set `capitalizeRoutes: true` in the
  `aws.protocols#awsJson1_0` or `aws.protocols#awsJson1_1` section of
  `customizationConfig.protocols`.
- Schema-serde RPC v2 CBOR streaming blob inputs use `ByteStream` instead of
  `Blob`. The CBOR document is collected and decoded before the handler is
  invoked; the stream contains the decoded blob bytes.
