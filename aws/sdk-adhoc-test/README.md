AWS SDK Adhoc Codegen Test
==========================

This module tests that adhoc SDKs can be generated without the rest of the
release automation machinery used to make the official SDK releases.
The `build.gradle.kts` generates a `smithy-build.json` file as part of
the build, and the Smithy build plugin then invokes the SDK codegen
to generate a client.

This module also exists to code generate and execute service specific protocol tests such as
[ApiGateway](https://github.com/awslabs/smithy/blob/main/smithy-aws-protocol-tests/model/restJson1/services/apigateway.smithy).

Usage
-----

```
# From repo root:
./gradlew :aws:sdk-adhoc-test:test
```
