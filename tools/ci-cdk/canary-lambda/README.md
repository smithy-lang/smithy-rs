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

Running the canary locally
--------------------------
For testing, it's helpful to be able to run the canary locally. To accomplish this, you first need to generate
Cargo.tomls and the WASM module:

```bash
cd ../canary-runner
# to use a version of the SDK, use `--sdk-version` instead
cargo run -- build-bundle \
  --sdk-path ../../../aws/sdk/build/aws-sdk/sdk/ \
  --canary-path ../canary-lambda \
```

Next, come back to the `canary-lambda` directory. Copy the WASM module from `smithy-rs/tools/target/wasm32-wasip2/release/aws_sdk_rust_lambda_canary_wasm.wasm`
into the top level of the `canary-lambda` crate. Then you can use `cargo run` in `--local` mode to
invoke the canary:

> Note: if your default configuration does not provide a region, you must provide a region via the `AWS_REGION`
> environment variable.

```bash
export CANARY_S3_BUCKET_NAME=<your bucket name>
export CANARY_S3_MRAP_BUCKET_ARN=<your MRAP bucket ARN>
export CANARY_S3_EXPRESS_BUCKET_NAME=<your express bucket name>
export AWS_REGION=<region>
# run with `--all-features` so you run all canaries (including canaries that don't work against older versions)
cargo run --all-features -- --local
```

Building locally for Lambda from Amazon Linux 2
-----------------------------------------------

1. Build a code bundle:

```
$ cd smithy-rs/tools/ci-cdk/canary-runner
$ cargo run -- build-bundle --canary-path ../canary-lambda --sdk-release-tag <release-tag> --musl
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
$ cd smithy-rs/tools/ci-cdk/canary-runner
$ cargo run -- build-bundle --canary-path ../canary-lambda --sdk-release-tag <release-tag> --musl
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
