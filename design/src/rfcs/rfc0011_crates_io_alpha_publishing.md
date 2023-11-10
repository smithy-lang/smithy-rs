RFC: Publishing the Alpha SDK to Crates.io
==========================================

> Status: Implemented

The AWS SDK for Rust and its supporting Smithy crates need to be published to [crates.io](https://crates.io/)
so that customers can include them in their projects and also publish crates of their own that depend on them.

This doc proposes a short-term solution for publishing to crates.io. This approach is intended to be executed
manually by a developer using scripts and an SOP no more than once per week, and should require less than a
dev week to implement.

Terminology
-----------

- **AWS SDK Crate**: A crate that provides a client for calling a given AWS service, such as `aws-sdk-s3` for calling S3.
- **AWS Runtime Crate**: Any runtime crate that the AWS SDK generated code relies on, such as `aws-types`.
- **Smithy Runtime Crate**: Any runtime crate that the smithy-rs generated code relies on, such as `smithy-types`.

Requirements
------------

### Versioning

Cargo uses [semver](https://github.com/dtolnay/semver#requirements) for versioning,
with a `major.minor.patch-pre` format:
- `major`: Incompatible API changes
- `minor`: Added functionality in backwards compatible manner
- `patch`: Backwards compatible bug fixes
- `pre`: Pre-release version tag (omitted for normal releases)

For now, AWS SDK crates (including `aws-config`) will maintain a consistent `major` and `minor` version number
across all services. The latest version of `aws-sdk-s3` will always have the same `major.minor` version as the
latest `aws-sdk-dynamodb`, for example. The `patch` version is allowed to be different between service crates,
but it is unlikely that we will make use of `patch` versions throughout alpha and dev preview.
Smithy runtime crates will have different version numbers from the AWS SDK crates, but will also maintain
a consistent `major.minor`.

The `pre` version tag will be `alpha` during the Rust SDK alpha, and will be removed once the SDK is in
dev preview.

During alpha, the `major` version will always be 0, and the `minor` will be bumped for all published
crates for every release. A later RFC may change the process during dev preview.

### Yanking

Mistakes will inevitably be made, and a mechanism is needed to yank packages while keeping the latest version
of the SDK successfully consumable from crates.io. To keep this simple, the entire published batch of crates
will be yanked if any crate in that batch needs to be yanked. For example, if 260 crates were published in a batch,
and it turns out there's a problem that requires yanking one of them, then all 260 will be yanked. Attempting to do
partial yanking will require a lot of effort and be difficult to get right. Yanking should be a last resort.

Concrete Scenarios
------------------

The following changes will be bundled together as a `minor` version bump during weekly releases:

- AWS model updates
- New features
- Bug fixes in runtime crates or codegen

In exceptional circumstances, a `patch` version will be issued if the fix doesn't require API breaking changes:

- CVE discovered in a runtime crate
- Buggy update to a runtime crate

In the event of a CVE being discovered in an external dependency, if the external dependency is
internal to a crate, then a `patch` revision can be issued for that crate to correct it. Otherwise if the CVE
is in a dependency that is part of the public API, a `minor` revision will be issued with an expedited release.

For a CVE in generated code, a `minor` revision will be issued with an expedited release.

Proposal
--------

The short-term approach builds off our pre-crates.io weekly release process. That process was the following:

1. Run script to update AWS models
2. Manually update AWS SDK version in `aws/sdk/gradle.properties` in smithy-rs
3. Tag smithy-rs
4. Wait for GitHub actions to generate AWS SDK using newly released smithy-rs
5. Check out aws-sdk-rust, delete existing SDK code, unzip generated SDK in place, and update readme
6. Tag aws-sdk-rust

To keep things simple:
- The Smithy runtime crates will have the same smithy-rs version
- All AWS crates will have the same AWS SDK version
- `patch` revisions are exceptional and will be one-off manually published by a developer

All runtime crate version numbers in smithy-rs will be locked at `0.0.0-smithy-rs-head`. This is a fake
version number that gets replaced when generating the SDK.

The SDK generator script in smithy-rs will be updated to:
- Replace Smithy runtime crate versions with the smithy-rs version from `aws/sdk/gradle.properties`
- Replace AWS runtime crate versions with AWS SDK version from `aws/sdk/gradle.properties`
- Add correct version numbers to all path dependencies in all the final crates that end up in the build artifacts

This will result in all the crates having the correct version and manifests when imported into aws-sdk-rust.
From there, a script needs to be written to determine crate dependency order, and publish crates (preferably
with throttling and retry) in the correct order. This script needs to be able to recover from an interruption
part way through publishing all the crates, and it also needs to output a list of all crate versions published
together. This crate list will be commented on the release issue so that yanking the batch can be done if
necessary.

The new release process would be:

1. Run script to update AWS models
2. Manually update _both_ the AWS SDK version _and_ the smithy-rs version in `aws/sdk/gradle.properties` in smithy-rs
3. Tag smithy-rs
4. Wait for automation to sync changes to `aws-sdk-rust/next`
5. Cut a PR to merge `aws-sdk-rust/next` into `aws-sdk-rust/main`
6. Tag aws-sdk-rust
7. Run publish script

### Short-term Changes Checklist

- [x] Prepare runtime crate manifests for publication to crates.io (https://github.com/smithy-lang/smithy-rs/pull/755)
- [x] Update SDK generator to set correct crate versions (https://github.com/smithy-lang/smithy-rs/pull/755)
- [x] Write bulk publish script
- [x] Write bulk yank script
- [x] Write automation to sync smithy-rs to aws-sdk-rust
