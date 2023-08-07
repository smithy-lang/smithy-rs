RFC: API for Presigned URLs
============================

> Status: Implemented

For a summarized list of proposed changes, see the [Changes Checklist](#changes-checklist) section.

Several AWS services allow for presigned requests in URL form, which is described well by
[S3's documentation on authenticating requests using query parameters](https://docs.aws.amazon.com/AmazonS3/latest/API/sigv4-query-string-auth.html).

This doc establishes the customer-facing API for creating these presigned URLs and how they will
be implemented in a generic fashion in the SDK codegen.

Terminology
-----------

To differentiate between the clients that are present in the generated SDK today, the following
terms will be used throughout this doc:

- **Smithy Client**: A `aws_smithy_client::Client<C, M, R>` struct that is responsible for gluing together
  the connector, middleware, and retry policy. This is not generated and lives in the `aws-smithy-client` crate.
- **Fluent Client**: A code-generated `Client<C, M, R>` that has methods for each service operation on it.
  A fluent builder is generated alongside it to make construction easier.

Presigned URL config
--------------------

Today, presigned URLs take an expiration time that's not part of the service API.
The SDK will make this configurable as a separate struct so that there's no chance of name collisions, and so
that additional fields can be added in the future. Fields added later will require defaulting for
backwards compatibility.

Customers should also be able to set a start time on the presigned URL's expiration so that they can
generate URLs that become active in the future. An optional `start_time` option will be available and
default to `SystemTime::now()`.

Construction `PresigningConfig` can be done with a builder, but a `PresigningConfig::expires_in`
convenience function will be provided to bypass the builder for the most frequent use-case.

```rust,ignore
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct PresigningConfig {
    start_time: SystemTime,
    expires_in: Duration,
}

#[non_exhaustive]
#[derive(Debug)]
pub struct Builder {
    start_time: Option<SystemTime>,
    expires_in: Option<Duration>,
}

impl Builder {
    pub fn start_time(self, start_time: SystemTime) -> Self { ... }
    pub fn set_start_time(&mut self, start_time: Option<SystemTime>) { ... }

    pub fn expires_in(self, expires_in: Duration) -> Self { ... }
    pub fn set_expires_in(&mut self, expires_in: Option<Duration>) { ... }

    // Validates `expires_in` is no greater than one week
    pub fn build(self) -> Result<PresigningConfig, Error> { ... }
}

impl PresigningConfig {
    pub fn expires_in(expires_in: Duration) -> PresigningConfig {
        Self::builder().expires(expires).build().unwrap()
    }

    pub fn builder() -> Builder { ... }
}
```

Construction of `PresigningConfig` will validate that `expires_in` is no greater than one week, as this
is the longest supported expiration time for SigV4. This validation will result in a panic.

It's not inconceivable that `PresigningConfig` will need additional service-specific parameters as customizations,
so it will be code generated with each service rather than living a shared location.

Fluent Presigned URL API
------------------------

The generated fluent builders for operations that support presigning will have a `presigned()` method
in addition to `send()` that will return a presigned URL rather than sending the request. For S3's GetObject,
the usage of this will look as follows:

```rust,ignore
let config = aws_config::load_config_from_environment().await;
let client = s3::Client::new(&config);
let presigning_config = PresigningConfig::expires_in(Duration::from_secs(86400));
let presigned: PresignedRequest = client.get_object()
    .bucket("example-bucket")
    .key("example-object")
    .presigned(presigning_config)
    .await?;
```

This API requires a client, and for use-cases where no actual service calls need to be made,
customers should be able to create presigned URLs without the overhead of an HTTP client.
Once the [HTTP Versions RFC](./rfc0002_http_versions.md) is implemented, the underlying HTTP client
won't be created until the first service call, so there will be no HTTP client overhead to
this approach.

In a step away from the general pattern of keeping fluent client capabilities in line with Smithy client capabilities,
creating presigned URLs directly from the Smithy client will not be supported. This is for two reasons:
- The Smithy client is not code generated, so adding a method to do presigning would apply to all operations,
  but not all operations can be presigned.
- Presigned URLs are not currently a Smithy concept ([although this may change soon](https://github.com/awslabs/smithy/pull/897)).

The result of calling `presigned()` is a `PresignedRequest`, which is a wrapper with delegating functions
around `http::Request<()>` so that the request method and additional signing headers are also made available.
This is necessary since there are some presignable POST operations that require the signature to be in the
headers rather than the query.

**Note:** Presigning *needs* to be `async` because the underlying credentials provider used to sign the
request *may* need to make service calls to acquire the credentials.

Input Presigned URL API
------------------------

Even though generating a presigned URL through the fluent client doesn't necessitate an HTTP client,
it will be clearer that this is the case by allowing the creation of presigned URLs directly from an input.
This would look as follows:

```rust,ignore
let config = aws_config::load_config_from_environment().await;
let presigning_config = PresigningConfig::expires_in(Duration::from_secs(86400));
let presigned: PresignedRequest = GetObjectInput::builder()
    .bucket("example-bucket")
    .key("example-bucket")
    .presigned(&config, presigning_config)
    .await?;
```

Creating the URL through the input will exercise the same code path as creating it through the client,
but it will be more apparent that the overhead of a client isn't present.

Behind the scenes
-----------------

From an SDK's perspective, the following are required to make a presigned URL:
- Valid request input
- Endpoint
- Credentials to sign with
- Signing implementation

The AWS middleware provides everything except the request, and the request is provided as part
of the fluent builder API. The generated code needs to be able to run the middleware to fully populate
a request property bag, but not actually dispatch it.  The `expires_in` value from the presigning config
needs to be piped all the way through to the signer. Additionally, the SigV4 signing needs to adjusted
to do query param signing, which is slightly different than its header signing.

Today, request dispatch looks as follows:
1. The customer creates a new fluent builder by calling `client.operation_name()`, fills in inputs, and then calls `send()`.
2. `send()`:
   1. Builds the final input struct, and then calls its `make_operation()` method with the stored config to create a Smithy `Operation`.
   2. Calls the underlying Smithy client with the operation.
3. The Smithy client constructs a Tower Service with AWS middleware and a dispatcher at the bottom, and then executes it.
4. The middleware acquire and add required signing parameters (region, credentials, endpoint, etc) to the request property bag.
5. The SigV4 signing middleware signs the request by adding HTTP headers to it.
6. The dispatcher makes the actual HTTP request and returns the response all the way back up the Tower.

Presigning will take advantage of a lot of these same steps, but will cut out the `Operation` and
replace the dispatcher with a presigned URL generator:
1. The customer creates a new fluent builder by calling `client.operation_name()`, fills in inputs, and then calls `presigned()`.
2. `presigned()`:
   1. Builds the final input struct, calls the `make_operation()` method with the stored config, and then extracts
      the request from the operation (discarding the rest).
   2. Mutates the `OperationSigningConfig` in the property bag to:
      - Change the `signature_type` to `HttpRequestQueryParams` so that the signer runs the correct signing logic.
      - Set `expires_in` to the value given by the customer in the presigning config.
   3. Constructs a Tower Service with `AwsMiddleware` layered in, and a `PresignedUrlGeneratorLayer` at the bottom.
   4. Calls the Tower Service and returns its result
3. The `AwsMiddleware` will sign the request.
4. The `PresignedUrlGeneratorLayer` directly returns the request since all of the work is done by the middleware.

It should be noted that the `presigned()` function above is on the generated input struct, so implementing this for
the input API is identical to implementing it for the fluent client.

All the code for the new `make_request()` is already in the existing `make_operation()` and will just need to be split out.

### Modeling Presigning

AWS models don't currently have any information about which operations can be presigned.
To work around this, the Rust SDK will create a synthetic trait to model presigning with, and
apply this trait to known presigned operations via customization. The code generator will
look for this synthetic trait when creating the fluent builders and inputs to know if a
`presigned()` method should be added.

### Avoiding name collision

If a presignable operation input has a member named `presigned`, then there will be a name collision with
the function to generate a presigned URL. To mitigate this, `RustReservedWords` will be updated
to rename the `presigned` member to `presigned_value`
[similar to how `send` is renamed](https://github.com/awslabs/smithy-rs/blob/3d61226b5d446f4cc20bf4969f0e56d106cf478b/codegen/src/main/kotlin/software/amazon/smithy/rust/codegen/rustlang/RustReservedWords.kt#L28).

Changes Checklist
-----------------

- [x] Update `aws-sigv4` to support query param signing
- [x] Create `PresignedOperationSyntheticTrait`
- [x] Customize models for known presigned operations
- [x] Create `PresigningConfig` and its builder
- [x] Implement `PresignedUrlGeneratorLayer`
- [x] Create new AWS codegen decorator to:
  - [x] Add new `presigned()` method to input code generator
  - [x] Add new `presigned()` method to fluent client generator
- [x] Update `RustReservedWords` to reserve `presigned()`
- [x] Add integration test to S3
- [x] Add integration test to Polly
- [x] Add examples for using presigning for:
  - [x] S3 GetObject and PutObject
  - [x] Polly SynthesizeSpeech
