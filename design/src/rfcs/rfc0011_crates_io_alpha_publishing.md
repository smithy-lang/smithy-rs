RFC: Publishing to Crates.io
============================

> Status: RFC

The AWS SDK for Rust and its supporting Smithy crates need to be published to [crates.io](https://crates.io/)
so that customers can include them in their projects and also publish crates of their own that depend on them.

This doc proposes a short-term and long-term solution for publishing to crates.io. The short-term approach is intended
to be executed manually by a developer using scripts and an SOP no more than once per week, and should require less
than a dev week to implement. The long-term approach is intended to be fully automated and run as frequently as
once per day.

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

The Rust SDK will make one small adjustment to the above requirements: the `patch` version will be incremented
for backwards compatible AWS model changes, and the `minor` version will be incremented for changes in Smithy runtime
dependencies. For example, if the `smithy-types` crate version changes at all, then any crate that depends on it
will have a `minor` version bump.

Additionally, AWS SDK crates will maintain a consistent `major` and `minor` version number across all services.
The latest version of `aws-sdk-s3` will always have the same `major.minor` version as the latest `aws-sdk-dynamodb`,
for example. The `patch` version is allowed to be different between service crates.

Smithy runtime crates will maintain their own versioning, and their version numbers are independent of each other.
For example, the latest `smithy-types` could be at `0.3` while `smithy-json` is at `0.2`. The `aws-config` crate
is an exception to this rule since customers depend on it directly. It should maintain the same `major.minor` as
the AWS SDK crates.

The `pre` version tag will be `alpha` during the Rust SDK alpha, `preview` during dev preview, and removed once the
SDK is generally available.

### Version Bumping

In general, model changes will be frequent (daily) while changes to runtime crates and code generation
will be infrequent (weekly to monthly). Model changes are supposed to _always_ be backwards compatible, so it
makes sense to apply these to `patch` revisions as much as possible. That way, if a model change was made only
to S3, then only the S3 crate would be changed rather than every AWS SDK crate, of which there are more than 260.

AWS SDK crates will follow this process for version bumps:

1. Bump the `minor` version in tandem with _all_ other SDK crates and `aws-config` if the smithy-rs version used to
   generate the SDK changed. This ensures changes to runtime crates and/or code generation logic will result in
   a `minor` version bump.
2. Bump the `patch` version if the smithy-rs version remained the same _AND_ if there are code changes to
   the SDK crate. This allows backwards compatible model changes to apply as patch revisions.

The version bump requirements for runtime crates are flexible and deferred to the short-term and long-term proposals.

### Yanking

Mistakes will inevitably be made, and a mechanism is needed to yank packages while keeping the latest version
of the SDK successfully consumable from crates.io. To keep this simple, the entire published batch of crates
will be yanked if any crate in that batch needs to be yanked. For example, if 260 crates were published in a batch,
and it turns out there's a problem that requires yanking one of them, then all 260 will be yanked. Attempting to do
partial yanking will require a lot of effort and be difficult to get right. Yanking should be a last resort.

Concrete Scenarios
------------------

The following are example scenarios that may occur that the publishing implementation must handle.

1. **AWS models were updated**: This results in `patch` version bumps for all AWS SDK crates _that had changes_.
2. **Bug fixed in runtime crate**: Runtime crate is `minor` version bumped. All runtime crates that depend on it are
   also `minor` version bumped, and all of the AWS SDK crates are `minor` version bumped.
3. **Bug fixed in codegen**: All AWS SDK crates are `minor` version bumped.
4. **CVE discovered in external dependency**: All manifests depending on the vulnerable dependency are
   updated to explicitly depend on the fixed version (or newer). If these changes are isolated to the runtime crates,
   then `patch` revisions of those crates can be published. Otherwise, this will result in a `minor` version bump
   in runtime crates and AWS SDK crates.
5. **CVE discovered in runtime crate**: If it can be fixed in a backwards compatible way, then `patch` revision the
   runtime crate with the fix. Otherwise, `minor` version bump the runtime crate and follow the normal process from there.
6. **CVE discovered in generated code**: Same process as a bug fix in codegen.
7. **Accidental release of breaking change**: Run publish process in reverse, yanking the published packages.
8. **Buggy update to runtime crate**: If possible, roll forward with a `patch` revision. Otherwise, run publish process
   in reverse yanking the published packages.

Short-term Proposal
-------------------

The short-term approach builds off our current weekly release process.
That process, at time of writing, is the following:

1. Run script to update AWS models
2. Manually update AWS SDK version in `aws/sdk/gradle.properties` in smithy-rs
3. Tag smithy-rs
4. Wait for GitHub actions to generate AWS SDK using newly released smithy-rs
5. Check out aws-sdk-rust, delete existing SDK code, unzip generated SDK in place, and update readme
6. Tag aws-sdk-rust

To keep things simple:
- The Smithy runtime crates will have the same smithy-rs version
- All AWS crates will have the same AWS SDK version
- There will _NOT_ be `patch` revisions

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
4. Wait for GitHub actions to generate the AWS SDK using the newly released smithy-rs
5. Check out aws-sdk-rust, delete existing SDK code, unzip generated SDK in place, and update readme
6. Tag aws-sdk-rust
7. Run publish script

### Short-term Changes Checklist

- [x] Prepare runtime crate manifests for publication to crates.io (https://github.com/awslabs/smithy-rs/pull/755)
- [x] Update SDK generator to set correct crate versions (https://github.com/awslabs/smithy-rs/pull/755)
- [ ] Write publish script

**Effort estimated:** 1 dev week

Long-term Proposal
------------------

When it comes time to build the long-term solution, this proposal should be expanded upon. It is kept
intentionally short here to just give a high-level overview.

The long-term approach completely discards the current release process in favor of automation.
Every change made to smithy-rs will automatically apply to aws-sdk-rust via a GitHub action called the
update process. Part of this process will be determining and recording what changed so that versioning
decisions can be made at publish time. Publishing will be automated, but initiating it will continue
to be a manual step.

The short-term process allows a developer to review the aggregate generated code changes before
deciding to release. This step is valuable and has caught bugs in the past, so it needs to become part of
the review process in smithy-rs. A CodeBuild action will be added to diff the current version of aws-sdk-rust
against the version generated by the PR. This diff will be uploaded as HTML to CloudFront backed by S3
and linked to the GitHub PR as a comment so that it can be reviewed. This comment will also include the overall
size of the diff so the reviewer can decide how much scrutiny it needs.

### Update process

Every time a PR is merged into smithy-rs, a GitHub Action will kick off to update aws-sdk-rust.
This process will:

1. Generate a full AWS SDK
2. Generate a version manifest file named `next_versions.json`. This file will include
   the latest git commit hashes of the following files and directories:
   - Model changes:
     - `aws/sdk/aws-models/*.json`
   - Codegen changes:
     - `codegen/`
     - `aws/sdk-codegen/`
     - root `gradle.properties`, `settings.gradle.kts`, and `build.gradle.kts`
   - Runtime crate changes:
     - `rust-runtime/*`
     - `aws/rust-runtime/*`

For example, this may look as follows:
```json5
{
   "model_versions": {
      // NOTE: It's important to use the module name rather than the model json file name
      "aws-sdk-s3": "b88b2c3539b33ec20ca9c38ff26107e013eaa98b",
      "aws-sdk-dynamodb": "14f435e864e20dcfe401d39e456e363e12030b94"
      // and so on...
   },
   "codegen_versions": {
      "codegen": "0ae12fd9c010fe89ccd200ffe49074af8ba14949",
      "aws/sdk-codegen": "275baef70a37392f3346348acaabea3eec9b7597",
      "gradle.properties": "f1a726c1d7b01b27ddceb33687970f4f514f9d2c",
      "settings.gradle.kts": "ada243be649ac3f9a8b837ec2e2456c871d2cfac",
      "build.gradle.kts": "f7ba94c16ca8f9126f1a01f5b99f8d3fc78795d0"
   },
   "runtime_versions": {
      "smithy-json": "4f898068b83842c187c30ce313f8fb57b2e26d42",
      "smithy-types": "bef53826506e00505e8ac2a66626864cd70301c7",
      "aws-types": "1ca2b469718c0c7e2a6255840e0e5cddedb3f34a",
      "aws-sig-auth": "275baef70a37392f3346348acaabea3eec9b7597"
      // and so on...
   }
}
```

3. Replace SDK code with new generated artifacts

### Publish process

The publish process relies on the existence of a `previous_versions.json`, which will have the same
contents as the `next_versions.json` that the update process creates, but will also have the crates.io
version of every crate listed in it. For example:

```json5
{
   "model_versions": {
      "aws-sdk-s3": "b88b2c3539b33ec20ca9c38ff26107e013eaa98b",
      "aws-sdk-dynamodb": "14f435e864e20dcfe401d39e456e363e12030b94"
      // and so on...
   },
   "codegen_versions": {
      "codegen": "0ae12fd9c010fe89ccd200ffe49074af8ba14949",
      "aws/sdk-codegen": "275baef70a37392f3346348acaabea3eec9b7597",
      "gradle.properties": "f1a726c1d7b01b27ddceb33687970f4f514f9d2c",
      "settings.gradle.kts": "ada243be649ac3f9a8b837ec2e2456c871d2cfac",
      "build.gradle.kts": "f7ba94c16ca8f9126f1a01f5b99f8d3fc78795d0"
   },
   "runtime_versions": {
      "smithy_runtime": "bef53826506e00505e8ac2a66626864cd70301c7",
      "aws_runtime": "bef53826506e00505e8ac2a66626864cd70301c7"
   },
   "crates": {
      "smithy-json": { "version":  "1.10.0" },
      "smithy-types": { "version":  "1.3.0" },
      "aws-types": { "version":  "1.2.0" },
      "aws-config": { "version":  "1.58.0" },
      "aws-sdk-s3": { "version":  "1.58.0" }
      // and so on...
   },
   "batched": {
      "smithy-json": { "version":  "1.10.0" },
      "aws-types": { "version":  "1.2.0" },
      "aws-sdk-s3": { "version":  "1.58.0" }
      // and so on...
   }
}
```

From here, it should be simple to calculate version numbers for the crates before publish:

1. Compare git commit hashes between `previous_versions.json` and `next_versions.json` to calculate a list
   of what changed (models, codegen, and runtime crates).
2. Minor version bump any changed runtime crates.
3. Recursively minor version bump runtime crates that depend on bumped runtime crates.
4. If the codegen or runtime crates changed, then minor version bump all SDK crates and the `aws-config` crate.
5. If only models changed, then patch version bump SDK crates whose models changed.
6. Update crate manifests with corrected version numbers
7. Update `previous_versions.json` with the new version numbers and commit hashes

Once that process is complete, the final publish can be kicked off, which is identical to the publish
process from the short-term proposal. The `previous_versions.json` has a list of all crate versions that were
published in `batched` as well, so yanking the entire batch should be possible. If necessary, a tool could be
built to do the yanking automatically.

### Long-term Changes Checklist

- [ ] Implement smithy-rs pull request codegen diffing action
- [ ] Implement update process action
- [ ] Implement publish process

**Effort estimated:** 5 dev weeks total
- Codegen diffing action: 1 dev weeks
- Update process action: 1 dev week
- Publish process: 1 dev weeks
- Security review: 1 dev week
- Buffer time: 1 dev week
