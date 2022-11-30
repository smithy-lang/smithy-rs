RFC: Better Constraint Violations
=================================

> Status: RFC
>
> Applies to: server

During and after [the design][constraint-traits-rfc] and [the core
implementation][builders-of-builders-pr] of [constraint traits] in the server
SDK, some problems relating to constraint violations were identified. This RFC
sets out to explain and address two of them: [impossible constraint
violations](#impossible-constraint-violations) and [collecting constraint
violations](#collecting-constraint-violations).

Note: code snippets from generated SDKs in this document are abridged so as to
be didactic and relevant to the point being made. They are accurate with
regards to commit [`2226fe`].

[constraint-traits-rfc]: https://github.com/awslabs/smithy-rs/pull/1199
[builders-of-builders-pr]: https://github.com/awslabs/smithy-rs/pull/1342
[`2226fe`]: https://github.com/awslabs/smithy-rs/tree/2226feff8f7fa884204f81a50d7e016386912acc
[constraint traits]: https://awslabs.github.io/smithy/2.0/spec/constraint-traits.html

Terminology
-----------

[The design][constraint-traits-rfc] and the description of [the
PR][builders-of-builders-pr] where the core implementation of constraint traits
was made are recommended prior reading to understand this RFC.

- **Shape closure**: the set of shapes a shape can "reach", including itself.
- **Transitively constrained shape**: a shape whose closure includes:
    1. a shape with a [constraint trait][constraint traits] attached,
    2. a (member) shape with a [`required` trait] attached,
    3. an [`enum` shape]; or
    4. an [`intEnum` shape].
- A **directly constrained shape** is any of these:
    1. a shape with a [constraint trait][constraint traits] attached,
    2. a (member) shape with a [`required` trait] attached,
    3. an [`enum` shape],
    4. an [`intEnum` shape]; or
    5. a [`structure` shape] with at least one `required` member shape.
- **Constrained type**: the Rust type a constrained shape gets rendered as. For
  shapes that are not `structure`, `union`, `enum` or `intEnum` shapes, these
  are wrapper [newtype]s.

[`required` trait]: https://smithy.io/2.0/spec/type-refinement-traits.html#required-trait
[`enum` shape]: https://smithy.io/2.0/spec/simple-types.html#enum
[`intEnum` shape]: https://smithy.io/2.0/spec/simple-types.html#intenum
[`structure` shape]: https://smithy.io/2.0/spec/aggregate-types.html#structure
[newtype]: https://doc.rust-lang.org/rust-by-example/generics/new_types.html

In the absence of a qualifier, "constrained shape" should be interpreted as
"transitively constrained shape".

Impossible constraint violations
--------------------------------

### Background

A constrained type has a fallible constructor by virtue of it implementing the
[`TryFrom`] trait. The error type this constructor may yield is known as a
**constraint violation**:

```rust
impl TryFrom<UnconstrainedType> for ConstrainedType {
    type Error = ConstraintViolation;

    fn try_from(value: UnconstrainedType) -> Result<Self, Self::Error> {
        ...
    }
}
```

The `ConstraintViolation` type is a Rust `enum` with one variant per way
"constraining" the input value may fail. So, for example, the following Smithy
model:

```smithy
structure A {
    @required
    member: String,
}
```

Yields:

```rust
/// See [`A`](crate::model::A).
pub mod a {
    #[derive(std::cmp::PartialEq, std::fmt::Debug)]
    /// Holds one variant for each of the ways the builder can fail.
    pub enum ConstraintViolation {
        /// `member` was not provided but it is required when building `A`.
        MissingMember,
    }
}
```

Constraint violations are always Rust `enum`s, even if they only have one
variant.

Constraint violations can occur in application code:

```rust
use my_server_sdk::model

let res = model::a::Builder::default().build(); // We forgot to set `member`.

match res {
    Ok(a) => { ... },
    Err(e) => {
        assert_eq!(model::a::ConstraintViolation::MissingMember, e);
    }
}
```

[`TryFrom`]: https://doc.rust-lang.org/std/convert/trait.TryFrom.html

### Problem

Currently, the constraint violation types we generate are used by _both_:

1. the server framework upon request deserialization; and
2. by users in application code.

However, the kinds of constraint violations that can occur in application code
can sometimes be a _strict subset_ of those that can occur during request
deserialization.

Consider the following model:

```smithy
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

Observe how the `ConstraintViolation::Value` variant is never constructed.
Indeed, this variant is impossible to be constructed _in application code_: a
user has to provide a map whose values are already constrained `LengthString`s
to the `try_from` constructor, which only enforces the map's `@length` trait.

The reason why these seemingly "impossible violations" are being generated is
because they can arise during request deserialization. Indeed, the server
framework deserializes requests into **fully unconstrained types**. These are
types holding unconstrained types all the way through their closures. For
instance, in the case of structure shapes, builder types (the unconstrained
type corresponding to the structure shape) [hold
builders][builders-of-builders-pr] all the way down.

In the case of the above model, below is the alternate `pub(crate)` constructor
the server framework uses upon deserialization. Observe how
`LengthMapOfLengthStringsUnconstrained` is _fully unconstrained_ and how the
`try_from` constructor can yield `ConstraintViolation::Value`.

```rust
pub(crate) mod length_map_of_length_strings_unconstrained {
    #[derive(Debug, Clone)]
    pub(crate) struct LengthMapOfLengthStringsUnconstrained(
        pub(crate) std::collections::HashMap<std::string::String, std::string::String>,
    );

    impl std::convert::TryFrom<LengthMapOfLengthStringsUnconstrained>
        for crate::model::LengthMapOfLengthStrings
    {
        type Error = crate::model::length_map_of_length_strings::ConstraintViolation;
        fn try_from(value: LengthMapOfLengthStringsUnconstrained) -> Result<Self, Self::Error> {
            let res: Result<
                std::collections::HashMap<std::string::String, crate::model::LengthString>,
                Self::Error,
            > = value
                .0
                .into_iter()
                .map(|(k, v)| {
                    let v: crate::model::LengthString = k.try_into().map_err(Self::Error::Key)?;

                    Ok((k, v))
                })
                .collect();
            let hm = res?;
            Self::try_from(hm)
        }
    }
}
```

In conclusion, the user is currently exposed to an internal detail of how the
framework operates that has no bearing on their application code. They
shouldn't be exposed to impossible constraint violation variants in their Rust
docs, nor have to `match` on these variants when handling errors.

Note: [this comment] alludes to the problem described above.

[this comment]: https://github.com/awslabs/smithy-rs/blob/27020be3421fb93e35692803f9a795f92feb1d19/codegen-server/src/main/kotlin/software/amazon/smithy/rust/codegen/server/smithy/generators/MapConstraintViolationGenerator.kt#L66-L69

### Solution proposal

The problem can be mitigated by adding `#[doc(hidden)]` to the internal
variants and `#[non_exhaustive]` to the enum. We're already doing this in some
constraint violation types.

However, a "less leaky" solution is achieved by _splitting_ the constraint
violation type into two types:

1. one for use by the framework, with `pub(crate)` visibility, named
   `ConstraintViolationException`; and
2. one for use by user application code, with `pub` visibility, named
   `ConstraintViolation`.

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

Note that, to some extent, the spirit of this approach is [already currently
present](https://github.com/awslabs/smithy-rs/blob/9a4c1f304f6f5237d480cfb56dad2951d927d424/codegen-server/src/main/kotlin/software/amazon/smithy/rust/codegen/server/smithy/generators/ServerBuilderGenerator.kt#L78-L81)
in the case of builder types when `publicConstrainedTypes` is set to `false`:

1. [`ServerBuilderGenerator.kt`] renders the usual builder type that enforces
   constraint traits, setting its visibility to `pub (crate)`, for exclusive
   use by the framework.
2. [`ServerBuilderGeneratorWithoutPublicConstrainedTypes.kt`] renders the
   builder type the user is exposed to: this builder does not take in
   constrained types and does not enforce all modeled constraints.

[`ServerBuilderGenerator.kt`]: https://github.com/awslabs/smithy-rs/blob/2226feff8f7fa884204f81a50d7e016386912acc/codegen-server/src/main/kotlin/software/amazon/smithy/rust/codegen/server/smithy/generators/ServerBuilderGenerator.kt
[`ServerBuilderGeneratorWithoutPublicConstrainedTypes.kt`]: https://github.com/awslabs/smithy-rs/blob/2226feff8f7fa884204f81a50d7e016386912acc/codegen-server/src/main/kotlin/software/amazon/smithy/rust/codegen/server/smithy/generators/ServerBuilderGeneratorWithoutPublicConstrainedTypes.kt

Collecting constraint violations
--------------------------------

### Background

Constrained operations are currently required to have
`smithy.framework#ValidationException` as a member in their [`errors`
property](https://smithy.io/2.0/spec/service-types.html#operation). This is the
shape that is rendered in responses when a request contains data that violates
the modeled constraints.

The shape is defined in the
[`smithy-validation-model`](https://search.maven.org/artifact/software.amazon.smithy/smithy-validation-model)
Maven package, [as
follows](https://github.com/awslabs/smithy/blob/main/smithy-validation-model/model/smithy.framework.validation.smithy):

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

It was mentioned in the [constraint traits
RFC](https://github.com/awslabs/smithy-rs/pull/1199#discussion_r809300673), and
implicit in the definition of Smithy's
[`smithy.framework.ValidationException`](https://github.com/awslabs/smithy/blob/main/smithy-validation-model/model/smithy.framework.validation.smithy)
shape, that server frameworks should respond with a _complete_ collection of
errors encountered during constraint trait enforcement to the client.

### Problem

As of writing, the `TryFrom` constructor of constrained types whose shapes have
more than one constraint trait attached can only yield a single error. For
example, the following shape:

```smithy
@pattern("[a-f0-5]*")
@length(min: 5, max: 10)
string LengthPatternString
```

Yields:

```rust
pub struct LengthPatternString(pub(crate) std::string::String);

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

    /// Constructs a `LengthPatternString` from an [`std::string::String`],
    /// failing when the provided value does not satisfy the modeled constraints.
    fn try_from(value: std::string::String) -> Result<Self, Self::Error> {
        Self::check_length(&value)?;

        let value = Self::check_pattern(value)?;

        Ok(Self(value))
    }
}
```

Observe how a failure to adhere to the `@length` trait will short-circuit the
evaluation of the constructor, when the value could technically also not adhere
with the `@pattern` trait.

Similarly, constrained structures fail upon encountering the first member that
violates a constraint.

Additionally, _in framework request deserialization code_:

- collections whose members are constrained fail upon encountering the first
  member that violates the constraint; and
- maps whose keys and/or values are constrained fail upon encountering the
  first violation; and

In summary, any shape that is transitively constrained yields types whose
constructors (both the internal one and the user-facing one) currently
short-circuit upon encountering the first violation.

### Solution proposal

The deserializing architecture lends itself to be easily refactored so that we
can collect constraint violations before returning them. Indeed, note that
deserializers enforce constraint traits in a two-step phase: first, the
_entirety_ of the unconstrained value is deserialized, _then_ constraint traits
are enforced by feeding the entire value to the `TryFrom` constructor.

We will introduce a `ConstraintViolations` type (note the plural) that
represents a collection of constraint violations that can occur _within user
application code_. Roughly:

```rust
pub ConstraintViolations<T>(pub(crate) Vec<T>);

impl<T> IntoIterator<Item = T> for ConstraintViolations<T> { ... }

impl std::convert::TryFrom<std::string::String> for LengthPatternString {
    type Error = ConstraintViolations<crate::model::length_pattern_string::ConstraintViolation>;

    fn try_from(value: std::string::String) -> Result<Self, Self::Error> {
        // Check constraints and collect violations.
        ...
    }
}
```

- The main reason for wrapping a vector in `ConstraintViolations` as opposed to
  directly returning the vector is forwards-compatibility: we may want to
  expand `ConstraintViolations` with conveniences.
- If the constrained type can only ever yield a single violation, we will
  dispense with `ConstraintViolations` and keep directly returning the
  `crate::model::shape_name::ConstraintViolation` type.

We will analogously introduce a `ConstraintViolationExceptions` type that
represents a collection of constraint violations that can occur _within the
framework's request deserialization code_. This type will be `pub(crate)` and
will be the one the framework will map to Smithy's `ValidationException` that
eventually gets serialized into the response.

### Unresolved questions

#### Collecting constraint violations may constitute a DOS attack vector

This is a problem that _already_ exists as of writing, but that collecting
constraint violations highlights, so it is a good opportunity, from a
pedagogical perspective, to explain it here. Consider the following model:

```smithy
@length(max: 3)
list ListOfPatternStrings {
    member: PatternString
}

@pattern("expensive regex to evaluate")
string PatternString
```

Our implementation currently enforces constraints _from the leaf to the root_:
when enforcing the `@length` constraint, the `TryFrom` constructor the server
framework uses gets a `Vec<String>` and _first_ checks the members adhere to
the `@pattern` trait, and only _after_ is the `@length` trait checked. This
means that if a client sends a request with `n >>> 3` list members, the
expensive check runs `n` times, when a constant-time check inspecting the
length of the input vector would have sufficed to reject the request.
Additionally, we may want to avoid serializing `n` `ValidationExceptionField`s
due to performance concerns.

- A possibility to circumvent this is making the `@length` validator special,
  having it bound the other validators via effectively permuting the order of
  the checks and thus short-circuiting.
  * In general, it's unclear what constraint traits should cause
    short-circuiting. A probably reasonable rule of thumb is to include
    traits that can be attached directly to aggregate shapes: as of writing,
    that would be `@uniqueItems` on list shapes and `@length` on list shapes.
- Another possiblity is to _do nothing_ and value _complete_ validation
  exception response messages over trying to mitigate this with special
  handling. One could argue that these kind of DOS attack vectors should be
  taken care of with a separate solution e.g. a layer that bounds a request
  body's size to a reasonable default (see [how Axum added
  this](https://github.com/tokio-rs/axum/pull/1420)). We will provide a similar
  request body limiting mechanism regardless.
