# AWS SDK Generator

This directory contains a gradle project to generate an AWS SDK. It uses the Smithy Build Plugin combined with the customizations specified in `aws/sdk-codegen` to generate an AWS SDK from Smithy models.

`build.gradle.kts` will generate a `smithy-build.json` dynamically from all models in the `models` directory.

## Usage

Generate an SDK:
`./gradlew :aws:sdk:assemble`

Generate, compile, and test an SDK:
`./gradlew :aws:sdk:build`

Run an SDK example:
`./gradlew :aws:sdk:runExample --example dynamo-helloworld`

## Controlling service generation
You can use gradle properties to opt/out of generating specific services:
```bash
# generate only s3,ec2,sts
./gradlew -Paws.services=+s3,+ec2,+sts :aws:sdk:assemble

# generate a complete SDK for release
./gradlew -Paws.fullsdk=true :aws:sdk:assemble
```

The generation logic is as follows:
1. If `aws.services` is specified, generate an SDK based on the inclusion/exclusion list.
2. Otherwise, if `aws.fullsdk` is specified generate an SDK based on `aws.services.fullsdk`.
3. Otherwise, generate an SDK based on `aws.services.group1`
