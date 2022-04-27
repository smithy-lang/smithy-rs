Canary Lambda
=============

This is a Rust Lambda that exercises the AWS SDK for Rust in a real Lambda
environment. It is intended to be rebuilt with the latest or previous AWS
SDK version in the `aws-sdk-rust` repository, deployed, and then run to
determine that that version is still OK.

For example, after releasing a new version of the SDK to crates.io, this
can be compiled against that new version, deployed, and run to verify the
deployed SDK is functioning correctly. Similarly, it can be used to verify
the previous version of the SDK continues to work after the deployment
of the new version.


Building locally for Lambda from Amazon Linux 2
-----------------------------------------------

1. Build a code bundle:

```
$ ./build-bundle --sdk-version <version>
```

This will place a zip file in `smithy-rs/target/release` that can be uploaded and tested against Lambda.


Building locally for Lambda from non Amazon Linux 2 system
----------------------------------------------------------

1. Make sure the `musl-gcc` wrapper is installed.
2. Add the musl target for Rust:

```
$ rustup target add x86_64-unknown-linux-musl
```

3. Build a code bundle:

```
$ ./build-bundle --sdk-version <version> --musl
```

This will place a zip file in `smithy-rs/target/x86_64-unknown-linux-musl/release` that can be
uploaded and tested against Lambda.


How to add a new canary
----------------------

The canary Lambda runs a list of canaries in parallel asynchronously. To add a new canary,
do the following:

1. If new permissions are required, grant them to the OIDC role that is used to
   run the canary Lambda using the CDK in the `tools/ci-cdk/` directory. Be sure
   to deploy these changes to the canary AWS account.
2. Add a new module to this `canary-lambda/` project to hold the canary code. This
   should be implemented as an async function that returns an empty result.
3. Wire up the new canary in the `get_canaries_to_run` function in the `canary` module.
