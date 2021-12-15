<!-- Do not manually edit this file, use `update-changelogs` -->
v0.33.1 (December 15th, 2021)
=============================
**New this release:**
- 🐛 (smithy-rs#979) Make `aws-smithy-client` a required dependency in generated services.



v0.33.0 (December 15th, 2021)
=============================
**Breaking Changes:**
- ⚠ (smithy-rs#930) Runtime crates no longer have default features. You must now specify the features that you want when you add a dependency to your `Cargo.toml`.

    **Upgrade guide**

    | before                          | after |
    |---------------------------------|-------|
    | `aws-smithy-async = "VERSION"`  | `aws-smithy-async = { version = "VERSION", features = ["rt-tokio"] }` |
    | `aws-smithy-client = "VERSION"` | `aws-smithy-client = { version = "VERSION", features = ["client-hyper", "rustls", "rt-tokio"] }` |
    | `aws-smithy-http = "VERSION"`   | `aws-smithy-http = { version = "VERSION", features = ["rt-tokio"] }` |
- ⚠ (smithy-rs#940) `aws_smithy_client::Client::https()` has been renamed to `dyn_https()`.
    This is to clearly distinguish it from `rustls` and `native_tls` which do not use a boxed connector.

**New this release:**
- 🐛 (smithy-rs#957) Include non-service-specific examples in the generated root Cargo workspace
- 🎉 (smithy-rs#922, smithy-rs#914) Add changelog automation to sdk-lints
- 🐛 (aws-sdk-rust#317, smithy-rs#907) Removed spamming log message when a client was used without a sleep implementation, and
    improved context and call to action in logged messages around missing sleep implementations.
- (smithy-rs#923) Use provided `sleep_impl` for retries instead of using Tokio directly.
- (smithy-rs#920) Fix typos in module documentation for generated crates
- 🐛 (aws-sdk-rust#301, smithy-rs#892) Avoid serializing repetitive `xmlns` attributes in generated XML serializers.
- 🐛 (smithy-rs#953, aws-sdk-rust#331) Fixed a bug where certain characters caused a panic during URI encoding.



v0.32.0 (December 2nd, 2021)
=======================

- This release was a version bump to fix a version number conflict in crates.io

v0.31.0 (December 2nd, 2021)
=======================
**New this week**
- Add docs.rs metadata section to all crates to document all features


v0.30.0-alpha (November 23rd, 2021)
===================================

**New this week**
- Improve docs on `aws-smithy-client` (smithy-rs#855)
- Fix http-body dependency version (smithy-rs#883, aws-sdk-rust#305)
- `SdkError` now includes a variant `TimeoutError` for when a request times out (smithy-rs#885)
- Timeouts for requests are now configurable. You can set separate timeouts for each individual request attempt and all attempts made for a request. (smithy-rs#831)

**Breaking Changes**
- (aws-smithy-client): Extraneous `pub use SdkSuccess` removed from `aws_smithy_client::hyper_ext`. (smithy-rs#855)


v0.29.0-alpha (November 11th, 2021)
===================================

**Breaking Changes**

Several breaking changes around `aws_smithy_types::Instant` were introduced by smithy-rs#849:
- `aws_smithy_types::Instant` from was renamed to `DateTime` to avoid confusion with the standard library's monotonically non-decreasing `Instant` type.
- `DateParseError` in `aws_smithy_types` has been renamed to `DateTimeParseError` to match the type that's being parsed.
- The `chrono-conversions` feature and associated functions have been moved to the `aws-smithy-types-convert` crate.
  - Calls to `Instant::from_chrono` should be changed to:
    ```rust
    use aws_smithy_types::DateTime;
    use aws_smithy_types_convert::date_time::DateTimeExt;

    // For chrono::DateTime<Utc>
    let date_time = DateTime::from_chrono_utc(chrono_date_time);
    // For chrono::DateTime<FixedOffset>
    let date_time = DateTime::from_chrono_offset(chrono_date_time);
    ```
  - Calls to `instant.to_chrono()` should be changed to:
    ```rust
    use aws_smithy_types_convert::date_time::DateTimeExt;

    date_time.to_chrono_utc();
    ```
- `Instant::from_system_time` and `Instant::to_system_time` have been changed to `From` trait implementations.
  - Calls to `from_system_time` should be changed to:
    ```rust
    DateTime::from(system_time);
    // or
    let date_time: DateTime = system_time.into();
    ```
  - Calls to `to_system_time` should be changed to:
    ```rust
    SystemTime::from(date_time);
    // or
    let system_time: SystemTime = date_time.into();
    ```
- Several functions in `Instant`/`DateTime` were renamed:
  - `Instant::from_f64` -> `DateTime::from_secs_f64`
  - `Instant::from_fractional_seconds` -> `DateTime::from_fractional_secs`
  - `Instant::from_epoch_seconds` -> `DateTime::from_secs`
  - `Instant::from_epoch_millis` -> `DateTime::from_millis`
  - `Instant::epoch_fractional_seconds` -> `DateTime::as_secs_f64`
  - `Instant::has_nanos` -> `DateTime::has_subsec_nanos`
  - `Instant::epoch_seconds` -> `DateTime::secs`
  - `Instant::epoch_subsecond_nanos` -> `DateTime::subsec_nanos`
  - `Instant::to_epoch_millis` -> `DateTime::to_millis`
- The `DateTime::fmt` method is now fallible and fails when a `DateTime`'s value is outside what can be represented by the desired date format.
- In `aws-sigv4`, the `SigningParams` builder's `date_time` setter was renamed to `time` and changed to take a `std::time::SystemTime` instead of a chrono's `DateTime<Utc>`.

**New this week**

- :warning: MSRV increased from 1.53.0 to 1.54.0 per our 3-behind MSRV policy.
- Conversions from `aws_smithy_types::DateTime` to `OffsetDateTime` from the `time` crate are now available from the `aws-smithy-types-convert` crate. (smithy-rs#849)
- Fixed links to Usage Examples (smithy-rs#862, @floric)

v0.28.0-alpha (November 11th, 2021)
===================================

No changes since last release except for version bumping since older versions
of the AWS SDK were failing to compile with the `0.27.0-alpha.2` version chosen
for the previous release.

v0.27.0-alpha.2 (November 9th, 2021)
=======================
**Breaking Changes**

- Members named `builder` on model structs were renamed to `builder_value` so that their accessors don't conflict with the existing `builder()` methods (smithy-rs#842)

**New this week**

- Fix epoch seconds date-time parsing bug in `aws-smithy-types` (smithy-rs#834)
- Omit trailing zeros from fraction when formatting HTTP dates in `aws-smithy-types` (smithy-rs#834)
- Generated structs now have accessor methods for their members (smithy-rs#842)

v0.27.0-alpha.1 (November 3rd, 2021)
====================================
**Breaking Changes**
- `<operation>.make_operation(&config)` is now an `async` function for all operations. Code should be updated to call `.await`. This will only impact users using the low-level API. (smithy-rs#797)

**New this week**
- SDK code generation now includes a version in addition to path parameters when the `version` parameter is included in smithy-build.json
- `moduleDescription` in `smithy-build.json` settings is now optional
- Upgrade to Smithy 1.12
- `hyper::Error(IncompleteMessage)` will now be retried (smithy-rs#815)
- Unions will optionally generate an `Unknown` variant to support parsing variants that don't exist on the client. These variants will fail to serialize if they are ever included in requests.
- Fix generated docs on unions. (smithy-rs#826)

v0.27 (October 20th, 2021)
==========================

**Breaking Changes**

- :warning: All Smithy runtime crates have been renamed to have an `aws-` prefix. This may require code changes:
  - _Cargo.toml_ changes:
    - `smithy-async` -> `aws-smithy-async`
    - `smithy-client` -> `aws-smithy-client`
    - `smithy-eventstream` -> `aws-smithy-eventstream`
    - `smithy-http` -> `aws-smithy-http`
    - `smithy-http-tower` -> `aws-smithy-http-tower`
    - `smithy-json` -> `aws-smithy-json`
    - `smithy-protocol-test` -> `aws-smithy-protocol-test`
    - `smithy-query` -> `aws-smithy-query`
    - `smithy-types` -> `aws-smithy-types`
    - `smithy-xml` -> `aws-smithy-xml`
  - Rust `use` statement changes:
    - `smithy_async` -> `aws_smithy_async`
    - `smithy_client` -> `aws_smithy_client`
    - `smithy_eventstream` -> `aws_smithy_eventstream`
    - `smithy_http` -> `aws_smithy_http`
    - `smithy_http_tower` -> `aws_smithy_http_tower`
    - `smithy_json` -> `aws_smithy_json`
    - `smithy_protocol_test` -> `aws_smithy_protocol_test`
    - `smithy_query` -> `aws_smithy_query`
    - `smithy_types` -> `aws_smithy_types`
    - `smithy_xml` -> `aws_smithy_xml`

**New this week**

- Filled in missing docs for services in the rustdoc documentation (smithy-rs#779)

v0.26 (October 15th, 2021)
=======================

**Breaking Changes**

- :warning: The `rust-codegen` plugin now requires a `moduleDescription` in the *smithy-build.json* file. This
  property goes into the generated *Cargo.toml* file as the package description. (smithy-rs#766)

**New this week**

- Add `RustSettings` to `CodegenContext` (smithy-rs#616, smithy-rs#752)
- Prepare crate manifests for publishing to crates.io (smithy-rs#755)
- Generated *Cargo.toml* files can now be customized (smithy-rs#766)

v0.25.1 (October 11th, 2021)
=========================
**New this week**
- :bug: Re-add missing deserialization operations that were missing because of a typo in `HttpBoundProtocolGenerator.kt`

v0.25 (October 7th, 2021)
=========================
**Breaking changes**
- :warning: MSRV increased from 1.52.1 to 1.53.0 per our 3-behind MSRV policy.
- :warning: `smithy_client::retry::Config` field `max_retries` is renamed to `max_attempts`
  - This also brings a change to the semantics of the field. In the old version, setting `max_retries` to 3 would mean
    that up to 4 requests could occur (1 initial request and 3 retries). In the new version, setting `max_attempts` to 3
    would mean that up to 3 requests could occur (1 initial request and 2 retries).
- :warning: `smithy_client::retry::Config::with_max_retries` method is renamed to `with_max_attempts`
- :warning: Several classes in the codegen module were renamed and/or refactored (smithy-rs#735):
  - `ProtocolConfig` became `CodegenContext` and moved to `software.amazon.smithy.rust.codegen.smithy`
  - `HttpProtocolGenerator` became `ProtocolGenerator` and was refactored
    to rely on composition instead of inheritance
  - `HttpProtocolTestGenerator` became `ProtocolTestGenerator`
  - `Protocol` moved into `software.amazon.smithy.rust.codegen.smithy.protocols`
- `SmithyConnector` and `DynConnector` now return `ConnectorError` instead of `Box<dyn Error>`. If you have written a custom connector, it will need to be updated to return the new error type. (#744)
- The `DispatchError` variant of `SdkError` now contains `ConnectorError` instead of `Box<dyn Error>` (#744).

**New this week**

- :bug: Fix an issue where `smithy-xml` may have generated invalid XML (smithy-rs#719)
- Add `RetryConfig` struct for configuring retry behavior (smithy-rs#725)
- :bug: Fix error when receiving empty event stream messages (smithy-rs#736)
- :bug: Fix bug in event stream receiver that could cause the last events in the response stream to be lost (smithy-rs#736)
- Add connect & HTTP read timeouts to IMDS, defaulting to 1 second
- IO and timeout errors from Hyper can now be retried (#744)

**Contributors**

Thank you for your contributions! :heart:
* @obi1kenobi (smithy-rs#719)
* @guyilin-amazon (smithy-rs#750)

v0.24 (September 24th, 2021)
============================

**New This Week**

- Add IMDS credential provider to `aws-config` (smithy-rs#709)
- Add IMDS client to `aws-config` (smithy-rs#701)
- Add `TimeSource` to `aws_types::os_shim_internal` (smithy-rs#701)
- User agent construction is now `const fn` (smithy-rs#701)
- Add `sts::AssumeRoleProvider` to `aws-config` (smithy-rs#703, aws-sdk-rust#3)
- Add IMDS region provider to `aws-config` (smithy-rs#715)
- Add query param signing to the `aws-sigv4` crate (smithy-rs#707)
- :bug: Update event stream `Receiver`s to be `Send` (smithy-rs#702, #aws-sdk-rust#224)

v0.23 (September 14th, 2021)
=======================

**New This Week**
- :bug: Fixes issue where `Content-Length` header could be duplicated leading to signing failure (aws-sdk-rust#220, smithy-rs#697)
- :bug: Fixes naming collision during generation of model shapes that collide with `<operationname>Input` and `<operationname>Output` (#699)

v0.22 (September 2nd, 2021)
===========================

This release adds support for three commonly requested features:
- More powerful credential chain
- Support for constructing multiple clients from the same configuration
- Support for Transcribe streaming and S3 Select

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

Upgrade Guide
-------------

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

Changes
-------

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

- :tada: Add profile file provider for region (#594, #682)
- :tada: Add support for shared configuration between multiple services (#673)
- :tada: Add support for Transcribe `StartStreamTranscription` and S3 `SelectObjectContent` operations (#667)
- :tada: Add support for new MemoryDB service (#677)
- Improve documentation on collection-aware builders (#664)
- Update AWS SDK models (#677)
- :bug: Fix sigv4 signing when request ALPN negotiates to HTTP/2. (#674)
- :bug: Fix integer size on S3 `Size` (#679, aws-sdk-rust#209)
- :bug: Fix JSON parsing issue for modeled empty structs (#683, aws-sdk-rust#212)
- :bug: Fix acronym case disagreement between FluentClientGenerator and HttpProtocolGenerator type aliasing (#668)

**Internal Changes**

- Add Event Stream support for restJson1 and restXml (#653, #667)
- Add NowOrLater future to smithy-async (#672)


v0.21 (August 19th, 2021)
=========================

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
=========================

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
========================

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
========================

- Remove timestreamwrite and timestreamquery from the generated services (#613)

v0.18 (July 27th 2021)
======================

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
======================

**New this Week**

- :tada: Add support for Autoscaling (#576, #582)
- `AsyncProvideCredentials` now introduces an additional lifetime parameter, simplifying bridging it
  with `#[async_trait]` interfaces
- Fix S3 bug when content type was set explicitly (aws-sdk-rust#131, #566, @eagletmt)

**Contributions**

Thank you for your contributions! :heart:

- @eagletmt (#566)

v0.16 (July 6th 2021)
=====================

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
======================

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
======================

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
======================

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
=====================

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
======================

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
