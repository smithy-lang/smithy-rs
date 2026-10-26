---
applies_to: ["server"]
authors: ["fahadzub"]
references: []
breaking: true
new_feature: true
bug_fix: false
---

Add schema-based serialization and multi-protocol routing to `aws-smithy-http-server`
0.69, enabled with the codegen setting `schemaSerde: true`. Protocol implementations
under `schema::protocol`, such as `RestJson1`, carry configuration; construct them
with `Default::default()`.

The server runtime uses Tower 0.5. Update application middleware to Tower 0.5
when moving to server 0.69.

Schema-serde middleware and extractor rejections use `IntoResponseWithProtocol`
to frame errors with the protocol selected for the request. Generated-serde
callers continue using `IntoResponse<P>`. Shared service and request-body
configuration lives under `aws_smithy_http_server::config`.

Both serde modes honor `customizationConfig.protocols.common.request.body.maxBytes`
for buffered request bodies. The deprecated `codegen.requestBodyMaxBytes` setting
remains a fallback and emits a generation warning; the nested setting takes
precedence, including an explicit zero that disables the limit.

AWS JSON routing in both serde modes now uses the modeled operation name.
For an operation named `getFoo`, the target is `Service.getFoo`. To also accept
the former generated routing spelling `Service.GetFoo`, set `capitalizeRoutes`
to `true` under `customizationConfig.protocols.aws.protocols#awsJson1_0` or
`customizationConfig.protocols.aws.protocols#awsJson1_1`. Both targets remain
available when this option is enabled.
