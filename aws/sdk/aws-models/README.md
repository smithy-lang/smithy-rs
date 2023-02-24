This directory contains a snapshot of a small subset of AWS service models to test the code generator against.
These were carefully selected to exercise every Smithy AWS protocol:

 - `@awsJson1_0`: dynamodb
 - `@awsJson1_1`: config
 - `@awsQuery`: sts
 - `@ec2Query`: ec2
 - `@restJson1`: polly
 - `@restXml`: s3
 - Allow-listed Event Stream: transcribestreaming

All other services in this directory not listed above have integration tests that need to run in CI.

When generating the full SDK for releases, the models in [awslabs/aws-sdk-rust]'s `aws-models` directory are used.

[awslabs/aws-sdk-rust]: https://github.com/awslabs/aws-sdk-rust
