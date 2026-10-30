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

- `aws-sdk-ecr`
- `aws-sdk-kms`
- `aws-sdk-cloudwatchlogs`
- `aws-sdk-secretsmanager`

`awsJson1_0`:

- `aws-sdk-opensearchserverless`
- `aws-sdk-proton`
- `aws-sdk-cloudcontrol`
- `aws-sdk-paymentcryptography`

`rpcv2Cbor`:

- `aws-sdk-cloudwatch`

`restJson1`:

- `aws-sdk-cleanrooms`
- `aws-sdk-glacier`
- `aws-sdk-mediapackage`
- `aws-sdk-networkmanager`
- `aws-sdk-neptunegraph`
- `aws-sdk-securitylake`
- `aws-sdk-marketplacecatalog`
- `aws-sdk-iot`
- `aws-sdk-outposts`
- `aws-sdk-managedblockchainquery`
- `aws-sdk-mediaconnect`
- `aws-sdk-vpclattice`
- `aws-sdk-grafana`
- `aws-sdk-lexruntimev2`
- `aws-sdk-ivs`
- `aws-sdk-amp`
- `aws-sdk-schemas`
- `aws-sdk-connectcases`
- `aws-sdk-medialive`
- `aws-sdk-ram`
- `aws-sdk-accessanalyzer`
- `aws-sdk-signer`
- `aws-sdk-lexmodelsv2`
- `aws-sdk-imagebuilder`
- `aws-sdk-emrcontainers`
- `aws-sdk-mediaconvert`
- `aws-sdk-quicksight`
- `aws-sdk-pipes`
- `aws-sdk-drs`
- `aws-sdk-guardduty`
- `aws-sdk-inspector2`
- `aws-sdk-pinpoint`
- `aws-sdk-appsync`
- `aws-sdk-groundstation`
- `aws-sdk-resourcegroups`
- `aws-sdk-bedrockagent`
- `aws-sdk-transcribestreaming`
- `aws-sdk-bedrockagentruntime`
- `aws-sdk-ssooidc`
- `aws-sdk-connect`
- `aws-sdk-mq`
- `aws-sdk-amplify`
- `aws-sdk-fis`
- `aws-sdk-bedrock`
- `aws-sdk-appconfig`

No change in behavior or public API is expected.
