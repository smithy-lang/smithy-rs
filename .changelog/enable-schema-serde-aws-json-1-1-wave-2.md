---
applies_to: ["aws-sdk-rust"]
authors: ["landonxjames"]
references: []
breaking: false
new_feature: true
bug_fix: false
---

The following AWS SDK clients now use schema-based serialization and
deserialization instead of the legacy per-shape `protocol_serde` code as the
next phase of the `awsJson1_1` rollout:

- `aws-sdk-servicediscovery`
- `aws-sdk-globalaccelerator`
- `aws-sdk-appstream`
- `aws-sdk-marketplaceentitlement`
- `aws-sdk-route53domains`
- `aws-sdk-comprehend`
- `aws-sdk-iotsecuretunneling`
- `aws-sdk-licensemanager`
- `aws-sdk-config`
- `aws-sdk-pi`
- `aws-sdk-codedeploy`
- `aws-sdk-codecommit`
- `aws-sdk-marketplacemetering`
- `aws-sdk-translate`
- `aws-sdk-transcribe`
- `aws-sdk-organizations`
- `aws-sdk-cognitoidentity`
- `aws-sdk-datasync`
- `aws-sdk-sagemaker`
- `aws-sdk-athena`
- `aws-sdk-storagegateway`
- `aws-sdk-codepipeline`
- `aws-sdk-ecrpublic`
- `aws-sdk-lightsail`

No change in behavior or public API is expected.
