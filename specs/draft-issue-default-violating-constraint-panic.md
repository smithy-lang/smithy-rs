# Server panics at request time when a member's implicit default violates the target shape's constraints

## Summary

A generated smithy-rs **server** panics while deserializing an otherwise valid request when:

1. a structure member targets a **constrained numeric shape** (e.g. `@range`), and
2. the member is a **non-boxed primitive** (Smithy 1.0 `integer`/`short`/`long`/`byte` without `@box`, i.e. it carries an implicit `@default` of `0`), and
3. the request **omits** that member, and
4. `0` (the implicit default) **violates** the target shape's constraint.

Instead of returning `400 ValidationException` — or, better, failing at code generation time — the generated builder calls `.expect(...)` on the default's `try_into()` conversion and panics. The panic message even states that the situation should have been caught at generation time.

The service produces **no HTTP response** for the request; unless the operator has installed a catch-panic layer, the connection is dropped.

## Minimal reproduction

### Model

```smithy
$version: "1.0"

namespace com.example

use aws.protocols#restJson1
use smithy.framework#ValidationException

@restJson1
@title("ConstraintsService")
service ConstraintsService {
    operations: [ConstrainedShapesOperation]
}

@http(uri: "/constrained-shapes-operation", method: "POST")
operation ConstrainedShapesOperation {
    input: ConstrainedShapesOperationInputOutput,
    output: ConstrainedShapesOperationInputOutput,
    errors: [ValidationException]
}

structure ConstrainedShapesOperationInputOutput {
    @required
    conA: ConA,
}

structure ConA {
    @required
    conB: ConB,

    // Non-boxed primitive member => non-`Option<i32>` in Rust, implicit default `0`.
    // `0` is outside the target shape's `@range`.
    fixedValueInteger: FixedValueInteger,
}

structure ConB {
    @required
    nice: String,
    @required
    int: Integer,
}

@range(min: 69, max: 69)
integer FixedValueInteger
```

This is distilled from `codegen-core/common-test-models/constraints.smithy`, which already contains exactly this pattern:

```smithy
structure ConA {
    // ...
    fixedValueInteger: FixedValueInteger,
    fixedValueShort: FixedValueShort,
    fixedValueLong: FixedValueLong,
    fixedValueByte: FixedValueByte,
    // ...
}

@range(min: 69, max: 69)
integer FixedValueInteger

@range(min: 10, max: 10)
short FixedValueShort

@range(min: 10, max: 10)
long FixedValueLong

@range(min: 10, max: 10)
byte FixedValueByte
```

### Request

Generate the server for the service above and send a request that is valid per the model but omits `fixedValueInteger`:

```
POST /constrained-shapes-operation
Content-Type: application/json

{"conA":{"conB":{"nice":"n","int":1}}}
```

### Observed

The service panics inside the generated `ConA` builder:

```
thread '...' panicked at src/model.rs:5118:
this check should have failed at generation time; please file a bug report under https://github.com/smithy-lang/smithy-rs/issues: Range(0)
```

No HTTP response is produced.

### Expected

Either:

- **(preferred) Fail at code generation time.** Reject a model in which a member's default value (explicit `@default` or the implicit default of a non-boxed primitive) violates the constraint traits on the member's target shape. The generated `.expect(...)` message already asserts that such a check exists; today it does not.
- **Or, return `400 ValidationException` at runtime**, reporting the constraint violation on the omitted member, consistent with how other constraint violations are surfaced.

Whichever is chosen, a valid, well-formed request must never take the server process into a panic.

## Root cause

`codegen-server/src/main/kotlin/software/amazon/smithy/rust/codegen/server/smithy/generators/ServerBuilderGeneratorCommon.kt`, in `generateFallbackCodeToDefaultValue`.

For a member with a default value, the generator emits code that is interpolated after an `Option<T>` and materializes the default when the field was not set. When the target shape has a public constrained wrapper tuple type, the raw default is funnelled through the wrapper's fallible `TryFrom` conversion and the failure case is `expect`ed:

```kotlin
if (targetShape.hasPublicConstrainedWrapperTupleType(model, publicConstrainedTypes)) {
    // TODO(https://github.com/smithy-lang/smithy-rs/issues/2134): Instead of panicking here, which will ungracefully
    //  shut down the service, perform the `try_into()` check _once_ at service startup time, perhaps
    //  storing the result in a `OnceCell` that could be reused.
    writer.rustTemplate(
        """
        .unwrap_or_else(||
            #{DefaultValue:W}
                .try_into()
                .expect("this check should have failed at generation time; please file a bug report under https://github.com/smithy-lang/smithy-rs/issues")
        )
        """,
        "DefaultValue" to defaultValue,
    )
}
```

(`ServerBuilderGeneratorCommon.kt` line 115 is the `.expect(...)` line.)

`defaultValue(...)` renders the bare literal for the target's primitive kind (e.g. `0i32` for an `IntegerShape`), so for `fixedValueInteger` the generated fallback is effectively:

```rust
.unwrap_or_else(|| 0i32.try_into().expect("this check should have failed at generation time; ..."))
```

`0i32.try_into()` into the `@range(min: 69, max: 69)` wrapper returns `Err(Range(0))`, and the `expect` panics. Nothing at codegen time compares the member's default against the target shape's constraint traits, so the invariant the `expect` message relies on is never actually established.

Note that the existing TODO for [#2134](https://github.com/smithy-lang/smithy-rs/issues/2134) proposes moving the check to service startup, which would turn this into a startup panic rather than a per-request one — still a panic, and still after codegen has accepted a model it should arguably have rejected.

## Why this hasn't shown up in tests

Generated builders **fail fast**: the first constraint violation encountered (in codegen member order) short-circuits `build()` and returns a `ConstraintViolation`. Existing constraint tests send payloads that violate an *earlier* member's constraint, so the builder returns before ever reaching the defaulted member's fallback. The panic is only reachable with a request that is valid for every member preceding the defaulted one — which is precisely the case for a legitimate client request that simply omits an optional-looking field.

## Affected versions

Present on `main`. Verified against codegen at commit `f97cba901`; the responsible line in `ServerBuilderGeneratorCommon.kt` is unchanged on `origin/main`. Any server generated from a model matching the pattern above is affected.
