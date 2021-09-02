vNext (Month Day, Year)
-----------------------
This release adds support for three commonly requested features:
- More powerful credential chain
- Support for constructing multiple clients from the same configuration
- Support for transcribe streaming and S3 Select

In addition, this overhauls client configuration which lead to a number of breaking changes. Detailed changes are inline.

Current Credential Provider Support:
- [x] Environment variables
- [x] Web Identity Token Credentials
- [ ] Profile file support (partial)
  - [ ] Credentials
    - [ ] SSO
    - [ ] ECS Credential source
    - [ ] IMDS credential source
    - [x] Assume role from source profile
    - [x] Static credentials source profile
    - [x] WebTokenIdentity provider
  - [x] Region
- [ ] IMDS
- [ ] ECS

## Upgrade Guide
### If you use `<sdk>::Client::from_env`
`from_env` loaded region & credentials from environment variables _only_. Default sources have been removed from the generated
SDK clients and moved to the `aws-config` package. Note that the `aws-config` package default chain adds support for
profile file and web identity token profiles.

1. Add a dependency on `aws-config`:
     ```toml
     [dependencies]
     aws-config = { git = "https://github.com/awslabs/aws-sdk-rust", tag = "v0.0.17-alpha" }
     ```
2. Update your client creation code:
   ```rust
   // `shared_config` can be used to construct multiple different service clients!
   let shared_config = aws_config::load_from_env().await;
   // before: <service>::Client::from_env();
   let client = <service>::Client::new(&shared_config)
   ```

### If you used `<client>::Config::builder()`
`Config::build()` has been modified to _not_ fallback to a default provider. Instead, use `aws-config` to load and modify
the default chain. Note that when you switch to `aws-config`, support for profile files and web identity tokens will be added.

1. Add a dependency on `aws-config`:
     ```toml
     [dependencies]
     aws-config = { git = "https://github.com/awslabs/aws-sdk-rust", tag = "v0.0.17-alpha" }
     ```

2. Update your client creation code:

   ```rust
   fn before() {
     let region = aws_types::region::ChainProvider::first_try(<1 provider>).or_default_provider();
     let config = <service>::Config::builder().region(region).build();
     let client = <service>::Client::from_conf(&config);
   }

   async fn after() {
     use aws_config::meta::region::RegionProviderChain;
     let region_provider = RegionProviderChain::first_try(<1 provider>).or_default_provider();
     // `shared_config` can be used to construct multiple different service clients!
     let shared_config = aws_config::from_env().region(region_provider).load().await;
     let client = <service>::Client::new(&shared_config)
   }
   ```

### If you used `aws-auth-providers`
All credential providers that were in `aws-auth-providers` have been moved to `aws-config`. Unless you have a specific use case
for a specific credential provider, you should use the default provider chain:

```rust
 let shared_config = aws_config::load_from_env().await;
 let client = <service>::Client::new(&shared_config);
```

### If you maintain your own credential provider
`AsyncProvideCredentials` has been renamed to `ProvideCredentials`. The trait has been moved from `aws-auth` to `aws-types`.
The original `ProvideCredentials` trait has been removed. The return type has been changed to by a custom future.

For synchronous use cases:
```rust
use aws_types::credentials::{ProvideCredentials, future};

#[derive(Debug)]
struct CustomCreds;
impl ProvideCredentials for CustomCreds {
  fn provide_credentials<'a>(&'a self) -> future::ProvideCredentials<'a>
    where
            Self: 'a,
  {
    // if your credentials are synchronous, use `::ready`
    // if your credentials are loaded asynchronously, use `::new`
    future::ProvideCredentials::ready(todo!()) // your credentials go here
  }
}
```

For asynchronous use cases:
```rust
use aws_types::credentials::{ProvideCredentials, future, Result};

#[derive(Debug)]
struct CustomAsyncCreds;
impl CustomAsyncCreds {
  async fn load_credentials(&self) -> Result {
    Ok(Credentials::from_keys("my creds...", "secret", None))
  }
}

impl ProvideCredentials for CustomCreds {
  fn provide_credentials<'a>(&'a self) -> future::ProvideCredentials<'a>
    where
            Self: 'a,
  {
    future::ProvideCredentials::new(self.load_credentials())
  }
}
```

**Breaking Changes**
- Credential providers from `aws-auth-providers` have been moved to `aws-config` (#678)
- `AsyncProvideCredentials` has been renamed to `ProvideCredentials`. The original non-async provide credentials has been
  removed. See the migration guide above.
- `<sevicename>::from_env()` has been removed (#675). A drop-in replacement is available:
  1. Add a dependency on `aws-config`:
     ```toml
     [dependencies]
     aws-config = { git = "https://github.com/awslabs/aws-sdk-rust", tag = "v0.0.17-alpha" }
     ```
  2. Update your client creation code:
     ```rust
     let client = <service>>::Client::new(&aws_config::load_from_env().await)
     ```

- `ProvideRegion` has been moved to `aws_config::meta::region::ProvideRegion`. (#675)
- `aws_types::region::ChainProvider` has been moved to `aws_config::meta::region::RegionProviderChain` (#675).
- `ProvideRegion` is now asynchronous. Code that called `provider.region()` must be changed to `provider.region().await`.
- `<awsservice>::Config::builder()` will **not** load a default region. To preserve previous behavior:
  1. Add a dependency on `aws-config`:
     ```toml
     [dependencies]
     aws-config = { git = "https://github.com/awslabs/aws-sdk-rust", tag = "v0.0.17-alpha" }
     ```
  2. ```rust
     let shared_config = aws_config::load_from_env().await;
     let config = <service>::config::Builder::from(&shared_config).<other builder modifications>.build();
     ```
- `Request` and `Response` in `smithy_http::operation` now use `SharedPropertyBag` instead of `Arc<Mutex<PropertyBag>>`. Use the `acquire` and `acquire_mut` methods to get a reference to the underlying `PropertyBag` to access properties. (#667)

**New this week**

- (When complete) Add Event Stream support (#653, #xyz)
- Add profile file provider for region (#594, #682)
- Improve documentation on collection-aware builders (#664)
- Add support for Transcribe `StartStreamTranscription` and S3 `SelectObjectContent` operations (#667)
- Add support for shared configuration between multiple services (#673)
- Update AWS SDK models (#677)
- :bug: Fix sigv4 signing when request ALPN negotiates to HTTP/2. (#674)
- :bug: Fix integer size on S3 `Size` (#679, aws-sdk-rust#209)

**Internal Changes**
- Add NowOrLater future to smithy-async (#672)


v0.21 (August 19th, 2021)
-------------------------

**New This Week**

- :tada: Add Chime Identity, Chime Messaging, and Snow Device Management support (#657)
- :tada: Add profile file credential provider implementation. This implementation currently does not support credential sources for assume role providers other than environment variables. (#640)
- :tada: Add support for WebIdentityToken providers via profile & environment variables. (#654)
- :bug: Fix name collision that occurred when a model had both a union and a structure named `Result` (#643)
- :bug: Fix STS Assume Role with WebIdentity & Assume role with SAML to support clients with no credentials provided (#652)
- Update AWS SDK models (#657)
- Add initial implementation of a default provider chain. (#650)

**Internal Changes**

- Update sigv4 tests to work around behavior change in httparse 1.5. (#656)
- Remove Bintray/JCenter source from gradle build. (#651)
- Add experimental `dvr` module to smithy-client. This will enable easier testing of HTTP traffic. (#640)
- Update smithy-client to simplify creating HTTP/HTTPS connectors (#650)
- Add Event Stream support to aws-sigv4 (#648)
- Add support for the smithy auth trait. This enables authorizations that explicitly disable authorization to work when no credentials have been provided. (#652)

v0.20 (August 10th, 2021)
--------------------------

**Breaking changes**

- (#635) The `config()`, `config_mut()`, `request()`, and `request_mut()` methods on `operation::Request` have been
  renamed to `properties()`, `properties_mut()`, `http()`, and `http_mut()` respectively.
- (#635) The `Response` type on Tower middleware has been changed from `http::Response<SdkBody>`
  to `operation::Response`. The HTTP response is still available from the `operation::Response` using its `http()`
  and `http_mut()` methods.
- (#635) The `ParseHttpResponse` trait's `parse_unloaded()` method now takes an `operation::Response` rather than
  an `http::Response<SdkBody>`.
- (#626) `ParseHttpResponse` no longer has a generic argument for the body type, but instead, always uses `SdkBody`.
  This may cause compilation failures for you if you are using Smithy generated types to parse JSON or XML without using
  a client to request data from a service. The fix should be as simple as removing `<SdkBody>` in the example below:

  Before:
  ```rust
  let output = <Query as ParseHttpResponse<SdkBody>>::parse_loaded(&parser, &response).unwrap();
  ```

  After:
  ```rust
  let output = <Query as ParseHttpResponse>::parse_loaded(&parser, &response).unwrap();
  ```

**New This Week**

- Add AssumeRoleProvider parser implementation. (#632)
- The closure passed to `provide_credentials_fn` can now borrow values (#637)
- Add `Sender`/`Receiver` implementations for Event Stream (#639)
- Bring in the latest AWS models (#630)

v0.19 (August 3rd, 2021)
------------------------

IoT Data Plane is now available! If you discover it isn't functioning as expected, please let us know!

This week also sees the addition of a robust async caching credentials provider. Take a look at the
[STS example](https://github.com/awslabs/smithy-rs/blob/7fa4af4a9367aeca6d55e26fc4d4ba93093b90c4/aws/sdk/examples/sts/src/bin/credentials-provider.rs)
to see how to use it.

**New This Week**

- :tada: Add IoT Data Plane (#624)
- :tada: Add LazyCachingCredentialsProvider to aws-auth for use with expiring credentials, such as STS AssumeRole.
  Update STS example to use this new provider (#578, #595)
- :bug: Correctly encode HTTP Checksums using base64 instead of hex. Fixes aws-sdk-rust#164. (#615)
- Update SDK gradle build logic to use gradle properties (#620)
- Overhaul serialization/deserialization of numeric/boolean types. This resolves issues around serialization of
  NaN/Infinity and should also reduce the number of allocations required during serialization. (#618)
- Update SQS example to clarify usage of FIFO vs. standard queues (#622, @trevorrobertsjr)
- Implement Event Stream frame encoding/decoding (#609, #619)

**Contributions**

Thank you for your contributions! :heart:

- @trevorrobertsjr (#622)

v0.18.1 (July 27th 2021)
------------------------

- Remove timestreamwrite and timestreamquery from the generated services (#613)

v0.18 (July 27th 2021)
----------------------

**Breaking changes**

- `test-util` has been made an optional dependency and has moved from aws-hyper to smithy-http. If you were relying
  on `aws_hyper::TestConnection`, add `smithy-client` as a dependency and enable the optional `test-util` feature. This
  prunes some unnecessary dependencies on `roxmltree` and `serde_json`
  for most users. (#608)

**New This Week**

- :tada: Release all but three remaining AWS services! Glacier, IoT Data Plane and Transcribe streaming will be
  available in a future release. If you discover that a service isn't functioning as expected please let us know! (#607)
- :bug: Bugfix: Fix parsing bug where parsing XML incorrectly stripped whitespace (#590, aws-sdk-rust#153)
- Establish common abstraction for environment variables (#594)
- Add windows to the test matrix (#594)
- :bug: Bugfix: Constrain RFC-3339 timestamp formatting to microsecond precision (#596)

v0.17 (July 15th 2021)
----------------------

**New this Week**

- :tada: Add support for Autoscaling (#576, #582)
- `AsyncProvideCredentials` now introduces an additional lifetime parameter, simplifying bridging it
  with `#[async_trait]` interfaces
- Fix S3 bug when content type was set explicitly (aws-sdk-rust#131, #566, @eagletmt)

**Contributions**

Thank you for your contributions! :heart:

- @eagletmt (#566)

v0.16 (July 6th 2021)
---------------------

**New this Week**

- :warning: **Breaking Change:** `ProvideCredentials` and `CredentialError` were both moved into `aws_auth::provider`
  when they were previously in `aws_auth` (#572)
- :tada: Add support for AWS Config (#570)
- :tada: Add support for EBS (#567)
- :tada: Add support for Cognito (#573)
- :tada: Add support for Snowball (#579, @landonxjames)
- Make it possible to asynchronously provide credentials with `provide_credentials_fn` (#572, #577)
- Improve RDS, QLDB, Polly, and KMS examples (#561, #560, #558, #556, #550)
- Update AWS SDK models (#575)
- :bug: Bugfix: Fill in message from error response even when it doesn't match the modeled case format (#565)

**Internal Changes**

- Add support for `@unsignedPayload` Smithy trait (#567)
- Strip service/api/client suffix from sdkId (#546)
- Remove idempotency token trait (#571)

**Contributions**

Thank you for your contributions! :heart:

- landonxjames (#579)

v0.15 (June 29th 2021)
----------------------

This week, we've added EKS, ECR and Cloudwatch. The JSON deserialization implementation has been replaced, please be on
the lookout for potential issues.

**New this Week**

- :tada: Add support for ECR (#557)
- :tada: Add support for Cloudwatch (#554)
- :tada: Add support for EKS (#553)
- :warn: **Breaking Change:** httpLabel no longer causes fields to be non-optional. (#537)
- :warn: **Breaking Change:** `Exception` is not renamed to `Error`. Code may need to be updated to replace `exception`
  with `error`
- Add more SES examples, and improve examples for Batch.
- Improved error handling ergonomics: Errors now provide `is_<variantname>()` methods to simplify error handling
- :bug: Bugfix: fix bug where invalid query strings could be generated (#531, @eagletmt)

**Internal Changes**

- Pin CI version to 1.52.1 (#532)
- New JSON deserializer implementation (#530)
- Fix numerous namespace collision bugs (#539)
- Gracefully handle empty response bodies during JSON parsing (#553)

**Contributors**

Thank you for your contributions! :heart:

- @eagletmt (#531)

v0.14 (June 22nd 2021)
----------------------

This week, we've added CloudWatch Logs support and fixed several bugs in the generated S3 clients. There are a few
breaking changes this week.

**New this Week**

- :tada: Add support for CloudWatch Logs (#526)
- :warning: **Breaking Change:** The `set_*` functions on generated Builders now always take an `Option` (#506)
- :warning: **Breaking Change:** Unions with Documents will see the inner document type change from `Option<Document>`
  to `Document` (#520)
- :warning: **Breaking Change:** The `as_*` functions on unions now return `Result` rather than `Option` to clearly
  indicate what the actual value is (#527)
- Add more S3 examples, and improve SNS, SQS, and SageMaker examples. Improve example doc comments (#490, #508, #509,
  #510, #511, #512, #513, #524)
- :bug: Bugfix: Show response body in trace logs for calls that don't return a stream (#514)
- :bug: Bugfix: Correctly parse S3's GetBucketLocation response (#516)
- :bug: Bugfix: Correctly URL-encode tilde characters before SigV4 signing (#519)
- :bug: Bugfix: Fix S3 PutBucketLifecycle operation by adding support for the `@httpChecksumRequired` Smithy trait (
  #523)
- :bug: Bugfix: Correctly parse non-list headers with commas in them (#525, @eagletmt)

**Internal Changes**

- Reduce name collisions in generated code (#502)
- Combine individual example packages into per-service example packages with multiple binaries (#481, #490)
- Re-export HyperAdapter in smithy-client (#515, @zekisherif)
- Add serialization/deserialization benchmark for DynamoDB to exercise restJson1 generated code (#507)

**Contributions**

Thank you for your contributions! :heart:

- @eagletmt (#525)
- @zekisherif (#515)

v0.13 (June 15th 2021)
----------------------

Smithy-rs now has codegen support for all AWS services! This week, we've added CloudFormation, SageMaker, EC2, and SES.
More details below.

**New this Week**

- :tada: Add support for CloudFormation (#500, @alistaim)
- :tada: Add support for SageMaker (#473, @alistaim)
- :tada: Add support for EC2 (#495)
- :tada: Add support for SES (#499)
- Add support for the EC2 Query protocol (#475)
- Generate fluent builders for all smithy-rs clients (#496, @jonhoo)
- :bug: Bugfix: RFC-3339 timestamps (`date-time` format in Smithy) are now formatted correctly (#479, #489)
- :bug: Bugfix: Union and enum variants named Self no longer cause compile errors in generated code (#492)

**Internal Changes**

- Combine individual example packages into per-service example packages with multiple binaries (#477, #480, #482, #484,
  #485, #486, #487, #491)
- Work towards JSON deserialization overhaul (#474)
- Make deserializer function naming consistent between XML and JSON deserializers (#497)

Contributors:

- @Doug-AWS
- @jdisanti
- @rcoh
- @alistaim
- @jonhoo

Thanks!!

v0.12 (June 8th 2021)
---------------------

Starting this week, smithy-rs now has codegen support for all AWS services except EC2. This week we’ve added MediaLive,
MediaPackage, SNS, Batch, STS, RDS, RDSData, Route53, and IAM. More details below.

**New this Week**

- :tada: Add support for MediaLive and MediaPackage (#449, @alastaim)
- :tada: Add support for SNS (#450)
- :tada: Add support for Batch (#452, @alistaim)
- :tada: Add support for STS. **Note:** This does not include support for an STS-based credential provider although an
  example is provided. (#453)
- :tada: Add support for RDS (#455) and RDS-Data (#470). (@LMJW)
- :tada: Add support for Route53 (#457, @alistaim)
- Support AWS Endpoints & Regions. With this update, regions like `iam-fips` and `cn-north-1` will now resolve to the
  correct endpoint. Please report any issues with endpoint resolution. (#468)
- :bug: Bugfix: Primitive numerics and booleans are now filtered from serialization when they are 0 and not marked as
  required. This resolves issues where maxResults needed to be set even though it is optional. (#451)
- :bug: Bugfix: S3 Head Object returned the wrong error when the object did not exist (#460, fixes #456)

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

v0.11 (June 1st, 2021)
----------------------

**New this week:**

- :tada: Add support for SQS. SQS is our first service to use the awsQuery protocol. Please report any issues you may
  encounter.
- :tada: Add support for ECS.
- **Breaking Change**: Refactored `smithy_types::Error` to be more flexible. Internal fields of `Error` are now private
  and can now be accessed accessor functions. (#426)
- `ByteStream::from_path` now accepts `implications AsRef<Path>` (@LMJW)
- Add support for S3 extended request id (#429)
- Add support for the awsQuery protocol. smithy-rs can now add support for all services except EC2.
- **Bugfix**: Timestamps that fell precisely on minute boundaries were not properly formatted (#435)
- Improve documentation for `ByteStream` & add `pub use` (#443)
- Add support for `EndpointPrefix` used
  by [`s3::WriteGetObjectResponse`](https://awslabs.github.io/aws-sdk-rust/aws_sdk_s3/operation/struct.WriteGetObjectResponse.html) (
  #420)

**Smithy Internals**

- Rewrite JSON serializer (#411, #423, #416, #427)
- Remove dead “rootProject” setting in `smithy-build.json`
- **Bugfix:** Idempotency tokens were not properly generated when operations were used by resources

Contributors:

- @jdisanti
- @rcoh
- @LMJW

Thanks!
