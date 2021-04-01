# Codegen Integration Test
This module defines an integration test of the code generation machinery for AWS services. `.build.gradle.kts` will generate a `smithy-build.json` file as part of the build. The Smithy build plugin then invokes our codegen machinery and generates Rust crates.

This module exists to code generate and execute service specific protocol tests like [ApiGateway](https://github.com/awslabs/smithy/blob/main/smithy-aws-protocol-tests/model/restJson1/services/apigateway.smithy).

## Usage
```
# From repo root:
./gradlew :aws:sdk-codegen-test:test
```
