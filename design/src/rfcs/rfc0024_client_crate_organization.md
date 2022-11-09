RFC: Client Crate Organization
==============================

> Status: RFC
>
> Applies to: clients (and impacts servers due to shared codegen)

This RFC proposes changing the organization structure of the generated client crates to:

1. Make discovery in the crate documentation easier.
2. Facilitate re-exporting types from runtime crates in related modules without name collisions.

Previous Organization
---------------------

Previously, crates were organized as such:

```text
.
├── client
|   ├── fluent_builders
|   |   └── <One fluent builder per operation>
|   ├── Builder (*)
|   └── Client
├── config
|   ├── retry
|   |   ├── RetryConfig (*)
|   |   ├── RetryConfigBuilder (*)
|   |   └── RetryMode (*)
|   ├── timeout
|   |   ├── TimeoutConfig (*)
|   |   └── TimeoutConfigBuilder (*)
|   ├── AsyncSleep (*)
|   ├── Builder
|   ├── Config
|   └── Sleep (*)
├── error
|   ├── <One module per error to contain a single struct named `Builder`>
|   ├── <One struct per error named `${error}`>
|   ├── <One struct per operation named `${operation}Error`>
|   └── <One enum per operation named `${operation}ErrorKind`>
├── http_body_checksum (empty)
├── input
|   ├── <One module per input to contain a single struct named `Builder`>
|   └── <One struct per input named `${operation}Input`>
├── lens (empty)
├── middleware
|   └── DefaultMiddleware
├── model
|   ├── <One module per shape to contain a single struct named `Builder`>
|   └── <One struct per shape>
├── operation
|   ├── customize
|   |   ├── ClassifyRetry (*)
|   |   ├── CustomizableOperation
|   |   ├── Operation (*)
|   |   ├── RetryKind (*)
|   └── <One struct per operation>
├── output
|   ├── <One module per output to contain a single struct named `Builder`>
|   └── <One struct per output named `${operation}Input`>
├── paginator
|   ├── <One struct per paginated operation named `${operation}Paginator`>
|   └── <Zero to one struct(s) per paginated operation named `${operation}PaginatorItems`>
├── presigning
|   ├── config
|   |   ├── Builder
|   |   ├── Error
|   |   └── PresigningConfig
|   └── request
|       └── PresignedRequest
├── types
|   ├── AggregatedBytes (*)
|   ├── Blob (*)
|   ├── ByteStream (*)
|   ├── DateTime (*)
|   └── SdkError (*)
├── AppName (*)
├── Client
├── Config
├── Credentials (*)
├── Endpoint (*)
├── Error
├── ErrorExt (for some services)
├── PKG_VERSION
└── Region (*)
```

`(*)` - signifies that a type is re-exported from one of the runtime crates

The Crate Root
--------------

The crate root should only host the most frequently used types, or phrased differently,
the types that are critical to making a service call with default configuration,
or that are required for the most frequent config changes (such as setting credentials,
or changing the region/endpoint).

Previously, the following were exported in root:
```
.
├── AppName
├── Client
├── Config
├── Credentials
├── Endpoint
├── Error
├── ErrorExt (for some services)
├── PKG_VERSION
└── Region
```

The `AppName` is infrequently set, and should be moved into `crate::config`. The `aws-config` crate
drastically reduces the usefulness of `Credentials`, `Endpoint`, and `Region`, so these can move
into `crate::config`. `ErrorExt` should be moved into `crate::error`, but `Error` should stay in
the crate root so that customers that alias the SDK crate can easily reference it in their `Result`s:

```rust
use aws_sdk_s3 as s3;

fn some_function(/* ... */) -> Result<(), s3::Error> {
    /* ... */
}
```

The `PKG_VERSION` should move into a new `meta` module, which can also include other values in the future
such as the SHA-256 hash of the model used to produce the crate, or the version of smithy-rs that generated it.

Builder Organization
--------------------

Builders (distinct from fluent builders) are generated alongside all inputs, outputs, models, and errors.
They all follow the same overall pattern (where `shapeType` is `Input`, `Output`, or empty for models/errors):

```text
.
└── module
    ├── <One module per shape to contain a single struct named `Builder`>
    └── <One struct per shape named `${prefix}${shapeType}`>
```

This results in large lists of modules that all have exactly one item in them, which makes browsing
the documentation difficult, and introduces the possibility of name collisions when re-exporting modules
from the runtime crates.

Builders should adopt a prefix and go into a single `builders` module, similar to how the fluent builders
currently work:

```text
.
├── module
|   └── builders
|       └── <One struct per shape named `${prefix}${shapeType}Builder`>
└──---- <One struct per shape named `${prefix}${shapeType}`>
```

Client Module
-------------

Previously, the Smithy `Client` builder was re-exported alongside the SDK fluent `Client`
so that non-SDK clients could easily customize the underlying Smithy client by using
the fluent client's `Client::with_config` function or `From<aws_smithy_client::client::Client<C, M, R>>`
trait implementation.

This makes sense for non-SDK clients where customization of the connector and middleware types is supported
generically, but less sense for SDKs since the SDK clients are hardcoded to use `DynConnector` and `DynMiddleware`.

Thus, the Smithy client `Builder` should not be re-exported for SDKs.

Model Module
------------

The name `model` is meaningless outside the context of code generation, but there is precedent for AWS SDKs
using the term (both the Java V2 and Kotlin SDKs use the term). Previously, this module held all the generated
structs/enums that are referenced by inputs, outputs, and errors.

This RFC proposes that the generated input, output, and error structs (and their associated builders)
be moved into the `model` module as submodules, so that it looks like the following:

```text
.
└── model
    ├── error
    |   ├── builders
    |   |   └── <One struct per error named `${error}Builder`>
    |   ├── <One struct per error named `${error}`>
    |   └── <One enum per operation named `${operation}Error`>
    ├── input
    |   ├── builders
    |   |   └── <One struct per input named `${operation}InputBuilder`>
    |   └── <One struct per input named `${operation}Input`>
    ├── output
    |   ├── builders
    |   |   └── <One struct per output named `${operation}OutputBuilder`>
    |   └── <One struct per output named `${operation}Output`>
    ├── builders
    |   └── <One struct per shape named `${shape}Builder`>
    └── <One struct per shape>
```

This won't have any impact on customers that use the fluent client since they won't need to import
inputs, outputs, and modeled errors (not including the root-level `Error` type). Having all the code
generated input, output, error, and data types in one module eliminates the name collision problem
for the `crate::error` module, which is covered next.

### Combining `Error` with `ErrorKind`

Each operation has both an `Error` and `ErrorKind` type generated. The `Error` type
holds information that is common across all operation errors: message, error code,
"extra" key/value pairs, and the request ID.

The `ErrorKind` is always nested inside the `Error`, which results in nested matching
in customer error handling code. For example, with S3's `GetObject` operation, you might
see:

```rust
match result {
    Ok(_output) => { /* ok */ }
    Err(err) => match err.into_service_error()? {
        GetObjectError { kind: GetObjectErrorKind::NoSuchKey(_), .. } => {
            // Do something with this information
        }
        err @ GetObjectError { .. } if err.code() == "SomeUnmodeledErrorCode" => { /* ... */ }
        err @ _ => return Err(err)
    }
}
```

If the `Error` and `ErrorKind` types are combined, then this becomes simpler:

```rust
match result {
    Ok(_output) => { /* ok */ }
    Err(err) => match err.into_service_error()? {
        GetObjectError::NoSuchKey(_) => {
            // Do something with this information
        }
        err @ _ if err.code() == "SomeUnmodeledErrorCode" => { /* ... */ }
        err @ _ => return Err(err)
    }
}
```

Combining the error types requires adding the general error metadata to each generated
error struct so that it's accessible by the enum error type. However, this aligns with
our tenet of making things easier for customers even if it makes it harder for ourselves.

Error Module
------------

The `error` module is greatly reduced after moving the generated errors into `crate::model::error`.
This top-level module is now available for re-exports and utilities.

The following will be re-exported in `crate::error`:
- `aws_smithy_http::result::SdkError`
- `aws_smithy_types::error::display::DisplayErrorContext`

For crates that have an `ErrorExt`, it will also be moved into `crate::error`.

Presigning Module
-----------------

The `crate::presigning` module only has four members, so it should be flattened from:

```text
.
└── presigning
    ├── config
    |   ├── Builder
    |   ├── Error
    |   └── PresigningConfig
    └── request
        └── PresignedRequest
```

to:

```text
.
└── presigning
    ├── PresigningConfigBuilder
    ├── PresigningConfigError
    ├── PresigningConfig
    └── PresignedRequest
```

At the same time, `Builder` and `Error` will be renamed to `PresigningConfigBuilder` and `PresigningConfigError`
respectively since these will rarely be referred to directly (preferring `PresigningConfig::builder()` instead; the
error will almost always be unwrapped).

Operation Module
----------------

The `crate::operation` module has a nested `customize` that re-exports types
that are useful when calling [`customize_operation()`](./rfc0017_customizable_client_operations.md)
on the fluent builders, and other types that are useful when implementing a custom
retry classifier for an operation.

The code generator creates a struct for every operation in `crate::operation`, and each of these
structs implements the `ParseResponse` trait. There really isn't a good reason for these to be
exposed directly, so they can either be made private or `#[doc(hidden)]`. They should
also be added to a `parsers` submodule to keep the namespace open for future re-exports.

The `crate::paginator` should also move into `crate::operation` since they are generated
per-operation and aren't relevant at the top-level since their entry point is the fluent
client's `into_paginator()` method.

After these changes, the `operation` module looks as follows:

```text
.
└── operation
    ├── customize
    |   ├── ClassifyRetry
    |   ├── CustomizableOperation
    |   ├── Operation
    |   └── RetryKind
    ├── paginator
    |   ├── <One struct per paginated operation named `${operation}Paginator`>
    |   └── <Zero to one struct(s) per paginated operation named `${operation}PaginatorItems`>
    └── parsers (private/doc hidden)
        └── <One struct per operation>
```

Types Module
------------

As a rule, the `crate::types` module re-exports types from `aws-smithy-types` that are
referenced by code generated structs/enums, and should only re-export the types
that are actually referenced.

Empty Modules
-------------

The `lens` and `http_body_checksum` modules have nothing inside them,
and their documentation descriptions are not useful to customers:

> `lens`: Generated accessors for nested fields

> `http_body_checksum`: Functions for modifying requests and responses for the purposes of checksum validation

These modules hold private functions that are used by other generated code, and should just be made
private or `#[doc(hidden)]` if necessary.

New Organization
----------------

All combined, the following is the new publicly visible organization:

```text
.
├── client
|   ├── fluent_builders
|   |   └── <One fluent builder per operation>
|   ├── Builder (only in non-SDK crates) (*)
|   └── Client
├── config
|   ├── retry
|   |   ├── RetryConfig (*)
|   |   ├── RetryConfigBuilder (*)
|   |   └── RetryMode (*)
|   ├── timeout
|   |   ├── TimeoutConfig (*)
|   |   └── TimeoutConfigBuilder (*)
|   ├── AppName (*)
|   ├── AsyncSleep (*)
|   ├── Builder
|   ├── Config
|   ├── Credentials (*)
|   ├── Endpoint (*)
|   ├── Region (*)
|   └── Sleep (*)
├── error
|   ├── DisplayErrorContext (*)
|   ├── ErrorExt (for some services)
|   └── SdkError (*)
├── meta
|   └── PKG_VERSION
├── middleware
|   └── DefaultMiddleware
├── model
|   ├── error
|   |   ├── builders
|   |   |   └── <One struct per error named `${error}Builder`>
|   |   ├── <One struct per error named `${error}`>
|   |   └── <One enum per operation named `${operation}Error`>
|   ├── input
|   |   ├── builders
|   |   |   └── <One struct per input named `${operation}InputBuilder`>
|   |   └── <One struct per input named `${operation}Input`>
|   ├── output
|   |   ├── builders
|   |   |   └── <One struct per output named `${operation}OutputBuilder`>
|   |   └── <One struct per output named `${operation}Output`>
|   ├── builders
|   |   └── <One struct per shape named `${shape}Builder`>
|   └── <One struct per shape>
├── operation
|   ├── customize
|   |   ├── ClassifyRetry (*)
|   |   ├── CustomizableOperation
|   |   ├── Operation (*)
|   |   └── RetryKind (*)
|   └── paginator
|       ├── <One struct per paginated operation named `${operation}Paginator`>
|       └── <Zero to one struct(s) per paginated operation named `${operation}PaginatorItems`>
├── presigning
|   ├── PresigningConfigBuilder
|   ├── PresigningConfigError
|   ├── PresigningConfig
|   └── PresignedRequest
├── types
|   ├── AggregatedBytes (*)
|   ├── Blob (*)
|   ├── ByteStream (*)
|   └── DateTime (*)
├── Client
├── Config
└── Error
```

`(*)` - signifies that a type is re-exported from one of the runtime crates

Changes Checklist
-----------------

- [ ] Move `crate::AppName`, `crate::Endpoint`, `crate::Credentials`, and `crate::Region` into `crate::config`
- [ ] Move `crate::PKG_VERSION` into a new `crate::meta` module
- [ ] Reorganize the builders
- [ ] Only re-export `aws_smithy_client::client::Builder` for non-SDK clients (remove from SDK clients)
- [ ] Move `crate::input`, `crate::output`, and `crate::error` into `crate::model`
- [ ] Combine `Error` and `ErrorKind` for operation errors
- [ ] Move `crate::ErrorExt` into `crate::error`
- [ ] Re-export `aws_smithy_types::error::display::DisplayErrorContext` and `aws_smithy_http::result::SdkError` in `crate::error`
- [ ] Move `crate::paginator` into `crate::operation`
- [ ] Flatten `crate::presigning`
- [ ] Remove/hide operation `ParseResponse` implementations in `crate::operation`
- [ ] Hide or remove `crate::lens` and `crate::http_body_checksum`
- [ ] Update "Crate Organization" top-level section in generated crate docs
