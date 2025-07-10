<!-- Do not manually edit this file. Use the `changelogger` tool. -->
July 8th, 2025
==============
**New this release:**
- (client, [smithy-rs#4076](https://github.com/smithy-lang/smithy-rs/issues/4076), [smithy-rs#4198](https://github.com/smithy-lang/smithy-rs/issues/4198)) Allows customers to configure the auth schemes and auth scheme resolver. For more information see the GitHub [discussion](https://github.com/smithy-lang/smithy-rs/discussions/4197).


June 30th, 2025
===============

June 27th, 2025
===============
**New this release:**
- :bug: (client) Fix hyper 1.x connection refused errors not marked as retryable
- (client, [smithy-rs#4186](https://github.com/smithy-lang/smithy-rs/issues/4186)) Make Rpc V2 CBOR a compatible protocol for `awsQuery` using `awsQueryCompatible` trait


June 11th, 2025
===============
**Breaking Changes:**
- :bug::warning: (server) Fixed SmithyRpcV2CBOR Router to properly respect case in service names, preventing routing failures for services with mixed-case service shape ID.

**New this release:**
- :bug: (client, [smithy-rs#4165](https://github.com/smithy-lang/smithy-rs/issues/4165)) Fix default supported protocols incorrectly ordered in `ClientProtocolLoader`.


June 3rd, 2025
==============
**New this release:**
- :bug: (client, [aws-sdk-rust#1272](https://github.com/awslabs/aws-sdk-rust/issues/1272)) Fix h2 GoAway errors not being retried by hyper legacy client


May 19th, 2025
==============
**New this release:**
- :tada: (client, [smithy-rs#4135](https://github.com/smithy-lang/smithy-rs/issues/4135)) Introduce a new `repeatedly()` function to `aws-smithy-mocks` sequence builder to build mock rules that behave as an
    infinite sequence.

    ```rust
    let rule = mock!(aws_sdk_s3::Client::get_object)
        .sequence()
        .http_status(503, None)
        .times(2)        // repeat the last output twice before moving onto the next response in the sequence
        .output(|| GetObjectOutput::builder()
            .body(ByteStream::from_static(b"success"))
            .build()
        )
        .repeatedly()    // repeat the last output forever
        .build();
    ```
- :bug: (client, [aws-sdk-rust#1291](https://github.com/awslabs/aws-sdk-rust/issues/1291)) Removing the `optimize_crc32_auto` feature flag from the `crc-fast` dependency of the `aws-smithy-checksums` crate since it was causing build issues for some customers.
- :bug: (client, [smithy-rs#4137](https://github.com/smithy-lang/smithy-rs/issues/4137)) Fix bug with enum codegen

    When the first enum generated has the `@sensitive` trait the opaque type
    underlying the `UnknownVariant` inherits that sensitivity. This means that
    it does not derive `Debug`. Since the module is only generated once this
    causes a problem for non-sensitive enums that rely on the type deriving
    `Debug` so that they can also derive `Debug`. We manually add `Debug` to
    the module so it will always be there since the `UnknownVariant` is not
    modeled and cannot be `@sensitive`.
- :bug: (client, [smithy-rs#4135](https://github.com/smithy-lang/smithy-rs/issues/4135)) fix simple rules behavior with `RuleMode::MatchAny`


May 15th, 2025
==============
**New this release:**
- :bug: (all, [smithy-rs#4132](https://github.com/smithy-lang/smithy-rs/issues/4132)) Smithy unions that contain members named "unknown" will now codegen correctly
- (all, [smithy-rs#4105](https://github.com/smithy-lang/smithy-rs/issues/4105), @FalkWoldmann) Replace once_cell with std equivalents

**Contributors**
Thank you for your contributions! ❤
- @FalkWoldmann ([smithy-rs#4105](https://github.com/smithy-lang/smithy-rs/issues/4105))


May 9th, 2025
=============
**Breaking Changes:**
- :warning: (all, [smithy-rs#4120](https://github.com/smithy-lang/smithy-rs/issues/4120)) Update MSRV to 1.82.0

**New this release:**
- :bug::tada: (client, [smithy-rs#4074](https://github.com/smithy-lang/smithy-rs/issues/4074), [smithy-rs#3926](https://github.com/smithy-lang/smithy-rs/issues/3926)) Promote `aws-smithy-mocks-experimental` to `aws-smithy-mocks`. This crate is now a recommended tool for testing
    generated SDK clients. This release includes several fixes as well as a new sequence builder API that can be
    used to test more complex scenarios such as retries.

    ```rust
    use aws_sdk_s3::operation::get_object::GetObjectOutput;
    use aws_sdk_s3::config::retry::RetryConfig;
    use aws_smithy_types::byte_stream::ByteStream;
    use aws_smithy_mocks::{mock, mock_client, RuleMode};

    #[tokio::test]
    async fn test_retry_behavior() {
        // Create a rule that returns 503 twice, then succeeds
        let retry_rule = mock!(aws_sdk_s3::Client::get_object)
            .sequence()
            .http_status(503, None)
            .times(2)                                            // Return 503 HTTP status twice
            .output(|| GetObjectOutput::builder()                // Finally return a successful output
                .body(ByteStream::from_static(b"success"))
                .build())
            .build();

        // Create a mocked client with the rule
        let s3 = mock_client!(
            aws_sdk_s3,
            RuleMode::Sequential,
            [&retry_rule],
            |client_builder| {
                client_builder.retry_config(RetryConfig::standard().with_max_attempts(3))
            }
        );

        // This should succeed after two retries
        let result = s3
            .get_object()
            .bucket("test-bucket")
            .key("test-key")
            .send()
            .await
            .expect("success after retries");

        // Verify the response
        let data = result.body.collect().await.expect("successful read").to_vec();
        assert_eq!(data, b"success");

        // Verify all responses were used
        assert_eq!(retry_rule.num_calls(), 3);
    }
    ```
- :bug: (all, [smithy-rs#4117](https://github.com/smithy-lang/smithy-rs/issues/4117)) Fix a bug where fields that were initially annotated with the `required` trait and later updated to use the `addedDefault` trait were not serialized when their values matched the default, even when the values were explicitly set. With this fix, fields with `addedDefault` are now always serialized.


May 2nd, 2025
=============

April 23rd, 2025
================
**Breaking Changes:**
- :warning: (client, [smithy-rs#3776](https://github.com/smithy-lang/smithy-rs/issues/3776)) [AuthSchemeId](https://docs.rs/aws-smithy-runtime-api/1.7.4/aws_smithy_runtime_api/client/auth/struct.AuthSchemeId.html) no longer implements the `Copy` trait. This type has primarily been used by the Smithy code generator, so this change is not expected to affect users of SDKs.

**New this release:**
- (all, [smithy-rs#4050](https://github.com/smithy-lang/smithy-rs/issues/4050), @FalkWoldmann) Replace the `once_cell` crate with the `std` counterpart in Smithy runtime crates.
- (client) remove redundant span attributes and improve log output format

**Contributors**
Thank you for your contributions! ❤
- @FalkWoldmann ([smithy-rs#4050](https://github.com/smithy-lang/smithy-rs/issues/4050))


March 27th, 2025
================

March 25th, 2025
================
**New this release:**
- :bug: (client, [smithy-rs#4054](https://github.com/smithy-lang/smithy-rs/issues/4054)) Fix traversal of operations bound to resources in several places including logic to determine if an event stream exists
- (client, [smithy-rs#4052](https://github.com/smithy-lang/smithy-rs/issues/4052)) Update spans to better align with spec.


March 10th, 2025
================
**New this release:**
- (client, [aws-sdk-rust#977](https://github.com/awslabs/aws-sdk-rust/issues/977), [smithy-rs#1925](https://github.com/smithy-lang/smithy-rs/issues/1925), [smithy-rs#3710](https://github.com/smithy-lang/smithy-rs/issues/3710)) Updates the default HTTP client to be based on the 1.x version of hyper and updates the default TLS provider to [rustls](https://github.com/rustls/rustls) with [aws-lc](https://github.com/aws/aws-lc-rs). For more information see the GitHub [discussion](https://github.com/awslabs/aws-sdk-rust/discussions/1257).


March 4th, 2025
===============
**New this release:**
- :tada: (client, [smithy-rs#121](https://github.com/smithy-lang/smithy-rs/issues/121)) Adds support for event stream operations with non-REST protocols such as RPC v2 CBOR.


February 20th, 2025
===================
**New this release:**
- :bug: (server) Fixed code generation failure that occurred when using `Result` as a shape name in Smithy models with constrained members by properly handling naming conflicts with Rust's built-in Result type
- :bug: (server) Previously, models would fail to generate when both the list and at least one of its members was directly constrained with documentation comments


February 12th, 2025
===================

February 3rd, 2025
==================

January 28th, 2025
==================

January 23rd, 2025
==================

January 17th, 2025
==================

January 14th, 2025
==================
**New this release:**
- :bug::tada: (client, [smithy-rs#3845](https://github.com/smithy-lang/smithy-rs/issues/3845)) S3 client behavior is updated to always calculate a checksum by default for operations that support it (such as PutObject or UploadPart), or require it (such as DeleteObjects). The default checksum algorithm is CRC32. Checksum behavior can be configured using `when_supported` and `when_required` options - in shared config using request_checksum_calculation, or as env variable using AWS_REQUEST_CHECKSUM_CALCULATION.

    The S3 client attempts to validate response checksums for all S3 API operations that support checksums. However, if the SDK has not implemented the specified checksum algorithm then this validation is skipped. Checksum validation behavior can be configured using `when_supported` and `when_required` options - in shared config using response_checksum_validation, or as env variable using AWS_RESPONSE_CHECKSUM_VALIDATION.
- :bug::tada: (client, [smithy-rs#3967](https://github.com/smithy-lang/smithy-rs/issues/3967)) Updates client generation to conform with Smithy's updates to the [httpChecksum trait](https://smithy.io/2.0/aws/aws-core.html#aws-protocols-httpchecksum-trait).
- :bug: (client, [aws-sdk-rust#1234](https://github.com/awslabs/aws-sdk-rust/issues/1234)) Fix token bucket not being set for standard and adaptive retry modes


December 30th, 2024
===================

December 26th, 2024
===================
**New this release:**
- :bug: (server, [smithy-rs#3890](https://github.com/smithy-lang/smithy-rs/issues/3890)) Fix bug in `serde` decorator that generated non-compiling code on some models


December 16th, 2024
===================

December 3rd, 2024
==================
**Breaking Changes:**
- :bug::warning: (server, [smithy-rs#3880](https://github.com/smithy-lang/smithy-rs/issues/3880)) Unnamed enums now validate assigned values and will raise a `ConstraintViolation` if an unknown variant is set.

    The following is an example of an unnamed enum:
    ```smithy
    @enum([
        { value: "MONDAY" },
        { value: "TUESDAY" }
    ])
    string UnnamedDayOfWeek
    ```


November 5th, 2024
==================

October 30th, 2024
==================

October 24th, 2024
==================

October 9th, 2024
=================
**New this release:**
- :bug: (client, [smithy-rs#3871](https://github.com/smithy-lang/smithy-rs/issues/3871), [aws-sdk-rust#1202](https://github.com/awslabs/aws-sdk-rust/issues/1202)) Fix minimum throughput detection for downloads to avoid incorrectly raising an error while the user is consuming data at a slow but steady pace.


October 5th, 2024
=================
**New this release:**
- :bug: (client, [smithy-rs#3852](https://github.com/smithy-lang/smithy-rs/issues/3852)) Fix AWS SDK generation examples in README in the `aws/sdk` directory.


October 4th, 2024
=================

October 3rd, 2024
=================
**Breaking Changes:**
- :warning: (server) The generated crates no longer have the `aws-lambda` feature flag enabled by default. This prevents the [aws-lambda](https://docs.rs/crate/aws-smithy-http-server/0.63.3/features#aws-lambda) feature from being automatically enabled in [aws-smithy-http-server](https://docs.rs/aws-smithy-http-server/0.63.3/aws_smithy_http_server/) when the SDK is not intended for AWS Lambda.

**New this release:**
- :tada: (server) All relevant types from [aws-smithy-http-server](https://docs.rs/aws-smithy-http-server/0.63.3/aws_smithy_http_server/) are now re-exported within the generated crates. This removes the need to explicitly depend on [aws-smithy-http-server](https://docs.rs/aws-smithy-http-server/0.63.3/aws_smithy_http_server/) in service handler code and prevents compilation errors caused by version mismatches.

- :tada: (all, [smithy-rs#3573](https://github.com/smithy-lang/smithy-rs/issues/3573)) Support for the [rpcv2Cbor](https://smithy.io/2.0/additional-specs/protocols/smithy-rpc-v2.html) protocol has been added, allowing services to serialize RPC payloads as CBOR (Concise Binary Object Representation), improving performance and efficiency in data transmission.


September 26th, 2024
====================
**New this release:**
- :bug: (client, [smithy-rs#3820](https://github.com/smithy-lang/smithy-rs/issues/3820)) Fixed a bug with the content length of compressed payloads that caused such requests to hang.


September 17th, 2024
====================

September 9th, 2024
===================
**Breaking Changes:**
- :bug::warning: (server, [smithy-rs#3813](https://github.com/smithy-lang/smithy-rs/issues/3813)) Operations with event stream member shapes must include `ValidationException` in the errors list. This is necessary because the member shape is a required field, and the builder for the operation input or output returns a `std::result::Result` with the error set to `crate::model::ValidationExceptionField`.

**New this release:**
- :tada: (server, [smithy-rs#3803](https://github.com/smithy-lang/smithy-rs/issues/3803)) Setting the `addValidationExceptionToConstrainedOperations` codegen flag adds `aws.smithy.framework#ValidationException` to operations with constrained inputs that do not already have this exception added.

    Sample `smithy-build-template.json`:

    ```
    {
        "...",
        "plugins": {
            "rust-server-codegen": {
                "service": "ServiceToGenerateSDKFor",
                    "module": "amzn-sample-server-sdk",
                    "codegen": {
                        "addValidationExceptionToConstrainedOperations": true,
                    }
            }
        }
    }
    ```
- :bug: (all, [smithy-rs#3805](https://github.com/smithy-lang/smithy-rs/issues/3805)) Fix bug in `DateTime::from_secs_f64` where certain floating point values could lead to a panic.


August 28th, 2024
=================
**Breaking Changes:**
- :warning: (all, [smithy-rs#3800](https://github.com/smithy-lang/smithy-rs/issues/3800)) Upgrade MSRV to Rust 1.78.0.

**New this release:**
- :bug: (client, [smithy-rs#3798](https://github.com/smithy-lang/smithy-rs/issues/3798)) Fix the execution order of [modify_before_serialization](https://docs.rs/aws-smithy-runtime-api/latest/aws_smithy_runtime_api/client/interceptors/trait.Intercept.html#method.modify_before_serialization) and [read_before_serialization](https://docs.rs/aws-smithy-runtime-api/latest/aws_smithy_runtime_api/client/interceptors/trait.Intercept.html#method.read_before_serialization) in the orchestrator. The `modify_before_serialization` method now executes before the `read_before_serialization` method. This adjustment may result in changes in behavior depending on how you customize interceptors.
- (client, [smithy-rs#1925](https://github.com/smithy-lang/smithy-rs/issues/1925)) Backport connection poisoning to hyper 1.x support
- :bug: (client, [aws-sdk-rust#821](https://github.com/awslabs/aws-sdk-rust/issues/821), [smithy-rs#3797](https://github.com/smithy-lang/smithy-rs/issues/3797)) Fix the [Length::UpTo](https://docs.rs/aws-smithy-types/1.2.2/aws_smithy_types/byte_stream/enum.Length.html) usage in [FsBuilder](https://docs.rs/aws-smithy-types/1.2.2/aws_smithy_types/byte_stream/struct.FsBuilder.html), ensuring that the specified length does not exceed the remaining file length.
- :bug: (client, [aws-sdk-rust#820](https://github.com/awslabs/aws-sdk-rust/issues/820)) Re-export `ByteStream`'s `Length` and `FsBuilder`. By making these types available directly within a client crate, customers can use `ByteStream::read_from` without needing to import them separately from the `aws-smithy-types` crate.


August 16th, 2024
=================

August 14th, 2024
=================

August 8th, 2024
================
**New this release:**
- :bug: (client, [smithy-rs#3767](https://github.com/smithy-lang/smithy-rs/issues/3767)) Fix client error correction to properly parse structure members that target a `Union` containing that structure recursively.
- :bug: (client, [smithy-rs#3765](https://github.com/smithy-lang/smithy-rs/issues/3765), [smithy-rs#3757](https://github.com/smithy-lang/smithy-rs/issues/3757)) Fix incorrect redaction of `@sensitive` types in maps and lists.
- (client, [smithy-rs#3779](https://github.com/smithy-lang/smithy-rs/issues/3779)) Improve error messaging when HTTP headers aren't valid UTF-8


July 16th, 2024
===============
**New this release:**
- (client, [smithy-rs#3742](https://github.com/smithy-lang/smithy-rs/issues/3742)) Support `stringArray` type in endpoints params
- (client, [smithy-rs#3755](https://github.com/smithy-lang/smithy-rs/issues/3755)) Add support for `operationContextParams` Endpoints trait
- (client, [smithy-rs#3591](https://github.com/smithy-lang/smithy-rs/issues/3591)) `aws_smithy_runtime_api::client::orchestrator::HttpRequest` and `aws_smithy_runtime_api::client::orchestrator::HttpResponse` are now re-exported in generated clients so that using these types does not require directly depending on `aws-smithy-runtime-api`.


July 9th, 2024
==============
**Breaking Changes:**
- :warning: (server, [smithy-rs#3746](https://github.com/smithy-lang/smithy-rs/issues/3746)) `FromParts<Protocol>::Rejection` must implement `std::fmt::Display`.

    Handlers can accept user-defined types if they implement 
    [FromParts<Protocol>](https://docs.rs/aws-smithy-http-server/latest/aws_smithy_http_server/request/trait.FromParts.html) with a `Rejection` 
    type that implements `std::fmt::Display` (preferably `std::error::Error`) to enable error logging when parameter construction from request parts fails.

    See the [changelog discussion for futher details](https://github.com/smithy-lang/smithy-rs/discussions/3749).

**New this release:**
- (client, [smithy-rs#3742](https://github.com/smithy-lang/smithy-rs/issues/3742)) Support `stringArray` type in endpoints params
- :bug: (client, [smithy-rs#3744](https://github.com/smithy-lang/smithy-rs/issues/3744)) Fix bug where stalled stream protection would panic with an underflow if the first event was logged too soon.


July 3rd, 2024
==============
**New this release:**
- :bug: (server, [smithy-rs#3643](https://github.com/smithy-lang/smithy-rs/issues/3643)) A feature, `aws-lambda`, has been added to generated SDKs to re-export types required for Lambda deployment.
- :bug: (server, [smithy-rs#3471](https://github.com/smithy-lang/smithy-rs/issues/3471), [smithy-rs#3724](https://github.com/smithy-lang/smithy-rs/issues/3724), @djedward) Content-Type header validation now ignores parameter portion of media types.

**Contributors**
Thank you for your contributions! ❤
- @djedward ([smithy-rs#3471](https://github.com/smithy-lang/smithy-rs/issues/3471), [smithy-rs#3724](https://github.com/smithy-lang/smithy-rs/issues/3724))


June 19th, 2024
===============
**Breaking Changes:**
- :bug::warning: (server, [smithy-rs#3690](https://github.com/smithy-lang/smithy-rs/issues/3690)) Fix request `Content-Type` header checking

    Two bugs related to how servers were checking the `Content-Type` header in incoming requests have been fixed:

    1. `Content-Type` header checking was incorrectly succeeding when no `Content-Type` header was present but one was expected.
    2. When a shape was @httpPayload`-bound, `Content-Type` header checking occurred even when no payload was being sent. In this case it is not necessary to check the header, since there is no content.

    This is a breaking change in that servers are now stricter at enforcing the expected `Content-Type` header is being sent by the client in general, and laxer when the shape is bound with `@httpPayload`.


June 17th, 2024
===============

June 12th, 2024
===============

June 10th, 2024
===============
**New this release:**
- (all, [smithy-rs#1925](https://github.com/smithy-lang/smithy-rs/issues/1925), [smithy-rs#3673](https://github.com/smithy-lang/smithy-rs/issues/3673)) Add support for v1 `http_body::Body` to `aws_smithy_types::byte_stream::bytestream_util::PathBody`.
- (all, [smithy-rs#3637](https://github.com/smithy-lang/smithy-rs/issues/3637), @khuey) Add conversions from smithy StatusCode to http StatusCode.
- :bug: (client, [smithy-rs#3675](https://github.com/smithy-lang/smithy-rs/issues/3675), @dastrom) Enable aws-smithy-runtime to compile in rustc 1.72.1

**Contributors**
Thank you for your contributions! ❤
- @dastrom ([smithy-rs#3675](https://github.com/smithy-lang/smithy-rs/issues/3675))
- @khuey ([smithy-rs#3637](https://github.com/smithy-lang/smithy-rs/issues/3637))


June 3rd, 2024
==============
**New this release:**
- (client, [smithy-rs#3664](https://github.com/smithy-lang/smithy-rs/issues/3664)) Reduce verbosity of various debug logs


May 28th, 2024
==============

May 22nd, 2024
==============
**New this release:**
- :bug: (client, [smithy-rs#3656](https://github.com/smithy-lang/smithy-rs/issues/3656), [smithy-rs#3657](https://github.com/smithy-lang/smithy-rs/issues/3657)) Fix the Content-Length enforcement so it is only applied to GET requests.


May 21st, 2024
==============
**Breaking Changes:**
- :warning::tada: (all, [smithy-rs#3653](https://github.com/smithy-lang/smithy-rs/issues/3653)) Update MSRV to `1.76.0`

**New this release:**
- :tada: (client, [smithy-rs#2891](https://github.com/smithy-lang/smithy-rs/issues/2891)) Compression is now supported for operations modeled with the `@requestCompression` trait.

    [**For more details, see the long-form changelog discussion**](https://github.com/smithy-lang/smithy-rs/discussions/3646).
- :bug: (client, [aws-sdk-rust#1133](https://github.com/awslabs/aws-sdk-rust/issues/1133)) Fix panics that occurred when `Duration` for exponential backoff could not be created from too big a float.
- :bug: (all, [smithy-rs#3491](https://github.com/smithy-lang/smithy-rs/issues/3491), [aws-sdk-rust#1079](https://github.com/awslabs/aws-sdk-rust/issues/1079)) Clients now enforce that the Content-Length sent by the server matches the length of the returned response body. In most cases, Hyper will enforce this behavior, however, in extremely rare circumstances where the Tokio runtime is dropped in between subsequent requests, this scenario can occur.
- :bug: (all, [aws-sdk-rust#1141](https://github.com/awslabs/aws-sdk-rust/issues/1141), [aws-sdk-rust#1146](https://github.com/awslabs/aws-sdk-rust/issues/1146), [aws-sdk-rust#1148](https://github.com/awslabs/aws-sdk-rust/issues/1148)) Fixes stalled upload stream protection to not apply to empty request bodies and to stop checking for violations once the request body has been read.


May 8th, 2024
=============
**Breaking Changes:**
- :warning::tada: (all, [smithy-rs#3527](https://github.com/smithy-lang/smithy-rs/issues/3527)) Stalled stream protection on uploads is now enabled by default behind `BehaviorVersion::v2024_03_28()`. If you're using `BehaviorVersion::latest()`, you will get this change automatically by running `cargo update`.

**New this release:**
- (all, [smithy-rs#3161](https://github.com/smithy-lang/smithy-rs/issues/3161), @mnissenb) Implement Debug for DateTime

**Contributors**
Thank you for your contributions! ❤
- @mnissenb ([smithy-rs#3161](https://github.com/smithy-lang/smithy-rs/issues/3161))


April 30th, 2024
================
**New this release:**
- :tada: (client, [smithy-rs#119](https://github.com/smithy-lang/smithy-rs/issues/119), [smithy-rs#3595](https://github.com/smithy-lang/smithy-rs/issues/3595), [smithy-rs#3593](https://github.com/smithy-lang/smithy-rs/issues/3593), [smithy-rs#3585](https://github.com/smithy-lang/smithy-rs/issues/3585), [smithy-rs#3571](https://github.com/smithy-lang/smithy-rs/issues/3571), [smithy-rs#3569](https://github.com/smithy-lang/smithy-rs/issues/3569)) Added support for waiters. Services that model waiters now have a `Waiters` trait that adds
    some methods prefixed with `wait_until` to the existing clients.

    For example, if there was a waiter modeled for "thing" that takes a "thing ID", using
    that waiter would look as follows:

    ```rust
    use my_generated_client::client::Waiters;

    let result = client.wait_until_thing()
        .thing_id("someId")
        .wait(Duration::from_secs(120))
        .await;
    ```
- :bug: (all, [smithy-rs#3603](https://github.com/smithy-lang/smithy-rs/issues/3603)) Fix event stream `:content-type` message headers for struct messages. Note: this was the `:content-type` header on individual event message frames that was incorrect, not the HTTP `content-type` header for the initial request.


April 19th, 2024
================
**New this release:**
- :tada: (server, [smithy-rs#3430](https://github.com/smithy-lang/smithy-rs/issues/3430)) Implement `std::error::Error` for `ConstraintViolation`
- (all, [smithy-rs#3553](https://github.com/smithy-lang/smithy-rs/issues/3553)) Upgraded MSRV to Rust 1.75


April 11th, 2024
================
**New this release:**
- :tada: (all, [smithy-rs#3485](https://github.com/smithy-lang/smithy-rs/issues/3485)) Stalled stream protection now supports request upload streams. It is currently off by default, but will be enabled by default in a future release. To enable it now, you can do the following:

    ```rust
    let config = my_service::Config::builder()
        .stalled_stream_protection(StalledStreamProtectionConfig::enabled().build())
        // ...
        .build();
    ```
- :bug: (all, [smithy-rs#3427](https://github.com/smithy-lang/smithy-rs/issues/3427)) `SharedIdentityResolver` now respects an existing cache partition when the `ResolveIdentity` implementation
    provides one already.
- :bug: (all, [smithy-rs#3485](https://github.com/smithy-lang/smithy-rs/issues/3485)) Stalled stream protection on downloads will now only trigger if the upstream source is too slow. Previously, stalled stream protection could be erroneously triggered if the user was slowly consuming the stream slower than the minimum speed limit.
- :bug: (all, [smithy-rs#2546](https://github.com/smithy-lang/smithy-rs/issues/2546)) Unions with unit target member shape are now fully supported


April 2nd, 2024
===============
**Breaking Changes:**
- :bug::warning: (client, [aws-sdk-rust#1111](https://github.com/awslabs/aws-sdk-rust/issues/1111), [smithy-rs#3513](https://github.com/smithy-lang/smithy-rs/issues/3513), @Ten0) Make `BehaviorVersion` be future-proof by disallowing it to be constructed via the `BehaviorVersion {}` syntax.

**New this release:**
- :tada: (all, [smithy-rs#3539](https://github.com/smithy-lang/smithy-rs/issues/3539)) Add FIPS support to our Hyper 1.0-based client. Customers can enable this mode by enabling the `crypto-aws-lc-fips` on `aws-smithy-experimental`. To construct a client using the new client, consult this [example](https://github.com/awslabs/aws-sdk-rust/blob/release-2024-03-29/sdk/s3/tests/hyper-10.rs).

    Please note that support for Hyper 1.0 remains experimental.
- (all, [smithy-rs#3389](https://github.com/smithy-lang/smithy-rs/issues/3389)) All requests are now retryable, even if they are deserialized successfully. Previously, this was not allowed.
- (all, [smithy-rs#3539](https://github.com/smithy-lang/smithy-rs/issues/3539)) Fix bug in Hyper 1.0 support where https URLs returned an error

**Contributors**
Thank you for your contributions! ❤
- @Ten0 ([aws-sdk-rust#1111](https://github.com/awslabs/aws-sdk-rust/issues/1111), [smithy-rs#3513](https://github.com/smithy-lang/smithy-rs/issues/3513))


March 25th, 2024
================
**New this release:**
- (all, [smithy-rs#3476](https://github.com/smithy-lang/smithy-rs/issues/3476), @landonxjames) Increased minimum version of wasi crate dependency in aws-smithy-wasm to 0.12.1.

**Contributors**
Thank you for your contributions! ❤
- @landonxjames ([smithy-rs#3476](https://github.com/smithy-lang/smithy-rs/issues/3476))


March 12th, 2024
================
**New this release:**
- :tada: (all, [smithy-rs#2087](https://github.com/smithy-lang/smithy-rs/issues/2087), [smithy-rs#2520](https://github.com/smithy-lang/smithy-rs/issues/2520), [smithy-rs#3409](https://github.com/smithy-lang/smithy-rs/issues/3409), [aws-sdk-rust#59](https://github.com/awslabs/aws-sdk-rust/issues/59), @landonxjames, @eduardomourar) Added aws-smithy-wasm crate to enable SDK use in WASI compliant environments
- :tada: (client, [smithy-rs#2087](https://github.com/smithy-lang/smithy-rs/issues/2087), [smithy-rs#2520](https://github.com/smithy-lang/smithy-rs/issues/2520), [smithy-rs#3409](https://github.com/smithy-lang/smithy-rs/issues/3409), @landonxjames, @eduardomourar) Added aws-smithy-wasm crate to enable SDK use in WASI compliant environments
- :tada: (all, [smithy-rs#3365](https://github.com/smithy-lang/smithy-rs/issues/3365), [aws-sdk-rust#1046](https://github.com/awslabs/aws-sdk-rust/issues/1046), @cayman-amzn) [`SdkBody`](https://docs.rs/aws-smithy-types/latest/aws_smithy_types/body/struct.SdkBody.html) now implements the 1.0 version of the `http_body::Body` trait.
- (all, [smithy-rs#3470](https://github.com/smithy-lang/smithy-rs/issues/3470)) Upgrade Smithy to 1.45.
- (client, [smithy-rs#3465](https://github.com/smithy-lang/smithy-rs/issues/3465), [smithy-rs#3477](https://github.com/smithy-lang/smithy-rs/issues/3477)) The `ResolveIdentity` trait is now aware of its `IdentityCache` location.
- (client, [smithy-rs#3465](https://github.com/smithy-lang/smithy-rs/issues/3465), [smithy-rs#3477](https://github.com/smithy-lang/smithy-rs/issues/3477)) `RuntimeComponents` can now be converted back to a `RuntimeComponentsBuilder`, using `.to_builder()`.

**Contributors**
Thank you for your contributions! ❤
- @cayman-amzn ([aws-sdk-rust#1046](https://github.com/awslabs/aws-sdk-rust/issues/1046), [smithy-rs#3365](https://github.com/smithy-lang/smithy-rs/issues/3365))
- @eduardomourar ([aws-sdk-rust#59](https://github.com/awslabs/aws-sdk-rust/issues/59), [smithy-rs#2087](https://github.com/smithy-lang/smithy-rs/issues/2087), [smithy-rs#2520](https://github.com/smithy-lang/smithy-rs/issues/2520), [smithy-rs#3409](https://github.com/smithy-lang/smithy-rs/issues/3409))
- @landonxjames ([aws-sdk-rust#59](https://github.com/awslabs/aws-sdk-rust/issues/59), [smithy-rs#2087](https://github.com/smithy-lang/smithy-rs/issues/2087), [smithy-rs#2520](https://github.com/smithy-lang/smithy-rs/issues/2520), [smithy-rs#3409](https://github.com/smithy-lang/smithy-rs/issues/3409))


February 22nd, 2024
===================
**New this release:**
- (all, [smithy-rs#3410](https://github.com/smithy-lang/smithy-rs/issues/3410)) The MSRV has been increase to 1.74.1


February 15th, 2024
===================
**Breaking Changes:**
- :bug::warning: (client, [smithy-rs#3405](https://github.com/smithy-lang/smithy-rs/issues/3405), [smithy-rs#3400](https://github.com/smithy-lang/smithy-rs/issues/3400), [smithy-rs#3258](https://github.com/smithy-lang/smithy-rs/issues/3258)) Fix bug where timeout settings where not merged properly. This will add a default connect timeout of 3.1s seconds for most clients.

    [**For more details see the long-form changelog discussion**](https://github.com/smithy-lang/smithy-rs/discussions/3408).

**New this release:**
- (all, [aws-sdk-rust#977](https://github.com/awslabs/aws-sdk-rust/issues/977), [smithy-rs#3365](https://github.com/smithy-lang/smithy-rs/issues/3365), [smithy-rs#3373](https://github.com/smithy-lang/smithy-rs/issues/3373)) Add `try_into_http1x` and `try_from_http1x` to Request and Response container types.
- (client, [smithy-rs#3336](https://github.com/smithy-lang/smithy-rs/issues/3336), [smithy-rs#3391](https://github.com/smithy-lang/smithy-rs/issues/3391), @iampkmone) Added impl `Display` to Enums.
- :bug: (all, [smithy-rs#3322](https://github.com/smithy-lang/smithy-rs/issues/3322)) Retry classifiers will now be sorted by priority. This change only affects requests
    that are retried. Some requests that were previously been classified as transient
    errors may now be classified as throttling errors.

    If you were

    - configuring multiple custom retry classifiers
    - that would disagree on how to classify a response
    - that have differing priorities

    you may see a behavior change in that classification for the same response is now
    dependent on the classifier priority instead of the order in which the classifier
    was added.
- :bug: (client, [smithy-rs#3402](https://github.com/smithy-lang/smithy-rs/issues/3402)) Cap the maximum jitter fraction for identity cache refresh buffer time to 0.5. It was previously 1.0, and if the fraction was randomly set to 1.0, it was equivalent to disregarding the buffer time for cache refresh.

**Contributors**
Thank you for your contributions! ❤
- @iampkmone ([smithy-rs#3336](https://github.com/smithy-lang/smithy-rs/issues/3336), [smithy-rs#3391](https://github.com/smithy-lang/smithy-rs/issues/3391))


February 8th, 2024
==================

January 24th, 2024
==================

January 18th, 2024
==================
**New this release:**
- (client, [smithy-rs#3318](https://github.com/smithy-lang/smithy-rs/issues/3318)) `EndpointPrefix` and `apply_endpoint` moved from aws-smithy-http to aws-smithy-runtime-api so that is in a stable (1.x) crate. A deprecated type alias was left in place with a note showing the new location.
- (client, [smithy-rs#3325](https://github.com/smithy-lang/smithy-rs/issues/3325)) The `Metadata` storable was moved from aws_smithy_http into aws_smithy_runtime_api. A deprecated type alias was left in place with a note showing where the new location is.


January 10th, 2024
==================
**New this release:**
- :tada: (all, [smithy-rs#3300](https://github.com/smithy-lang/smithy-rs/issues/3300), [aws-sdk-rust#977](https://github.com/awslabs/aws-sdk-rust/issues/977)) Add support for constructing [`SdkBody`] and [`ByteStream`] from `http-body` 1.0 bodies. Note that this is initial support and works via a backwards compatibility shim to http-body 0.4. Hyper 1.0 is not supported.
- :tada: (all, [smithy-rs#3333](https://github.com/smithy-lang/smithy-rs/issues/3333), [aws-sdk-rust#998](https://github.com/awslabs/aws-sdk-rust/issues/998), [aws-sdk-rust#1010](https://github.com/awslabs/aws-sdk-rust/issues/1010)) Add `as_service_err()` to `SdkError` to allow checking the type of an error is without taking ownership.
- (client, [smithy-rs#3299](https://github.com/smithy-lang/smithy-rs/issues/3299), @Ploppz)  Add `PaginationStreamExt` extension trait to `aws-smithy-types-convert` behind the `convert-streams` feature. This makes it possible to treat a paginator as a [`futures_core::Stream`](https://docs.rs/futures-core/latest/futures_core/stream/trait.Stream.html), allowing customers to use stream combinators like [`map`](https://docs.rs/tokio-stream/latest/tokio_stream/trait.StreamExt.html#method.map) and [`filter`](https://docs.rs/tokio-stream/latest/tokio_stream/trait.StreamExt.html#method.filter).

    Example:

    ```rust
    use aws_smithy_types_convert::stream::PaginationStreamExt
    let stream = s3_client.list_objects_v2().bucket("...").into_paginator().send().into_stream_03x();
    ```
- :bug: (client, [smithy-rs#3252](https://github.com/smithy-lang/smithy-rs/issues/3252), [smithy-rs#3312](https://github.com/smithy-lang/smithy-rs/issues/3312), @milesziemer) Serialize 0/false in query parameters, and ignore actual default value during serialization instead of just 0/false. See [changelog discussion](https://github.com/smithy-lang/smithy-rs/discussions/3312) for details.
- (all, [smithy-rs#3292](https://github.com/smithy-lang/smithy-rs/issues/3292)) `requireEndpointResolver: false` is no longer required to remove the need for an endpoint resolver. Instead, `"awsSdkBuilder"` (default false), now _removes_ that requirement.

**Contributors**
Thank you for your contributions! ❤
- @Ploppz ([smithy-rs#3299](https://github.com/smithy-lang/smithy-rs/issues/3299))
- @milesziemer ([smithy-rs#3252](https://github.com/smithy-lang/smithy-rs/issues/3252), [smithy-rs#3312](https://github.com/smithy-lang/smithy-rs/issues/3312))


December 13th, 2023
===================

December 11th, 2023
===================
**New this release:**
- :bug: (client, [smithy-rs#3305](https://github.com/smithy-lang/smithy-rs/issues/3305)) `crate::event_receiver::EventReceiver` is now re-exported as `crate::primitives::event_stream::EventReceiver` when a service supports event stream operations.


December 8th, 2023
==================
**New this release:**
- :tada: (all, [smithy-rs#3121](https://github.com/smithy-lang/smithy-rs/issues/3121), [smithy-rs#3295](https://github.com/smithy-lang/smithy-rs/issues/3295)) All generated docs now include docsrs labels when features are required
- :bug: (client, [smithy-rs#3262](https://github.com/smithy-lang/smithy-rs/issues/3262)) Loading native TLS trusted certs for the default HTTP client now only occurs if the default HTTP client is not overridden in config.
- (client, [smithy-rs#3277](https://github.com/smithy-lang/smithy-rs/issues/3277)) Improve the error messages for when auth fails to select an auth scheme for a request.
- (client, [smithy-rs#3282](https://github.com/smithy-lang/smithy-rs/issues/3282)) Fix documentation and examples on HyperConnector and HyperClientBuilder.
- (client, [aws-sdk-rust#990](https://github.com/awslabs/aws-sdk-rust/issues/990), @declanvk) Expose local socket address from ConnectionMetadata.
- (all, [smithy-rs#3294](https://github.com/smithy-lang/smithy-rs/issues/3294)) [`Number`](https://docs.rs/aws-smithy-types/latest/aws_smithy_types/enum.Number.html) `TryInto` implementations now succesfully convert from `f64` to numeric types when no precision is lost. This fixes some deserialization issues where numbers like `25.0` were sent when `Byte` fields were expected.

**Contributors**
Thank you for your contributions! ❤
- @declanvk ([aws-sdk-rust#990](https://github.com/awslabs/aws-sdk-rust/issues/990))


December 1st, 2023
==================
**New this release:**
- (client, [smithy-rs#3278](https://github.com/smithy-lang/smithy-rs/issues/3278)) `RuntimeComponentsBuilder::push_identity_resolver` is now deprecated since it does not replace the existing identity resolver of a given auth scheme ID. Use `RuntimeComponentsBuilder::set_identity_resolver` instead.


November 27th, 2023
===================
**New this release:**
- (client, [aws-sdk-rust#738](https://github.com/awslabs/aws-sdk-rust/issues/738), [aws-sdk-rust#858](https://github.com/awslabs/aws-sdk-rust/issues/858)) Retry additional classes of H2 errors (H2 GoAway & H2 ResetStream)


November 26th, 2023
===================

November 25th, 2023
===================

November 21st, 2023
===================
**Internal changes only with this release**


November 17th, 2023
===================
**Breaking Changes:**
- :warning::tada: (client, [smithy-rs#3202](https://github.com/smithy-lang/smithy-rs/issues/3202)) Add configurable stalled-stream protection for downloads.

    When making HTTP calls,
    it's possible for a connection to 'stall out' and emit no more data due to server-side issues.
    In the event this happens, it's desirable for the stream to error out as quickly as possible.
    While timeouts can protect you from this issue, they aren't adaptive to the amount of data
    being sent and so must be configured specifically for each use case. When enabled, stalled-stream
    protection will ensure that bad streams error out quickly, regardless of the amount of data being
    downloaded.

    Protection is enabled by default for all clients but can be configured or disabled.
    See [this discussion](https://github.com/awslabs/aws-sdk-rust/discussions/956) for more details.
- :warning: (client, [smithy-rs#3222](https://github.com/smithy-lang/smithy-rs/issues/3222)) Types/functions that were deprecated in previous releases were removed. Unfortunately, some of these deprecations
    were ignored by the Rust compiler (we found out later that `#[deprecated]` on `pub use` doesn't work). See
    the [deprecations removal list](https://github.com/smithy-lang/smithy-rs/discussions/3223) for more details.
- :warning: (all, [smithy-rs#3236](https://github.com/smithy-lang/smithy-rs/issues/3236)) Conversions for HTTP request in aws-smithy-runtime-api are now feature gated behind the `http-02x` feature

**New this release:**
- :tada: (all, [smithy-rs#3183](https://github.com/smithy-lang/smithy-rs/issues/3183), @HakanVardarr) Add `Display` impl for `DateTime`.
- :bug: (client, [smithy-rs#3229](https://github.com/smithy-lang/smithy-rs/issues/3229), [aws-sdk-rust#960](https://github.com/awslabs/aws-sdk-rust/issues/960)) Prevent multiplication overflow in backoff computation
- (client, [smithy-rs#3226](https://github.com/smithy-lang/smithy-rs/issues/3226)) Types/functions that were previously `#[doc(hidden)]` in `aws-smithy-async`, `aws-smithy-runtime-api`, `aws-smithy-runtime`, `aws-smithy-types`, and the SDK crates are now visible. For those that are not intended to be used directly, they are called out in their docs as such.

**Contributors**
Thank you for your contributions! ❤
- @HakanVardarr ([smithy-rs#3183](https://github.com/smithy-lang/smithy-rs/issues/3183))


November 16th, 2023
===================
**Breaking Changes:**
- :warning: (client, [smithy-rs#3205](https://github.com/smithy-lang/smithy-rs/issues/3205)) SignableRequest::apply_to_request in aws_sigv4 has been renamed `apply_to_request_http0x`


November 15th, 2023
===================
**Breaking Changes:**
- :warning: (all, [smithy-rs#3138](https://github.com/smithy-lang/smithy-rs/issues/3138), [smithy-rs#3148](https://github.com/smithy-lang/smithy-rs/issues/3148)) [Upgrade guidance for HTTP Request/Response changes](https://github.com/awslabs/smithy-rs/discussions/3154). HTTP request types moved, and a new HTTP response type was added.
- :warning: (all, [smithy-rs#3139](https://github.com/smithy-lang/smithy-rs/issues/3139)) `Message`, `Header`, `HeaderValue`, and `StrBytes` have been moved to `aws-smithy-types` from `aws-smithy-eventstream`. `Message::read_from` and `Message::write_to` remain in `aws-smithy-eventstream` but they are converted to free functions with the names `read_message_from` and `write_message_to` respectively.
- :warning: (client, [smithy-rs#3100](https://github.com/smithy-lang/smithy-rs/issues/3100), [smithy-rs#3114](https://github.com/smithy-lang/smithy-rs/issues/3114)) An operation output that supports receiving events from stream now provides a new-type wrapping `aws_smithy_http::event_stream::receiver::Receiver`. The new-type supports the `.recv()` method whose signature is the same as [`aws_smithy_http::event_stream::receiver::Receiver::recv`](https://docs.rs/aws-smithy-http/0.57.0/aws_smithy_http/event_stream/struct.Receiver.html#method.recv).
- :warning: (all, [smithy-rs#3151](https://github.com/smithy-lang/smithy-rs/issues/3151)) Clients now require a `BehaviorVersion` to be provided. For must customers, `latest` is the best choice. This will be enabled automatically if you enable the `behavior-version-latest` cargo feature on `aws-config` or on an SDK crate. For customers that wish to pin to a specific behavior major version, it can be set in `aws-config` or when constructing the service client.

    ```rust
    async fn example() {
        // when creating a client
        let client = my_service::Client::from_conf(my_service::Config::builder().behavior_version(..).<other params>.build());
    }
    ```
- :warning: (client, [smithy-rs#3189](https://github.com/smithy-lang/smithy-rs/issues/3189)) Remove deprecated error kind type aliases.
- :warning: (client, [smithy-rs#3191](https://github.com/smithy-lang/smithy-rs/issues/3191)) Unhandled errors have been made opaque to ensure code is written in a future-proof manner. Where previously, you
    might have:
    ```rust
    match service_error.err() {
        GetStorageError::StorageAccessNotAuthorized(_) => { /* ... */ }
        GetStorageError::Unhandled(unhandled) if unhandled.code() == Some("SomeUnmodeledErrorCode") {
            // unhandled error handling
        }
        _ => { /* ... */ }
    }
    ```
    It should now look as follows:
    ```rust
    match service_error.err() {
        GetStorageError::StorageAccessNotAuthorized(_) => { /* ... */ }
        err if err.code() == Some("SomeUnmodeledErrorCode") {
            // unhandled error handling
        }
        _ => { /* ... */ }
    }
    ```
    The `Unhandled` variant should never be referenced directly.

**New this release:**
- :tada: (client, [aws-sdk-rust#780](https://github.com/awslabs/aws-sdk-rust/issues/780), [smithy-rs#3189](https://github.com/smithy-lang/smithy-rs/issues/3189)) Add `ProvideErrorMetadata` impl for service `Error` type.
- :bug: (client, [smithy-rs#3182](https://github.com/smithy-lang/smithy-rs/issues/3182), @codypenta) Fix rendering of @error structs when fields have default values

**Contributors**
Thank you for your contributions! ❤
- @codypenta ([smithy-rs#3182](https://github.com/smithy-lang/smithy-rs/issues/3182))


November 1st, 2023
==================
**New this release:**
- (client, [smithy-rs#3112](https://github.com/smithy-lang/smithy-rs/issues/3112), [smithy-rs#3116](https://github.com/smithy-lang/smithy-rs/issues/3116)) Upgrade `ring` to 0.17.5.


October 31st, 2023
==================
**Breaking Changes:**
- :warning::tada: (client, [smithy-rs#2417](https://github.com/smithy-lang/smithy-rs/issues/2417), [smithy-rs#3018](https://github.com/smithy-lang/smithy-rs/issues/3018)) Retry classifiers are now configurable at the service and operation levels. Users may also define their own custom retry classifiers.

    For more information, see the [guide](https://github.com/smithy-lang/smithy-rs/discussions/3050).
- :warning: (client, [smithy-rs#3011](https://github.com/smithy-lang/smithy-rs/issues/3011)) HTTP connector configuration has changed significantly. See the [upgrade guidance](https://github.com/smithy-lang/smithy-rs/discussions/3022) for details.
- :warning: (client, [smithy-rs#3038](https://github.com/smithy-lang/smithy-rs/issues/3038)) The `enableNewSmithyRuntime: middleware` opt-out flag in smithy-build.json has been removed and no longer opts out of the client orchestrator implementation. Middleware is no longer supported. If you haven't already upgraded to the orchestrator, see [the guide](https://github.com/smithy-lang/smithy-rs/discussions/2887).
- :warning: (client, [smithy-rs#2909](https://github.com/smithy-lang/smithy-rs/issues/2909)) It's now possible to nest runtime components with the `RuntimePlugin` trait. A `current_components` argument was added to the `runtime_components` method so that components configured from previous runtime plugins can be referenced in the current runtime plugin. Ordering of runtime plugins was also introduced via a new `RuntimePlugin::order` method.
- :warning: (all, [smithy-rs#2948](https://github.com/smithy-lang/smithy-rs/issues/2948)) Update MSRV to Rust 1.70.0
- :warning: (client, [smithy-rs#2970](https://github.com/smithy-lang/smithy-rs/issues/2970)) `aws_smithy_client::hyper_ext::Adapter` was moved/renamed to `aws_smithy_runtime::client::connectors::hyper_connector::HyperConnector`.
- :warning: (client, [smithy-rs#2970](https://github.com/smithy-lang/smithy-rs/issues/2970)) Test connectors moved into `aws_smithy_runtime::client::connectors::test_util` behind the `test-util` feature.
- :warning: (client, [smithy-rs#2970](https://github.com/smithy-lang/smithy-rs/issues/2970)) DVR's RecordingConnection and ReplayingConnection were renamed to RecordingConnector and ReplayingConnector respectively.
- :warning: (client, [smithy-rs#2970](https://github.com/smithy-lang/smithy-rs/issues/2970)) TestConnection was renamed to EventConnector.
- :warning: (all, [smithy-rs#2973](https://github.com/smithy-lang/smithy-rs/issues/2973)) Remove `once_cell` from public API.
- :warning: (all, [smithy-rs#2995](https://github.com/smithy-lang/smithy-rs/issues/2995)) Structure members with the type `Option<Vec<T>>` now produce an accessor with the type `&[T]` instead of `Option<&[T]>`. This is enabled by default for clients and can be disabled by updating your smithy-build.json with the following setting:
    ```json
    {
      "codegen": {
        "flattenCollectionAccessors": false,
        ...
      }
    }
    ```
- :warning: (client, [smithy-rs#2978](https://github.com/smithy-lang/smithy-rs/issues/2978)) The `futures_core::stream::Stream` trait has been removed from public API. `FnStream` only supports `next`, `try_next`, `collect`, and `try_collect` methods. [`TryFlatMap::flat_map`](https://docs.rs/aws-smithy-async/latest/aws_smithy_async/future/pagination_stream/struct.TryFlatMap.html#method.flat_map) returns [`PaginationStream`](https://docs.rs/aws-smithy-async/latest/aws_smithy_async/future/pagination_stream/struct.PaginationStream.html), which should be preferred to `FnStream` at an interface level. Other stream operations that were previously available through the trait or its extension traits can be added later in a backward compatible manner. Finally, `fn_stream` has been moved to be a child module of `pagination_stream`.
- :warning: (client, [smithy-rs#2983](https://github.com/smithy-lang/smithy-rs/issues/2983)) The `futures_core::stream::Stream` trait has been removed from [`ByteStream`](https://docs.rs/aws-smithy-http/latest/aws_smithy_http/byte_stream/struct.ByteStream.html). The methods mentioned in the [doc](https://docs.rs/aws-smithy-http/latest/aws_smithy_http/byte_stream/struct.ByteStream.html#getting-data-out-of-a-bytestream) will continue to be supported. Other stream operations that were previously available through the trait or its extension traits can be added later in a backward compatible manner.
- :warning: (client, [smithy-rs#2997](https://github.com/smithy-lang/smithy-rs/issues/2997)) `StaticUriEndpointResolver`'s `uri` constructor now takes a `String` instead of a `Uri`.
- :warning: (server, [smithy-rs#3038](https://github.com/smithy-lang/smithy-rs/issues/3038)) `SdkError` is no longer re-exported in generated server crates.
- :warning: (client, [smithy-rs#3039](https://github.com/smithy-lang/smithy-rs/issues/3039)) The `customize()` method is now sync and infallible. Remove any `await`s and error handling from it to make things compile again.
- :bug::warning: (all, [smithy-rs#3037](https://github.com/smithy-lang/smithy-rs/issues/3037), [aws-sdk-rust#756](https://github.com/awslabs/aws-sdk-rust/issues/756)) Our algorithm for converting identifiers to `snake_case` has been updated. This may result in a small change for some identifiers, particularly acronyms ending in `s`, e.g. `ACLs`.
- :warning: (client, [smithy-rs#3055](https://github.com/smithy-lang/smithy-rs/issues/3055)) The future return types on traits `EndpointResolver` and `IdentityResolver` changed to new-types `EndpointFuture` and `IdentityFuture` respectively.
- :warning: (client, [smithy-rs#3032](https://github.com/smithy-lang/smithy-rs/issues/3032)) [`EndpointPrefix::new`](https://docs.rs/aws-smithy-http/latest/aws_smithy_http/endpoint/struct.EndpointPrefix.html#method.new) no longer returns `crate::operation::error::BuildError` for an Err variant, instead returns a more specific [`InvalidEndpointError`](https://docs.rs/aws-smithy-http/latest/aws_smithy_http/endpoint/error/struct.InvalidEndpointError.html).
- :warning: (client, [smithy-rs#3061](https://github.com/smithy-lang/smithy-rs/issues/3061)) Lifetimes have been added to the `EndpointResolver` trait.
- :warning: (client, [smithy-rs#3065](https://github.com/smithy-lang/smithy-rs/issues/3065)) Several traits have been renamed from noun form to verb form to be more idiomatic:
    - `AuthSchemeOptionResolver` -> `ResolveAuthSchemeOptions`
    - `EndpointResolver` -> `ResolveEndpoint`
    - `IdentityResolver` -> `ResolveIdentity`
    - `Signer` -> `Sign`
    - `RequestSerializer` -> `SerializeRequest`
    - `ResponseDeserializer` -> `DeserializeResponse`
    - `Interceptor` -> `Intercept`
- :warning: (client, [smithy-rs#3059](https://github.com/smithy-lang/smithy-rs/issues/3059)) **This change has [detailed upgrade guidance](https://github.com/smithy-lang/smithy-rs/discussions/3067)**. A summary is below.<br><br> The `HttpRequest` type alias now points to `aws-smithy-runtime-api::client::http::Request`. This is a first-party request type to allow us to gracefully support `http = 1.0` when it arrives. Most customer code using this method should be unaffected. `TryFrom`/`TryInto` conversions are provided for `http = 0.2.*`.
- :warning: (client, [smithy-rs#2917](https://github.com/smithy-lang/smithy-rs/issues/2917)) `RuntimeComponents` have been added as an argument to the `IdentityResolver::resolve_identity` trait function.
- :warning: (client, [smithy-rs#3072](https://github.com/smithy-lang/smithy-rs/issues/3072)) The `idempotency_provider` field has been removed from config as a public field. If you need access to this field, it is still available from the context of an interceptor.
- :warning: (client, [smithy-rs#3078](https://github.com/smithy-lang/smithy-rs/issues/3078)) The `config::Builder::endpoint_resolver` method no longer accepts `&'static str`. Use `config::Builder::endpoint_url` instead.
- :warning: (client, [smithy-rs#3043](https://github.com/smithy-lang/smithy-rs/issues/3043), [smithy-rs#3078](https://github.com/smithy-lang/smithy-rs/issues/3078)) **This change has [detailed upgrade guidance](https://github.com/smithy-lang/smithy-rs/discussions/3079).** <br><br>The endpoint interfaces from `aws-smithy-http` have been removed. Service-specific endpoint resolver traits have been added.
- :warning: (all, [smithy-rs#3054](https://github.com/smithy-lang/smithy-rs/issues/3054), [smithy-rs#3070](https://github.com/smithy-lang/smithy-rs/issues/3070)) `aws_smithy_http::operation::error::{BuildError, SerializationError}` have been moved to `aws_smithy_types::error::operation::{BuildError, SerializationError}`. Type aliases for them are left in `aws_smithy_http` for backwards compatibility but are deprecated.
- :warning: (all, [smithy-rs#3076](https://github.com/smithy-lang/smithy-rs/issues/3076)) `aws_smithy_http::body::{BoxBody, Error, SdkBody}` have been moved to `aws_smithy_types::body::{BoxBody, Error, SdkBody}`. Type aliases for them are left in `aws_smithy_http` for backwards compatibility but are deprecated.
- :warning: (all, [smithy-rs#3076](https://github.com/smithy-lang/smithy-rs/issues/3076), [smithy-rs#3091](https://github.com/smithy-lang/smithy-rs/issues/3091)) `aws_smithy_http::byte_stream::{AggregatedBytes, ByteStream, error::Error, FsBuilder, Length}` have been moved to `aws_smithy_types::byte_stream::{AggregatedBytes, ByteStream, error::Error, FsBuilder, Length}`. Type aliases for them are left in `aws_smithy_http` for backwards compatibility but are deprecated.
- :warning: (client, [smithy-rs#3077](https://github.com/smithy-lang/smithy-rs/issues/3077)) **Behavior Break!** Identities for auth are now cached by default. See the `Config` builder's `identity_cache()` method docs for an example of how to disable this caching.
- :warning: (all, [smithy-rs#3033](https://github.com/smithy-lang/smithy-rs/issues/3033), [smithy-rs#3088](https://github.com/smithy-lang/smithy-rs/issues/3088), [smithy-rs#3101](https://github.com/smithy-lang/smithy-rs/issues/3101)) Publicly exposed types from `http-body` and `hyper` crates within `aws-smithy-types` are now feature-gated. See the [upgrade guidance](https://github.com/smithy-lang/smithy-rs/discussions/3089) for details.
- :warning: (all, [smithy-rs#3033](https://github.com/smithy-lang/smithy-rs/issues/3033), [smithy-rs#3088](https://github.com/smithy-lang/smithy-rs/issues/3088)) `ByteStream::poll_next` is now feature-gated. You can turn on a cargo feature `byte-stream-poll-next` in `aws-smithy-types` to use it.
- :warning: (client, [smithy-rs#3092](https://github.com/smithy-lang/smithy-rs/issues/3092), [smithy-rs#3093](https://github.com/smithy-lang/smithy-rs/issues/3093)) The [`connection`](https://docs.rs/aws-smithy-http/latest/aws_smithy_http/connection/index.html) and [`result`](https://docs.rs/aws-smithy-http/latest/aws_smithy_http/result/index.html) modules in `aws-smithy-http` have been moved to `aws-smithy-runtime-api`. Type aliases for all affected pub items, except for a trait, are left in `aws-smithy-http` for backwards compatibility but are deprecated. Due to lack of trait aliases, the moved trait `CreateUnhandledError` needs to be used from `aws-smithy-runtime-api`.
- :bug::warning: (server, [smithy-rs#3095](https://github.com/smithy-lang/smithy-rs/issues/3095), [smithy-rs#3096](https://github.com/smithy-lang/smithy-rs/issues/3096)) Service builder initialization now takes in a `${serviceName}Config` object on which plugins and layers should be registered. The `builder_with_plugins` and `builder_without_plugins` methods on the service builder, as well as the `layer` method on the built service have been deprecated, and will be removed in a future release. See the [upgrade guidance](https://github.com/smithy-lang/smithy-rs/discussions/3096) for more details.

**New this release:**
- :tada: (client, [smithy-rs#2916](https://github.com/smithy-lang/smithy-rs/issues/2916), [smithy-rs#1767](https://github.com/smithy-lang/smithy-rs/issues/1767)) Support for Smithy IDLv2 nullability is now enabled by default. You can maintain the old behavior by setting `nullabilityCheckMode: "CLIENT_ZERO_VALUE_V1" in your codegen config.
    For upgrade guidance and more info, see [here](https://github.com/smithy-lang/smithy-rs/discussions/2929).
- :tada: (server, [smithy-rs#3005](https://github.com/smithy-lang/smithy-rs/issues/3005)) Python middleware can set URI. This can be used to route a request to a different handler.
- :tada: (client, [smithy-rs#3071](https://github.com/smithy-lang/smithy-rs/issues/3071)) Clients now have a default async sleep implementation so that one does not need to be specified if you're using Tokio.
- :bug: (client, [smithy-rs#2944](https://github.com/smithy-lang/smithy-rs/issues/2944), [smithy-rs#2951](https://github.com/smithy-lang/smithy-rs/issues/2951)) `CustomizableOperation`, created as a result of calling the `.customize` method on a fluent builder, ceased to be `Send` and `Sync` in the previous releases. It is now `Send` and `Sync` again.
- :bug: (client, [smithy-rs#2960](https://github.com/smithy-lang/smithy-rs/issues/2960)) Generate a region setter when a model uses SigV4.
- :bug: (all, [smithy-rs#2969](https://github.com/smithy-lang/smithy-rs/issues/2969), [smithy-rs#1896](https://github.com/smithy-lang/smithy-rs/issues/1896)) Fix code generation for union members with the `@httpPayload` trait.
- (client, [smithy-rs#2964](https://github.com/smithy-lang/smithy-rs/issues/2964)) Required members with @contextParam are now treated as client-side required.
- :bug: (client, [smithy-rs#2926](https://github.com/smithy-lang/smithy-rs/issues/2926), [smithy-rs#2972](https://github.com/smithy-lang/smithy-rs/issues/2972)) Fix regression with redacting sensitive HTTP response bodies.
- :bug: (all, [smithy-rs#2831](https://github.com/smithy-lang/smithy-rs/issues/2831), [aws-sdk-rust#818](https://github.com/awslabs/aws-sdk-rust/issues/818)) Omit fractional seconds from `http-date` format.
- :bug: (client, [smithy-rs#2985](https://github.com/smithy-lang/smithy-rs/issues/2985)) Source defaults from the default trait instead of implicitly based on type. This has minimal changes in the generated code.
- (client, [smithy-rs#2996](https://github.com/smithy-lang/smithy-rs/issues/2996)) Produce better docs when items are marked @required
- :bug: (client, [smithy-rs#3034](https://github.com/smithy-lang/smithy-rs/issues/3034), [smithy-rs#3087](https://github.com/smithy-lang/smithy-rs/issues/3087)) Enable custom auth schemes to work by changing the code generated auth options to be set at the client level at `DEFAULTS` priority.


August 22nd, 2023
=================
**Breaking Changes:**
- :bug::warning: (client, [smithy-rs#2931](https://github.com/smithy-lang/smithy-rs/issues/2931), [aws-sdk-rust#875](https://github.com/awslabs/aws-sdk-rust/issues/875)) Fixed re-exported `SdkError` type. The previous release had the wrong type for `SdkError` when generating code for orchestrator mode, which caused projects to fail to compile when upgrading.

**New this release:**
- (client, [smithy-rs#2904](https://github.com/smithy-lang/smithy-rs/issues/2904)) `RuntimeComponents` and `RuntimeComponentsBuilder` are now re-exported in generated clients so that implementing a custom interceptor or runtime plugin doens't require directly depending on `aws-smithy-runtime-api`.
- :bug: (client, [smithy-rs#2914](https://github.com/smithy-lang/smithy-rs/issues/2914), [aws-sdk-rust#825](https://github.com/awslabs/aws-sdk-rust/issues/825)) Fix incorrect summary docs for builders
- :bug: (client, [smithy-rs#2934](https://github.com/smithy-lang/smithy-rs/issues/2934), [aws-sdk-rust#872](https://github.com/awslabs/aws-sdk-rust/issues/872)) Logging via `#[instrument]` in the `aws_smithy_runtime::client::orchestrator` module is now emitted at the `DEBUG` level to reduce the amount of logging when emitted at the `INFO` level.
- :bug: (client, [smithy-rs#2935](https://github.com/smithy-lang/smithy-rs/issues/2935)) Fix `SDK::Endpoint` built-in for `@endpointRuleSet`.


August 1st, 2023
================
**Breaking Changes:**
- ⚠🎉 (server, [smithy-rs#2740](https://github.com/smithy-lang/smithy-rs/issues/2740), [smithy-rs#2759](https://github.com/smithy-lang/smithy-rs/issues/2759), [smithy-rs#2779](https://github.com/smithy-lang/smithy-rs/issues/2779), [smithy-rs#2827](https://github.com/smithy-lang/smithy-rs/issues/2827), @hlbarber) The middleware system has been reworked as we push for a unified, simple, and consistent API. The following changes have been made in service of this goal:

    - A `ServiceShape` trait has been added.
    - The `Plugin` trait has been simplified.
    - The `HttpMarker` and `ModelMarker` marker traits have been added to better distinguish when plugins run and what they have access to.
    - The `Operation` structure has been removed.
    - A `Scoped` `Plugin` has been added.

    The `Plugin` trait has now been simplified and the `Operation` struct has been removed.

    ## Addition of `ServiceShape`

    Since the [0.52 release](https://github.com/smithy-lang/smithy-rs/releases/tag/release-2022-12-12) the `OperationShape` has existed.

    ```rust
    /// Models the [Smithy Operation shape].
    ///
    /// [Smithy Operation shape]: https://awslabs.github.io/smithy/1.0/spec/core/model.html#operation
    pub trait OperationShape {
        /// The ID of the operation.
        const ID: ShapeId;

        /// The operation input.
        type Input;
        /// The operation output.
        type Output;
        /// The operation error. [`Infallible`](std::convert::Infallible) in the case where no error
        /// exists.
        type Error;
    }
    ```

    This allowed `Plugin` authors to access these associated types and constants. See the [`PrintPlugin`](https://github.com/smithy-lang/smithy-rs/blob/main/examples/pokemon-service/src/plugin.rs) as an example.

    We continue with this approach and introduce the following trait:

    ```rust
    /// Models the [Smithy Service shape].
    ///
    /// [Smithy Service shape]: https://smithy.io/2.0/spec/service-types.html
    pub trait ServiceShape {
        /// The [`ShapeId`] of the service.
        const ID: ShapeId;

        /// The version of the service.
        const VERSION: Option<&'static str>;

        /// The [Protocol] applied to this service.
        ///
        /// [Protocol]: https://smithy.io/2.0/spec/protocol-traits.html
        type Protocol;

        /// An enumeration of all operations contained in this service.
        type Operations;
    }
    ```

    With the changes to `Plugin`, described below, middleware authors now have access to this information at compile time.

    ## Simplication of the `Plugin` trait

    Previously,

    ```rust
    trait Plugin<P, Op, S, L> {
        type Service;
        type Layer;

        fn map(&self, input: Operation<S, L>) -> Operation<Self::Service, Self::Layer>;
    }
    ```

    modified an `Operation`.

    Now,

    ```rust
    trait Plugin<Service, Operation, T> {
        type Output;

        fn apply(&self, input: T) -> Self::Output;
    }
    ```

    maps a `tower::Service` to a `tower::Service`. This is equivalent to `tower::Layer` with two extra type parameters: `Service` and `Operation`, which implement `ServiceShape` and `OperationShape` respectively.

    Having both `Service` and `Operation` as type parameters also provides an even surface for advanced users to extend the codegenerator in a structured way. See [this issue](https://github.com/smithy-lang/smithy-rs/issues/2777) for more context.

    The following middleware setup

    ```rust
    pub struct PrintService<S> {
        inner: S,
        name: &'static str,
    }

    impl<R, S> Service<R> for PrintService<S>
    where
        S: Service<R>,
    {
        async fn call(&mut self, req: R) -> Self::Future {
            println!("Hi {}", self.name);
            self.inner.call(req)
        }
    }

    pub struct PrintLayer {
        name: &'static str,
    }

    impl<S> Layer<S> for PrintLayer {
        type Service = PrintService<S>;

        fn layer(&self, service: S) -> Self::Service {
            PrintService {
                inner: service,
                name: self.name,
            }
        }
    }

    pub struct PrintPlugin;

    impl<P, Op, S, L> Plugin<P, Op, S, L> for PrintPlugin
    where
        Op: OperationShape,
    {
        type Service = S;
        type Layer = Stack<L, PrintLayer>;

        fn map(&self, input: Operation<S, L>) -> Operation<Self::Service, Self::Layer> {
            input.layer(PrintLayer { name: Op::NAME })
        }
    }
    ```

    now becomes

    ```rust
    pub struct PrintService<S> {
        inner: S,
        name: &'static str,
    }

    impl<R, S> Service<R> for PrintService<S>
    where
        S: Service<R>,
    {
        async fn call(&mut self, req: R) -> Self::Future {
            println!("Hi {}", self.name);
            self.inner.call(req)
        }
    }

    pub struct PrintPlugin;

    impl<Service, Op, T> Plugin<Service, Operation, T> for PrintPlugin
    where
        Op: OperationShape,
    {
        type Output = PrintService<S>;

        fn apply(&self, inner: T) -> Self::Output {
            PrintService { inner, name: Op::ID.name() }
        }
    }

    impl HttpMarker for PrintPlugin { }
    ```

    Alternatively, using the new `ServiceShape`, implemented on `Ser`:

    ```rust
    impl<Service, Operation, T> Plugin<Service, Operation, T> for PrintPlugin
    where
        Ser: ServiceShape,
    {
        type Service = PrintService<S>;

        fn apply(&self, inner: T) -> Self::Service {
            PrintService { inner, name: Ser::ID.name() }
        }
    }
    ```

    A single `Plugin` can no longer apply a `tower::Layer` on HTTP requests/responses _and_ modelled structures at the same time (see middleware positions [C](https://smithy-lang.github.io/smithy-rs/design/server/middleware.html#c-operation-specific-http-middleware) and [D](https://smithy-lang.github.io/smithy-rs/design/server/middleware.html#d-operation-specific-model-middleware). Instead one `Plugin` must be specified for each and passed to the service builder constructor separately:

    ```rust
    let app = PokemonService::builder_with_plugins(/* HTTP plugins */, /* model plugins */)
        /* setters */
        .build()
        .unwrap();
    ```

    To better distinguish when a plugin runs and what it has access to, `Plugin`s now have to additionally implement the `HttpMarker` marker trait, the `ModelMarker` marker trait, or both:

    - A HTTP plugin acts on the HTTP request before it is deserialized, and acts on the HTTP response after it is serialized.
    - A model plugin acts on the modeled operation input after it is deserialized, and acts on the modeled operation output or the modeled operation error before it is serialized.

    The motivation behind this change is to simplify the job of middleware authors, separate concerns, accomodate common cases better, and to improve composition internally.

    Because `Plugin` is now closer to `tower::Layer` we have two canonical converters:

    ```rust
    use aws_smithy_http_server::plugin::{PluginLayer, LayerPlugin};

    // Convert from `Layer` to `Plugin` which applies uniformly across all operations
    let layer = /* some layer */;
    let plugin = PluginLayer(layer);

    // Convert from `Plugin` to `Layer` for some fixed protocol and operation
    let plugin = /* some plugin */;
    let layer = LayerPlugin::new::<SomeProtocol, SomeOperation>(plugin);
    ```

    ## Removal of `PluginPipeline`

    Since plugins now come in two flavors (those marked with `HttpMarker` and those marked with `ModelMarker`) that shouldn't be mixed in a collection of plugins, the primary way of concatenating plugins, `PluginPipeline` has been removed in favor of the `HttpPlugins` and `ModelPlugins` types, which eagerly check that whenever a plugin is pushed, it is of the expected type.

    This worked before, but you wouldn't be able to do apply this collection of plugins anywhere; if you tried to, the compilation error messages would not be very helpful:

    ```rust
    use aws_smithy_http_server::plugin::PluginPipeline;

    let pipeline = PluginPipeline::new().push(http_plugin).push(model_plugin);
    ```

    Now collections of plugins must contain plugins of the same flavor:

    ```rust
    use aws_smithy_http_server::plugin::{HttpPlugins, ModelPlugins};

    let http_plugins = HttpPlugins::new()
        .push(http_plugin)
        // .push(model_plugin) // This fails to compile with a helpful error message.
        .push(&http_and_model_plugin);
    let model_plugins = ModelPlugins::new()
        .push(model_plugin)
        .push(&http_and_model_plugin);
    ```

    In the above example, `&http_and_model_plugin` implements both `HttpMarker` and `ModelMarker`, so we can add it to both collections.

    ## Removal of `Operation`

    The `aws_smithy_http_server::operation::Operation` structure has now been removed. Previously, there existed a `{operation_name}_operation` setter on the service builder, which accepted an `Operation`. This allowed users to

    ```rust
    let operation /* : Operation<_, _> */ = GetPokemonSpecies::from_service(/* tower::Service */);

    let app = PokemonService::builder_without_plugins()
        .get_pokemon_species_operation(operation)
        /* other setters */
        .build()
        .unwrap();
    ```

    to set an operation with a `tower::Service`, and

    ```rust
    let operation /* : Operation<_, _> */ = GetPokemonSpecies::from_service(/* tower::Service */).layer(/* layer */);
    let operation /* : Operation<_, _> */ = GetPokemonSpecies::from_handler(/* closure */).layer(/* layer */);

    let app = PokemonService::builder_without_plugins()
        .get_pokemon_species_operation(operation)
        /* other setters */
        .build()
        .unwrap();
    ```

    to add a `tower::Layer` (acting on HTTP requests/responses post-routing) to a single operation.

    We have seen little adoption of this API and for this reason we have opted instead to introduce a new setter, accepting a `tower::Service`, on the service builder:

    ```rust
    let app = PokemonService::builder_without_plugins()
        .get_pokemon_species_service(/* tower::Service */)
        /* other setters */
        .build()
        .unwrap();
    ```

    Applying a `tower::Layer` to a _subset_ of operations is should now be done through the `Plugin` API via `filter_by_operation_id`

    ```rust
    use aws_smithy_http_server::plugin::{PluginLayer, filter_by_operation_name, IdentityPlugin};

    let plugin = PluginLayer(/* layer */);
    let scoped_plugin = filter_by_operation_name(plugin, |id| id == GetPokemonSpecies::ID);

    let app = PokemonService::builder_with_plugins(scoped_plugin, IdentityPlugin)
        .get_pokemon_species(/* handler */)
        /* other setters */
        .build()
        .unwrap();
    ```

    or the new `Scoped` `Plugin` introduced below.

    # Addition of `Scoped`

    Currently, users can selectively apply a `Plugin` via the `filter_by_operation_id` function

    ```rust
    use aws_smithy_http_server::plugin::filter_by_operation_id;
    // Only apply `plugin` to `CheckHealth` and `GetStorage` operation
    let filtered_plugin = filter_by_operation_id(plugin, |name| name == CheckHealth::ID || name == GetStorage::ID);
    ```

    In addition to this, we now provide `Scoped`, which selectively applies a `Plugin` at _compiletime_. Users should prefer this to `filter_by_operation_id` when applicable.

    ```rust
    use aws_smithy_http_server::plugin::Scoped;
    use pokemon_service_server_sdk::scoped;

    scope! {
        /// Includes only the `CheckHealth` and `GetStorage` operation.
        struct SomeScope {
            includes: [CheckHealth, GetStorage]
        }
    }
    let scoped_plugin = Scoped::new::<SomeScope>(plugin);
    ```

- ⚠ (all, [smithy-rs#2675](https://github.com/smithy-lang/smithy-rs/issues/2675)) Remove native-tls and add a migration guide.
- ⚠ (client, [smithy-rs#2671](https://github.com/smithy-lang/smithy-rs/issues/2671)) <details>
    <summary>Breaking change in how event stream signing works (click to expand more details)</summary>

    This change will only impact you if you are wiring up their own event stream signing/authentication scheme. If you're using `aws-sig-auth` to use AWS SigV4 event stream signing, then this change will **not** impact you.

    Previously, event stream signing was configured at codegen time by placing a `new_event_stream_signer` method on the `Config`. This function was called at serialization time to connect the signer to the streaming body. Now, instead, a special `DeferredSigner` is wired up at serialization time that relies on a signing implementation to be sent on a channel by the HTTP request signer. To do this, a `DeferredSignerSender` must be pulled out of the property bag, and its `send()` method called with the desired event stream signing implementation.

    See the changes in https://github.com/smithy-lang/smithy-rs/pull/2671 for an example of how this was done for SigV4.
    </details>
- ⚠ (all, [smithy-rs#2673](https://github.com/smithy-lang/smithy-rs/issues/2673)) For event stream operations, the `EventStreamSender` in inputs/outputs now requires the passed in `Stream` impl to implement `Sync`.
- ⚠ (server, [smithy-rs#2539](https://github.com/smithy-lang/smithy-rs/issues/2539)) Code generation will abort if the `ignoreUnsupportedConstraints` codegen flag has no effect, that is, if all constraint traits used in your model are well-supported. Please remove the flag in such case.
- ⚠ (client, [smithy-rs#2728](https://github.com/smithy-lang/smithy-rs/issues/2728), [smithy-rs#2262](https://github.com/smithy-lang/smithy-rs/issues/2262), [aws-sdk-rust#2087](https://github.com/awslabs/aws-sdk-rust/issues/2087)) The property bag type for Time is now `SharedTimeSource`, not `SystemTime`. If your code relies on setting request time, use `aws_smithy_async::time::SharedTimeSource`.
- ⚠ (server, [smithy-rs#2676](https://github.com/smithy-lang/smithy-rs/issues/2676), [smithy-rs#2685](https://github.com/smithy-lang/smithy-rs/issues/2685)) Bump dependency on `lambda_http` by `aws-smithy-http-server` to 0.8.0. This version of `aws-smithy-http-server` is only guaranteed to be compatible with 0.8.0, or semver-compatible versions of 0.8.0 of the `lambda_http` crate. It will not work with versions prior to 0.8.0 _at runtime_, making requests to your smithy-rs service unroutable, so please make sure you're running your service in a compatible configuration
- ⚠ (server, [smithy-rs#2457](https://github.com/smithy-lang/smithy-rs/issues/2457), @hlbarber) Remove `PollError` from an operations `Service::Error`.

    Any [`tower::Service`](https://docs.rs/tower/latest/tower/trait.Service.html) provided to
    [`Operation::from_service`](https://docs.rs/aws-smithy-http-server/latest/aws_smithy_http_server/operation/struct.Operation.html#method.from_service)
    no longer requires `Service::Error = OperationError<Op::Error, PollError>`, instead requiring just `Service::Error = Op::Error`.
- ⚠ (client, [smithy-rs#2742](https://github.com/smithy-lang/smithy-rs/issues/2742)) A newtype wrapper `SharedAsyncSleep` has been introduced and occurrences of `Arc<dyn AsyncSleep>` that appear in public APIs have been replaced with it.
- ⚠ (all, [smithy-rs#2893](https://github.com/smithy-lang/smithy-rs/issues/2893)) Update MSRV to Rust 1.69.0
- ⚠ (server, [smithy-rs#2678](https://github.com/smithy-lang/smithy-rs/issues/2678)) `ShapeId` is the new structure used to represent a shape, with its absolute name, namespace and name.
    `OperationExtension`'s members are replaced by the `ShapeId` and operations' names are now replced by a `ShapeId`.

    Before you had an operation and an absolute name as its `NAME` member. You could apply a plugin only to some selected operation:

    ```
    filter_by_operation_name(plugin, |name| name != Op::ID);
    ```

    Your new filter selects on an operation's absolute name, namespace or name.

    ```
    filter_by_operation_id(plugin, |id| id.name() != Op::ID.name());
    ```

    The above filter is applied to an operation's name, the one you use to specify the operation in the Smithy model.

    You can filter all operations in a namespace or absolute name:

    ```
    filter_by_operation_id(plugin, |id| id.namespace() != "namespace");
    filter_by_operation_id(plugin, |id| id.absolute() != "namespace#name");
    ```
- ⚠ (client, [smithy-rs#2758](https://github.com/smithy-lang/smithy-rs/issues/2758)) The occurrences of `Arc<dyn ResolveEndpoint>` have now been replaced with `SharedEndpointResolver` in public APIs.
- ⚠ (server, [smithy-rs#2740](https://github.com/smithy-lang/smithy-rs/issues/2740), [smithy-rs#2759](https://github.com/smithy-lang/smithy-rs/issues/2759), [smithy-rs#2779](https://github.com/smithy-lang/smithy-rs/issues/2779), @hlbarber) Remove `filter_by_operation_id` and `plugin_from_operation_id_fn` in favour of `filter_by_operation` and `plugin_from_operation_fn`.

    Previously, we provided `filter_by_operation_id` which filtered `Plugin` application via a predicate over the Shape ID.

    ```rust
    use aws_smithy_http_server::plugin::filter_by_operation_id;
    use pokemon_service_server_sdk::operation_shape::CheckHealth;

    let filtered = filter_by_operation_id(plugin, |name| name != CheckHealth::NAME);
    ```

    This had the problem that the user is unable to exhaustively match over a `&'static str`. To remedy this we have switched to `filter_by_operation` which is a predicate over an enum containing all operations contained in the service.

    ```rust
    use aws_smithy_http_server::plugin::filter_by_operation_id;
    use pokemon_service_server_sdk::service::Operation;

    let filtered = filter_by_operation(plugin, |op: Operation| op != Operation::CheckHealth);
    ```

    Similarly, `plugin_from_operation_fn` now allows for

    ```rust
    use aws_smithy_http_server::plugin::plugin_from_operation_fn;
    use pokemon_service_server_sdk::service::Operation;

    fn map<S>(op: Operation, inner: S) -> PrintService<S> {
        match op {
            Operation::CheckHealth => PrintService { name: op.shape_id().name(), inner },
            Operation::GetPokemonSpecies => PrintService { name: "hello world", inner },
            _ => todo!()
        }
    }

    let plugin = plugin_from_operation_fn(map);
    ```
- ⚠ (client, [smithy-rs#2783](https://github.com/smithy-lang/smithy-rs/issues/2783)) The naming `make_token` for fields and the API of `IdempotencyTokenProvider` in service configs and their builders has now been updated to `idempotency_token_provider`.
- ⚠ (client, [smithy-rs#2845](https://github.com/smithy-lang/smithy-rs/issues/2845)) `aws_smithy_async::future::rendezvous::Sender::send` no longer exposes `tokio::sync::mpsc::error::SendError` for the error of its return type and instead exposes a new-type wrapper called `aws_smithy_async::future::rendezvous::error::SendError`. In addition, the `aws_smithy_xml` crate no longer exposes types from `xmlparser`.
- ⚠ (client, [smithy-rs#2848](https://github.com/smithy-lang/smithy-rs/issues/2848)) The implementation `From<bytes_utils::segmented::SegmentedBuf>` for `aws_smithy_http::event_stream::RawMessage` has been removed.
- ⚠ (server, [smithy-rs#2865](https://github.com/smithy-lang/smithy-rs/issues/2865)) The `alb_health_check` module has been moved out of the `plugin` module into a new `layer` module. ALB health checks should be enacted before routing, and plugins run after routing, so the module location was misleading. Examples have been corrected to reflect the intended application of the layer.
- ⚠ (client, [smithy-rs#2873](https://github.com/smithy-lang/smithy-rs/issues/2873)) The `test-util` feature in aws-smithy-client has been split to include a separate `wiremock` feature. This allows test-util to be used without a Hyper server dependency making it usable in webassembly targets.
- ⚠ (client) The entire architecture of generated clients has been overhauled. See the [upgrade guide](https://github.com/smithy-lang/smithy-rs/discussions/2887) to get your code working again.

**New this release:**
- 🎉 (all, [smithy-rs#2647](https://github.com/smithy-lang/smithy-rs/issues/2647), [smithy-rs#2645](https://github.com/smithy-lang/smithy-rs/issues/2645), [smithy-rs#2646](https://github.com/smithy-lang/smithy-rs/issues/2646), [smithy-rs#2616](https://github.com/smithy-lang/smithy-rs/issues/2616), @thomas-k-cameron) Implement unstable serde support for the `Number`, `Blob`, `Document`, `DateTime` primitives
- 🎉 (client, [smithy-rs#2652](https://github.com/smithy-lang/smithy-rs/issues/2652), @thomas-k-cameron) Add a `send_with` function on `-Input` types for sending requests without fluent builders
- (client, [smithy-rs#2791](https://github.com/smithy-lang/smithy-rs/issues/2791), @davidsouther) Add accessors to Builders
- (all, [smithy-rs#2786](https://github.com/smithy-lang/smithy-rs/issues/2786), @yotamofek) Avoid intermediate vec allocations in AggregatedBytes::to_vec.
- 🐛 (server, [smithy-rs#2733](https://github.com/smithy-lang/smithy-rs/issues/2733), @thor-bjorgvinsson) Fix bug in AWS JSON 1.x routers where, if a service had more than 14 operations, the router was created without the route for the 15th operation.
- (client, [smithy-rs#2728](https://github.com/smithy-lang/smithy-rs/issues/2728), [smithy-rs#2262](https://github.com/smithy-lang/smithy-rs/issues/2262), [aws-sdk-rust#2087](https://github.com/awslabs/aws-sdk-rust/issues/2087)) Time is now controlled by the `TimeSource` trait. This facilitates testing as well as use cases like WASM where `SystemTime::now()` is not supported.
- 🐛 (client, [smithy-rs#2767](https://github.com/smithy-lang/smithy-rs/issues/2767), @mcmasn-amzn) Fix bug in client generation when using smithy.rules#endpointTests and operation and service shapes are in different namespaces.
- (client, [smithy-rs#2854](https://github.com/smithy-lang/smithy-rs/issues/2854)) Public fields in structs are no longer marked as `#[doc(hidden)]`, and they are now visible.
- (server, [smithy-rs#2866](https://github.com/smithy-lang/smithy-rs/issues/2866)) [RestJson1](https://awslabs.github.io/smithy/2.0/aws/protocols/aws-restjson1-protocol.html#operation-error-serialization) server SDKs now serialize only the [shape name](https://smithy.io/2.0/spec/model.html#shape-id) in operation error responses. Previously (from versions 0.52.0 to 0.55.4), the full shape ID was rendered.
    Example server error response by a smithy-rs server version 0.52.0 until 0.55.4:
    ```
    HTTP/1.1 400 Bad Request
    content-type: application/json
    x-amzn-errortype: com.example.service#InvalidRequestException
    ...
    ```
    Example server error response now:
    ```
    HTTP/1.1 400 Bad Request
    content-type: application/json
    x-amzn-errortype: InvalidRequestException
    ...
    ```

**Contributors**
Thank you for your contributions! ❤
- @davidsouther ([smithy-rs#2791](https://github.com/smithy-lang/smithy-rs/issues/2791))
- @hlbarber ([smithy-rs#2457](https://github.com/smithy-lang/smithy-rs/issues/2457), [smithy-rs#2740](https://github.com/smithy-lang/smithy-rs/issues/2740), [smithy-rs#2759](https://github.com/smithy-lang/smithy-rs/issues/2759), [smithy-rs#2779](https://github.com/smithy-lang/smithy-rs/issues/2779), [smithy-rs#2827](https://github.com/smithy-lang/smithy-rs/issues/2827))
- @mcmasn-amzn ([smithy-rs#2767](https://github.com/smithy-lang/smithy-rs/issues/2767))
- @thomas-k-cameron ([smithy-rs#2616](https://github.com/smithy-lang/smithy-rs/issues/2616), [smithy-rs#2645](https://github.com/smithy-lang/smithy-rs/issues/2645), [smithy-rs#2646](https://github.com/smithy-lang/smithy-rs/issues/2646), [smithy-rs#2647](https://github.com/smithy-lang/smithy-rs/issues/2647), [smithy-rs#2652](https://github.com/smithy-lang/smithy-rs/issues/2652))
- @thor-bjorgvinsson ([smithy-rs#2733](https://github.com/smithy-lang/smithy-rs/issues/2733))
- @yotamofek ([smithy-rs#2786](https://github.com/smithy-lang/smithy-rs/issues/2786))


May 23rd, 2023
==============
**New this release:**
- (all, [smithy-rs#2612](https://github.com/smithy-lang/smithy-rs/issues/2612)) The `Debug` implementation for `PropertyBag` now prints a list of the types it contains. This significantly improves debuggability.
- (all, [smithy-rs#2653](https://github.com/smithy-lang/smithy-rs/issues/2653), [smithy-rs#2656](https://github.com/smithy-lang/smithy-rs/issues/2656), @henriiik) Implement `Ord` and `PartialOrd` for `DateTime`.
- 🐛 (client, [smithy-rs#2696](https://github.com/smithy-lang/smithy-rs/issues/2696)) Fix compiler errors in generated code when naming shapes after types in the Rust standard library prelude.

**Contributors**
Thank you for your contributions! ❤
- @henriiik ([smithy-rs#2653](https://github.com/smithy-lang/smithy-rs/issues/2653), [smithy-rs#2656](https://github.com/smithy-lang/smithy-rs/issues/2656))


April 26th, 2023
================
**Breaking Changes:**
- ⚠ (all, [smithy-rs#2611](https://github.com/smithy-lang/smithy-rs/issues/2611)) Update MSRV to Rust 1.67.1

**New this release:**
- 🎉 (server, [smithy-rs#2540](https://github.com/smithy-lang/smithy-rs/issues/2540)) Implement layer for servers to handle [ALB health checks](https://docs.aws.amazon.com/elasticloadbalancing/latest/application/target-group-health-checks.html).
    Take a look at `aws_smithy_http_server::plugin::alb_health_check` to learn about it.
- 🎉 (client, [smithy-rs#2254](https://github.com/smithy-lang/smithy-rs/issues/2254), @eduardomourar) Clients now compile for the `wasm32-unknown-unknown` and `wasm32-wasi` targets when no default features are enabled. WebAssembly is not officially supported yet, but this is a great first step towards it!
- (server, [smithy-rs#2540](https://github.com/smithy-lang/smithy-rs/issues/2540)) Implement `PluginPipeline::http_layer` which allows you to apply a `tower::Layer` to all operations.
- (client, [aws-sdk-rust#784](https://github.com/awslabs/aws-sdk-rust/issues/784), @abusch) Implement std::error::Error#source() properly for the service meta Error enum.
- 🐛 (all, [smithy-rs#2496](https://github.com/smithy-lang/smithy-rs/issues/2496)) The outputs for event stream operations now implement the `Sync` auto-trait.
- 🐛 (all, [smithy-rs#2495](https://github.com/smithy-lang/smithy-rs/issues/2495)) Streaming operations now emit the request ID at the `debug` log level like their non-streaming counterparts.
- 🐛 (client, [smithy-rs#2495](https://github.com/smithy-lang/smithy-rs/issues/2495)) Streaming operations now emit the request ID at the `debug` log level like their non-streaming counterparts.
- (client, [smithy-rs#2507](https://github.com/smithy-lang/smithy-rs/issues/2507)) The `enableNewCrateOrganizationScheme` codegen flag has been removed. If you opted out of the new crate organization scheme, it must be adopted now in order to upgrade (see [the upgrade guidance](https://github.com/smithy-lang/smithy-rs/discussions/2449) from March 23rd's release).
- (client, [smithy-rs#2534](https://github.com/smithy-lang/smithy-rs/issues/2534)) `aws_smithy_types::date_time::Format` has been re-exported in service client crates.
- 🐛 (server, [smithy-rs#2582](https://github.com/smithy-lang/smithy-rs/issues/2582), [smithy-rs#2585](https://github.com/smithy-lang/smithy-rs/issues/2585)) Fix generation of constrained shapes reaching `@sensitive` shapes
- 🐛 (server, [smithy-rs#2583](https://github.com/smithy-lang/smithy-rs/issues/2583), [smithy-rs#2584](https://github.com/smithy-lang/smithy-rs/issues/2584)) Fix server code generation bug affecting constrained shapes bound with `@httpPayload`
- (client, [smithy-rs#2603](https://github.com/smithy-lang/smithy-rs/issues/2603)) Add a sensitive method to `ParseHttpResponse`. When this returns true, logging of the HTTP response body will be suppressed.

**Contributors**
Thank you for your contributions! ❤
- @abusch ([aws-sdk-rust#784](https://github.com/awslabs/aws-sdk-rust/issues/784))
- @eduardomourar ([smithy-rs#2254](https://github.com/smithy-lang/smithy-rs/issues/2254))


April 11th, 2023
================


March 23rd, 2023
================
**Breaking Changes:**
- ⚠🎉 (all, [smithy-rs#2467](https://github.com/smithy-lang/smithy-rs/issues/2467)) Update MSRV to 1.66.1
- ⚠ (client, [smithy-rs#76](https://github.com/smithy-lang/smithy-rs/issues/76), [smithy-rs#2129](https://github.com/smithy-lang/smithy-rs/issues/2129)) Generic clients no longer expose a `request_id()` function on errors. To get request ID functionality, use the SDK code generator.
- ⚠ (client, [smithy-rs#76](https://github.com/smithy-lang/smithy-rs/issues/76), [smithy-rs#2129](https://github.com/smithy-lang/smithy-rs/issues/2129)) The `message()` and `code()` methods on errors have been moved into `ProvideErrorMetadata` trait. This trait will need to be imported to continue calling these.
- ⚠ (client, [smithy-rs#76](https://github.com/smithy-lang/smithy-rs/issues/76), [smithy-rs#2129](https://github.com/smithy-lang/smithy-rs/issues/2129), [smithy-rs#2075](https://github.com/smithy-lang/smithy-rs/issues/2075)) The `*Error` and `*ErrorKind` types have been combined to make error matching simpler.
    <details>
    <summary>Example with S3</summary>
    **Before:**
    ```rust
    let result = client
        .get_object()
        .bucket(BUCKET_NAME)
        .key("some-key")
        .send()
        .await;
    match result {
        Ok(_output) => { /* Do something with the output */ }
        Err(err) => match err.into_service_error() {
            GetObjectError { kind, .. } => match kind {
                GetObjectErrorKind::InvalidObjectState(value) => println!("invalid object state: {:?}", value),
                GetObjectErrorKind::NoSuchKey(_) => println!("object didn't exist"),
            }
            err @ GetObjectError { .. } if err.code() == Some("SomeUnmodeledError") => {}
            err @ _ => return Err(err.into()),
        },
    }
    ```
    **After:**
    ```rust
    // Needed to access the `.code()` function on the error type:
    use aws_sdk_s3::types::ProvideErrorMetadata;
    let result = client
        .get_object()
        .bucket(BUCKET_NAME)
        .key("some-key")
        .send()
        .await;
    match result {
        Ok(_output) => { /* Do something with the output */ }
        Err(err) => match err.into_service_error() {
            GetObjectError::InvalidObjectState(value) => {
                println!("invalid object state: {:?}", value);
            }
            GetObjectError::NoSuchKey(_) => {
                println!("object didn't exist");
            }
            err if err.code() == Some("SomeUnmodeledError") => {}
            err @ _ => return Err(err.into()),
        },
    }
    ```
    </details>
- ⚠ (client, [smithy-rs#76](https://github.com/smithy-lang/smithy-rs/issues/76), [smithy-rs#2129](https://github.com/smithy-lang/smithy-rs/issues/2129)) `aws_smithy_types::Error` has been renamed to `aws_smithy_types::error::ErrorMetadata`.
- ⚠ (server, [smithy-rs#2436](https://github.com/smithy-lang/smithy-rs/issues/2436)) Remove unnecessary type parameter `B` from `Upgrade` service.
- 🐛⚠ (server, [smithy-rs#2382](https://github.com/smithy-lang/smithy-rs/issues/2382)) Smithy members named `send` were previously renamed to `send_value` at codegen time. These will now be called `send` in the generated code.
- ⚠ (client, [smithy-rs#2448](https://github.com/smithy-lang/smithy-rs/issues/2448)) The modules in generated client crates have been reorganized. See the [Client Crate Reorganization Upgrade Guidance](https://github.com/smithy-lang/smithy-rs/discussions/2449) to see how to fix your code after this change.
- ⚠ (server, [smithy-rs#2438](https://github.com/smithy-lang/smithy-rs/issues/2438)) Servers can send the `ServerRequestId` in the response headers.
    Servers need to create their service using the new layer builder `ServerRequestIdProviderLayer::new_with_response_header`:
    ```
    let app = app
        .layer(&ServerRequestIdProviderLayer::new_with_response_header(HeaderName::from_static("x-request-id")));
    ```

**New this release:**
- 🐛🎉 (client, [aws-sdk-rust#740](https://github.com/awslabs/aws-sdk-rust/issues/740)) Fluent builder methods on the client are now marked as deprecated when the related operation is deprecated.
- 🎉 (all, [smithy-rs#2398](https://github.com/smithy-lang/smithy-rs/issues/2398)) Add support for the `awsQueryCompatible` trait. This allows services to continue supporting a custom error code (via the `awsQueryError` trait) when the services migrate their protocol from `awsQuery` to `awsJson1_0` annotated with `awsQueryCompatible`.
    <details>
    <summary>Click to expand for more details...</summary>

    After the migration, services will include an additional header `x-amzn-query-error` in their responses whose value is in the form of `<error code>;<error type>`. An example response looks something like
    ```
    HTTP/1.1 400
    x-amzn-query-error: AWS.SimpleQueueService.NonExistentQueue;Sender
    Date: Wed, 08 Sep 2021 23:46:52 GMT
    Content-Type: application/x-amz-json-1.0
    Content-Length: 163

    {
        "__type": "com.amazonaws.sqs#QueueDoesNotExist",
        "message": "some user-visible message"
    }
    ```
    `<error code>` is `AWS.SimpleQueueService.NonExistentQueue` and `<error type>` is `Sender`.

    If an operation results in an error that causes a service to send back the response above, you can access `<error code>` and `<error type>` as follows:
    ```rust
    match client.some_operation().send().await {
        Ok(_) => { /* success */ }
        Err(sdk_err) => {
            let err = sdk_err.into_service_error();
            assert_eq!(
                error.meta().code(),
                Some("AWS.SimpleQueueService.NonExistentQueue"),
            );
            assert_eq!(error.meta().extra("type"), Some("Sender"));
        }
    }
    </details>
    ```
- 🎉 (client, [smithy-rs#2428](https://github.com/smithy-lang/smithy-rs/issues/2428), [smithy-rs#2208](https://github.com/smithy-lang/smithy-rs/issues/2208)) `SdkError` variants can now be constructed for easier unit testing.
- 🐛 (server, [smithy-rs#2441](https://github.com/smithy-lang/smithy-rs/issues/2441)) Fix `FilterByOperationName` plugin. This previous caused services with this applied to fail to compile due to mismatched bounds.
- (client, [smithy-rs#2437](https://github.com/smithy-lang/smithy-rs/issues/2437), [aws-sdk-rust#600](https://github.com/awslabs/aws-sdk-rust/issues/600)) Add more client re-exports. Specifically, it re-exports `aws_smithy_http::body::SdkBody`, `aws_smithy_http::byte_stream::error::Error`, and `aws_smithy_http::operation::{Request, Response}`.
- 🐛 (all, [smithy-rs#2226](https://github.com/smithy-lang/smithy-rs/issues/2226)) Fix bug in timestamp format resolution. Prior to this fix, the timestamp format may have been incorrect if set on the target instead of on the member.
- (all, [smithy-rs#2226](https://github.com/smithy-lang/smithy-rs/issues/2226)) Add support for offsets when parsing datetimes. RFC3339 date times now support offsets like `-0200`
- (client, [aws-sdk-rust#160](https://github.com/awslabs/aws-sdk-rust/issues/160), [smithy-rs#2445](https://github.com/smithy-lang/smithy-rs/issues/2445)) Reconnect on transient errors.

    Note: **this behavior is disabled by default for generic clients**. It can be enabled with
    `aws_smithy_client::Builder::reconnect_on_transient_errors`

    If a transient error (timeout, 500, 503, 503) is encountered, the connection will be evicted from the pool and will not
    be reused.
- (all, [smithy-rs#2474](https://github.com/smithy-lang/smithy-rs/issues/2474)) Increase Tokio version to 1.23.1 for all crates. This is to address [RUSTSEC-2023-0001](https://rustsec.org/advisories/RUSTSEC-2023-0001)


January 25th, 2023
==================
**New this release:**
- 🐛 (server, [smithy-rs#920](https://github.com/smithy-lang/smithy-rs/issues/920)) Fix bug in `OperationExtensionFuture`s `Future::poll` implementation


January 24th, 2023
==================
**Breaking Changes:**
- ⚠ (server, [smithy-rs#2161](https://github.com/smithy-lang/smithy-rs/issues/2161)) Remove deprecated service builder, this includes:

    - Remove `aws_smithy_http_server::routing::Router` and `aws_smithy_http_server::request::RequestParts`.
    - Move the `aws_smithy_http_server::routers::Router` trait and `aws_smithy_http_server::routing::RoutingService` into `aws_smithy_http_server::routing`.
    - Remove the following from the generated SDK:
        - `operation_registry.rs`
        - `operation_handler.rs`
        - `server_operation_handler_trait.rs`

    If migration to the new service builder API has not already been completed a brief summary of required changes can be seen in [previous release notes](https://github.com/smithy-lang/smithy-rs/releases/tag/release-2022-12-12) and in API documentation of the root crate.

**New this release:**
- 🐛 (server, [smithy-rs#2213](https://github.com/smithy-lang/smithy-rs/issues/2213)) `@sparse` list shapes and map shapes with constraint traits and with constrained members are now supported
- 🐛 (server, [smithy-rs#2200](https://github.com/smithy-lang/smithy-rs/pull/2200)) Event streams no longer generate empty error enums when their operations don’t have modeled errors
- (all, [smithy-rs#2223](https://github.com/smithy-lang/smithy-rs/issues/2223)) `aws_smithy_types::date_time::DateTime`, `aws_smithy_types::Blob` now implement the `Eq` and `Hash` traits
- (server, [smithy-rs#2223](https://github.com/smithy-lang/smithy-rs/issues/2223)) Code-generated types for server SDKs now implement the `Eq` and `Hash` traits when possible


January 12th, 2023
==================
**New this release:**
- 🐛 (server, [smithy-rs#2201](https://github.com/smithy-lang/smithy-rs/issues/2201)) Fix severe bug where a router fails to deserialize percent-encoded query strings, reporting no operation match when there could be one. If your Smithy model uses an operation with a request URI spec containing [query string literals](https://smithy.io/2.0/spec/http-bindings.html#query-string-literals), you are affected. This fix was released in `aws-smithy-http-server` v0.53.1.


January 11th, 2023
==================
**Breaking Changes:**
- ⚠ (client, [smithy-rs#2099](https://github.com/smithy-lang/smithy-rs/issues/2099)) The Rust client codegen plugin is now called `rust-client-codegen` instead of `rust-codegen`. Be sure to update your `smithy-build.json` files to refer to the correct plugin name.
- ⚠ (client, [smithy-rs#2099](https://github.com/smithy-lang/smithy-rs/issues/2099)) Client codegen plugins need to define a service named `software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator` (this is the new file name for the plugin definition in `resources/META-INF/services`).
- ⚠ (server, [smithy-rs#2099](https://github.com/smithy-lang/smithy-rs/issues/2099)) Server codegen plugins need to define a service named `software.amazon.smithy.rust.codegen.server.smithy.customize.ServerCodegenDecorator` (this is the new file name for the plugin definition in `resources/META-INF/services`).

**New this release:**
- 🐛 (server, [smithy-rs#2103](https://github.com/smithy-lang/smithy-rs/issues/2103)) In 0.52, `@length`-constrained collection shapes whose members are not constrained made the server code generator crash. This has been fixed.
- (server, [smithy-rs#1879](https://github.com/smithy-lang/smithy-rs/issues/1879)) Servers support the `@default` trait: models can specify default values. Default values will be automatically supplied when not manually set.
- (server, [smithy-rs#2131](https://github.com/smithy-lang/smithy-rs/issues/2131)) The constraint `@length` on non-streaming blob shapes is supported.
- 🐛 (client, [smithy-rs#2150](https://github.com/smithy-lang/smithy-rs/issues/2150)) Fix bug where string default values were not supported for endpoint parameters
- 🐛 (all, [smithy-rs#2170](https://github.com/smithy-lang/smithy-rs/issues/2170), [aws-sdk-rust#706](https://github.com/awslabs/aws-sdk-rust/issues/706)) Remove the webpki-roots feature from `hyper-rustls`
- 🐛 (server, [smithy-rs#2054](https://github.com/smithy-lang/smithy-rs/issues/2054)) Servers can generate a unique request ID and use it in their handlers.


December 12th, 2022
===================
**Breaking Changes:**
- ⚠🎉 (all, [smithy-rs#1938](https://github.com/smithy-lang/smithy-rs/issues/1938), @jjantdev) Upgrade Rust MSRV to 1.62.1
- ⚠🎉 (server, [smithy-rs#1199](https://github.com/smithy-lang/smithy-rs/issues/1199), [smithy-rs#1342](https://github.com/smithy-lang/smithy-rs/issues/1342), [smithy-rs#1401](https://github.com/smithy-lang/smithy-rs/issues/1401), [smithy-rs#1998](https://github.com/smithy-lang/smithy-rs/issues/1998), [smithy-rs#2005](https://github.com/smithy-lang/smithy-rs/issues/2005), [smithy-rs#2028](https://github.com/smithy-lang/smithy-rs/issues/2028), [smithy-rs#2034](https://github.com/smithy-lang/smithy-rs/issues/2034), [smithy-rs#2036](https://github.com/smithy-lang/smithy-rs/issues/2036)) [Constraint traits](https://awslabs.github.io/smithy/2.0/spec/constraint-traits.html) in server SDKs are beginning to be supported. The following are now supported:

    * The `length` trait on `string` shapes.
    * The `length` trait on `map` shapes.
    * The `length` trait on `list` shapes.
    * The `range` trait on `byte` shapes.
    * The `range` trait on `short` shapes.
    * The `range` trait on `integer` shapes.
    * The `range` trait on `long` shapes.
    * The `pattern` trait on `string` shapes.

    Upon receiving a request that violates the modeled constraints, the server SDK will reject it with a message indicating why.

    Unsupported (constraint trait, target shape) combinations will now fail at code generation time, whereas previously they were just ignored. This is a breaking change to raise awareness in service owners of their server SDKs behaving differently than what was modeled. To continue generating a server SDK with unsupported constraint traits, set `codegen.ignoreUnsupportedConstraints` to `true` in your `smithy-build.json`.

    ```json
    {
        ...
        "rust-server-codegen": {
            ...
            "codegen": {
                "ignoreUnsupportedConstraints": true
            }
        }
    }
    ```
- ⚠🎉 (server, [smithy-rs#1342](https://github.com/smithy-lang/smithy-rs/issues/1342), [smithy-rs#1119](https://github.com/smithy-lang/smithy-rs/issues/1119)) Server SDKs now generate "constrained types" for constrained shapes. Constrained types are [newtypes](https://rust-unofficial.github.io/patterns/patterns/behavioural/newtype.html) that encapsulate the modeled constraints. They constitute a [widespread pattern to guarantee domain invariants](https://www.lpalmieri.com/posts/2020-12-11-zero-to-production-6-domain-modelling/) and promote correctness in your business logic. So, for example, the model:

    ```smithy
    @length(min: 1, max: 69)
    string NiceString
    ```

    will now render a `struct NiceString(String)`. Instantiating a `NiceString` is a fallible operation:

    ```rust
    let data: String = ... ;
    let nice_string = NiceString::try_from(data).expect("data is not nice");
    ```

    A failed attempt to instantiate a constrained type will yield a `ConstraintViolation` error type you may want to handle. This type's API is subject to change.

    Constrained types _guarantee_, by virtue of the type system, that your service's operation outputs adhere to the modeled constraints. To learn more about the motivation for constrained types and how they work, see [the RFC](https://github.com/smithy-lang/smithy-rs/pull/1199).

    If you'd like to opt-out of generating constrained types, you can set `codegen.publicConstrainedTypes` to `false`. Note that if you do, the generated server SDK will still honor your operation input's modeled constraints upon receiving a request, but will not help you in writing business logic code that adheres to the constraints, and _will not prevent you from returning responses containing operation outputs that violate said constraints_.

    ```json
    {
        ...
        "rust-server-codegen": {
            ...
            "codegen": {
                "publicConstrainedTypes": false
            }
        }
    }
    ```
- 🐛⚠🎉 (server, [smithy-rs#1714](https://github.com/smithy-lang/smithy-rs/issues/1714), [smithy-rs#1342](https://github.com/smithy-lang/smithy-rs/issues/1342)) Structure builders in server SDKs have undergone significant changes.

    The API surface has been reduced. It is now simpler and closely follows what you would get when using the [`derive_builder`](https://docs.rs/derive_builder/latest/derive_builder/) crate:

    1. Builders no longer have `set_*` methods taking in `Option<T>`. You must use the unprefixed method, named exactly after the structure's field name, and taking in a value _whose type matches exactly that of the structure's field_.
    2. Builders no longer have convenience methods to pass in an element for a field whose type is a vector or a map. You must pass in the entire contents of the collection up front.
    3. Builders no longer implement [`PartialEq`](https://doc.rust-lang.org/std/cmp/trait.PartialEq.html).

    Bug fixes:

    4. Builders now always fail to build if a value for a `required` member is not provided. Previously, builders were falling back to a default value (e.g. `""` for `String`s) for some shapes. This was a bug.

    Additions:

    5. A structure `Structure` with builder `Builder` now implements `TryFrom<Builder> for Structure` or `From<Builder> for Structure`, depending on whether the structure [is constrained](https://awslabs.github.io/smithy/2.0/spec/constraint-traits.html) or not, respectively.

    To illustrate how to migrate to the new API, consider the example model below.

    ```smithy
    structure Pokemon {
        @required
        name: String,
        @required
        description: String,
        @required
        evolvesTo: PokemonList
    }

    list PokemonList {
        member: Pokemon
    }
    ```

    In the Rust code below, note the references calling out the changes described in the numbered list above.

    Before:

    ```rust
    let eevee_builder = Pokemon::builder()
        // (1) `set_description` takes in `Some<String>`.
        .set_description(Some("Su código genético es muy inestable. Puede evolucionar en diversas razas de Pokémon.".to_owned()))
        // (2) Convenience method to add one element to the `evolvesTo` list.
        .evolves_to(vaporeon)
        .evolves_to(jolteon)
        .evolves_to(flareon);

    // (3) Builder types can be compared.
    assert_ne!(eevee_builder, Pokemon::builder());

    // (4) Builds fine even though we didn't provide a value for `name`, which is `required`!
    let _eevee = eevee_builder.build();
    ```

    After:

    ```rust
    let eevee_builder = Pokemon::builder()
        // (1) `set_description` no longer exists. Use `description`, which directly takes in `String`.
        .description("Su código genético es muy inestable. Puede evolucionar en diversas razas de Pokémon.".to_owned())
        // (2) Convenience methods removed; provide the entire collection up front.
        .evolves_to(vec![vaporeon, jolteon, flareon]);

    // (3) Binary operation `==` cannot be applied to `pokemon::Builder`.
    // assert_ne!(eevee_builder, Pokemon::builder());

    // (4) `required` member `name` was not set.
    // (5) Builder type can be fallibly converted to the structure using `TryFrom` or `TryInto`.
    let _error = Pokemon::try_from(eevee_builder).expect_err("name was not provided");
    ```
- ⚠🎉 (server, [smithy-rs#1620](https://github.com/smithy-lang/smithy-rs/issues/1620), [smithy-rs#1666](https://github.com/smithy-lang/smithy-rs/issues/1666), [smithy-rs#1731](https://github.com/smithy-lang/smithy-rs/issues/1731), [smithy-rs#1736](https://github.com/smithy-lang/smithy-rs/issues/1736), [smithy-rs#1753](https://github.com/smithy-lang/smithy-rs/issues/1753), [smithy-rs#1738](https://github.com/smithy-lang/smithy-rs/issues/1738), [smithy-rs#1782](https://github.com/smithy-lang/smithy-rs/issues/1782), [smithy-rs#1829](https://github.com/smithy-lang/smithy-rs/issues/1829), [smithy-rs#1837](https://github.com/smithy-lang/smithy-rs/issues/1837), [smithy-rs#1891](https://github.com/smithy-lang/smithy-rs/issues/1891), [smithy-rs#1840](https://github.com/smithy-lang/smithy-rs/issues/1840), [smithy-rs#1844](https://github.com/smithy-lang/smithy-rs/issues/1844), [smithy-rs#1858](https://github.com/smithy-lang/smithy-rs/issues/1858), [smithy-rs#1930](https://github.com/smithy-lang/smithy-rs/issues/1930), [smithy-rs#1999](https://github.com/smithy-lang/smithy-rs/issues/1999), [smithy-rs#2003](https://github.com/smithy-lang/smithy-rs/issues/2003), [smithy-rs#2008](https://github.com/smithy-lang/smithy-rs/issues/2008), [smithy-rs#2010](https://github.com/smithy-lang/smithy-rs/issues/2010), [smithy-rs#2019](https://github.com/smithy-lang/smithy-rs/issues/2019), [smithy-rs#2020](https://github.com/smithy-lang/smithy-rs/issues/2020), [smithy-rs#2021](https://github.com/smithy-lang/smithy-rs/issues/2021), [smithy-rs#2038](https://github.com/smithy-lang/smithy-rs/issues/2038), [smithy-rs#2039](https://github.com/smithy-lang/smithy-rs/issues/2039), [smithy-rs#2041](https://github.com/smithy-lang/smithy-rs/issues/2041)) ### Plugins/New Service Builder API

    The `Router` struct has been replaced by a new `Service` located at the root of the generated crate. Its name coincides with the same name as the Smithy service you are generating.

    ```rust
    use pokemon_service_server_sdk::PokemonService;
    ```

    The new service builder infrastructure comes with a `Plugin` system which supports middleware on `smithy-rs`. See the [mididleware documentation](https://github.com/smithy-lang/smithy-rs/blob/main/design/src/server/middleware.md) and the [API documentation](https://docs.rs/aws-smithy-http-server/latest/aws_smithy_http_server/plugin/index.html) for more details.

    Usage of the new service builder API:

    ```rust
    // Apply a sequence of plugins using `PluginPipeline`.
    let plugins = PluginPipeline::new()
        // Apply the `PrintPlugin`.
        // This is a dummy plugin found in `rust-runtime/aws-smithy-http-server/examples/pokemon-service/src/plugin.rs`
        .print()
        // Apply the `InstrumentPlugin` plugin, which applies `tracing` instrumentation.
        .instrument();

    // Construct the service builder using the `plugins` defined above.
    let app = PokemonService::builder_with_plugins(plugins)
        // Assign all the handlers.
        .get_pokemon_species(get_pokemon_species)
        .get_storage(get_storage)
        .get_server_statistics(get_server_statistics)
        .capture_pokemon(capture_pokemon)
        .do_nothing(do_nothing)
        .check_health(check_health)
        // Construct the `PokemonService`.
        .build()
        // If handlers are missing a descriptive error will be provided.
        .expect("failed to build an instance of `PokemonService`");
    ```

    See the `rust-runtime/aws-smithy-http-server/examples/pokemon-service/src/bin` folder for various working examples.

    ### Public `FromParts` trait

    Previously, we only supported one [`Extension`](https://docs.rs/aws-smithy-http-server/latest/aws_smithy_http_server/request/struct.Extension.html) as an additional argument provided to the handler. This number has been increased to 8 and the argument type has been broadened to any struct which implements the [`FromParts`](https://docs.rs/aws-smithy-http-server/latest/aws_smithy_http_server/request/trait.FromParts.html) trait. The trait is publicly exported and therefore provides customers with the ability to extend the domain of the handlers.

    As noted, a ubiqutious example of a struct that implements `FromParts` is the `Extension` struct, which extracts state from the `Extensions` typemap of a [`http::Request`](https://docs.rs/http/latest/http/request/struct.Request.html). A new example is the `ConnectInfo` struct which allows handlers to access the connection data. See the `rust-runtime/aws-smithy-http-server/examples/pokemon-service/src/bin/pokemon-service-connect-info.rs` example.

    ```rust
    fn get_pokemon_species(
        input: GetPokemonSpeciesInput,
        state: Extension<State>,
        address: ConnectInfo<SocketAddr>
    ) -> Result<GetPokemonSpeciesOutput, GetPokemonSpeciesError> {
        todo!()
    }
    ```

    In addition to the [`ConnectInfo`](https://docs.rs/aws-smithy-http-server/latest/aws_smithy_http_server/request/connect_info/struct.ConnectInfo.html) extractor, we also have added [lambda extractors](https://docs.rs/aws-smithy-http-server/latest/aws_smithy_http_server/request/lambda/index.html) which are feature gated with `aws-lambda`.

    [`FromParts` documentation](https://github.com/smithy-lang/smithy-rs/blob/main/design/src/server/from_parts.md) has been added.

    ### New Documentation

    New sections to have been added to the [server side of the book](https://github.com/smithy-lang/smithy-rs/blob/main/design/src/server/overview.md).

    These include:

    - [Middleware](https://github.com/smithy-lang/smithy-rs/blob/main/design/src/server/middleware.md)
    - [Accessing Un-modelled Data](https://github.com/smithy-lang/smithy-rs/blob/main/design/src/server/from_parts.md)
    - [Anatomy of a Service](https://github.com/smithy-lang/smithy-rs/blob/main/design/src/server/anatomy.md)

    This release also introduces extensive documentation at the root of the generated crate. For best results compile documentation with `cargo +nightly doc --open`.

    ### Deprecations

    The existing service builder infrastructure, `OperationRegistryBuilder`/`OperationRegistry`/`Router`, is now deprecated. Customers should migrate to the newer scheme described above. The deprecated types will be removed in a future release.
- ⚠ (client, [smithy-rs#1875](https://github.com/smithy-lang/smithy-rs/issues/1875)) Replace bool with enum for a function parameter of `label::fmt_string`.
- ⚠ (all, [smithy-rs#1980](https://github.com/smithy-lang/smithy-rs/issues/1980)) aws_smithy_types_convert::date_time::DateTimeExt::to_chrono_utc returns a Result<>
- ⚠ (client, [smithy-rs#1926](https://github.com/smithy-lang/smithy-rs/issues/1926), [smithy-rs#1819](https://github.com/smithy-lang/smithy-rs/issues/1819)) Several breaking changes have been made to errors. See [the upgrade guide](https://github.com/smithy-lang/smithy-rs/issues/1950) for more information.
- 🐛⚠ (server, [smithy-rs#1714](https://github.com/smithy-lang/smithy-rs/issues/1714), [smithy-rs#1342](https://github.com/smithy-lang/smithy-rs/issues/1342), [smithy-rs#1860](https://github.com/smithy-lang/smithy-rs/issues/1860)) Server SDKs now correctly reject operation inputs that don't set values for `required` structure members. Previously, in some scenarios, server SDKs would accept the request and set a default value for the member (e.g. `""` for a `String`), even when the member shape did not have [Smithy IDL v2's `default` trait](https://awslabs.github.io/smithy/2.0/spec/type-refinement-traits.html#smithy-api-default-trait) attached. The `default` trait is [still unsupported](https://github.com/smithy-lang/smithy-rs/issues/1860).
- ⚠ (client, [smithy-rs#1945](https://github.com/smithy-lang/smithy-rs/issues/1945)) Generate enums that guide the users to write match expressions in a forward-compatible way.
    Before this change, users could write a match expression against an enum in a non-forward-compatible way:
    ```rust
    match some_enum {
        SomeEnum::Variant1 => { /* ... */ },
        SomeEnum::Variant2 => { /* ... */ },
        Unknown(value) if value == "NewVariant" => { /* ... */ },
        _ => { /* ... */ },
    }
    ```
    This code can handle a case for "NewVariant" with a version of SDK where the enum does not yet include `SomeEnum::NewVariant`, but breaks with another version of SDK where the enum defines `SomeEnum::NewVariant` because the execution will hit a different match arm, i.e. the last one.
    After this change, users are guided to write the above match expression as follows:
    ```rust
    match some_enum {
        SomeEnum::Variant1 => { /* ... */ },
        SomeEnum::Variant2 => { /* ... */ },
        other @ _ if other.as_str() == "NewVariant" => { /* ... */ },
        _ => { /* ... */ },
    }
    ```
    This is forward-compatible because the execution will hit the second last match arm regardless of whether the enum defines `SomeEnum::NewVariant` or not.
- ⚠ (client, [smithy-rs#1984](https://github.com/smithy-lang/smithy-rs/issues/1984), [smithy-rs#1496](https://github.com/smithy-lang/smithy-rs/issues/1496)) Functions on `aws_smithy_http::endpoint::Endpoint` now return a `Result` instead of panicking.
- ⚠ (client, [smithy-rs#1984](https://github.com/smithy-lang/smithy-rs/issues/1984), [smithy-rs#1496](https://github.com/smithy-lang/smithy-rs/issues/1496)) `Endpoint::mutable` now takes `impl AsRef<str>` instead of `Uri`. For the old functionality, use `Endpoint::mutable_uri`.
- ⚠ (client, [smithy-rs#1984](https://github.com/smithy-lang/smithy-rs/issues/1984), [smithy-rs#1496](https://github.com/smithy-lang/smithy-rs/issues/1496)) `Endpoint::immutable` now takes `impl AsRef<str>` instead of `Uri`. For the old functionality, use `Endpoint::immutable_uri`.
- ⚠ (server, [smithy-rs#1982](https://github.com/smithy-lang/smithy-rs/issues/1982)) [RestJson1](https://awslabs.github.io/smithy/2.0/aws/protocols/aws-restjson1-protocol.html#operation-error-serialization) server SDKs now serialize the [full shape ID](https://smithy.io/2.0/spec/model.html#shape-id) (including namespace) in operation error responses.

    Example server error response before:

    ```
    HTTP/1.1 400 Bad Request
    content-type: application/json
    x-amzn-errortype: InvalidRequestException
    ...
    ```

    Example server error response now:

    ```
    HTTP/1.1 400 Bad Request
    content-type: application/json
    x-amzn-errortype: com.example.service#InvalidRequestException
    ...
    ```
- ⚠ (server, [smithy-rs#2035](https://github.com/smithy-lang/smithy-rs/issues/2035)) All types that are exclusively relevant within the context of an AWS Lambda function are now gated behind the
    `aws-lambda` feature flag.

    This will reduce the number of dependencies (and improve build times) for users that are running their Smithy services
    in non-serverless environments (e.g. via `hyper`).
- ⚠ (all, [smithy-rs#1983](https://github.com/smithy-lang/smithy-rs/issues/1983), [smithy-rs#2029](https://github.com/smithy-lang/smithy-rs/issues/2029)) Implementation of the Debug trait for container shapes now redacts what is printed per the sensitive trait.
- ⚠ (client, [smithy-rs#2065](https://github.com/smithy-lang/smithy-rs/issues/2065)) `SdkBody` callbacks have been removed. If you were using these, please [file an issue](https://github.com/smithy-lang/smithy-rs/issues/new) so that we can better understand your use-case and provide the support you need.
- ⚠ (client, [smithy-rs#2063](https://github.com/smithy-lang/smithy-rs/issues/2063)) Added SmithyEndpointStage which can be used to set an endpoint for smithy-native clients
- ⚠ (all, [smithy-rs#1989](https://github.com/smithy-lang/smithy-rs/issues/1989)) The Unit type for a Union member is no longer rendered. The serializers and parsers generated now function accordingly in the absence of the inner data associated with the Unit type.

**New this release:**
- 🎉 (all, [smithy-rs#1929](https://github.com/smithy-lang/smithy-rs/issues/1929)) Upgrade Smithy to v1.26
- 🎉 (client, [smithy-rs#2044](https://github.com/smithy-lang/smithy-rs/issues/2044), [smithy-rs#371](https://github.com/smithy-lang/smithy-rs/issues/371)) Fixed and improved the request `tracing` span hierarchy to improve log messages, profiling, and debuggability.
- 🐛 (all, [smithy-rs#1847](https://github.com/smithy-lang/smithy-rs/issues/1847)) Support Sigv4 signature generation on PowerPC 32 and 64 bit. This architecture cannot compile `ring`, so the implementation has been updated to rely on `hamc` + `sha2` to achive the same result with broader platform compatibility and higher performance. We also updated the CI which is now running as many tests as possible against i686 and PowerPC 32 and 64 bit.
- 🐛 (server, [smithy-rs#1910](https://github.com/smithy-lang/smithy-rs/issues/1910)) `aws_smithy_http_server::routing::Router` is exported from the crate root again. This reverts unintentional breakage that was introduced in `aws-smithy-http-server` v0.51.0 only.
- 🐛 (client, [smithy-rs#1903](https://github.com/smithy-lang/smithy-rs/issues/1903), [smithy-rs#1902](https://github.com/smithy-lang/smithy-rs/issues/1902)) Fix bug that can cause panics in paginators
- (client, [smithy-rs#1919](https://github.com/smithy-lang/smithy-rs/issues/1919)) Operation metadata is now added to the property bag before sending requests allowing middlewares to behave
    differently depending on the operation being sent.
- (all, [smithy-rs#1907](https://github.com/smithy-lang/smithy-rs/issues/1907)) Fix cargo audit issue on chrono.
- 🐛 (client, [smithy-rs#1957](https://github.com/smithy-lang/smithy-rs/issues/1957)) It was previously possible to send requests without setting query parameters modeled as required. Doing this may cause a
    service to interpret a request incorrectly instead of just sending back a 400 error. Now, when an operation has query
    parameters that are marked as required, the omission of those query parameters will cause a BuildError, preventing the
    invalid operation from being sent.
- (all, [smithy-rs#1972](https://github.com/smithy-lang/smithy-rs/issues/1972)) Upgrade to Smithy 1.26.2
- (all, [smithy-rs#2011](https://github.com/smithy-lang/smithy-rs/issues/2011), @lsr0) Make generated enum `values()` functions callable in const contexts.
- (client, [smithy-rs#2064](https://github.com/smithy-lang/smithy-rs/issues/2064), [aws-sdk-rust#632](https://github.com/awslabs/aws-sdk-rust/issues/632)) Clients now default max idle connections to 70 (previously unlimited) to reduce the likelihood of hitting max file handles in AWS Lambda.
- (client, [smithy-rs#2057](https://github.com/smithy-lang/smithy-rs/issues/2057), [smithy-rs#371](https://github.com/smithy-lang/smithy-rs/issues/371)) Add more `tracing` events to signing and event streams

**Contributors**
Thank you for your contributions! ❤
- @jjantdev ([smithy-rs#1938](https://github.com/smithy-lang/smithy-rs/issues/1938))
- @lsr0 ([smithy-rs#2011](https://github.com/smithy-lang/smithy-rs/issues/2011))

October 24th, 2022
==================
**Breaking Changes:**
- ⚠ (all, [smithy-rs#1825](https://github.com/smithy-lang/smithy-rs/issues/1825)) Bump MSRV to be 1.62.0.
- ⚠ (server, [smithy-rs#1825](https://github.com/smithy-lang/smithy-rs/issues/1825)) Bump pyo3 and pyo3-asyncio from 0.16.x to 0.17.0 for aws-smithy-http-server-python.
- ⚠ (client, [smithy-rs#1811](https://github.com/smithy-lang/smithy-rs/issues/1811)) Replace all usages of `AtomicU64` with `AtomicUsize` to support 32bit targets.
- ⚠ (server, [smithy-rs#1803](https://github.com/smithy-lang/smithy-rs/issues/1803)) Mark `operation` and `operation_handler` modules as private in the generated server crate.
    Both modules did not contain any public types, therefore there should be no actual breakage when updating.
- ⚠ (client, [smithy-rs#1740](https://github.com/smithy-lang/smithy-rs/issues/1740), [smithy-rs#256](https://github.com/smithy-lang/smithy-rs/issues/256)) A large list of breaking changes were made to accomodate default timeouts in the AWS SDK.
    See [the smithy-rs upgrade guide](https://github.com/smithy-lang/smithy-rs/issues/1760) for a full list
    of breaking changes and how to resolve them.
- ⚠ (server, [smithy-rs#1829](https://github.com/smithy-lang/smithy-rs/issues/1829)) Remove `Protocol` enum, removing an obstruction to extending smithy to third-party protocols.
- ⚠ (server, [smithy-rs#1829](https://github.com/smithy-lang/smithy-rs/issues/1829)) Convert the `protocol` argument on `PyMiddlewares::new` constructor to a type parameter.
- ⚠ (server, [smithy-rs#1753](https://github.com/smithy-lang/smithy-rs/issues/1753)) `aws_smithy_http_server::routing::Router` is no longer exported from the crate root. This was unintentional breakage that will be reverted in the next release.

**New this release:**
- (server, [smithy-rs#1811](https://github.com/smithy-lang/smithy-rs/issues/1811)) Replace all usages of `AtomicU64` with `AtomicUsize` to support 32bit targets.
- 🐛 (all, [smithy-rs#1802](https://github.com/smithy-lang/smithy-rs/issues/1802)) Sensitive fields in errors now respect @sensitive trait and are properly redacted.
- (server, [smithy-rs#1727](https://github.com/smithy-lang/smithy-rs/issues/1727), @GeneralSwiss) Pokémon Service example code now runs clippy during build.
- (server, [smithy-rs#1734](https://github.com/smithy-lang/smithy-rs/issues/1734)) Implement support for pure Python request middleware. Improve idiomatic logging support over tracing.
- 🐛 (client, [aws-sdk-rust#620](https://github.com/awslabs/aws-sdk-rust/issues/620), [smithy-rs#1748](https://github.com/smithy-lang/smithy-rs/issues/1748)) Paginators now stop on encountering a duplicate token by default rather than panic. This behavior can be customized by toggling the `stop_on_duplicate_token` property on the paginator before calling `send`.
- 🐛 (all, [smithy-rs#1817](https://github.com/smithy-lang/smithy-rs/issues/1817), @ethyi) Update aws-types zeroize to flexible version to prevent downstream version conflicts.
- (all, [smithy-rs#1852](https://github.com/smithy-lang/smithy-rs/issues/1852), @ogudavid) Enable local maven repo dependency override.

**Contributors**
Thank you for your contributions! ❤
- @GeneralSwiss ([smithy-rs#1727](https://github.com/smithy-lang/smithy-rs/issues/1727))
- @ethyi ([smithy-rs#1817](https://github.com/smithy-lang/smithy-rs/issues/1817))
- @ogudavid ([smithy-rs#1852](https://github.com/smithy-lang/smithy-rs/issues/1852))

September 20th, 2022
====================
**Breaking Changes:**
- ⚠ (client, [smithy-rs#1603](https://github.com/smithy-lang/smithy-rs/issues/1603), [aws-sdk-rust#586](https://github.com/awslabs/aws-sdk-rust/issues/586)) `aws_smithy_types::RetryConfig` no longer implements `Default`, and its `new` function has been replaced with `standard`.
- ⚠ (client, [smithy-rs#1603](https://github.com/smithy-lang/smithy-rs/issues/1603), [aws-sdk-rust#586](https://github.com/awslabs/aws-sdk-rust/issues/586)) Client creation now panics if retries or timeouts are enabled without an async sleep implementation.
    If you're using the Tokio runtime and have the `rt-tokio` feature enabled (which is enabled by default),
    then you shouldn't notice this change at all.
    Otherwise, if using something other than Tokio as the async runtime, the `AsyncSleep` trait must be implemented,
    and that implementation given to the config builder via the `sleep_impl` method. Alternatively, retry can be
    explicitly turned off by setting `max_attempts` to 1, which will result in successful client creation without an
    async sleep implementation.
- ⚠ (client, [smithy-rs#1603](https://github.com/smithy-lang/smithy-rs/issues/1603), [aws-sdk-rust#586](https://github.com/awslabs/aws-sdk-rust/issues/586)) The `default_async_sleep` method on the `Client` builder has been removed. The default async sleep is
    wired up by default if none is provided.
- ⚠ (client, [smithy-rs#976](https://github.com/smithy-lang/smithy-rs/issues/976), [smithy-rs#1710](https://github.com/smithy-lang/smithy-rs/issues/1710)) Removed the need to generate operation output and retry aliases in codegen.
- ⚠ (client, [smithy-rs#1715](https://github.com/smithy-lang/smithy-rs/issues/1715), [smithy-rs#1717](https://github.com/smithy-lang/smithy-rs/issues/1717)) `ClassifyResponse` was renamed to `ClassifyRetry` and is no longer implemented for the unit type.
- ⚠ (client, [smithy-rs#1715](https://github.com/smithy-lang/smithy-rs/issues/1715), [smithy-rs#1717](https://github.com/smithy-lang/smithy-rs/issues/1717)) The `with_retry_policy` and `retry_policy` functions on `aws_smithy_http::operation::Operation` have been
    renamed to `with_retry_classifier` and `retry_classifier` respectively. Public member `retry_policy` on
    `aws_smithy_http::operation::Parts` has been renamed to `retry_classifier`.

**New this release:**
- 🎉 (client, [smithy-rs#1647](https://github.com/smithy-lang/smithy-rs/issues/1647), [smithy-rs#1112](https://github.com/smithy-lang/smithy-rs/issues/1112)) Implemented customizable operations per [RFC-0017](https://smithy-lang.github.io/smithy-rs/design/rfcs/rfc0017_customizable_client_operations.html).

    Before this change, modifying operations before sending them required using lower-level APIs:

    ```rust
    let input = SomeOperationInput::builder().some_value(5).build()?;

    let operation = {
        let op = input.make_operation(&service_config).await?;
        let (request, response) = op.into_request_response();

        let request = request.augment(|req, _props| {
            req.headers_mut().insert(
                HeaderName::from_static("x-some-header"),
                HeaderValue::from_static("some-value")
            );
            Result::<_, Infallible>::Ok(req)
        })?;

        Operation::from_parts(request, response)
    };

    let response = smithy_client.call(operation).await?;
    ```

    Now, users may easily modify operations before sending with the `customize` method:

    ```rust
    let response = client.some_operation()
        .some_value(5)
        .customize()
        .await?
        .mutate_request(|mut req| {
            req.headers_mut().insert(
                HeaderName::from_static("x-some-header"),
                HeaderValue::from_static("some-value")
            );
        })
        .send()
        .await?;
    ```
- (client, [smithy-rs#1735](https://github.com/smithy-lang/smithy-rs/issues/1735), @vojtechkral) Lower log level of two info-level log messages.
- (all, [smithy-rs#1710](https://github.com/smithy-lang/smithy-rs/issues/1710)) Added `writable` property to `RustType` and `RuntimeType` that returns them in `Writable` form
- (all, [smithy-rs#1680](https://github.com/smithy-lang/smithy-rs/issues/1680), @ogudavid) Smithy IDL v2 mixins are now supported
- 🐛 (client, [smithy-rs#1715](https://github.com/smithy-lang/smithy-rs/issues/1715), [smithy-rs#1717](https://github.com/smithy-lang/smithy-rs/issues/1717)) Generated clients now retry transient errors without replacing the retry policy.
- 🐛 (all, [smithy-rs#1725](https://github.com/smithy-lang/smithy-rs/issues/1725), @sugmanue) Correctly determine nullability of members in IDLv2 models

**Contributors**
Thank you for your contributions! ❤
- @ogudavid ([smithy-rs#1680](https://github.com/smithy-lang/smithy-rs/issues/1680))
- @sugmanue ([smithy-rs#1725](https://github.com/smithy-lang/smithy-rs/issues/1725))
- @vojtechkral ([smithy-rs#1735](https://github.com/smithy-lang/smithy-rs/issues/1735))

August 31st, 2022
=================
**Breaking Changes:**
- ⚠🎉 (client, [smithy-rs#1598](https://github.com/smithy-lang/smithy-rs/issues/1598)) Previously, the config customizations that added functionality related to retry configs, timeout configs, and the
    async sleep impl were defined in the smithy codegen module but were being loaded in the AWS codegen module. They
    have now been updated to be loaded during smithy codegen. The affected classes are all defined in the
    `software.amazon.smithy.rust.codegen.smithy.customizations` module of smithy codegen.` This change does not affect
    the generated code.

    These classes have been removed:
    - `RetryConfigDecorator`
    - `SleepImplDecorator`
    - `TimeoutConfigDecorator`

    These classes have been renamed:
    - `RetryConfigProviderConfig` is now `RetryConfigProviderCustomization`
    - `PubUseRetryConfig` is now `PubUseRetryConfigGenerator`
    - `SleepImplProviderConfig` is now `SleepImplProviderCustomization`
    - `TimeoutConfigProviderConfig` is now `TimeoutConfigProviderCustomization`
- ⚠🎉 (all, [smithy-rs#1635](https://github.com/smithy-lang/smithy-rs/issues/1635), [smithy-rs#1416](https://github.com/smithy-lang/smithy-rs/issues/1416), @weihanglo) Support granular control of specifying runtime crate versions.

    For code generation, the field `runtimeConfig.version` in smithy-build.json has been removed.
    The new field `runtimeConfig.versions` is an object whose keys are runtime crate names (e.g. `aws-smithy-http`),
    and values are user-specified versions.

    If you previously set `version = "DEFAULT"`, the migration path is simple.
    By setting `versions` with an empty object or just not setting it at all,
    the version number of the code generator will be used as the version for all runtime crates.

    If you specified a certain version such as `version = "0.47.0", you can migrate to a special reserved key `DEFAULT`.
    The equivalent JSON config would look like:

    ```json
    {
      "runtimeConfig": {
          "versions": {
              "DEFAULT": "0.47.0"
          }
      }
    }
    ```

    Then all runtime crates are set with version 0.47.0 by default unless overridden by specific crates. For example,

    ```json
    {
      "runtimeConfig": {
          "versions": {
              "DEFAULT": "0.47.0",
              "aws-smithy-http": "0.47.1"
          }
      }
    }
    ```

    implies that we're using `aws-smithy-http` 0.47.1 specifically. For the rest of the crates, it will default to 0.47.0.
- ⚠ (all, [smithy-rs#1623](https://github.com/smithy-lang/smithy-rs/issues/1623), @ogudavid) Remove @sensitive trait tests which applied trait to member. The ability to mark members with @sensitive was removed in Smithy 1.22.
- ⚠ (server, [smithy-rs#1544](https://github.com/smithy-lang/smithy-rs/issues/1544)) Servers now allow requests' ACCEPT header values to be:
    - `*/*`
    - `type/*`
    - `type/subtype`
- 🐛⚠ (all, [smithy-rs#1274](https://github.com/smithy-lang/smithy-rs/issues/1274)) Lossy converters into integer types for `aws_smithy_types::Number` have been
    removed. Lossy converters into floating point types for
    `aws_smithy_types::Number` have been suffixed with `_lossy`. If you were
    directly using the integer lossy converters, we recommend you use the safe
    converters.
    _Before:_
    ```rust
    fn f1(n: aws_smithy_types::Number) {
        let foo: f32 = n.to_f32(); // Lossy conversion!
        let bar: u32 = n.to_u32(); // Lossy conversion!
    }
    ```
    _After:_
    ```rust
    fn f1(n: aws_smithy_types::Number) {
        use std::convert::TryInto; // Unnecessary import if you're using Rust 2021 edition.
        let foo: f32 = n.try_into().expect("lossy conversion detected"); // Or handle the error instead of panicking.
        // You can still do lossy conversions, but only into floating point types.
        let foo: f32 = n.to_f32_lossy();
        // To lossily convert into integer types, use an `as` cast directly.
        let bar: u32 = n as u32; // Lossy conversion!
    }
    ```
- ⚠ (all, [smithy-rs#1699](https://github.com/smithy-lang/smithy-rs/issues/1699)) Bump [MSRV](https://github.com/awslabs/aws-sdk-rust#supported-rust-versions-msrv) from 1.58.1 to 1.61.0 per our policy.

**New this release:**
- 🎉 (all, [smithy-rs#1623](https://github.com/smithy-lang/smithy-rs/issues/1623), @ogudavid) Update Smithy dependency to 1.23.1. Models using version 2.0 of the IDL are now supported.
- 🎉 (server, [smithy-rs#1551](https://github.com/smithy-lang/smithy-rs/issues/1551), @hugobast) There is a canonical and easier way to run smithy-rs on Lambda [see example].

    [see example]: https://github.com/smithy-lang/smithy-rs/blob/main/rust-runtime/aws-smithy-http-server/examples/pokemon-service/src/lambda.rs
- 🐛 (all, [smithy-rs#1623](https://github.com/smithy-lang/smithy-rs/issues/1623), @ogudavid) Fix detecting sensitive members through their target shape having the @sensitive trait applied.
- (all, [smithy-rs#1623](https://github.com/smithy-lang/smithy-rs/issues/1623), @ogudavid) Fix SetShape matching needing to occur before ListShape since it is now a subclass. Sets were deprecated in Smithy 1.22.
- (all, [smithy-rs#1623](https://github.com/smithy-lang/smithy-rs/issues/1623), @ogudavid) Fix Union shape test data having an invalid empty union. Break fixed from Smithy 1.21 to Smithy 1.22.
- (all, [smithy-rs#1612](https://github.com/smithy-lang/smithy-rs/issues/1612), @unexge) Add codegen version to generated package metadata
- (client, [aws-sdk-rust#609](https://github.com/awslabs/aws-sdk-rust/issues/609)) It is now possible to exempt specific operations from XML body root checking. To do this, add the `AllowInvalidXmlRoot`
    trait to the output struct of the operation you want to exempt.

**Contributors**
Thank you for your contributions! ❤
- @hugobast ([smithy-rs#1551](https://github.com/smithy-lang/smithy-rs/issues/1551))
- @ogudavid ([smithy-rs#1623](https://github.com/smithy-lang/smithy-rs/issues/1623))
- @unexge ([smithy-rs#1612](https://github.com/smithy-lang/smithy-rs/issues/1612))
- @weihanglo ([smithy-rs#1416](https://github.com/smithy-lang/smithy-rs/issues/1416), [smithy-rs#1635](https://github.com/smithy-lang/smithy-rs/issues/1635))

August 4th, 2022
================
**Breaking Changes:**
- ⚠🎉 (all, [smithy-rs#1570](https://github.com/smithy-lang/smithy-rs/issues/1570), @weihanglo) Support @deprecated trait for aggregate shapes
- ⚠ (all, [smithy-rs#1157](https://github.com/smithy-lang/smithy-rs/issues/1157)) Rename EventStreamInput to EventStreamSender
- ⚠ (all, [smithy-rs#1157](https://github.com/smithy-lang/smithy-rs/issues/1157)) The type of streaming unions that contain errors is generated without those errors.
    Errors in a streaming union `Union` are generated as members of the type `UnionError`.
    Taking Transcribe as an example, the `AudioStream` streaming union generates, in the client, both the `AudioStream` type:
    ```rust
    pub enum AudioStream {
        AudioEvent(crate::model::AudioEvent),
        Unknown,
    }
    ```
    and its error type,
    ```rust
    pub struct AudioStreamError {
        /// Kind of error that occurred.
        pub kind: AudioStreamErrorKind,
        /// Additional metadata about the error, including error code, message, and request ID.
        pub(crate) meta: aws_smithy_types::Error,
    }
    ```
    `AudioStreamErrorKind` contains all error variants for the union.
    Before, the generated code looked as:
    ```rust
    pub enum AudioStream {
        AudioEvent(crate::model::AudioEvent),
        ... all error variants,
        Unknown,
    }
    ```
- ⚠ (all, [smithy-rs#1157](https://github.com/smithy-lang/smithy-rs/issues/1157)) `aws_smithy_http::event_stream::EventStreamSender` and `aws_smithy_http::event_stream::Receiver` are now generic over `<T, E>`,
    where `T` is a streaming union and `E` the union's errors.
    This means that event stream errors are now sent as `Err` of the union's error type.
    With this example model:
    ```smithy
    @streaming union Event {
        throttlingError: ThrottlingError
    }
    @error("client") structure ThrottlingError {}
    ```
    Before:
    ```rust
    stream! { yield Ok(Event::ThrottlingError ...) }
    ```
    After:
    ```rust
    stream! { yield Err(EventError::ThrottlingError ...) }
    ```
    An example from the SDK is in [transcribe streaming](https://github.com/smithy-lang/smithy-rs/blob/4f51dd450ea3234a7faf481c6025597f22f03805/aws/sdk/integration-tests/transcribestreaming/tests/test.rs#L80).

**New this release:**
- 🎉 (all, [smithy-rs#1482](https://github.com/smithy-lang/smithy-rs/issues/1482)) Update codegen to generate support for flexible checksums.
- (all, [smithy-rs#1520](https://github.com/smithy-lang/smithy-rs/issues/1520)) Add explicit cast during JSON deserialization in case of custom Symbol providers.
- (all, [smithy-rs#1578](https://github.com/smithy-lang/smithy-rs/issues/1578), @lkts) Change detailed logs in CredentialsProviderChain from info to debug
- (all, [smithy-rs#1573](https://github.com/smithy-lang/smithy-rs/issues/1573), [smithy-rs#1569](https://github.com/smithy-lang/smithy-rs/issues/1569)) Non-streaming struct members are now marked `#[doc(hidden)]` since they will be removed in the future

**Contributors**
Thank you for your contributions! ❤
- @lkts ([smithy-rs#1578](https://github.com/smithy-lang/smithy-rs/issues/1578))
- @weihanglo ([smithy-rs#1570](https://github.com/smithy-lang/smithy-rs/issues/1570))

July 20th, 2022
===============
**New this release:**
- 🎉 (all, [aws-sdk-rust#567](https://github.com/awslabs/aws-sdk-rust/issues/567)) Updated the smithy client's retry behavior to allow for a configurable initial backoff. Previously, the initial backoff
    (named `r` in the code) was set to 2 seconds. This is not an ideal default for services that expect clients to quickly
    retry failed request attempts. Now, users can set quicker (or slower) backoffs according to their needs.
- (all, [smithy-rs#1263](https://github.com/smithy-lang/smithy-rs/issues/1263)) Add checksum calculation and validation wrappers for HTTP bodies.
- (all, [smithy-rs#1263](https://github.com/smithy-lang/smithy-rs/issues/1263)) `aws_smithy_http::header::append_merge_header_maps`, a function for merging two `HeaderMap`s, is now public.


v0.45.0 (June 28th, 2022)
=========================
**Breaking Changes:**
- ⚠ ([smithy-rs#932](https://github.com/smithy-lang/smithy-rs/issues/932)) Replaced use of `pin-project` with equivalent `pin-project-lite`. For pinned enum tuple variants and tuple structs, this
    change requires that we switch to using enum struct variants and regular structs. Most of the structs and enums that
    were updated had only private fields/variants and so have the same public API. However, this change does affect the
    public API of `aws_smithy_http_tower::map_request::MapRequestFuture<F, E>`. The `Inner` and `Ready` variants contained a
    single value. Each have been converted to struct variants and the inner value is now accessible by the `inner` field
    instead of the `0` field.

**New this release:**
- 🎉 ([smithy-rs#1411](https://github.com/smithy-lang/smithy-rs/issues/1411), [smithy-rs#1167](https://github.com/smithy-lang/smithy-rs/issues/1167)) Upgrade to Gradle 7. This change is not a breaking change, however, users of smithy-rs will need to switch to JDK 17
- 🐛 ([smithy-rs#1505](https://github.com/smithy-lang/smithy-rs/issues/1505), @kiiadi) Fix issue with codegen on Windows where module names were incorrectly determined from filenames

**Contributors**
Thank you for your contributions! ❤
- @kiiadi ([smithy-rs#1505](https://github.com/smithy-lang/smithy-rs/issues/1505))
<!-- Do not manually edit this file, use `update-changelogs` -->
v0.44.0 (June 22nd, 2022)
=========================
**New this release:**
- ([smithy-rs#1460](https://github.com/smithy-lang/smithy-rs/issues/1460)) Fix a potential bug with `ByteStream`'s implementation of `futures_core::stream::Stream` and add helpful error messages
    for users on 32-bit systems that try to stream HTTP bodies larger than 4.29Gb.
- 🐛 ([smithy-rs#1427](https://github.com/smithy-lang/smithy-rs/issues/1427), [smithy-rs#1465](https://github.com/smithy-lang/smithy-rs/issues/1465), [smithy-rs#1459](https://github.com/smithy-lang/smithy-rs/issues/1459)) Fix RustWriter bugs for `rustTemplate` and `docs` utility methods
- 🐛 ([aws-sdk-rust#554](https://github.com/awslabs/aws-sdk-rust/issues/554)) Requests to Route53 that return `ResourceId`s often come with a prefix. When passing those IDs directly into another
    request, the request would fail unless they manually stripped the prefix. Now, when making a request with a prefixed ID,
    the prefix will be stripped automatically.


v0.43.0 (June 9th, 2022)
========================
**New this release:**
- 🎉 ([smithy-rs#1381](https://github.com/smithy-lang/smithy-rs/issues/1381), @alonlud) Add ability to sign a request with all headers, or to change which headers are excluded from signing
- 🎉 ([smithy-rs#1390](https://github.com/smithy-lang/smithy-rs/issues/1390)) Add method `ByteStream::into_async_read`. This makes it easy to convert `ByteStream`s into a struct implementing `tokio:io::AsyncRead`. Available on **crate feature** `rt-tokio` only.
- ([smithy-rs#1404](https://github.com/smithy-lang/smithy-rs/issues/1404), @petrosagg) Add ability to specify a different rust crate name than the one derived from the package name
- ([smithy-rs#1404](https://github.com/smithy-lang/smithy-rs/issues/1404), @petrosagg) Switch to [RustCrypto](https://github.com/RustCrypto)'s implementation of MD5.

**Contributors**
Thank you for your contributions! ❤
- @alonlud ([smithy-rs#1381](https://github.com/smithy-lang/smithy-rs/issues/1381))
- @petrosagg ([smithy-rs#1404](https://github.com/smithy-lang/smithy-rs/issues/1404))

v0.42.0 (May 13th, 2022)
========================
**Breaking Changes:**
- ⚠🎉 ([aws-sdk-rust#494](https://github.com/awslabs/aws-sdk-rust/issues/494), [aws-sdk-rust#519](https://github.com/awslabs/aws-sdk-rust/issues/519)) The `aws_smithy_http::byte_stream::bytestream_util::FsBuilder` has been updated to allow for easier creation of
    multi-part requests.

    - `FsBuilder::offset` is a new method allowing users to specify an offset to start reading a file from.
    - `FsBuilder::file_size` has been reworked into `FsBuilder::length` and is now used to specify the amount of data to read.

    With these two methods, it's now simple to create a `ByteStream` that will read a single "chunk" of a file. The example
    below demonstrates how you could divide a single `File` into consecutive chunks to create multiple `ByteStream`s.

    ```rust
    let example_file_path = Path::new("/example.txt");
    let example_file_size = tokio::fs::metadata(&example_file_path).await.unwrap().len();
    let chunks = 6;
    let chunk_size = file_size / chunks;
    let mut byte_streams = Vec::new();

    for i in 0..chunks {
        let length = if i == chunks - 1 {
            // If we're on the last chunk, the length to read might be less than a whole chunk.
            // We substract the size of all previous chunks from the total file size to get the
            // size of the final chunk.
            file_size - (i * chunk_size)
        } else {
            chunk_size
        };

        let byte_stream = ByteStream::read_from()
            .path(&file_path)
            .offset(i * chunk_size)
            .length(length)
            .build()
            .await?;

        byte_streams.push(byte_stream);
    }

    for chunk in byte_streams {
        // Make requests to a service
    }
    ```

**New this release:**
- ([smithy-rs#1352](https://github.com/smithy-lang/smithy-rs/issues/1352)) Log a debug event when a retry is going to be peformed
- ([smithy-rs#1332](https://github.com/smithy-lang/smithy-rs/issues/1332), @82marbag) Update generated crates to Rust 2021

**Contributors**
Thank you for your contributions! ❤
- @82marbag ([smithy-rs#1332](https://github.com/smithy-lang/smithy-rs/issues/1332))

0.41.0 (April 28th, 2022)
=========================
**Breaking Changes:**
- ⚠ ([smithy-rs#1318](https://github.com/smithy-lang/smithy-rs/issues/1318)) Bump [MSRV](https://github.com/awslabs/aws-sdk-rust#supported-rust-versions-msrv) from 1.56.1 to 1.58.1 per our "two versions behind" policy.

**New this release:**
- ([smithy-rs#1307](https://github.com/smithy-lang/smithy-rs/issues/1307)) Add new trait for HTTP body callbacks. This is the first step to enabling us to implement optional checksum verification of requests and responses.
- ([smithy-rs#1330](https://github.com/smithy-lang/smithy-rs/issues/1330)) Upgrade to Smithy 1.21.0


0.40.2 (April 14th, 2022)
=========================

**Breaking Changes:**
- ⚠ ([aws-sdk-rust#490](https://github.com/awslabs/aws-sdk-rust/issues/490)) Update all runtime crates to [edition 2021](https://blog.rust-lang.org/2021/10/21/Rust-1.56.0.html)

**New this release:**
- ([smithy-rs#1262](https://github.com/smithy-lang/smithy-rs/issues/1262), @liubin) Fix link to Developer Guide in crate's README.md
- ([smithy-rs#1301](https://github.com/smithy-lang/smithy-rs/issues/1301), @benesch) Update urlencoding crate to v2.1.0

**Contributors**
Thank you for your contributions! ❤
- @benesch ([smithy-rs#1301](https://github.com/smithy-lang/smithy-rs/issues/1301))
- @liubin ([smithy-rs#1262](https://github.com/smithy-lang/smithy-rs/issues/1262))

0.39.0 (March 17, 2022)
=======================
**Breaking Changes:**
- ⚠ ([aws-sdk-rust#406](https://github.com/awslabs/aws-sdk-rust/issues/406)) `aws_types::config::Config` has been renamed to `aws_types:sdk_config::SdkConfig`. This is to better differentiate it
    from service-specific configs like `aws_s3_sdk::Config`. If you were creating shared configs with
    `aws_config::load_from_env()`, then you don't have to do anything. If you were directly referring to a shared config,
    update your `use` statements and `struct` names.

    _Before:_
    ```rust
    use aws_types::config::Config;

    fn main() {
        let config = Config::builder()
        // config builder methods...
        .build()
        .await;
    }
    ```

    _After:_
    ```rust
    use aws_types::SdkConfig;

    fn main() {
        let config = SdkConfig::builder()
        // config builder methods...
        .build()
        .await;
    }
    ```
- ⚠ ([smithy-rs#724](https://github.com/smithy-lang/smithy-rs/issues/724)) Timeout configuration has been refactored a bit. If you were setting timeouts through environment variables or an AWS
    profile, then you shouldn't need to change anything. Take note, however, that we don't currently support HTTP connect,
    read, write, or TLS negotiation timeouts. If you try to set any of those timeouts in your profile or environment, we'll
    log a warning explaining that those timeouts don't currently do anything.

    If you were using timeouts programmatically,
    you'll need to update your code. In previous versions, timeout configuration was stored in a single `TimeoutConfig`
    struct. In this new version, timeouts have been broken up into several different config structs that are then collected
    in a `timeout::Config` struct. As an example, to get the API per-attempt timeout in previous versions you would access
    it with `<your TimeoutConfig>.api_call_attempt_timeout()` and in this new version you would access it with
    `<your timeout::Config>.api.call_attempt_timeout()`. We also made some unimplemented timeouts inaccessible in order to
    avoid giving users the impression that setting them had an effect. We plan to re-introduce them once they're made
    functional in a future update.

**New this release:**
- ([smithy-rs#1225](https://github.com/smithy-lang/smithy-rs/issues/1225)) `DynMiddleware` is now `clone`able
- ([smithy-rs#1257](https://github.com/smithy-lang/smithy-rs/issues/1257)) HTTP request property bag now contains list of desired HTTP versions to use when making requests. This list is not currently used but will be in an upcoming update.


0.38.0 (Februrary 24, 2022)
===========================
**Breaking Changes:**
- ⚠ ([smithy-rs#1197](https://github.com/smithy-lang/smithy-rs/issues/1197)) `aws_smithy_types::retry::RetryKind` had its `NotRetryable` variant split into `UnretryableFailure` and `Unnecessary`. If you implement the `ClassifyResponse`, then successful responses need to return `Unnecessary`, and failures that shouldn't be retried need to return `UnretryableFailure`.
- ⚠ ([smithy-rs#1209](https://github.com/smithy-lang/smithy-rs/issues/1209)) `aws_smithy_types::primitive::Encoder` is now a struct rather than an enum, but its usage remains the same.
- ⚠ ([smithy-rs#1217](https://github.com/smithy-lang/smithy-rs/issues/1217)) `ClientBuilder` helpers `rustls()` and `native_tls()` now return `DynConnector` and use dynamic dispatch rather than returning their concrete connector type that would allow static dispatch. If static dispatch is desired, then manually construct a connector to give to the builder. For example, for rustls: `builder.connector(Adapter::builder().build(aws_smithy_client::conns::https()))` (where `Adapter` is in `aws_smithy_client::hyper_ext`).

**New this release:**
- 🐛 ([smithy-rs#1197](https://github.com/smithy-lang/smithy-rs/issues/1197)) Fixed a bug that caused clients to eventually stop retrying. The cross-request retry allowance wasn't being reimbursed upon receiving a successful response, so once this allowance reached zero, no further retries would ever be attempted.


0.37.0 (February 18th, 2022)
============================
**Breaking Changes:**
- ⚠ ([smithy-rs#1144](https://github.com/smithy-lang/smithy-rs/issues/1144)) Some APIs required that timeout configuration be specified with an `aws_smithy_client::timeout::Settings` struct while
    others required an `aws_smithy_types::timeout::TimeoutConfig` struct. Both were equivalent. Now `aws_smithy_types::timeout::TimeoutConfig`
    is used everywhere and `aws_smithy_client::timeout::Settings` has been removed. Here's how to migrate code your code that
    depended on `timeout::Settings`:

    The old way:
    ```rust
    let timeout = timeout::Settings::new()
        .with_connect_timeout(Duration::from_secs(1))
        .with_read_timeout(Duration::from_secs(2));
    ```

    The new way:
    ```rust
    // This example is passing values, so they're wrapped in `Option::Some`. You can disable a timeout by passing `None`.
    let timeout = TimeoutConfig::new()
        .with_connect_timeout(Some(Duration::from_secs(1)))
        .with_read_timeout(Some(Duration::from_secs(2)));
    ```
- ⚠ ([smithy-rs#1085](https://github.com/smithy-lang/smithy-rs/issues/1085)) Moved the following re-exports into a `types` module for all services:
    - `<service>::AggregatedBytes` -> `<service>::types::AggregatedBytes`
    - `<service>::Blob` -> `<service>::types::Blob`
    - `<service>::ByteStream` -> `<service>::types::ByteStream`
    - `<service>::DateTime` -> `<service>::types::DateTime`
    - `<service>::SdkError` -> `<service>::types::SdkError`
- ⚠ ([smithy-rs#1085](https://github.com/smithy-lang/smithy-rs/issues/1085)) `AggregatedBytes` and `ByteStream` are now only re-exported if the service has streaming operations,
    and `Blob`/`DateTime` are only re-exported if the service uses them.
- ⚠ ([smithy-rs#1130](https://github.com/smithy-lang/smithy-rs/issues/1130)) MSRV increased from `1.54` to `1.56.1` per our 2-behind MSRV policy.

**New this release:**
- ([smithy-rs#1144](https://github.com/smithy-lang/smithy-rs/issues/1144)) `MakeConnectorFn`, `HttpConnector`, and `HttpSettings` have been moved from `aws_config::provider_config` to
    `aws_smithy_client::http_connector`. This is in preparation for a later update that will change how connectors are
    created and configured.
- ([smithy-rs#1123](https://github.com/smithy-lang/smithy-rs/issues/1123)) Refactor `Document` shape parser generation
- ([smithy-rs#1085](https://github.com/smithy-lang/smithy-rs/issues/1085)) The `Client` and `Config` re-exports now have their documentation inlined in the service docs


0.36.0 (January 26, 2022)
=========================
**New this release:**
- ([smithy-rs#1087](https://github.com/smithy-lang/smithy-rs/issues/1087)) Improve docs on `Endpoint::{mutable, immutable}`
- ([smithy-rs#1118](https://github.com/smithy-lang/smithy-rs/issues/1118)) SDK examples now come from [`awsdocs/aws-doc-sdk-examples`](https://github.com/awsdocs/aws-doc-sdk-examples) rather than from `smithy-rs`
- ([smithy-rs#1114](https://github.com/smithy-lang/smithy-rs/issues/1114), @mchoicpe-amazon) Provide SigningService creation via owned String

**Contributors**
Thank you for your contributions! ❤
- @mchoicpe-amazon ([smithy-rs#1114](https://github.com/smithy-lang/smithy-rs/issues/1114))


0.35.2 (January 20th, 2022)
===========================
_Changes only impact generated AWS SDK_

v0.35.1 (January 19th, 2022)
============================
_Changes only impact generated AWS SDK_


0.35.0 (January 19, 2022)
=========================
**New this release:**
- ([smithy-rs#1053](https://github.com/smithy-lang/smithy-rs/issues/1053)) Upgraded Smithy to 1.16.1
- 🐛 ([smithy-rs#1069](https://github.com/smithy-lang/smithy-rs/issues/1069)) Fix broken link to `RetryMode` in client docs
- 🐛 ([smithy-rs#1069](https://github.com/smithy-lang/smithy-rs/issues/1069)) Fix several doc links to raw identifiers (identifiers excaped with `r#`)
- 🐛 ([smithy-rs#1069](https://github.com/smithy-lang/smithy-rs/issues/1069)) Reduce dependency recompilation in local dev
- 🐛 ([aws-sdk-rust#405](https://github.com/awslabs/aws-sdk-rust/issues/405), [smithy-rs#1083](https://github.com/smithy-lang/smithy-rs/issues/1083)) Fixed paginator bug impacting EC2 describe VPCs (and others)



v0.34.1 (January 10, 2022)
==========================
**New this release:**
- 🐛 (smithy-rs#1054, aws-sdk-rust#391) Fix critical paginator bug where an empty outputToken lead to a never ending stream.



0.34.0 (January 6th, 2022)
==========================
**Breaking Changes:**
- ⚠ (smithy-rs#990) Codegen will no longer produce builders and clients with methods that take `impl Into<T>` except for strings and boxed types.
- ⚠ (smithy-rs#1003) The signature of `aws_smithy_protocol_test::validate_headers` was made more flexible but may require adjusting invocations slightly.

**New this release:**
- 🎉 (aws-sdk-rust#47, smithy-rs#1006) Add support for paginators! Paginated APIs now include `.into_paginator()` and (when supported) `.into_paginator().items()` to enable paginating responses automatically. The paginator API should be considered in preview and is subject to change pending customer feedback.
- 🐛 (aws-sdk-rust#357) Generated docs will convert `<a>` tags with no `href` attribute to `<pre>` tags
- (aws-sdk-rust#254, @jacco) Made fluent operation structs cloneable

**Contributors**
Thank you for your contributions! ❤
- @jacco (aws-sdk-rust#254)


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
[STS example](https://github.com/smithy-lang/smithy-rs/blob/7fa4af4a9367aeca6d55e26fc4d4ba93093b90c4/aws/sdk/examples/sts/src/bin/credentials-provider.rs)
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
