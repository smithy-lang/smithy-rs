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

`awsJson1_1`:

- `aws-sdk-cloudtrail`
- `aws-sdk-resourcegroupstagging`

`awsJson1_0`:

- `aws-sdk-sfn`
- `aws-sdk-apprunner`

`restJson1`:

- `aws-sdk-securityhub`
- `aws-sdk-account`
- `aws-sdk-batch`
- `aws-sdk-iotdataplane`
- `aws-sdk-appmesh`
- `aws-sdk-elasticsearch`
- `aws-sdk-greengrass`
- `aws-sdk-bedrockruntime`
- `aws-sdk-dsql`
- `aws-sdk-backup`
- `aws-sdk-codeguruprofiler`
- `aws-sdk-kafka`
- `aws-sdk-apigateway`

`restXml`:

- `aws-sdk-cloudfront`

No change in behavior or public API is expected.
