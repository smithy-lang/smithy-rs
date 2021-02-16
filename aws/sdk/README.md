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
