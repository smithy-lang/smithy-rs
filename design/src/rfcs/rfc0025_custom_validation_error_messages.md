RFC: Better Constraint Violations
=================================

> Status: RFC
>
> Applies to: server

For a summarized list of proposed changes, see the [Changes Checklist](#changes-checklist) section.

<!-- Insert a short paragraph explaining, at a high level, what this RFC is for -->
This RFC defines how...

Terminology
-----------

- **Shape closure**: the set of shapes a shape can "reach", including itself.
- **Transitively constrained shape**: a shape whose closure includes: 
    1. a shape with a [constraint trait] attached,
    2. a (member) shape with a [`required` trait] attached; or
    3. an [`enum` shape].
- A **directly constrained shape** is a any of these:
    1. a shape with a [constraint trait] attached,
    2. a (member) shape with a [`required` trait] attached,
    3. an [`enum` shape]; or
    4. a [`structure` shape] with at least one `required` member shape.
- **Constrained type**: the Rust type a constrained shape gets rendered as. For shapes that are not `structure`, `union`, or `enum` shapes, these are wrapper [newtype]s.
- **Constrained operation**: an operation whose input shape is transitively constrained.

In the absence of a qualifier, "constrained shape" should be interpreted as "transitively constrained shape".

Impossible constraint violations
--------------------------------

### Background

A constrained type has a fallible constructor by virtue of it implementing the [`TryFrom`] trait. The error type this constructor may yield is known as a constraint violation:

```rust
impl TryFrom<UnconstrainedType> for ConstrainedType {
    type Error = ConstraintViolation;

    fn try_from(value: UnconstrainedType) -> Result<Self, Self::Error> {
        ...
    }
}
```

The `ConstraintViolation` type is currently a Rust `enum` with one variant per way "constraining" the input value may fail. So, for example, the following Smithy model:


Yields:


```rust

```

Constraint violations are always Rust `enum`s, even if they only have one variant.

### Problem

Currently, the constraint violation types we generate are used by _both_:

1. the server framework upon request deserialization; and
2. by application code.

However, the kinds of constraint violations that can occur in application code can sometimes be a strict subset of those that can occur during request deserialization.

Consider the following model:

```
@length(min: 1, max: 69)
map LengthMap {
    key: String,
    value: LengthString
}

@length(min: 2, max: 69)
string LengthString
```

This produces:

```rust
pub struct LengthMap(
    pub(crate) std::collections::HashMap<std::string::String, crate::model::LengthString>,
);

impl
    std::convert::TryFrom<
        std::collections::HashMap<std::string::String, crate::model::LengthString>,
    > for LengthMap
{
    type Error = crate::model::length_map::ConstraintViolation;

    /// Constructs a `LengthMap` from an
    /// [`std::collections::HashMap<std::string::String,
    /// crate::model::LengthString>`], failing when the provided value does not
    /// satisfy the modeled constraints.
    fn try_from(
        value: std::collections::HashMap<std::string::String, crate::model::LengthString>,
    ) -> Result<Self, Self::Error> {
        let length = value.len();
        if (1..=69).contains(&length) {
            Ok(Self(value))
        } else {
            Err(crate::model::length_map::ConstraintViolation::Length(length))
        }
    }
}

pub mod length_map {
    pub enum ConstraintViolation {
        Length(usize),
        Value(
            std::string::String,
            crate::model::length_string::ConstraintViolation,
        ),
    }
    ...
}
```

Observe how the `ConstraintViolation::Value` variant is never constructed. Indeed, this variant is impossible to be constructed _in application code_: a user has to provide a map whose values are already constrained `LengthString`s to the `try_from` constructor, which only enforces the map's `@length` trait.

The reason why these seemingly "impossible violations" are being generated is because they can arise during request deserialization. Indeed, the server framework deserializes requests into _fully unconstrained types_. These are types holding unconstrained types all the way through their closures. For instance, in the case of structure shapes, builder types (the unconstrained type for the structure shape) [hold builders] all the way down.

In the case of the above model, below is the alternate constructor the server framework uses upon deserialization. Observe how `LengthMapOfLengthStringsUnconstrained` is _fully unconstrained_ and how the `TryFrom` constructor can yield `ConstraintViolation::Value`.

```rust
pub(crate) mod length_map_of_length_strings_unconstrained {
    #[derive(Debug, Clone)]
    pub(crate) struct LengthMapOfLengthStringsUnconstrained(
        pub(crate) std::collections::HashMap<std::string::String, std::string::String>,
    );

    impl From<LengthMapOfLengthStringsUnconstrained>
        for crate::constrained::MaybeConstrained<crate::model::LengthMapOfLengthStrings>
    {
        fn from(value: LengthMapOfLengthStringsUnconstrained) -> Self {
            Self::Unconstrained(value)
        }
    }
    impl std::convert::TryFrom<LengthMapOfLengthStringsUnconstrained>
        for crate::model::LengthMapOfLengthStrings
    {
        type Error = crate::model::length_map_of_length_strings::ConstraintViolation;
        fn try_from(value: LengthMapOfLengthStringsUnconstrained) -> Result<Self, Self::Error> {
            let res: Result<
                std::collections::HashMap<crate::model::LengthString, std::string::String>,
                Self::Error,
            > = value
                .0
                .into_iter()
                .map(|(k, v)| {
                    let k: crate::model::LengthString = k.try_into().map_err(Self::Error::Key)?;

                    Ok((k, v))
                })
                .collect();
            let hm = res?;
            Self::try_from(hm)
        }
    }
}
```

In conclusion, the user is currently exposed to an internal detail of how the framework operates that has no bearing on their application code. They shouldn't be exposed to impossible constraint violation variants in their Rust docs, nor have to `match` on these variants when handling errors.

Note: [this comment] alludes to the problem described above.

[hold builders]:
[this comment]:

### Solution proposal

The problem can be mitigated by adding `#[doc(hidden)]` to the internal variants and `#[non_exhaustive]` to the enum. We're already doing this in some constraint violation types.

However, a "less leaky" solution is achieved by _splitting_ the constraint violation type into two types:

1. one for use by the framework, with `pub (crate)` visibility, named `ConstraintViolationException`; and
2. one for use by user application code, with `pub` visibility, named `ConstraintViolation`.

```rust
pub mod length_map {
    pub enum ConstraintViolation {
        Length(usize),
    }
    pub (crate) enum ConstraintViolationException {
        Length(usize),
        Value(
            std::string::String,
            crate::model::length_string::ConstraintViolation,
        ),
    }
}
```

Note that, to some extent, this solution is [already currently present](https://github.com/awslabs/smithy-rs/blob/9a4c1f304f6f5237d480cfb56dad2951d927d424/codegen-server/src/main/kotlin/software/amazon/smithy/rust/codegen/server/smithy/generators/ServerBuilderGenerator.kt#L78-L81) in the case of builder types when `publicConstrainedTypes` is set to `false`:

1. `ServerBuilderGenerator.kt` renders the usual builder type that enforces constraint traits, setting its visibility to `pub (crate)`, for exclusive use by the framework.
2. `ServerBuilderGeneratorWithoutPublicConstrainedTypes` renders the builder type the user is exposed to: this builder does not take in constrained types and does not enforce all modeled constraints.

Collecting constraint violations
--------------------------------

### Background

It was mentioned in the [constraint traits RFC](https://github.com/awslabs/smithy-rs/pull/1199#discussion_r809300673), and implicit in the definition of Smithy's [`smithy.framework.ValidationException`](https://github.com/awslabs/smithy/blob/main/smithy-validation-model/model/smithy.framework.validation.smithy) shape, that server frameworks should respond with a _full_ collection of errors encountered during constraint trait enforcement to the client.

### Problem

As of writing, the `TryFrom` constructor of constrained types whose shapes have more than one constraint trait attached can only yield a single error. For example, the following shape:

```smithy
@pattern("[a-f0-5]*")
@length(min: 5, max: 10)
string LengthPatternString
```

Is rendered as:

```rust
impl LengthPatternString {
    fn check_length(
        string: &str,
    ) -> Result<(), crate::model::length_pattern_string::ConstraintViolation> {
        let length = string.chars().count();

        if (5..=10).contains(&length) {
            Ok(())
        } else {
            Err(crate::model::length_pattern_string::ConstraintViolation::Length(length))
        }
    }

    fn check_pattern(
        string: String,
    ) -> Result<String, crate::model::length_pattern_string::ConstraintViolation> {
        let regex = Self::compile_regex();

        if regex.is_match(&string) {
            Ok(string)
        } else {
            Err(crate::model::length_pattern_string::ConstraintViolation::Pattern(string))
        }
    }

    pub fn compile_regex() -> &'static regex::Regex {
        static REGEX: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| {
            regex::Regex::new(r#"[a-f0-5]*"#).expect(r#"The regular expression [a-f0-5]* is not supported by the `regex` crate; feel free to file an issue under https://github.com/awslabs/smithy-rs/issues for support"#)
        });

        &REGEX
    }
}
impl std::convert::TryFrom<std::string::String> for LengthPatternString {
    type Error = crate::model::length_pattern_string::ConstraintViolation;

    /// Constructs a `LengthPatternString` from an [`std::string::String`], failing when the provided value does not satisfy the modeled constraints.
    fn try_from(value: std::string::String) -> Result<Self, Self::Error> {
        Self::check_length(&value)?;

        let value = Self::check_pattern(value)?;

        Ok(Self(value))
    }
}
```

Observe how a failure to adhere to the `@length` trait will short-circuit the evaluation of the constructor, when the value could technically also not adhere with the `@pattern` trait.

Similarly,

* collections whose members are constrained fail upon encountering the first member that violates the constraint,
* maps whose keys and/or values are constrained fail upon encountering the first violation; and
* constrained structures fail upon encountering the first member that violates a constraint.

In summary, any shape that is transitively constrained yields types whose constructors currently fail upon encountering the first violation.

### Solution proposal

The deserializing architecture lends itself to be easily refactored so that we can collect constraint violations before returning them. Indeed, note that deserializers enforce constraint traits in a two-step phase: first, the _entirety_ of the unconstrained value is deserialized, _then_ constraint traits are enforced by feeding the entire value to the `TryFrom` constructor.

We will introduce a `ConstraintViolations` type (note the plural) that represents a collection of constraint violations. Roughly:

```rust
pub ConstraintViolations<T>(pub(crate) Vec<T>);

impl<T> IntoIterator<Item = T> for ConstraintViolations<T> { ... }

impl std::convert::TryFrom<std::string::String> for LengthPatternString {
    type Error = ConstraintViolations<crate::model::length_patterh_string::ConstraintViolation>;

    fn try_from(value: std::string::String) -> Result<Self, Self::Error> {
        // Check constraints and collect violations.
        ...
    }
}
```

* The main reason for wrapping a vector in `ConstraintViolations` as opposed to directly returning the vector is forwards-compatibility: we may want to expand `ConstraintViolations` with conveniences (see below).
* If the constrained type can only ever yield a single violation, we will dispense with `ConstraintViolations` and keep directly returning the `crate::model::shape_name::ConstraintViolation` type.

### Unresolved questions

#### "Tightness" of constraint violations

`ConstraintViolations` is

#### Collecting constraint violations may constitute a DOS attack vector

This is a problem that _already_ exists as of writing, but that collecting constraint violations highlights, so it is a good opportunity, from a pedagogical perspective, to explain it here. Consider the following model:

```smithy
@length(max: 3)
list ListOfPatternStrings {
    member: PatternString
}

@pattern("expensive regex to evaluate")
string PatternString
```

Our implementation currently enforces constraints _from the leaf to the root_: when enforcing the `@length` constraint, the `TryFrom` constructor the server framework uses gets a `Vec<String>` and _first_ checks the members adhere to the `@pattern` trait, and only _after_ is the `@length` trait checked at the end. This means that if a client sends a request `n >>> 3` times, the expensive check runs `n` times, when a constant-time check inspecting the length of the input vector would have sufficed to reject the request.

* A possibility to circumvent this is making the `@length` validator special, having it bound the other validators via effectively permuting the order of the checks and thus short-circuiting. In general, it's unclear what constraint traits should cause short-circuiting. A probably reasonable rule of thumb is to include traits that can be attached directly to aggregate shapes: as of writing, that would be `@uniqueItems` on list shapes and `@length` on list shapes.
* Another possiblity is to _do nothing_ and value _complete_ validation exception response messages over trying to mitigate this with special handling. One could argue that these kind of DOS attack vectors should be taken care of with a separate solution e.g. a layer that bounds a request body's size to a reasonable default (see [how Axum added this](https://github.com/tokio-rs/axum/pull/1420)).

TODO: Mention stack overflow DOS attack vector in comment.

Custom error response messages
------------------------------

### Background

Constrained operations are currently required to have `smithy.framework#ValidationException` as a member in their [`errors` property](https://smithy.io/2.0/spec/service-types.html#operation). This is the shape that is rendered in responses when a request contains data that violates the modeled constraints.

The shape is defined in the [`smithy-validation-model` Maven package](https://search.maven.org/artifact/software.amazon.smithy/smithy-validation-model), [as follows](https://github.com/awslabs/smithy/blob/main/smithy-validation-model/model/smithy.framework.validation.smithy):

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

Smithy's protocol tests provide a normative reference for what the error message contents' should be. For example, the [RestJsonMalformedPatternMapValue](https://github.com/awslabs/smithy/blob/31ddf685d7e3fe287eac51442621975d585972fd/smithy-aws-protocol-tests/model/restJson1/validation/malformed-pattern.smithy#L154-L154) test stipulates that upon receiving the JSON value:

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

Currently, the framework is mapping from the operation input shape's `ConstraintViolation` to a `ValidationException`. This mapping is hardcoded so as to make the protocol tests pass.

### Problem

Some users have expressed interest in being able to customize the error responses. So far their use cases mainly revolve around using smithy-rs server SDKs in frontend services that are invoked by user interface code: for example, said service might want to internationalize or enhance error messages that are displayed directly to users in a web form.

Currently, it is a _strict requirement_ that constrained operations's errors contain `smithy.framework#ValidationException`, and its error messages are not configurable.

### Solution proposal

It is worth noting that the _vast_ majority of smithy-rs server SDK users are operating within a backend service, and that the default behavior prescribed by Smithy's protocol tests is satisfactory for their use cases.

Therefore, it is reasonable to expect that a solution to lift the aforementioned restriction be _opt-in_. We will introduce a `codegenConfig.disableDefaultValidation` flag in `smithy-build.json` with the following semantics:

* If `disableDefaultValidation` is set to `false`, we will continue operating as we currently do: all constrained operations _must_ have `smithy.framework#ValidationException` attached.
* If `disableDefaultValidation` is set to `true`, any constrained operation that does not have `smithy.framework#ValidationException` attached will make it so that the generated SDK forces the user to provide a [`core::ops::Fn`](https://doc.rust-lang.org/nightly/core/ops/trait.Fn.html) that maps from the constrained operation's `ConstraintViolations` container newtype to any modeled error shape attached to the operation.

For example, an operation like:

```smithy
operation Operation {
    input: OperationInput,
    output: OperationOutput,
    errors: [ValidationCustomError, OperationError1, ... OperationErrorN]
}

@error("client")
structure ValidationCustomError {
    validationErrorMessage: String
}
```

in a model generated with `codegenConfig.disableDefaultValidation` set to `true` will be implemented in user code as:

```rust
use my_server_sdk::{input, output, error}

fn map_operation_validation_failure_to_error(errors: operation::operation_input::ConstraintViolations) -> error::OperationError {
    let mut validation_error_message: String::new();

    for e in errors.into_iter() {
        match e {
            // ...
        }
    }

    error::OperationError {
        validation_error_message
    }
}

fn operation_handler(input: input::OperationInput) -> Result<output::OperationOutput, error::OperationError> {
    // ...
}
```

Since the operation does not have `smithy.framework#ValidationException` attached, when registering the handler in the service builder, the user must provide the mapping to an `error::OperationError` variant when constraint trait enforcement fails.

```rust
let my_service = MyService::builder_without_plugins()
    .my_operation(operation_handler)
    .on_my_operation_validation_failure(map_operation_validation_failure_to_error)
    .build();
```

Alternative: register a tuple in the service builder.

This solution is ~shamelessly pilfered~ inspired by what the smithy-typescript server SDK implements, which is described [in this message].

Note: [this comment] in `ValidateUnsupportedConstraints.kt` alludes to the solution proposed above.

[in this message]: https://github.com/awslabs/smithy-rs/pull/1199#discussion_r809424783

[this comment]: https://github.com/awslabs/smithy-rs/blob/0adad6a72dd872044765e9fbbb220264c849ce39/codegen-server/src/main/kotlin/software/amazon/smithy/rust/codegen/server/smithy/ValidateUnsupportedConstraints.kt#L150C6-L153

### Unresolved questions

How much freedom

Changes checklist
-----------------

- [x] Create new struct `NewFeature`
