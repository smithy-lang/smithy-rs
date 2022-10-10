RFC: Error Context and Compatibility
====================================

> Status: RFC
>
> Applies to: Generated clients and shared rust-runtime crates

This RFC proposes a pattern for writing Rust errors to provide consistent
error context AND forwards/backwards compatibility. The goal is to strike
a balance between these four goals:

1. Errors are forwards compatible, and changes to errors are backwards compatible
2. Errors are ergonomic
3. Error messages are easy to debug
4. Errors adhere to Rust's `Error` trait

_Note:_ This RFC is _not_ about error backwards compatibility when it comes to error serialization/deserialization
for transfer over the wire. The Smithy protocols cover that aspect.

Past approaches in smithy-rs
----------------------------

This section examines some examples found in `aws-config` that illustrate different problems
that this RFC will attempt to solve, and calls out what was done well, and what could be improved upon.

### Case study: `InvalidFullUriError`

To start, let's examine `InvalidFullUriError` (doc comments omitted):
```rust
#[derive(Debug)]
#[non_exhaustive]
pub enum InvalidFullUriError {
    #[non_exhaustive] InvalidUri(InvalidUri),
    #[non_exhaustive] NoDnsService,
    #[non_exhaustive] MissingHost,
    #[non_exhaustive] NotLoopback,
    DnsLookupFailed(io::Error),
}

impl Display for InvalidFullUriError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            InvalidFullUriError::InvalidUri(err) => write!(f, "URI was invalid: {}", err),
            InvalidFullUriError::MissingHost => write!(f, "URI did not specify a host"),
            // ... omitted ...
        }
    }
}

impl Error for InvalidFullUriError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            InvalidFullUriError::InvalidUri(err) => Some(err),
            InvalidFullUriError::DnsLookupFailed(err) => Some(err),
            _ => None,
        }
    }
}
```

This error does a few things well:
1. Using `#[non_exhaustive]` on the enum allows new errors to be added in the future.
2. Breaking out different error types allows for more useful error messages, potentially with error-specific context.
   Customers can match on these different error variants to change their program flow, although it's not immediately
   obvious if such use cases exist for this error.
4. The error cause is available through the `Error::source()` impl for variants that have a cause.

However, there are also a number of things that could be improved:
1. All tuple/struct enum members are public, and `InvalidUri` is an error from the `http` crate.
   Exposing a type from another crate can potentially lock the GA SDK into a specific crate version
   if breaking changes are ever made to the exposed types.
2. `DnsLookupFailed` is missing `#[non_exhaustive]`, so new members can never be added to it.
3. Use of enum tuples, even with `#[non_exhaustive]`, adds friction to evolving the API since the
   tuple members cannot be named.
4. Printing the source error in the `Display` impl leads to error repetition by reporters
   that examine the full source chain.
5. The `source()` impl has a `_` matcher, which means implementers may forget to propagate
   a source when adding a new variant.
6. The error source can be downcasted to `InvalidUri` type from `http` in customer code. This is
   a leaky abstraction where customers can start to rely on the underlying library the SDK uses
   in its implementation, and if that library is replaced/changed, it can silently break the
   customer's application. _Note:_ later in the RFC, we'll see that fixing this issue is not practical.

### Case study: `ProfileParseError`

Next, let's look at a much simpler error. The `ProfileParseError` is focused purely on the parsing
logic for the SDK config file:

```rust
#[derive(Debug, Clone)]
pub struct ProfileParseError {
    location: Location,
    message: String,
}

impl Display for ProfileParseError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "error parsing {} on line {}:\n  {}",
            self.location.path, self.location.line_number, self.message
        )
    }
}

impl Error for ProfileParseError {}
```

What this error does well:
- The members are private, so `#[non_exhaustive]` isn't even necessary
- The error is completely opaque (maximizing compatibility) while still being
  debuggable thanks to the flexible messaging

What could be improved:
- It needlessly implements `Clone`, which may prevent it from holding
  an error source in the future since errors are often not `Clone`.
- In the future, if more error variants are needed, a private inner error
  kind enum could be added to change messaging, but there's no way to expose
  information specific to the new error variant to the customer.
- Programmatic access to the error `Location` may be desired, but
  this can be trivially added in the future without a breaking change.

### Case study: code generated client errors

The SDK currently generates errors such as the following (from S3):

```rust
#[non_exhaustive]
pub enum Error {
    BucketAlreadyExists(BucketAlreadyExists),
    BucketAlreadyOwnedByYou(BucketAlreadyOwnedByYou),
    InvalidObjectState(InvalidObjectState),
    NoSuchBucket(NoSuchBucket),
    NoSuchKey(NoSuchKey),
    NoSuchUpload(NoSuchUpload),
    NotFound(NotFound),
    ObjectAlreadyInActiveTierError(ObjectAlreadyInActiveTierError),
    ObjectNotInActiveTierError(ObjectNotInActiveTierError),
    Unhandled(Box<dyn Error + Send + Sync + 'static>),
}
```

Each error variant gets its own struct, which can hold error-specific contextual information.
Except for the `Unhandled` variant, both the error enum and the details on each variant are extensible.
The `Unhandled` variant should move the error source into a struct so that its type can be hidden.
Otherwise, the code generated errors are already aligned with the goals for this RFC.

Approaches from other projects
------------------------------

### `std::io::Error`

The standard library uses an `Error` struct with an accompanying `ErrorKind` enum
for its IO error. Roughly:

```rust
#[derive(Debug)]
#[non_exhaustive]
pub enum ErrorKind {
    NotFound,
    // ... omitted ...
    Other,
}

#[derive(Debug)]
pub struct Error {
    kind: ErrorKind,
    source: Box<dyn std::error::Error + Send + Sync>,
}
```

What this error does well:
- It is extensible since the `ErrorKind` is non-exhaustive
- It has an `Other` error type that can be instantiated by users in unit tests,
  making it easier to unit test error handling

What could be improved:
- There isn't an ergonomic way to add programmatically accessible error-specific context
  to this error in the future
- The source error can be downcasted, which could be a trap for backwards compatibility.

### Hyper 1.0

Hyper is has outlined [some problems they want to address with errors](https://github.com/hyperium/hyper/blob/bd7928f3dd6a8461f0f0fdf7ee0fd95c2f156f88/docs/ROADMAP.md#errors)
for the coming 1.0 release. To summarize:

- It's difficult to match on specific errors (Hyper 0.x's `Error` relies
  on `is_x` methods for error matching rather than enum matching).
- Error reporters duplicate information since the hyper 0.x errors include the display of their error sources
- `Error::source()` can leak internal dependencies

Opaque Error Sources
--------------------

There is [discussion in the errors working group](https://github.com/rust-lang/project-error-handling/issues/53)
about how to avoid leaking internal dependency error types through error source downcasting. One option is to
create an opaque error wrapping new-type that removes the ability to downcast to the other library's error.
This, however, can be circumvented via unsafe code, and also breaks the ability for error reporters to
properly display the error (for example, if the error has backtrace information, that would be
inaccessible to the reporter).

This situation might improve if the nightly `request_value`/`request_ref`/`provide` functions on
`std::error::Error` are stabilized, since then contextual information needed for including things
such as a backtrace could still be retrieved through the opaque error new-type.

This RFC proposes that error types from other libraries not be directly exposed in the API, but rather,
be exposed indirectly through `Error::source` as `&dyn Error + 'static`.

Ideally, errors should contain enough useful information on them that downcasting the underlying cause
is not necessary. Customers needing to downcast the error source should be a last resort, and with the
understanding that the type could change at a later date with no compile-time guarantees.

Error Proposal
--------------

Taking a customer's perspective, there are two broad categories of errors:

1. **Actionable:** Errors that can/should influence program flow
2. **Informative:** Errors that inform that something went wrong, but where
   it's not useful to match on the error to change program flow

This RFC proposes that a consistent pattern be introduced to cover these two use cases for
all errors in the public API for the Rust runtime crates and generated client crates.

### Actionable error pattern

Actionable errors must be represented as enums. If an error variant has an error source or additional
contextual information, it must use a struct that is either embedded inside the enum, or separate where it makes
sense to have private fields. For example:

```rust
pub enum Error {
    // Good
    #[non_exhaustive]
    VariantA,

    // Good
    #[non_exhaustive]
    VariantB {
        some_additional_info: u32,
        source: AnotherError // AnotherError is from this crate
    },

    // Good: This is exhaustive and uses a tuple, but its sole member is an extensible struct
    VariantC(VariantC),

    // Bad: Not ergonomic if additional context is added later
    #[non_exhaustive]
    VariantD(AnotherError),

    // Bad: Exposes another library's error type in the public API,
    // which makes upgrading or replacing that library a breaking change
    #[non_exhaustive]
    VariantF {
        source: http::uri::InvalidUri
    },

    // Bad: The error source type is public, and even though its a boxed error, it won't
    // be possible to change it to an opaque error type later (for example, if/when
    // opaque errors become practical due to standard library stabilizations).
    #[non_exhaustive]
    VariantG {
        source: Box<dyn Error + Send + Sync + 'static>,
    }
}

pub struct VariantC { some_private_field: u32 }
```

When the error variants have a source, the `Error::source` method must be implemented to return that source.
The `source` implementation _should not_ use the catch all (`_`) match arm, as this makes it easy to miss
adding a new error variant's source at a later date.

The error `Display` implementation _must not_ include the source in its output:

```rust
// Good
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::VariantA => write!(f, "variant a"),
            Self::VariantB { some_additional_info, .. } => write!(f, "variant b ({some_additional_info})"),
            // ... and so on
        }
    }
}

// Bad
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::VariantA => write!(f, "variant a"),
            // Bad: includes the source in the `Display` output, which leads to duplicate error information
            Self::VariantB { some_additional_info, source } => write!(f, "variant b ({some_additional_info}): {source}"),
            // ... and so on
        }
    }
}
```

### Informative error pattern

Informative errors must be represented as structs. If error messaging changes based on an underlying cause, then a
private error kind enum can be used internally for this purpose. For example:

```rust
#[derive(Debug)]
pub struct InformativeError {
    some_additional_info: u32,
    source: AnotherError,
}

impl fmt::Display for InformativeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "some informative message with {}", self.some_additional_info)
    }
}

impl Error for InformativeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.source)
    }
}
```

In general, informative errors should be referenced by variants in actionable errors since they cannot be converted
to adaptive errors at a later date without a breaking change. This is not a hard rule, however. Use your best judgement
for the situation.

### Displaying full error context

In places where errors are logged rather than returned to the customer, the full error source chain
must be displayed. This will be made easy by placing a `DisplayErrorContext` struct in `aws-smithy-types` that
is used as a wrapper to get the better error formatting:

```rust
tracing::warn!(err = %DisplayErrorContext(err), "some message");
```

This might be implemented as follows:

```rust
#[derive(Debug)]
pub struct DisplayErrorContext<E: Error>(pub E);

impl<E: Error> fmt::Display for DisplayErrorContext<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_err(f, &self.0)
    }
}

fn write_err(f: &mut fmt::Formatter<'_>, err: &dyn Error) -> fmt::Result {
    write!(f, "{}", err)?;
    if let Some(source) = err.source() {
        write!(f, ": ")?;
        write_err(f, source)?;
    }
    Ok(())
}
```

Changes Checklist
-----------------

- [ ] Update every struct/enum that implements `Error` in all the non-server Rust runtime crates
- [ ] Hide error source type in `Unhandled` variant in code generated errors
