RFC: Improve Client Error Ergonomics
====================================

> Status: Implemented
>
> Applies to: clients

This RFC proposes some changes to code generated errors to make them easier to use for customers.
With the SDK and code generated clients, customers have two primary use-cases that should be made
easy without compromising the compatibility rules established in [RFC-0022]:

1. Checking the error type
2. Retrieving information specific to that error type

Case Study: Handling an error in S3
-----------------------------------

The following is an example of handling errors with S3 with the latest generated (and unreleased)
SDK as of 2022-12-07:

```rust,ignore
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

The refactor that implemented [RFC-0022] added the `into_service_error()` method on `SdkError` that
infallibly converts the `SdkError` into the concrete error type held by the `SdkError::ServiceError` variant.
This improvement lets customers discard transient failures and immediately handle modeled errors
returned by the service.

Despite this, the code is still quite verbose.

Proposal: Combine `Error` and `ErrorKind`
-----------------------------------------

At time of writing, each operation has both an `Error` and `ErrorKind` type generated.
The `Error` type holds information that is common across all operation errors: message,
error code, "extra" key/value pairs, and the request ID.

The `ErrorKind` is always nested inside the `Error`, which results in the verbose
nested matching shown in the case study above.

To make error handling more ergonomic, the code generated `Error` and `ErrorKind` types
should be combined. Hypothetically, this would allow for the case study above to look as follows:

```rust,ignore
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
        err if err.is_no_such_key() => {
            println!("object didn't exist");
        }
        err if err.code() == Some("SomeUnmodeledError") => {}
        err @ _ => return Err(err.into()),
    },
}
```

If a customer only cares about checking one specific error type, they can also do:

```rust,ignore
match result {
    Ok(_output) => { /* Do something with the output */ }
    Err(err) => {
        let err = err.into_service_error();
        if err.is_no_such_key() {
            println!("object didn't exist");
        } else {
            return Err(err);
        }
    }
}
```

The downside of this is that combining the error types requires adding the general error
metadata to each generated error struct so that it's accessible by the enum error type.
However, this aligns with our tenet of making things easier for customers even if it
makes it harder for ourselves.

Changes Checklist
-----------------

- [x] Merge the `${operation}Error`/`${operation}ErrorKind` code generators to only generate an `${operation}Error` enum:
  - Preserve the `is_${variant}` methods
  - Preserve error metadata by adding it to each individual variant's context struct
- [x] Write upgrade guidance
- [x] Fix examples

[RFC-0022]: ./rfc0022_error_context_and_compatibility.md
