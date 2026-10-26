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

Next phase of the `awsJson1_1` rollout:

- `aws-sdk-fsx`
- `aws-sdk-codebuild`
- `aws-sdk-firehose`
- `aws-sdk-workspaces`
- `aws-sdk-acmpca`
- `aws-sdk-transfer`
- `aws-sdk-cloudhsmv2`
- `aws-sdk-acm`
- `aws-sdk-kinesis`
- `aws-sdk-route53resolver`
- `aws-sdk-ecs`
- `aws-sdk-cognitoidentityprovider`

Canary phase of the `awsJson1_0`, `rpcv2Cbor`, and `restJson1` rollouts:

- `aws-sdk-swf` (`awsJson1_0`)
- `aws-sdk-marketplaceentitlement` (`rpcv2Cbor`)
- `aws-sdk-wisdom` (`restJson1`)

No change in behavior or public API is expected.
