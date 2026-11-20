---
applies_to: ["aws-sdk-rust"]
authors: ["landonxjames"]
references: []
breaking: false
new_feature: true
bug_fix: false
---

The following AWS SDK clients now use schema-based serialization and
deserialization instead of the legacy per-shape `protocol_serde` code.

`awsJson1_1` is now fully enabled: every client that uses it, including clients
added in the future, uses schema-based serde.

For other protocols the following were enabled:

`awsJson1_0`:

- `aws-sdk-sqs`
- `aws-sdk-dynamodb`

`restJson1`:

- `aws-sdk-apigatewayv2`
- `aws-sdk-lambda`
- `aws-sdk-resourceexplorer2`
- `aws-sdk-eks`

No change in behavior or public API is expected.
