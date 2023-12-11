RFC: Client Crate Organization
==============================

> Status: Implemented
>
> Applies to: clients (and may impact servers due to shared codegen)

This RFC proposes changing the organization structure of the generated client crates to:

1. Make discovery in the crate documentation easier.
2. Facilitate re-exporting types from runtime crates in related modules without name collisions.
3. Facilitate feature gating operations for faster compile times in the future.

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

Proposed Changes
----------------

This RFC proposes reorganizing types by operation first and foremost, and then rearranging
other pieces to reduce codegen collision risk.

### Establish a pattern for builder organization

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

### Organize code generated types by operation

All code generated for an operation that isn't shared between operations will go into
operation-specific modules. This includes inputs, outputs, errors, parsers, and paginators.
Types shared across operations will remain in another module (discussed below), and
serialization/deserialization logic for those common types will also reside in that
common location for now. If operation feature gating occurs in the future, further
optimization can be done to track which of these are used by feature, or they can
be reorganized (this would be discussed in a future RFC and is out of scope here).

With code generated operations living in `crate::operation`, there is a high chance of name
collision with the `customize` module. To resolve this, `customize` will be moved into
`crate::client`.

The new `crate::operation` module will look as follows:

```text
.
└── operation
    └── <One module per operation named after the operation in lower_snake_case>
        ├── paginator
        |   ├── `${operation}Paginator`
        |   └── `${operation}PaginatorItems`
        ├── builders
        |   ├── `${operation}FluentBuilder`
        |   ├── `${operation}InputBuilder`
        |   └── `${operation}OutputBuilder`
        ├── `${operation}Error`
        ├── `${operation}Input`
        ├── `${operation}Output`
        └── `${operation}Parser` (private/doc hidden)
```

### Reorganize the crate root

The crate root should only host the most frequently used types, or phrased differently,
the types that are critical to making a service call with default configuration,
or that are required for the most frequent config changes (such as setting credentials,
or changing the region/endpoint).

Previously, the following were exported in root:
```text
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

The `AppName` is infrequently set, and will be moved into `crate::config`. Customers are encouraged
to use `aws-config` crate to resolve credentials, region, and endpoint. Thus, these types no longer
need to be at the top-level, and will be moved into `crate::config`. `ErrorExt` will be moved into
`crate::error`, but `Error` will stay in the crate root so that customers that alias the SDK crate
can easily reference it in their `Result`s:

```rust,ignore
use aws_sdk_s3 as s3;

fn some_function(/* ... */) -> Result<(), s3::Error> {
    /* ... */
}
```

The `PKG_VERSION` should move into a new `meta` module, which can also include other values in the future
such as the SHA-256 hash of the model used to produce the crate, or the version of smithy-rs that generated it.

### Conditionally remove `Builder` from `crate::client`

Previously, the Smithy `Client` builder was re-exported alongside the SDK fluent `Client`
so that non-SDK clients could easily customize the underlying Smithy client by using
the fluent client's `Client::with_config` function or `From<aws_smithy_client::client::Client<C, M, R>>`
trait implementation.

This makes sense for non-SDK clients where customization of the connector and middleware types is supported
generically, but less sense for SDKs since the SDK clients are hardcoded to use `DynConnector` and `DynMiddleware`.

Thus, the Smithy client `Builder` should not be re-exported for SDKs.

### Create a `primitives` module

Previously, `crate::types` held re-exported types from `aws-smithy-types` that are used
by code generated structs/enums.

This module will be renamed to `crate::primitives` so that the name `types` can be
repurposed in the next section.

### Repurpose the `types` module

The name `model` is meaningless outside the context of code generation (although there is precedent
since both the Java V2 and Kotlin SDKs use the term). Previously, this module held all the generated
structs/enums that are referenced by inputs, outputs, and errors.

This RFC proposes that this module be renamed to `types`, and that all code generated types
for shapes that are reused between operations (basically anything that is not an input, output,
or error) be moved here. This would look as follows:

```text
.
└── types
    ├── error
    |   ├── builders
    |   |   └── <One struct per error named `${error}Builder`>
    |   └── <One struct per error named `${error}`>
    ├── builders
    |   └── <One struct per shape named `${shape}Builder`>
    └── <One struct per shape>
```

Customers using the fluent builder should be able to just `use ${crate}::types::*;` to immediately
get access to all the shared types needed by the operations they are calling.

Additionally, moving the top-level code generated error types into `crate::types` will eliminate a name
collision issue in the `crate::error` module.

### Repurpose the original `crate::error` module

The `error` module is significantly smaller after all the code generated error types
are moved out of it. This top-level module is now available for re-exports and utilities.

The following will be re-exported in `crate::error`:
- `aws_smithy_http::result::SdkError`
- `aws_smithy_types::error::display::DisplayErrorContext`

For crates that have an `ErrorExt`, it will also be moved into `crate::error`.

### Flatten the `presigning` module

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

### Remove the empty modules

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
|   ├── customize
|   |   ├── ClassifyRetry (*)
|   |   ├── CustomizableOperation
|   |   ├── Operation (*)
|   |   └── RetryKind (*)
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
├── operation
|   └── <One module per operation named after the operation in lower_snake_case>
|       ├── paginator
|       |   ├── `${operation}Paginator`
|       |   └── `${operation}PaginatorItems`
|       ├── builders
|       |   ├── `${operation}FluentBuilder`
|       |   ├── `${operation}InputBuilder`
|       |   └── `${operation}OutputBuilder`
|       ├── `${operation}Error`
|       ├── `${operation}Input`
|       ├── `${operation}Output`
|       └── `${operation}Parser` (private/doc hidden)
├── presigning
|   ├── PresigningConfigBuilder
|   ├── PresigningConfigError
|   ├── PresigningConfig
|   └── PresignedRequest
├── primitives
|   ├── AggregatedBytes (*)
|   ├── Blob (*)
|   ├── ByteStream (*)
|   └── DateTime (*)
├── types
|   ├── error
|   |   ├── builders
|   |   |   └── <One struct per error named `${error}Builder`>
|   |   └── <One struct per error named `${error}`>
|   ├── builders
|   |   └── <One struct per shape named `${shape}Builder`>
|   └── <One struct per shape>
├── Client
├── Config
└── Error
```

`(*)` - signifies that a type is re-exported from one of the runtime crates

Changes Checklist
-----------------

- [x] Move `crate::AppName` into `crate::config`
- [x] Move `crate::PKG_VERSION` into a new `crate::meta` module
- [x] Move `crate::Endpoint` into `crate::config`
- [x] Move `crate::Credentials` into `crate::config`
- [x] Move `crate::Region` into `crate::config`
- [x] Move `crate::operation::customize` into `crate::client`
- [x] Finish refactor to decouple client/server modules
- [x] Organize code generated types by operation
- [x] Reorganize builders
- [x] Rename `crate::types` to `crate::primitives`
- [x] Rename `crate::model` to `crate::types`
- [x] Move `crate::error` into `crate::types`
- [x] Only re-export `aws_smithy_client::client::Builder` for non-SDK clients (remove from SDK clients)
- [x] Move `crate::ErrorExt` into `crate::error`
- [x] Re-export `aws_smithy_types::error::display::DisplayErrorContext` and `aws_smithy_http::result::SdkError` in `crate::error`
- [x] Move `crate::paginator` into `crate::operation`
- [x] Flatten `crate::presigning`
- [x] Hide or remove `crate::lens` and `crate::http_body_checksum`
- [x] Move fluent builders into `crate::operation::x::builders`
- [x] Remove/hide operation `ParseResponse` implementations in `crate::operation`
- [x] Update "Crate Organization" top-level section in generated crate docs
- [x] Update all module docs
- [x] Break up modules/files so that they're not 30k lines of code
  - [x] models/types; each struct/enum should probably get its own file with pub-use
  - [x] models/types::builders: now this needs to get split up
  - [x] `client.rs`
- [x] Fix examples
- [x] Write changelog
