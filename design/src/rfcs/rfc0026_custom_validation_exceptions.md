RFC: Custom validation exceptions
=================================

> Status: RFC
>
> Applies to: server

This RFC describes a feature whereby server SDK application owners will be able
to return custom error messages when validation of [constraint
traits][constraint-traits] fails for an operation's input.

[constraint-traits]: https://awslabs.github.io/smithy/2.0/spec/constraint-traits.html

Terminology
-----------

- **Shape closure**: the set of shapes a shape can "reach", including itself.
- **Transitively constrained shape**: a shape whose closure includes: 
    1. a shape with a [constraint trait][constraint-traits] attached,
    2. a (member) shape with a [`required` trait] attached,
    3. an [`enum` shape]; or
    4. an [`intEnum` shape].
- A **directly constrained shape** is any of these:
    1. a shape with a [constraint trait][constraint-traits] attached,
    2. a (member) shape with a [`required` trait] attached,
    3. an [`enum` shape],
    4. an [`intEnum` shape]; or
    5. a [`structure` shape] with at least one `required` member shape.
- **Constrained type**: the Rust type a constrained shape gets rendered as. For
  shapes that are not `structure`, `union`, `enum`, or `intEnum` shapes, these
  are wrapper [newtype]s.
- **`ConstraintViolation`**: the Rust type a constrained type's [`TryFrom`]
  constructor exposes to user code when a value violates the direct modeled
  constraints.
- **`ConstraintViolationException`**: the Rust type a constrained type's
  _internal_ framework-exclusive constructor returns when a value violates the
  modeled constraints.
- **Constrained operation**: an operation whose input shape is transitively
  constrained.

In the absence of a qualifier, "constrained shape" should be interpreted as
"transitively constrained shape".

To learn more about the design of constraint traits in server SDKs and more
in-depth explanations of the above terminology, the following RFCs are
recommended reading:

- [RFC: Constraint traits](https://github.com/awslabs/smithy-rs/pull/1199).
- [RFC: Better Constraint Violations][better-constraint-violations].

[better-constraint-violations]: https://github.com/awslabs/smithy-rs/pull/2040
[`required` trait]: https://smithy.io/2.0/spec/type-refinement-traits.html#required-trait
[`enum` shape]: https://smithy.io/2.0/spec/simple-types.html#enum
[`intEnum` shape]: https://smithy.io/2.0/spec/simple-types.html#intenum
[`structure` shape]: https://smithy.io/2.0/spec/aggregate-types.html#structure
[newtype]: https://doc.rust-lang.org/rust-by-example/generics/new_types.html
[`TryFrom`]: https://doc.rust-lang.org/std/convert/trait.TryFrom.html

### Background

Constrained operations are currently required to have
`smithy.framework#ValidationException` as a member in their [`errors`
property](https://smithy.io/2.0/spec/service-types.html#operation). This is the
shape that is rendered in responses when a request contains data that violates
the modeled constraints.

The shape is defined in the [`smithy-validation-model` Maven
package](https://search.maven.org/artifact/software.amazon.smithy/smithy-validation-model),
[as follows](https://github.com/awslabs/smithy/blob/main/smithy-validation-model/model/smithy.framework.validation.smithy):

```smithy
$version: "2.0"

namespace smithy.framework

/// A standard error for input validation failures.
/// This should be thrown by services when a member of the input structure
/// falls outside of the modeled or documented constraints.
@error("client")
structure ValidationException {

    /// A summary of the validation failure.
    @required
    message: String,

    /// A list of specific failures encountered while validating the input.
    /// A member can appear in this list more than once if it failed to satisfy multiple constraints.
    fieldList: ValidationExceptionFieldList
}

/// Describes one specific validation failure for an input member.
structure ValidationExceptionField {
    /// A JSONPointer expression to the structure member whose value failed to satisfy the modeled constraints.
    @required
    path: String,

    /// A detailed description of the validation failure.
    @required
    message: String
}

list ValidationExceptionFieldList {
    member: ValidationExceptionField
}
```

Smithy's protocol tests provide a normative reference for what the error
message contents' should be. For example, the
[RestJsonMalformedPatternMapValue](https://github.com/awslabs/smithy/blob/31ddf685d7e3fe287eac51442621975d585972fd/smithy-aws-protocol-tests/model/restJson1/validation/malformed-pattern.smithy#L154-L154)
test stipulates that upon receiving the JSON value:

```json
{ "map" : { "abc": "ABC" } }
```

for the modeled map:

```smithy
map PatternMap {
    key: PatternString,
    value: PatternString
}

@pattern("^[a-m]+$")
string PatternString
```

the JSON response should be:

```json
{
    { 
        "message" : "1 validation error detected. Value abc at '/map/abc' failed to satisfy constraint: Member must satisfy regular expression pattern: ^[a-m]+$$",
        "fieldList" : [
            {
                "message": "Value ABC at '/map/abc' failed to satisfy constraint: Member must satisfy regular expression pattern: ^[a-m]+$$",
                "path": "/map/abc"
            }
        ]
    }
}
```

Currently, the framework is mapping from the operation input shape's
`ConstraintViolationExceptions` (a [newtype] around a container of
`ConstraintViolationException`s) to a `ValidationException`. This mapping is
hardcoded so as to make the protocol tests pass.

[newtype]: https://doc.rust-lang.org/rust-by-example/generics/new_types.html

### Problem

Some users have expressed interest in being able to customize the error
responses. So far their use cases mainly revolve around:

- using smithy-rs server SDKs in frontend services that are invoked by user
  interface code: for example, said service might want to internationalize or
  enhance error messages that are displayed directly to users in a web form.
- backwards compatibility: a service that has historically returned a custom
  response body for validation failures would be able to be migrated to use a
  smithy-rs server SDK.

Currently, it is a _strict requirement_ that constrained operations's errors
contain `smithy.framework#ValidationException`, and its error messages are not
configurable.

### Solution proposal

It is worth noting that the _vast_ majority of smithy-rs server SDK users are
operating within a backend service, and that the default behavior prescribed by
Smithy's protocol tests is satisfactory for their use cases. Therefore, it is
reasonable to expect that a solution to lift the aforementioned restriction be
_opt-in_.

We will introduce a `codegenConfig.disableDefaultValidation` flag in
`smithy-build.json` with the following semantics:

* If `disableDefaultValidation` is set to `false`, we will continue operating
  as we currently do: all constrained operations _must_ have
  `smithy.framework#ValidationException` attached.
* If `disableDefaultValidation` is set to `true`, any constrained operation
  that does not have `smithy.framework#ValidationException` attached will be
  code generated so as to force the user to provide a [`core::ops::Fn`] value
  that maps from the constrained operation's `ConstraintViolationExceptions`
  container newtype to any modeled error shape attached to the operation.

For example, an operation like:

```smithy
operation MyOperation {
    input: MyOperationInput,
    output: MyOperationOutput,
    errors: [ValidationCustomError, OperationError1, ... OperationErrorN]
}

@error("client")
structure ValidationCustomError {
    @required
    validationErrorMessage: String
}
```

in a model generated with `codegenConfig.disableDefaultValidation` set to
`true` will be implemented in user code as:

```rust
use my_server_sdk::{input, output, error}

fn map_operation_validation_failure_to_error(errors: input::my_operation_input::ConstraintViolationExceptions) -> error::OperationError {
    let mut validation_error_message: String::new();

    for e in errors.into_iter() {
        match e {
            // ...
        }
    }

    // We can return _any_ error attached to the operation.
    error::OperationError::ValidationCustomError(
        error::ValidationCustomError {
            validation_error_message
        }
    )
}

async fn operation_handler(input: input::MyOperationInput) -> Result<output::MyOperationOutput, error::MyOperationError> {
    // ...
}
```

Since the operation does not have `smithy.framework#ValidationException`
attached, when registering the handler in the service builder, the user must
provide the mapping to an `error::MyOperationError` variant when constraint
trait enforcement fails.

```rust
let my_service = MyService::builder_without_plugins()
    .my_operation(operation_handler)
    .on_my_operation_validation_failure(map_operation_validation_failure_to_error)
    .build()
    .unwrap();
```

This solution is ~~shamelessly pilfered~~ inspired by what the
smithy-typescript server SDK implements, which is described [in this comment]
and in [the documentation][smithy-typescript-custom-validation].

Two last observations:

1. The fact that this solution only allows service owners to map the constraint
   violation exceptions to a modeled error shape, as opposed to giving them the
   freedom of returning _anything_ that can be converted into an HTTP response
   is _intentional_. Too much freedom would allow a user to return HTTP
   responses that could break the underlying protocol.
2. `ConstraintViolationException`s have until now been `pub(crate)`, for
   exclusive use by the framework. Opting into `disableDefaultValidation`
   should unhide only those associated to constrained operations where
   `smithy.framework#ValidationException` is not attached to the operation. The
   user shouldn't have a need to reach for the other operation's constraint
   violation exceptions.

Note: [this comment] in `ValidateUnsupportedConstraints.kt` alludes to the solution proposed in this section.

[smithy-typescript-custom-validation]: https://smithy.io/ts-ssdk/validation.html#custom-validation
[`core::ops::Fn`]: https://doc.rust-lang.org/nightly/core/ops/trait.Fn.html
[in this comment]: https://github.com/awslabs/smithy-rs/pull/1199#discussion_r809424783
[this comment]: https://github.com/awslabs/smithy-rs/blob/0adad6a72dd872044765e9fbbb220264c849ce39/codegen-server/src/main/kotlin/software/amazon/smithy/rust/codegen/server/smithy/ValidateUnsupportedConstraints.kt#L150C6-L153

### Unresolved questions and alternative designs

#### Usability concerns about likely reuse of custom validation error mappings

For maximum freedom, the solution described above allows the service owner to
configure, _per-operation_, how they'd like to map from constraint violation
exceptions to their user-defined error shapes. Operation `A` should be able to
map to a `ValidationCustomErrorA` while operation `B` can map to a
`ValidationCustomErrorB`. However, the custom responses are likely to be
homogeneous, since validation is a service-level concern.

In the case where a service owner would e.g. just like to swap
`smithy.framework#ValidationException` for another `ValidationCustomError`
shape _across all constrained operations_, they can very easily add the error
shape to _all_ operations in their Smithy model, by leveraging the [`service`
shape](https://smithy.io/2.0/spec/service-types.html#service)'s `errors`
property:

```smithy
service MyService {
    errors: [ValidationCustomError]
    operations: [Operation1, ... OperationN]
}

@error("client")
structure ValidationCustomError { 
    validationErrorMessage: String
}
```

However, in Rust, they'd need `N` different functions, each with different
inputs and outputs, even if their core logic is likely to be the same:

```rust
fn map_operation1_validation_failure_to_error(errors: input::operation1_input::ConstraintViolationExceptions) -> error::Operation1Error { /* ... */ }
...
fn map_operation2_validation_failure_to_error(errors: input::operation1_input::ConstraintViolationExceptions) -> error::Operation2Error { /* ... */ }
```

```rust
let my_service = MyService::builder_without_plugins()
    .operation1(operation_handler1)
    .on_operation1_validation_failure(map_operation1_validation_failure_to_error)
    ...
    .operationN(operation_handlerN)
    .on_operationN_validation_failure(map_operationN_validation_failure_to_error)
    .build()
    .unwrap();
```

Perhaps this is just a papercut, arguing that Rust macros exist precisely to
help in reducing this verbosity.

If we instead have service owners provide the mapping from the _common_
`ValidationException` type corresponding to the
`smithy.framework#ValidationException` into the _common_
`ValidationCustomError` error type they registered in their `service` shape,
they'd be able to write:

```rust
fn common_map_validation_failure_to_error(errors: error::ValidationException) -> error::ValidationCustomError { 
    // Translate the error messages to Spanish.
    // ...
}
```

```rust
let my_service = MyService::builder_without_plugins()
    .operation1(operation_handler1)
    ...
    .operationN(operation_handlerN)
    .on_validation_failure(common_map_validation_failure_to_error)
    .build()
    .unwrap();
```

The framework would have to wrap the common `error::ValidationCustomError` into
each of the `error::Operation1Error`, ... `error::OperationNError` enums under
the hood.

This approach has of course the obvious disadvantage that the application owner
loses out in terms of:

- customizability: they are no longer able to tweak the mapping for a specific
  operation; and
- type safety / expressiveness: they now have to work with essentially a vector
  of strings as input.

A combined solution where we let service owners register a common
`on_validation_failure` callback while retaining the ability to customize the
mapping for concrete operations:

```rust
let my_service = MyService::builder_without_plugins()
    .operation1(operation_handler1)
    .operation2(operation_handler2)
    ...
    .operationI(operation_handlerI)
    .on_operationI_validation_failure(map_operationI_validation_failure_to_error)
    ...
    .operationN(operation_handlerN)
    .on_validation_failure(common_map_validation_failure_to_error)
    .build()
    .unwrap();
```

is likely to lead to confusion.

#### Registering the custom validation error mapping together with the operation

The mapping from an operation's `ConstraintViolationExceptions` to the
operation error shape is likely to live next to the operation handler
implementation. Likewise, registering the mapping and the operation handler on
the service builder is likely to occur in neighboring lines. Since the builder
will fail if either of the two are not provided, we could couple the
registration of both in a single call by passing in a pair:

```rust
let my_service = MyService::builder_without_plugins()
    .my_operation((operation_handler, map_operation_validation_failure_to_error))
    .build()
    .unwrap();
```

This API [resembles more][smithy-typescript-custom-validation]
smithy-typescript's:

```typescript
const handler = getGetForecastHandler(async (input) => getForecast(input),
    (ctx, failures) => {
        return new BadInputError(`${failures.length} bad inputs detected.`);
    };
});
```

#### Synchronicity of the custom validation error mapping

Should the `ConstraintViolationExceptions` mapping be `async`? In other words,
should we require a [`core::ops::Fn`], or a future type? All use cases so far
would be solved with a synchronous mapping, but a priori there should be no
problem in allowing async functions. As a data point, axum's
[`IntoResponse`](https://docs.rs/axum-core/latest/axum_core/response/trait.IntoResponse.html)
is synchronous.

#### Access to extractors in the custom validation error mapping

Should the `ConstraintViolationExceptions` mapping have access to request
extractors, like operation handlers do?

```rust
fn map_operation_validation_failure_to_error(
    errors: input::my_operation_input::ConstraintViolationExceptions,
    request_id: Extension<ServerRequestId>,
    global_state: Extension<GlobalState>,
) -> error::OperationError {
    // ...
}
```

Most mappings will probably be stateless and only require knowledge of the
modeled data, but a priori there should be no problem in allowing access to
extractors. As a data point,
[smithy-typescript][smithy-typescript-custom-validation] does allow access to
the incoming request's context.

Since granting access to extractors is a backwards-compatible change, we could
punt on implementing this until user interest is expressed and we learn about
use cases.

#### "Tightness" of constraint violations

`ConstraintViolationExceptions` [is not
"tight"](https://www.ecorax.net/tightness/) in that there's nothing in the type
system that indicates to the user, when writing the custom validation error
mapping function, that the iterator will not return a sequence of
`ConstraintViolationException`s that is actually impossible to occur in
practice.

Recall that `ConstraintViolationException`s are `enum`s that model both direct
constraint violations as well as transitive ones. For example, given the model:

```smithy
@length(min: 1, max: 69)
map LengthMap {
    key: String,
    value: LengthString
}

@length(min: 2, max: 69)
string LengthString
```

The corresponding `ConstraintViolationException` Rust type for the `LengthMap`
shape is:

```rust
pub mod length_map {
    pub enum ConstraintViolation {
        Length(usize),
    }
    pub (crate) enum ConstraintViolationException {
        Length(usize),
        Value(
            std::string::String,
            crate::model::length_string::ConstraintViolationException,
        ),
    }
}
```

`ConstraintViolationExceptions` is just a container over this type:

```rust
pub ConstraintViolationExceptions<T>(pub(crate) Vec<T>);

impl<T> IntoIterator<Item = T> for ConstraintViolationExceptions<T> { ... }
```

There might be multiple map values that fail to adhere to the constraints in
`LengthString`, which would make the iterator yield multiple
`length_map::ConstraintViolationException::Value`s; however, at most one
`length_map::ConstraintViolationException::Length` can be yielded _in
practice_. This might be obvious to the service owner when inspecting the model
and the Rust docs, but it's not expressed in the type system.

At the risk of increasing API complexity and hence user confusion, a possible
solution could be to make the API of `ConstraintViolationExceptions` _vary by
constrained shape_, with those corresponding to some aggregate shapes better
expressing what can actually occur in practice. For example:

```rust
pub mod length_map {
    pub enum ConstraintViolation {
        Length(usize),
    }

    // Everything below would be `pub(crate)` in the case where the user has
    // not opted into `disableDefaultValidation` or the `LengthMap` shape does not
    // lie in the closure of an operation that has
    // `smithy.framework#ValidationException` attached.

    // This is what the custom validation error mapping function would receive
    // as input.
    pub struct ConstraintViolationExceptions {
        pub length: Option<constraint_violation_exception::Length>,

        // Would be `Option<T>` in the case of an aggregate shape that is _not_ a
        // list shape or a map shape.
        pub member_exceptions: constraint_violation_exception::Members,
    }

    pub mod constraint_violation_exception {
        // Note that this could now live outside the `length_map` module and be
        // reused across all `@length`-constrained shapes, if we expanded it with
        // another `usize` indicating the _modeled_ value in the `@length` trait; by
        // keeping it inside `length_map` we can hardcode that value in the
        // implementation of e.g. error messages.
        pub struct Length(usize);

        pub struct Members {
            pub(crate) Vec<Member>
        }

        // This is always an `enum`, even when it only has one variant.
        pub struct Member {
            // If the map's key shape were constrained, we'd have a `key`
            // field here too.

            value: Option<Value>

        }

        pub struct Value(
            std::string::String,
            crate::model::length_string::ConstraintViolationException,
        );

        impl IntoIterator<Item = Member> for Members { ... }
    }
}
```

With the above, the iterator is now over the constraint violation exceptions
that can be yielded by the map's _contents_.

`IntoIterator` would only be implemented for constraint violations of aggregate
shape's members such that the aggregate shape is a collection. In other words,
map shapes and list shapes only. It wouldn't affect how constraint violations
corresponding to member shapes of structure or union shapes get rendered. For
example, the model:

```smithy
structure A {
    @required
    member: String,

    @required
    length_map: LengthMap,
}
```

would yield:

```rust
pub mod a {
    pub enum ConstraintViolation {
        MissingMember,
        MissingLengthMap,
    }

    pub struct ConstraintViolationExceptions {
        // All fields must be `Option`, despite the members being `@required`,
        // since no violations for their values might have occurred.

        pub missing_member: Option<constraint_violation_exception::MissingMember>,
        pub missing_length_map: Option<constraint_violation_exception::MissingLengthMap>,
        pub length_map_exceptions: Option<crate::model::length_map::ConstraintViolationExceptions>,
    }

    pub mod constraint_violation_exception {
        pub struct MissingMember;
        pub struct MissingLengthMap;
    }
}
```

---

The above tightness problem has been formulated in terms of
`ConstraintViolationExceptions`, because the fact that
`ConstraintViolationExceptions` contain transitive constraint violations
highlights the tightness problem. The attentive reader will have noticed that
the samples above have featured the `ConstraintViolation` enum, unchanged.
Note, however, that **the tightness problem also afflicts
`ConstraintViolations`**, and that the `ConstraintViolation` enum would
disappear too were we to tackle the problem in a similar fashion.

Indeed, consider the following model:

```smithy
@pattern("[a-f0-5]*")
@length(min: 5, max: 10)
string LengthPatternString
```

This yields, as per [RFC: Better Constraint
Violations][better-constraint-violations]:

```rust
pub ConstraintViolations<T>(pub(crate) Vec<T>);

impl<T> IntoIterator<Item = T> for ConstraintViolations<T> { ... }

pub mod length_pattern_string {
    pub enum ConstraintViolation {
        Length(usize),
        Pattern(String)
    }
}

impl std::convert::TryFrom<std::string::String> for LengthPatternString {
    type Error = ConstraintViolations<crate::model::length_pattern_string::ConstraintViolation>;

    fn try_from(value: std::string::String) -> Result<Self, Self::Error> {
        // Check constraints and collect violations.
        ...
    }
}
```

Observe how the iterator of an instance of
`ConstraintViolations<crate::model::length_pattern_string::ConstraintViolation>`,
may, a priori, yield e.g. the
`length_pattern_string::ConstraintViolation::Length` variant twice, when it's
clear that the iterator should contain _at most one_ of each of
`length_pattern_string::ConstraintViolation`'s variants.

A tighter API design, similar to the one proposed above for the
`ConstraintViolationExceptions` case, would make the `ConstraintViolation`
enum, as well as the iterator, disappear:

```rust
pub mod length_pattern_string {
    pub struct ConstraintViolations {
        pub length: Option<constraint_violation::Length>,
        pub pattern: Option<constraint_violation::Pattern>,
    }

    pub mod constraint_violation {
        pub struct Length(usize);
        pub struct Pattern(String);
    }
}

impl std::convert::TryFrom<std::string::String> for LengthPatternString {
    type Error = length_pattern_string::ConstraintViolations;

    fn try_from(value: std::string::String) -> Result<Self, Self::Error> {
        // Check constraints and collect violations.
        ...
    }
}
```
