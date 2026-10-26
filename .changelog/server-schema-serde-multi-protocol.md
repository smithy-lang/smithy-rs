---
applies_to: ["server"]
authors: ["fahadzub"]
references: []
breaking: true
new_feature: true
bug_fix: false
---

Add schema-based serialization and multi-protocol routing to `aws-smithy-http-server`,
enabled with the codegen setting `schemaSerde: true`. Protocol types such as `RestJson1`
are no longer unit structs; construct them with `Default::default()`.

Schema-serde middleware and extractor rejections use `IntoResponseWithProtocol`
to frame errors with the protocol selected for the request. Generated-serde
callers continue using `IntoResponse<P>`. Shared service configuration is exposed through
`aws_smithy_http_server::schema::ServiceConfig`.

AWS JSON routing in both serde modes now uses the modeled operation name.
For an operation named `getFoo`, the target is `Service.getFoo`. To also accept
the former generated routing spelling `Service.GetFoo`, set `capitalizeRoutes`
to `true` under `customizationConfig.protocols.aws.protocols#awsJson1_0` or
`customizationConfig.protocols.aws.protocols#awsJson1_1`. Both targets remain
available when this option is enabled.
