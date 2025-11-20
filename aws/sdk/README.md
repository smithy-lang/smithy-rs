AWS SDK Generator
=================

This directory contains a gradle project to generate an AWS SDK. It uses the Smithy Build Plugin combined with the customizations specified in `aws/codegen-aws-sdk` to generate an AWS SDK from Smithy models.

`build.gradle.kts` will generate a `smithy-build.json` dynamically from all models in the `models` directory.

Usage
-----

Generate an SDK:
`./gradlew :aws:sdk:assemble`

Generate, compile, and test an SDK:
`./gradlew :aws:sdk:check`

Run an SDK example:
`./gradlew :aws:sdk:runExample --example dynamo-helloworld`

Controlling service generation
------------------------------

You can use gradle properties to opt/out of generating specific services:
```bash
# Generate only S3,EC2
# sts, sso, ssooidc, and signin must be included, as they are internally used by S3 and EC2
./gradlew -Paws.services=+s3,+ec2,+sts,+sso,+ssooidc,+signin :aws:sdk:assemble

# Generate all AWS services using models from the aws-sdk-rust repo
./gradlew \
  -Paws.sdk.models.path=<absolute path to the aws-sdk-rust repo>/aws-models \
  :aws:sdk:assemble

# Generate only S3 from using the model from the aws-sdk-rust repo
# sts, sso, ssooidc, and signin must be included, as they are internally used by S3
./gradlew \
  -Paws.sdk.models.path=<absolute path to the aws-sdk-rust repo>/aws-models \
  -Paws.services=+s3,+sts,+sso,+ssooidc,+signin \
  :aws:sdk:assemble

# Start with a clean slate
./gradlew aws:sdk:clean
```

The generation logic is as follows:
1. If `aws.sdk.models.path` is specified, take models from that path. Otherwise take them from the local `aws-models` directory.
2. Reference the `aws.services` property to determine which models to include/exclude, based on the service module names.

Debugging with IntelliJ
-----------------------

The easiest way to debug codegen is to attach a remote debugger to the Gradle daemon and then run `aws:sdk:assemble`.
To do this:

1. Temporarily modify the root `gradle.properties` file to have the following:
```
org.gradle.jvmargs=-agentlib:jdwp=transport=dt_socket,server=y,suspend=y,address=localhost:5006
```
2. Run `./gradlew --stop` to kill any Gradle daemons that are running without that property.
3. Configure IntelliJ to remote debug on port 5006 (or whichever port was configured above).
4. Run `./gradlew aws:sdk:assemble` (with any additional properties to limit the services generated)
5. It will hang on "Starting Daemon". This is because the Gradle daemon is waiting for a remote debugger
   to start up. Attaching IntelliJ's debugger will make the build proceed, but now you can stop execution
   on breakpoints and examine values.
