<!-- Give your RFC a descriptive name saying what it would accomplish or what feature it defines -->
RFC: Forward Compatible Errors
=============

<!-- RFCs start with the "RFC" status and are then either "Implemented" or "Rejected".  -->
> Status: RFC
>
> Applies to: client

<!-- A great RFC will include a list of changes at the bottom so that the implementor can be sure they haven't missed anything -->
For a summarized list of proposed changes, see the [Changes Checklist](#changes-checklist) section.

<!-- Insert a short paragraph explaining, at a high level, what this RFC is for -->
This RFC defines an approach for making it forwards-compatible to convert **unmodeled** `Unhandled` errors into modeled ones. This occurs as servers update their models to include errors that were previously unmodeled.

Currently, SDK errors are **not** forward compatible in this way. If a customer matches `Unhandled` in addition to the `_` branch and a new variant is added, **they will fail to match the new variant**. We currently handle this issue with enums by prevent useful information from being readable from the `Unknown` variant.

This is related to ongoing work on the [`non_exhaustive_omitted_patterns` lint](https://github.com/rust-lang/rust/issues/89554) which would produce a compiler warning when a new variant was added even when `_` was used.

<!-- The "Terminology" section is optional but is really useful for defining the technical terms you're using in the RFC -->
Terminology
-----------

For purposes of discussion, consider the following error:
```rust,ignore
#[non_exhaustive]
pub enum AbortMultipartUploadError {
    NoSuchUpload(NoSuchUpload),
    Unhandled(Unhandled),
}
```

- **Modeled Error**: An error with an named variant, e.g. `NoSuchUpload` above
- **Unmodeled Error**: Any other error, e.g. if the server returned `ValidationException` for the above operation.
- **Error code**: All errors across all protocols provide a `code`, a unique method to identify an error across the service closure.

<!-- Explain how users will use this new feature and, if necessary, how this compares to the current user experience -->
The user experience if this RFC is implemented
----------------------------------------------

In the current version of the SDK, users match the `Unhandled` variant. They can then read the code from the `Unhandled` variant because [`Unhandled`](https://docs.rs/aws-smithy-types/0.56.1/aws_smithy_types/error/struct.Unhandled.html) implements the `ProvideErrorMetadata` trait as well as the standard-library `std::error::Error` trait.

> Note: It's possible to write correct code today because the operation-level and service-level errors already expose `code()` via `ProvideErrorMetadata`. This RFC describes mechanisms to guide customers to write forward-compatible code.

```rust,ignore
# fn docs() {
    match client.get_object().send().await {
        Ok(obj) => { ... },
        Err(e) => match e.into_service_error() {
            GetObjectError::NotFound => { ... },
            GetObjectError::Unhandled(err) if err.code() == "ValidationException" => { ... }
            other => { /** do something with this variant */ }
        }
    }
# }
```

We must instead guide customers into the following pattern:
```rust,ignore
# fn docs() {
    match client.get_object().send().await {
        Ok(obj) => { ... },
        Err(e) => match e.into_service_error() {
            GetObjectError::NotFound => { ... },
            err if err.code() == "ValidationException" => { ... },
            err => warn!("{}", err.code()),
        }
    }
# }
```

In this example, because customers are _not_ matching on the `Unhandled` variant explicitly this code is forward compatible for `ValidationException` being introduced in the future.

**Guiding Customers to this Pattern**
There are two areas we need to handle:
1. Prevent customers from extracting useful information from `Unhandled`
2. Alert customers _currently_ using unhandled what to use instead. For example, the following code is still problematic:
    ```rust,ignore
        match err {
            GetObjectError::NotFound => { ... },
            err @ GetObject::Unhandled(_) if err.code() == Some("ValidationException") => { ... }
        }
    ```

For `1`, we need to remove the `ProvideErrorMetadata` trait implementation from `Unhandled`. We would expose this isntead through a layer of indirection to enable code generated to code to still read the data.

For `2`, we would deprecate the `Unhandled` variants with a message clearly indicating how this code should be written.

How to actually implement this RFC
----------------------------------

### Locking down `Unhandled`
In order to prevent accidental matching on `Unhandled`, we need to make it hard to extract useful information from `Unhandled` itself. We will do this by removing the `ProvideErrorMetadata` trait implementation and exposing the following method:

```rust,ignore
#[doc(hidden)]
/// Introspect the error metadata of this error.
///
/// This method should NOT be used from external code because matching on `Unhandled` directly is a backwards-compatibility
/// hazard. See `RFC-0039` for more information.
pub fn introspect(&self) -> impl ProvideErrorMetadata + '_ {
   struct Introspected<'a>(&'a Unhandled);
   impl ProvideErrorMetadata for Introspected { ... }
   Introspected(self)
}
```

Generated code would this use `introspect` when supporting **top-level** `ErrorMetadata` (e.g. for [`aws_sdk_s3::Error`](https://docs.rs/aws-sdk-s3/latest/aws_sdk_s3/enum.Error.html)).

### Deprecating the Variant
The `Unhandled` variant will be deprecated to prevent users from matching on it inadvertently.

```rust,ignore
enum GetObjectError {
   NotFound(NotFound),
   #[deprecated("Matching on `Unhandled` directly is a backwards compatibility hazard. Use `err if err.error_code() == ...` instead. See [here](<docs about using errors>) for more information.")]
   Unhandled(Unhandled)
}
```

###

<!-- Include a checklist of all the things that need to happen for this RFC's implementation to be considered complete -->
Changes checklist
-----------------

- [ ] Generate code to deprecate unhandled variants. Determine the best way to allow `Unhandled` to continue to be constructed in client code
- [ ] Generate code to deprecate the `Unhandled` variant for the service meta-error. Consider how this interacts with non-service errors.
- [ ] Update `Unhandled` to make it useless on its own and expose information via an `Introspect` doc hidden struct.
- [ ] Update developer guide to address this issue.
- [ ] Changelog & Upgrade Guidance
