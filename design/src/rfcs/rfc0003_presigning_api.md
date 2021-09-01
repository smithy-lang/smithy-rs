RFC: API for Pre-signed URLs
============================

> Status: RFC

For a summarized list of proposed changes, see the [Changes Checklist](#changes-checklist) section.

Several AWS services allow for pre-signed requests in URL form, which is described well by
[S3's documentation on authenticating requests using query parameters](https://docs.aws.amazon.com/AmazonS3/latest/API/sigv4-query-string-auth.html).

This doc establishes the customer-facing API for creating these pre-signed URLs and how they will
be implemented in a generic fashion in the SDK codegen.

Terminology
-----------

To differentiate between the clients that are present in the generated SDK today, the following
terms will be used throughout this doc:

- **Smithy Client**: A `smithy_client::Client<C, M, R>` struct that is responsible for gluing together
  the connector, middleware, and retry policy. This is not generated and lives in the `smithy-client` crate.
- **Fluent Client**: A code-generated `Client<C, M, R>` that has methods for each service operation on it.
  A fluent builder is generated alongside it to make construction easier.

Pre-signed URL config
---------------------

Today, pre-signed URLs take an expiration time that's not part of the service API.
The SDK will make this configurable as a separate struct so that there's no chance of name collisions, and so
that additional fields can be added in the future. Fields added later will require defaulting for
backwards compatibility.

Since there's only one configuration field today, an `expires` function will be added to conveniently
construct `PresigningConfig` without a builder. At the same time, a builder will be present for future-proofing:

```rust
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct PresigningConfig {
    expires: Duration,
}

#[non_exhaustive]
#[derive(Debug)]
pub struct Builder {
  expires: Option<Duration>,
}

impl Builder {
  pub fn expires(self, expires: Duration) -> Self { ... }
  pub fn set_expires(self, expires: Option<Duration>) -> Self { ... }

  pub fn build(self) -> PresigningConfig { ... }
}

impl PresigningConfig {
    pub fn expires(expires: Duration) -> PresigningConfig {
        Self::builder().expires(expires).build()
    }

    pub fn builder() -> Builder { ... }
}
```

`PresigningConfig` is intended to be shared across AWS services, and will be passed by reference where it
is taken as an argument.

Fluent Pre-signed URL API
-------------------------

The generated fluent builders for operations that support pre-signing will have a `presigned()` method
in addition to `send()` that will return a pre-signed URL rather than sending the request. For S3's GetObject,
the usage of this will look as follows:

```rust
let config = aws_config::load_config_from_environment().await;
let client = s3::Client::new(&config);
let presigning_config = PresigningConfig::expires(Duration::from_secs(86400));
let presigned_url: String = client.get_object()
    .bucket("example-bucket")
    .key("example-object")
    .presigned(&presigning_config)
    .await?;
```

This API requires a client, and for use-cases where no actual service calls need to be made,
customers should be able to create pre-signed URLs without the overhead of an HTTP client.
Once the [HTTP Versions RFC](./rfc0002_http_versions.md) is implemented, the underlying HTTP client
won't be created until the first service call, so there will be no HTTP client overhead to
this approach.

In a step away from the general pattern of keeping fluent client capabilities in line with Smithy client capabilities,
creating pre-signed URLs directly from the Smithy client will not be supported. This is for two reasons:
- Pre-signed URLs are not currently a Smithy concept.
- The Smithy client is not code generated, so adding a method to do pre-signing would apply to all operations,
  but not all operations can be pre-signed.

**Note:** Pre-signing *needs* to be `async` because the underlying credentials provider used to sign the
request *may* need to make service calls to acquire the credentials.

Input Pre-signed URL API
------------------------

Even though generating a pre-signed URL through the fluent client doesn't necessitate an HTTP client,
it will be clearer that this is the case by allowing the creation of pre-signed URLs directly from an input.
This would look as follows:

```rust
let config = aws_config::load_config_from_environment().await;
let presigning_config = PresigningConfig::expires(Duration::from_secs(86400));
let presigned_url: String = GetObjectInput::builder()
    .bucket("example-bucket")
    .key("example-bucket")
    .presigned(&config, &presigning_config)
    .await?;
```

Creating the URL through the input will exercise the same code path as creating it through the client,
but it will be more apparent that the overhead of a client isn't present.

Behind the scenes
-----------------

From an SDK's perspective, the following are required to make a pre-signed URL:
- Valid request input
- Endpoint
- Credentials to sign with
- Signing implementation

The AWS middleware provides everything except the request, and the request is provided as part
of the fluent builder API. The generated code needs to be able to run the middleware to fully populate
a request property bag, but not actually dispatch it. Additionally, the signing headers added by the signing
implementation will need to be moved into query parameters.

Today, request dispatch looks as follows:
1. The customer creates a new fluent builder by calling `client.operation_name()`, fills in inputs, and then calls `send()`.
2. `send()`:
   1. Builds the final input struct, and then calls its `make_operation()` method with the stored config to create a Smithy `Operation`.
   2. Calls the underlying Smithy client with the operation.
3. The Smithy client constructs a Tower Service with AWS middleware and a dispatcher at the bottom, and then executes it.
4. The middleware acquire and add required signing parameters (region, credentials, endpoint, etc) to the request property bag.
5. The SigV4 signing middleware signs the request by adding HTTP headers to it.
6. The dispatcher makes the actual HTTP request and returns the response all the way back up the Tower.

Pre-signing will take advantage of a lot of these same steps, but will cut out the `Operation` and
replace the dispatcher with a pre-signed URL generator:
1. The customer creates a new fluent builder by calling `client.operation_name()`, fills in inputs, and then calls `presigned()`.
2. `presigned()`:
   1. Builds the final input struct, and then calls a new `make_request()` method with the stored config.
   2. Constructs a Tower Service with `AwsMiddleware` layered in, and a `PresignedUrlGeneratorLayer` at the bottom.
   3. Calls the Tower Service and returns its result
3. The `AwsMiddleware` will sign the request by adding signature headers to it.
4. `PresignedUrlGeneratorLayer` would take the requests URI, and append signature headers to it as query parameters. Then return the result as a string.

It should be noted that the `presigned()` function above is on the generated input struct, so implementing this for
the input API is identical to implementing it for the fluent client.

All the code for the new `make_request()` is already in the existing `make_operation()` and will just need to be split out.

### Modeling Pre-signing

AWS models don't currently have any information about which operations can be pre-signed.
To work around this, the Rust SDK will create a synthetic trait to model pre-signing with, and
apply this trait to known pre-signed operations via customization. The code generator will
look for this synthetic trait when creating the fluent builders and inputs to know if a
`presigned()` method should be added.

Unresolved Questions
--------------------

- Where should `PresigningConfig` live?
- What should we do if an operation input has a member named `presigned` to avoid name collision with
  the generated method?
  - Option 1: Make `presigned` a reserved word [similar to `send`](https://github.com/awslabs/smithy-rs/blob/3d61226b5d446f4cc20bf4969f0e56d106cf478b/codegen/src/main/kotlin/software/amazon/smithy/rust/codegen/rustlang/RustReservedWords.kt#L28) so that colliding members get renamed to `presigned_value`
  - Option 2: Rename the `presigned()` method to `presigned_url()` when there is a collision
  - In either option, there's still a chance of a secondary collision

Changes Checklist
-----------------

- [ ] Create `PresigningConfig` and its builder
- [ ] Refactor input code generator to create `make_request()` function
- [ ] Implement `PresignedUrlGeneratorLayer`
- [ ] Add new `presigned()` method to input code generator
- [ ] Add new `presigned()` method to fluent client generator
- [ ] Create `PresignedOperationSyntheticTrait`
- [ ] Customize models for known pre-signed operations
- [ ] Add integration test to S3
- [ ] Add examples for using pre-signing for:
  - [ ] S3 GetObject and PutObject
  - [ ] CloudFront download URLs
  - [ ] Polly SynthesizeSpeech
