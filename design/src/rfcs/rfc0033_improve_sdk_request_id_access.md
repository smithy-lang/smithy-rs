RFC: Improving access to request IDs in SDK clients
===================================================

> Status: Implemented in [#2129](https://github.com/awslabs/smithy-rs/pull/2129)
>
> Applies to: AWS SDK clients

At time of writing, customers can retrieve a request ID in one of four ways in the Rust SDK:

1. For error cases where the response parsed successfully, the request ID can be retrieved
   via accessor method on operation error. This also works for unmodeled errors so long as
   the response parsing succeeds.
2. For error cases where a response was received but parsing fails, the response headers
   can be retrieved from the raw response on the error, but customers have to manually extract
   the request ID from those headers (there's no convenient accessor method).
3. For all error cases where the request ID header was sent in the response, customers can
   call `SdkError::into_service_error` to transform the `SdkError` into an operation error,
   which has a `request_id` accessor on it.
4. For success cases, the customer can't retrieve the request ID at all if they use the fluent
   client. Instead, they must manually make the operation and call the underlying Smithy client
   so that they have access to `SdkSuccess`, which provides the raw response where the request ID
   can be manually extracted from headers.

Only one of these mechanisms is convenient and ergonomic. The rest need considerable improvements.
Additionally, the request ID should be attached to tracing events where possible so that enabling
debug logging reveals the request IDs without any code changes being necessary.

This RFC proposes changes to make the request ID easier to access.

Terminology
-----------

- **Request ID:** A unique identifier assigned to and associated with a request to AWS that is
  sent back in the response headers. This identifier is useful to customers when requesting support.
- **Operation Error:** Operation errors are code generated for each operation in a Smithy model.
  They are an enum of every possible modeled error that that operation can respond with, as well
  as an `Unhandled` variant for any unmodeled or unrecognized errors.
- **Modeled Errors:** Any error that is represented in a Smithy model with the `@error` trait.
- **Unmodeled Errors:** Errors that a service responds with that do not appear in the Smithy model.
- **SDK Clients:** Clients generated for the AWS SDK, including "adhoc" or "one-off" clients.
- **Smithy Clients:** Any clients not generated for the AWS SDK, excluding "adhoc" or "one-off" clients.

SDK/Smithy Purity
-----------------

Before proposing any changes, the topic of purity needs to be covered. Request IDs are not
currently a Smithy concept. However, at time of writing, the request ID concept is leaked into
the non-SDK rust runtime crates and generated code via the [generic error] struct and the
`request_id` functions on generated operation errors (e.g., [`GetObjectError` example in S3]).

This RFC attempts to remove these leaks from Smithy clients.

Proposed Changes
----------------

First, we'll explore making it easier to retrieve a request ID from errors,
and then look at making it possible to retrieve them from successful responses.
To see the customer experience of these changes, see the **Example Interactions**
section below.

### Make request ID retrieval on errors consistent

One could argue that customers being able to convert a `SdkError` into an operation error
that has a request ID on it is sufficient. However, there's no way to write a function
that takes an error from any operation and logs a request ID, so it's still not ideal.

The `aws-http` crate needs to have a `RequestId` trait on it to facilitate generic
request ID retrieval:

```rust
pub trait RequestId {
    /// Returns the request ID if it's available.
    fn request_id(&self) -> Option<&str>;
}
```

This trait will be implemented for `SdkError` in `aws-http` where it is declared,
complete with logic to pull the request ID header out of the raw HTTP responses
(it will always return `None` for event stream `Message` responses; an additional
trait may need to be added to `aws-smithy-http` to facilitate access to the headers).
This logic will try different request ID header names in order of probability
since AWS services have a couple of header name variations. `x-amzn-requestid` is
the most common, with `x-amzn-request-id` being the second most common.

`aws-http` will also implement `RequestId` for `aws_smithy_types::error::Error`,
and the `request_id` method will be removed from `aws_smithy_types::error::Error`.
Places that construct `Error` will place the request ID into its `extras` field,
where the `RequestId` trait implementation can retrieve it.

A codegen decorator will be added to `sdk-codegen` to implement `RequestId` for
operation errors, and the existing `request_id` accessors will be removed from
`CombinedErrorGenerator` in `codegen-core`.

With these changes, customers can directly access request IDs from `SdkError` and
operations errors by importing the `RequestId` trait. Additionally, the Smithy/SDK
purity is improved since both places where request IDs are leaked to Smithy clients
will be resolved.

### Implement `RequestId` for outputs

To make it possible to retrieve request IDs when using the fluent client, the new
`RequestId` trait can be implemented for outputs.

Some services (e.g., Transcribe Streaming) model the request ID header in their
outputs, while other services (e.g., Directory Service) model a request ID
field on errors. In some cases, services take `RequestId` as a modeled input
(e.g., IoT Event Data). It follows that it is possible, but unlikely, that
a service could have a field named `RequestId` that is not the same concept
in the future.

Thus, name collisions are going to be a concern for putting a request ID accessor
on output. However, if it is implemented as a trait, then this concern is partially
resolved. In the vast majority of cases, importing `RequestId` will provide the
accessor without any confusion. In cases where it is already modeled and is the
same concept, customers will likely just use it and not even realize they didn't
import the trait. The only concern is future cases where it is modeled as a
separate concept, and as long as customers don't import `RequestId` for something
else in the same file, that confusion can be avoided.

In order to implement `RequestId` for outputs, either the original response needs
to be stored on the output, or the request ID needs to be extracted earlier and
stored on the output. The latter will lead to a small amount of header lookup
code duplication.

In either case, the `StructureGenerator` needs to be customized in `sdk-codegen`
(Appendix B outlines an alternative approach to this and why it was dismissed).
This will be done by adding customization hooks to `StructureGenerator` similar
to the ones for `ServiceConfigGenerator` so that a `sdk-codegen` decorator can
conditionally add fields and functions to any generated structs. A hook will
also be needed to additional trait impl blocks.

Once the hooks are in place, a decorator will be added to store either the original
response or the request ID on outputs, and then the `RequestId` trait will be
implemented for them. The `ParseResponse` trait implementation will be customized
to populate this new field.

Note: To avoid name collisions of the request ID or response on the output struct,
these fields can be prefixed with an underscore. It shouldn't be possible for SDK
fields to code generate with this prefix given the model validation rules in place.

### Implement `RequestId` for `Operation` and `operation::Response`

In the case that a customer wants to ditch the fluent client, it should still
be easy to retrieve a request ID. To do this, `aws-http` will provide `RequestId`
implementations for `Operation` and `operation::Response`. These implementations
will likely make the other `RequestId` implementations easier to implement as well.

### Implement `RequestId` for `Result`

The `Result` returned by the SDK should directly implement `RequestId` when both
its `Ok` and `Err` variants implement `RequestId`. This will make it possible
for a customer to feed the return value from `send()` directly to a request ID logger.

Example Interactions
--------------------

### Generic Handling Case

```rust,ignore
// A re-export of the RequestId trait
use aws_sdk_service::primitives::RequestId;

fn my_request_id_logging_fn(request_id: &dyn RequestId) {
    println!("request ID: {:?}", request_id.request_id());
}

let result = client.some_operation().send().await?;
my_request_id_logging_fn(&result);
```

### Success Case

```rust,ignore
use aws_sdk_service::primitives::RequestId;

let output = client.some_operation().send().await?;
println!("request ID: {:?}", output.request_id());
```

### Error Case with `SdkError`

```rust,ignore
use aws_sdk_service::primitives::RequestId;

match client.some_operation().send().await {
    Ok(_) => { /* handle OK */ }
    Err(err) => {
        println!("request ID: {:?}", output.request_id());
    }
}
```

### Error Case with operation error

```rust,ignore
use aws_sdk_service::primitives::RequestId;

match client.some_operation().send().await {
    Ok(_) => { /* handle OK */ }
    Err(err) => match err.into_service_err() {
        err @ SomeOperationError::SomeError(_) => { println!("request ID: {:?}", err.request_id()); }
        _ => { /* don't care */ }
    }
}
```

Changes Checklist
-----------------

- [x] Create the `RequestId` trait in `aws-http`
- [x] Implement for errors
  - [x] Implement `RequestId` for `SdkError` in `aws-http`
  - [x] Remove `request_id` from `aws_smithy_types::error::Error`, and store request IDs in its `extras` instead
  - [x] Implement `RequestId` for `aws_smithy_types::error::Error` in `aws-http`
  - [x] Remove generation of `request_id` accessors from `CombinedErrorGenerator` in `codegen-core`
- [x] Implement for outputs
  - [x] Add customization hooks to `StructureGenerator`
  - [x] Add customization hook to `ParseResponse`
  - [x] Add customization hook to `HttpBoundProtocolGenerator`
  - [x] Customize output structure code gen in `sdk-codegen` to add either a request ID or a response field
  - [x] Customize `ParseResponse` in `sdk-codegen` to populate the outputs
- [x] Implement `RequestId` for `Operation` and `operation::Response`
- [x] Implement `RequestId` for `Result<O, E>` where `O` and `E` both implement `RequestId`
- [x] Re-export `RequestId` in generated crates
- [x] Add integration tests for each request ID access point

Appendix A: Alternate solution for access on successful responses
-----------------------------------------------------------------

Alternatively, for successful responses, a second `send` method (that is difficult to name)w
be added to the fluent client that has a return value that includes both the output and
the request ID (or entire response).

This solution was dismissed due to difficulty naming, and the risk of name collision.

Appendix B: Adding `RequestId` as a string to outputs via model transform
-------------------------------------------------------------------------

The request ID could be stored on outputs by doing a model transform in `sdk-codegen` to add a
`RequestId` member field. However, this causes problems when an output already has a `RequestId` field,
and requires the addition of a synthetic trait to skip binding the field in the generated
serializers/deserializers.

[generic error]: https://docs.rs/aws-smithy-types/0.51.0/aws_smithy_types/error/struct.Error.html
[`GetObjectError` example in S3]: https://docs.rs/aws-sdk-s3/0.21.0/aws_sdk_s3/error/struct.GetObjectError.html#method.request_id
