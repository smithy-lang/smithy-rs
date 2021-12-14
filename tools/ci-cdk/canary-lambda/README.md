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

Building locally for Lambda
---------------------------

1. Make sure the `musl-gcc` wrapper is installed.
2. Add the musl target for Rust:

```
$ rustup target add x86_64-unknown-linux-musl
```

3. Build a code bundle:

```
$ ./build-bundle.sh
```

This will place a zip file in `smithy-rs/target/x86_64-unknown-linux-musl/release` that can be
uploaded and tested against Lambda.
