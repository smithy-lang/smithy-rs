## vNext (Month Day Year)

**Breaking changes**
* `test-util` has been made an optional dependency and has moved from
  aws-hyper to smithy-http. If you were relying on `aws_hyper::TestConnection`, add `smithy-client` as a dependency
  and enable the optional `test-util` feature. This prunes some unnecessary dependencies on `roxmltree` and `serde_json`
  for most users. (#608)

**New This Week**
- :bug: Bugfix: Fix parsing bug where whitespace was stripped when parsing XML (#590)
- Establish common abstraction for environment variables (#594)
- Add windows to the test matrix (#594)
- (When complete) Add profile file provider for region (#594, #xyz)
- :bug: Bugfix: Constrain RFC-3339 timestamp formatting to microsecond precision (#596)

## v0.15 (June 29th 2021)
This week, we've added EKS, ECR and Cloudwatch. The JSON deserialization implementation has been replaced, please be
on the lookout for potential issues.

**New this Week**
- 🎉 Add support for ECR (#557)
- 🎉 Add support for Cloudwatch (#554)
- 🎉 Add support for EKS (#553)
- ⚠️ **Breaking Change:** httpLabel no longer causes fields to be non-optional. (#537)
- ⚠️ **Breaking Change:** `Exception` is not renamed to `Error`. Code may need to be updated to replace `exception` with `error`
- Add more SES examples, and improve examples for Batch.
- Improved error handling ergonomics: Errors now provide `is_<variantname>()` methods to simplify error handling
- 🐛 Bugfix: fix bug where invalid query strings could be generated (#531, @eagletmt)

**Internal Changes**
- Pin CI version to 1.52.1 (#532)
- New JSON deserializer implementation (#530)
- Fix numerous namespace collision bugs (#539)
- Gracefully handle empty response bodies during JSON parsing (#553)

**Contributors**

Thank you for your contributions! ❤️

* @eagletmt (#531)


## v0.14 (June 22nd 2021)
This week, we've added CloudWatch Logs support and fixed several bugs in the generated S3 clients.
There are a few breaking changes this week.

**New this Week**
- 🎉 Add support for CloudWatch Logs (#526)
- ⚠️ **Breaking Change:** The `set_*` functions on generated Builders now always take an `Option` (#506)
- ⚠️ **Breaking Change:** Unions with Documents will see the inner document type change from `Option<Document>` to `Document` (#520)
- ⚠️ **Breaking Change:** The `as_*` functions on unions now return `Result` rather than `Option` to clearly indicate what the actual value is (#527)
- Add more S3 examples, and improve SNS, SQS, and SageMaker examples. Improve example doc comments (#490, #508, #509, #510, #511, #512, #513, #524)
- 🐛 Bugfix: Show response body in trace logs for calls that don't return a stream (#514)
- 🐛 Bugfix: Correctly parse S3's GetBucketLocation response (#516)
- 🐛 Bugfix: Correctly URL-encode tilde characters before SigV4 signing (#519)
- 🐛 Bugfix: Fix S3 PutBucketLifecycle operation by adding support for the `@httpChecksumRequired` Smithy trait (#523)
- 🐛 Bugfix: Correctly parse non-list headers with commas in them (#525, @eagletmt)

**Internal Changes**
- Reduce name collisions in generated code (#502)
- Combine individual example packages into per-service example packages with multiple binaries (#481, #490)
- Re-export HyperAdapter in smithy-client (#515, @zekisherif)
- Add serialization/deserialization benchmark for DynamoDB to exercise restJson1 generated code (#507)

**Contributions**

Thank you for your contributions! ❤️

- @eagletmt (#525)
- @zekisherif (#515)

## v0.13 (June 15th 2021)
Smithy-rs now has codegen support for all AWS services! This week, we've added CloudFormation, SageMaker, EC2, and SES. More details below.

**New this Week**
- 🎉 Add support for CloudFormation (#500, @alistaim)
- 🎉 Add support for SageMaker (#473, @alistaim)
- 🎉 Add support for EC2 (#495)
- 🎉 Add support for SES (#499)
- Add support for the EC2 Query protocol (#475)
- Generate fluent builders for all smithy-rs clients (#496, @jonhoo)
- 🐛 Bugfix: RFC-3339 timestamps (`date-time` format in Smithy) are now formatted correctly (#479, #489)
- 🐛 Bugfix: Union and enum variants named Self no longer cause compile errors in generated code (#492)

**Internal Changes**
- Combine individual example packages into per-service example packages with multiple binaries (#477, #480, #482, #484, #485, #486, #487, #491)
- Work towards JSON deserialization overhaul (#474)
- Make deserializer function naming consistent between XML and JSON deserializers (#497)

Contributors:
- @Doug-AWS
- @jdisanti
- @rcoh
- @alistaim
- @jonhoo

Thanks!!

## v0.12 (June 8th 2021)
Starting this week, smithy-rs now has codegen support for all AWS services except EC2. This week we’ve added MediaLive, MediaPackage, SNS, Batch, STS, RDS, RDSData, Route53, and IAM. More details below.

**New this Week**
- :tada: Add support for MediaLive and MediaPackage (#449, @alastaim)
- :tada: Add support for SNS (#450)
- :tada: Add support for Batch (#452, @alistaim)
- :tada: Add support for STS. **Note:** This does not include support for an STS-based credential provider although an example is provided. (#453)
- :tada: Add support for RDS (#455) and RDS-Data (#470). (@LMJW)
- :tada: Add support for Route53 (#457, @alistaim)
- Support AWS Endpoints & Regions. With this update, regions like `iam-fips` and `cn-north-1` will now resolve to the correct endpoint. Please report any issues with endpoint resolution. (#468)
- 🐛 Bugfix: Primitive numerics and booleans are now filtered from serialization when they are 0 and not marked as required. This resolves issues where maxResults needed to be set even though it is optional. (#451)
- 🐛 Bugfix: S3 Head Object returned the wrong error when the object did not exist (#460, fixes #456)


**Internal Changes**

- Remove unused key “build” from smithy-build.json and Rust settings (#447)
- Split SDK CI jobs for faster builds & reporting (#446)
- Fix broken doc link in JSON serializer (@LMJW)
- Work towards JSON deserialization overhaul (#454, #462)

Contributors:
- @rcoh
- @jdisanti
- @alistaim
- @LMJW

Thanks!!

## v0.11 (June 1st, 2021)
**New this week:**
- :tada: Add support for SQS. SQS is our first service to use the awsQuery protocol. Please report any issues you may encounter.
- :tada: Add support for ECS.
- **Breaking Change**: Refactored `smithy_types::Error` to be more flexible. Internal fields of `Error` are now private and can now be accessed accessor functions. (#426)
- `ByteStream::from_path` now accepts `implications AsRef<Path>` (@LMJW)
- Add support for S3 extended request id (#429)
- Add support for the awsQuery protocol. smithy-rs can now add support for all services except EC2.
- **Bugfix**: Timestamps that fell precisely on minute boundaries were not properly formatted (#435)
- Improve documentation for `ByteStream` & add `pub use` (#443)
- Add support for `EndpointPrefix` used by [`s3::WriteGetObjectResponse`](https://awslabs.github.io/aws-sdk-rust/aws_sdk_s3/operation/struct.WriteGetObjectResponse.html) (#420)

## Smithy Internals
- Rewrite JSON serializer (#411, #423, #416, #427)
- Remove dead “rootProject” setting in `smithy-build.json`
- **Bugfix:** Idempotency tokens were not properly generated when operations were used by resources

Contributors:
- @jdisanti
- @rcoh
- @LMJW

Thanks!
